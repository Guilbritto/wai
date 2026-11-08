use wai_agent::{
    CommandInfo, FileDiff, ModeInfo, PermissionRequest, QuestionRequest, SessionEvent, StopReason,
    ToolCallPatch, ToolContent, ToolStatus,
};

pub const TOOL_OUTPUT_MAX_LINES: usize = 2000;

/// What wai says when an agent is not signed in: it never shows a login of its own.
pub fn signed_out(agent: &str) -> String {
    format!(
        "{agent} is not signed in. Open a terminal and run its login (for Claude: claude /login)."
    )
}

#[derive(Debug, Clone, PartialEq)]
pub enum TurnState {
    Idle,
    Working,
    NeedsYou,
    Error(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoticeKind {
    Info,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ChatItem {
    User {
        text: String,
    },
    Agent {
        text: String,
    },
    Thought {
        text: String,
    },
    Tool(ToolItem),
    Permission {
        request: PermissionRequest,
        answered: Option<String>,
    },
    /// `answered` is what the card shows once closed: `Answered: …`, `Declined` or `Cancelled`.
    Question {
        request: QuestionRequest,
        answered: Option<String>,
    },
    Notice {
        text: String,
        kind: NoticeKind,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolItem {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: ToolStatus,
    pub text: String,
    pub diffs: Vec<FileDiff>,
    pub expanded: bool,
}

impl ToolItem {
    /// Output truncated to `TOOL_OUTPUT_MAX_LINES` unless expanded, plus the hidden line count.
    pub fn visible_text(&self) -> (&str, Option<usize>) {
        if self.expanded {
            return (&self.text, None);
        }
        match self.text.match_indices('\n').nth(TOOL_OUTPUT_MAX_LINES - 1) {
            Some((i, _)) => {
                let hidden = self.text[i + 1..].lines().count();
                if hidden == 0 {
                    (&self.text, None)
                } else {
                    (&self.text[..i], Some(hidden))
                }
            }
            None => (&self.text, None),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct UsageInfo {
    pub used: u64,
    pub size: u64,
    pub cost_usd: Option<f64>,
    pub rate_limit_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatModel {
    pub items: Vec<ChatItem>,
    pub state: TurnState,
    pub title: Option<String>,
    pub usage: Option<UsageInfo>,
    pub modes: Vec<ModeInfo>,
    pub current_mode: Option<String>,
    pub commands: Vec<CommandInfo>,
}

impl Default for ChatModel {
    fn default() -> Self {
        Self {
            items: Vec::new(),
            state: TurnState::Idle,
            title: None,
            usage: None,
            modes: Vec::new(),
            current_mode: None,
            commands: Vec::new(),
        }
    }
}

fn split_content(content: &[ToolContent]) -> (String, Vec<FileDiff>) {
    let mut parts = Vec::new();
    let mut diffs = Vec::new();
    for c in content {
        match c {
            ToolContent::Text(t) | ToolContent::Terminal(t) => parts.push(t.as_str()),
            ToolContent::Diff(d) => diffs.push(d.clone()),
        }
    }
    (parts.join("\n"), diffs)
}

impl ChatModel {
    fn push(&mut self, item: ChatItem) -> usize {
        self.items.push(item);
        self.items.len() - 1
    }

    pub fn notice(&mut self, text: String, kind: NoticeKind) {
        self.push(ChatItem::Notice { text, kind });
    }

    pub fn push_user(&mut self, text: &str) -> usize {
        self.state = TurnState::Working;
        self.push(ChatItem::User {
            text: text.to_string(),
        })
    }

    pub fn apply(&mut self, ev: &SessionEvent) -> Option<usize> {
        match ev {
            SessionEvent::AgentText(t) => Some(match self.items.last_mut() {
                Some(ChatItem::Agent { text }) => {
                    text.push_str(t);
                    self.items.len() - 1
                }
                _ => self.push(ChatItem::Agent { text: t.clone() }),
            }),
            SessionEvent::AgentThought(t) => Some(match self.items.last_mut() {
                Some(ChatItem::Thought { text }) => {
                    text.push_str(t);
                    self.items.len() - 1
                }
                _ => self.push(ChatItem::Thought { text: t.clone() }),
            }),
            SessionEvent::UserText(t) => Some(match self.items.last_mut() {
                Some(ChatItem::User { text }) => {
                    text.push_str(t);
                    self.items.len() - 1
                }
                _ => self.push(ChatItem::User { text: t.clone() }),
            }),
            SessionEvent::ToolCall(v) => {
                let (text, diffs) = split_content(&v.content);
                Some(self.push(ChatItem::Tool(ToolItem {
                    id: v.id.clone(),
                    title: v.title.clone(),
                    kind: v.kind.clone(),
                    status: v.status,
                    text,
                    diffs,
                    expanded: false,
                })))
            }
            SessionEvent::ToolCallUpdate(p) => self.patch_tool(p),
            SessionEvent::Usage {
                used,
                size,
                cost_usd,
                rate_limit,
            } => {
                self.usage = Some(UsageInfo {
                    used: *used,
                    size: *size,
                    cost_usd: *cost_usd,
                    rate_limit_status: rate_limit
                        .as_ref()
                        .and_then(|v| v.get("status"))
                        .and_then(|s| s.as_str())
                        .map(str::to_string),
                });
                None
            }
            SessionEvent::Title(t) => {
                self.title = Some(t.clone());
                None
            }
            SessionEvent::ModeChanged(m) => {
                self.current_mode = Some(m.clone());
                None
            }
            SessionEvent::Commands(c) => {
                self.commands = c.clone();
                None
            }
            // ponytail: plans are not rendered in phase 1
            SessionEvent::Plan(_) => None,
        }
    }

    fn patch_tool(&mut self, p: &ToolCallPatch) -> Option<usize> {
        let idx = self
            .items
            .iter()
            .rposition(|i| matches!(i, ChatItem::Tool(t) if t.id == p.id))?;
        let ChatItem::Tool(t) = &mut self.items[idx] else {
            return None;
        };
        if let Some(title) = &p.title {
            t.title = title.clone();
        }
        if let Some(status) = p.status {
            t.status = status;
        }
        if let Some(content) = &p.content {
            (t.text, t.diffs) = split_content(content);
        }
        Some(idx)
    }

    pub fn on_permission(&mut self, req: PermissionRequest) -> usize {
        self.state = TurnState::NeedsYou;
        self.push(ChatItem::Permission {
            request: req,
            answered: None,
        })
    }

    /// Whether this model holds the permission request (or the question) `id`.
    fn set_answer(&mut self, question: bool, id: u64, answer: &str, overwrite: bool) -> bool {
        let mut held = false;
        for item in &mut self.items {
            let answered = match item {
                ChatItem::Permission { request, answered } if !question && request.id == id => {
                    answered
                }
                ChatItem::Question { request, answered } if question && request.id == id => {
                    answered
                }
                _ => continue,
            };
            held = true;
            if overwrite || answered.is_none() {
                *answered = Some(answer.to_string());
            }
        }
        held
    }

    pub fn on_permission_answered(&mut self, id: u64, choice_name: &str) {
        self.set_answer(false, id, choice_name, true);
        self.state = TurnState::Working;
    }

    /// The broker dropped the request (e.g. the turn was cancelled).
    /// Resolutions are broadcast to every session: ids this model does not hold are ignored.
    pub fn on_permission_resolved(&mut self, id: u64) {
        if self.set_answer(false, id, "Cancelled", false) && self.state == TurnState::NeedsYou {
            self.state = TurnState::Working;
        }
    }

    pub fn on_question(&mut self, req: QuestionRequest) -> usize {
        self.state = TurnState::NeedsYou;
        self.push(ChatItem::Question {
            request: req,
            answered: None,
        })
    }

    pub fn on_question_answered(&mut self, id: u64, summary: &str) {
        self.set_answer(true, id, summary, true);
        self.state = TurnState::Working;
    }

    /// Cancelled with the turn or withdrawn by the agent; ids of other sessions are ignored.
    pub fn on_question_resolved(&mut self, id: u64) {
        if self.set_answer(true, id, "Cancelled", false) && self.state == TurnState::NeedsYou {
            self.state = TurnState::Working;
        }
    }

    pub fn on_turn_ended(&mut self, reason: &StopReason) {
        match reason {
            StopReason::EndTurn => self.state = TurnState::Idle,
            StopReason::Cancelled => {
                self.state = TurnState::Idle;
                self.notice("Cancelled".into(), NoticeKind::Info);
            }
            StopReason::MaxTokens => {
                self.state = TurnState::Idle;
                self.notice("Stopped: token limit reached".into(), NoticeKind::Error);
            }
            StopReason::Refusal => {
                self.state = TurnState::Idle;
                self.notice("The agent refused to continue".into(), NoticeKind::Error);
            }
            // handled by on_auth_required
            StopReason::Other(r) if r == "auth_required" => {}
            StopReason::Other(r) => {
                self.state = TurnState::Idle;
                self.notice(format!("Turn ended: {r}"), NoticeKind::Info);
            }
        }
    }

    pub fn on_auth_required(&mut self, agent: &str) {
        let text = signed_out(agent);
        self.state = TurnState::Error(text.clone());
        self.notice(text, NoticeKind::Error);
    }

    pub fn on_exited(&mut self, error: Option<&str>) {
        let msg = error.unwrap_or("The agent exited");
        self.state = TurnState::Error(msg.to_string());
        self.notice(format!("Agent exited: {msg}"), NoticeKind::Error);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wai_agent::{ChoiceKind, PermissionChoice, ToolCallView};

    fn tool(id: &str, status: ToolStatus) -> SessionEvent {
        SessionEvent::ToolCall(ToolCallView {
            id: id.into(),
            title: "t".into(),
            kind: "execute".into(),
            status,
            content: vec![],
        })
    }

    fn text(s: &str) -> SessionEvent {
        SessionEvent::AgentText(s.into())
    }

    #[test]
    fn agent_chunks_merge_into_one_message() {
        let mut m = ChatModel::default();
        m.apply(&text("He"));
        m.apply(&text("llo"));
        assert_eq!(
            m.items,
            vec![ChatItem::Agent {
                text: "Hello".into()
            }]
        );
    }

    #[test]
    fn tool_call_splits_agent_messages() {
        let mut m = ChatModel::default();
        m.apply(&text("a"));
        m.apply(&tool("t1", ToolStatus::Pending));
        m.apply(&text("b"));
        assert_eq!(m.items.len(), 3);
        assert!(matches!(&m.items[2], ChatItem::Agent { text } if text == "b"));
    }

    #[test]
    fn tool_update_patches_by_id() {
        let mut m = ChatModel::default();
        m.apply(&tool("t1", ToolStatus::Pending));
        let idx = m.apply(&SessionEvent::ToolCallUpdate(ToolCallPatch {
            id: "t1".into(),
            title: None,
            status: Some(ToolStatus::Completed),
            content: Some(vec![ToolContent::Text("out".into())]),
        }));
        assert_eq!(idx, Some(0));
        let ChatItem::Tool(t) = &m.items[0] else {
            panic!()
        };
        assert_eq!(t.status, ToolStatus::Completed);
        assert_eq!(t.text, "out");
        assert_eq!(t.title, "t");
    }

    #[test]
    fn tool_output_truncated_after_2000_lines() {
        let mut item = ToolItem {
            id: "t".into(),
            title: String::new(),
            kind: String::new(),
            status: ToolStatus::Completed,
            text: (0..5000)
                .map(|i| i.to_string())
                .collect::<Vec<_>>()
                .join("\n"),
            diffs: vec![],
            expanded: false,
        };
        let (v, hidden) = item.visible_text();
        assert_eq!(v.lines().count(), 2000);
        assert_eq!(hidden, Some(3000));
        item.expanded = true;
        let (v, hidden) = item.visible_text();
        assert_eq!(v.lines().count(), 5000);
        assert_eq!(hidden, None);
    }

    fn perm() -> PermissionRequest {
        PermissionRequest {
            id: 7,
            session: "s".into(),
            tool_call_id: "t1".into(),
            title: "Run".into(),
            diff: None,
            options: vec![PermissionChoice {
                id: "y".into(),
                name: "Yes".into(),
                kind: ChoiceKind::AllowOnce,
            }],
        }
    }

    #[test]
    fn permission_sets_needs_you_then_working() {
        let mut m = ChatModel::default();
        m.on_permission(perm());
        assert_eq!(m.state, TurnState::NeedsYou);
        m.on_permission_answered(7, "Yes");
        assert_eq!(m.state, TurnState::Working);
        assert!(
            matches!(&m.items[0], ChatItem::Permission { answered, .. } if answered.as_deref() == Some("Yes"))
        );
    }

    #[test]
    fn resolution_of_another_sessions_request_is_ignored() {
        let mut m = ChatModel::default();
        m.on_permission(perm());
        m.on_permission_resolved(8);
        assert_eq!(m.state, TurnState::NeedsYou);
        m.on_permission_resolved(7);
        assert_eq!(m.state, TurnState::Working);
        assert!(
            matches!(&m.items[0], ChatItem::Permission { answered, .. } if answered.as_deref() == Some("Cancelled"))
        );
    }

    #[test]
    fn question_sets_needs_you_until_resolved() {
        let mut m = ChatModel::default();
        let question = |id| QuestionRequest {
            id,
            session: "s".into(),
            message: "Pick".into(),
            fields: vec![],
        };
        m.on_question(question(7));
        assert_eq!(m.state, TurnState::NeedsYou);
        m.on_permission_resolved(7); // a permission with the same id is another request
        assert_eq!(m.state, TurnState::NeedsYou);
        m.on_question_resolved(7);
        assert_eq!(m.state, TurnState::Working);
        m.on_question(question(8));
        m.on_question_answered(8, "Answered: Blue");
        m.on_question_resolved(8); // the broker reports the answer too
        let answers: Vec<_> = m
            .items
            .iter()
            .map(|item| match item {
                ChatItem::Question { answered, .. } => answered.as_deref(),
                _ => None,
            })
            .collect();
        assert_eq!(answers, [Some("Cancelled"), Some("Answered: Blue")]);
        assert_eq!(m.state, TurnState::Working);
    }

    #[test]
    fn turn_end_cancelled_adds_notice() {
        let mut m = ChatModel::default();
        m.push_user("x");
        m.on_turn_ended(&StopReason::Cancelled);
        assert_eq!(m.state, TurnState::Idle);
        assert_eq!(
            m.items.last(),
            Some(&ChatItem::Notice {
                text: "Cancelled".into(),
                kind: NoticeKind::Info
            })
        );
    }

    #[test]
    fn auth_required_adds_notice() {
        let mut m = ChatModel::default();
        m.on_turn_ended(&StopReason::Other("auth_required".into()));
        m.on_auth_required("claude");
        let want = "claude is not signed in. Open a terminal and run its login (for Claude: claude /login).";
        assert!(
            matches!(&m.items[..], [ChatItem::Notice { text, kind: NoticeKind::Error }] if text == want)
        );
        assert_eq!(m.state, TurnState::Error(want.into()));
    }

    #[test]
    fn exit_with_error_sets_error_state() {
        let mut m = ChatModel::default();
        m.on_exited(Some("boom"));
        assert_eq!(m.state, TurnState::Error("boom".into()));
        assert!(
            matches!(m.items.last(), Some(ChatItem::Notice { text, .. }) if text.contains("boom"))
        );
    }

    #[test]
    fn usage_reads_rate_limit_status() {
        let mut m = ChatModel::default();
        m.apply(&SessionEvent::Usage {
            used: 1,
            size: 2,
            cost_usd: None,
            rate_limit: Some(serde_json::json!({"status":"allowed"})),
        });
        assert_eq!(
            m.usage.unwrap().rate_limit_status.as_deref(),
            Some("allowed")
        );
    }

    #[test]
    fn user_chunks_merge_into_one_message() {
        let mut m = ChatModel::default();
        m.apply(&SessionEvent::UserText("a".into()));
        m.apply(&SessionEvent::UserText("b".into()));
        assert_eq!(m.items, vec![ChatItem::User { text: "ab".into() }]);
    }

    #[test]
    fn replay_creates_user_items() {
        let mut m = ChatModel::default();
        m.apply(&SessionEvent::UserText("hi".into()));
        assert_eq!(m.items, vec![ChatItem::User { text: "hi".into() }]);
    }
}
