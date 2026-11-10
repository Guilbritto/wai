use std::io;
use std::path::{Path, PathBuf};

use wai_agent::{
    CommandInfo, FileDiff, Location, ModeInfo, PermissionRequest, PlanEntry, QuestionRequest,
    SessionEvent, StopReason, ToolCallPatch, ToolContent, ToolStatus,
};

pub const TOOL_OUTPUT_MAX_LINES: usize = 2000;
/// "Show all" stops here; the whole output opens in the editor instead.
pub const TOOL_OUTPUT_EXPANDED_LINES: usize = 20_000;

pub const UNSUPPORTED_QUESTION: &str =
    "The agent asked a question wai can't display yet; it was declined.";

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
    /// Ends a turn that edited files: one row per file, in the order first edited.
    Edits {
        turn: usize,
        files: Vec<EditedFile>,
    },
}

/// A file a turn edited: its steps (indices into `turn_edits`, oldest first) and how many of the
/// latest ones Revert undid.
#[derive(Debug, Clone, PartialEq)]
pub struct EditedFile {
    pub path: PathBuf,
    pub steps: Vec<usize>,
    pub reverted: usize,
    /// Why the last Revert did nothing.
    pub failed: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolItem {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: ToolStatus,
    pub text: String,
    pub diffs: Vec<FileDiff>,
    pub locations: Vec<Location>,
    /// R34: each file this call names (diff or location, as the agent spelled it) and whether
    /// it existed when wai first saw it. Only a file seen absent before the call ran was created
    /// by it.
    pub first_sight: Vec<(PathBuf, bool)>,
    pub expanded: bool,
}

impl ToolItem {
    /// Output truncated to `TOOL_OUTPUT_MAX_LINES` (`TOOL_OUTPUT_EXPANDED_LINES` once expanded),
    /// plus the hidden line count.
    pub fn visible_text(&self) -> (&str, Option<usize>) {
        let max = match self.expanded {
            true => TOOL_OUTPUT_EXPANDED_LINES,
            false => TOOL_OUTPUT_MAX_LINES,
        };
        match self.text.match_indices('\n').nth(max - 1) {
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

/// The "Show all" button's label for `hidden` lines past the first 2,000. R41: it never promises
/// more than expanding shows (up to 20,000 lines in all).
pub fn show_label(hidden: usize) -> String {
    let shown = TOOL_OUTPUT_EXPANDED_LINES - TOOL_OUTPUT_MAX_LINES;
    if hidden <= shown {
        format!("Show all ({hidden} more lines)")
    } else {
        format!(
            "Show more ({} of {} more lines)",
            grouped(shown),
            grouped(hidden)
        )
    }
}

/// `1234567` as `1,234,567`.
fn grouped(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, digit) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
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
    /// The agent's latest plan: a new one replaces it whole.
    pub plan: Vec<PlanEntry>,
    /// The last turn of this run ended with `EndTurn` (the R13 "done"); a new prompt clears it.
    pub done: bool,
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
            plan: Vec::new(),
            done: false,
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
        self.done = false;
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
                    locations: v.locations.clone(),
                    first_sight: Vec::new(),
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
            SessionEvent::ConfigChanged(_) => None,
            SessionEvent::Commands(c) => {
                self.commands = c.clone();
                None
            }
            SessionEvent::Plan(entries) => {
                self.plan = entries.clone();
                None
            }
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
        if let Some(locations) = &p.locations {
            t.locations = locations.clone();
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

    /// `NeedsYou` while any permission or question card is unanswered (parallel tool calls can
    /// open several), else `Working`.
    fn after_answer(&self) -> TurnState {
        let open = self.items.iter().any(|item| {
            matches!(
                item,
                ChatItem::Permission { answered: None, .. }
                    | ChatItem::Question { answered: None, .. }
            )
        });
        if open {
            TurnState::NeedsYou
        } else {
            TurnState::Working
        }
    }

    pub fn on_permission_answered(&mut self, id: u64, choice_name: &str) {
        self.set_answer(false, id, choice_name, true);
        self.state = self.after_answer();
    }

    /// The broker dropped the request (e.g. the turn was cancelled).
    /// Resolutions are broadcast to every session: ids this model does not hold are ignored.
    pub fn on_permission_resolved(&mut self, id: u64) {
        if self.set_answer(false, id, "Cancelled", false) && self.state == TurnState::NeedsYou {
            self.state = self.after_answer();
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
        self.state = self.after_answer();
    }

    /// Cancelled with the turn or withdrawn by the agent; ids of other sessions are ignored.
    pub fn on_question_resolved(&mut self, id: u64) {
        if self.set_answer(true, id, "Cancelled", false) && self.state == TurnState::NeedsYou {
            self.state = self.after_answer();
        }
    }

    /// The diffs of turn `turn` (0-based, one per user message), in order. Only completed tool
    /// calls count: a rejected or cancelled edit never reached the disk.
    pub fn turn_edits(&self, turn: usize) -> Vec<&FileDiff> {
        let steps = self.turn_steps(turn).into_iter();
        steps.map(|(tool, k)| &tool.diffs[k]).collect()
    }

    /// `turn_edits` as (tool call, index into its diffs): the call also knows the diff's location
    /// and whether its file existed before.
    pub fn turn_steps(&self, turn: usize) -> Vec<(&ToolItem, usize)> {
        let mut users = 0;
        let mut steps = Vec::new();
        for item in &self.items {
            match item {
                ChatItem::User { .. } if users > turn => break,
                ChatItem::User { .. } => users += 1,
                ChatItem::Tool(t) if users == turn + 1 && t.status == ToolStatus::Completed => {
                    steps.extend((0..t.diffs.len()).map(|k| (t, k)))
                }
                _ => {}
            }
        }
        steps
    }

    /// R34: stats each file tool row `ix` names for the first time (`cwd` resolves relative
    /// paths). Before the call runs (Pending, InProgress) a missing file means the call will
    /// create it; once it ran, wai can no longer tell, and the file counts as existing. So does a
    /// file wai cannot stat.
    pub fn note_files(&mut self, ix: usize, cwd: &Path) {
        let Some(ChatItem::Tool(t)) = self.items.get_mut(ix) else {
            return;
        };
        let ran = matches!(t.status, ToolStatus::Completed | ToolStatus::Failed);
        let paths = (t.diffs.iter().map(|d| &d.path)).chain(t.locations.iter().map(|l| &l.path));
        let paths: Vec<PathBuf> = paths.cloned().collect();
        for path in paths {
            if t.first_sight.iter().any(|(seen, _)| *seen == path) {
                continue;
            }
            let missing = matches!(
                std::fs::symlink_metadata(cwd.join(&path)),
                Err(e) if e.kind() == io::ErrorKind::NotFound
            );
            t.first_sight.push((path, ran || !missing));
        }
    }

    /// The Edits block of the turn that just ended, if it edited files.
    fn push_edits(&mut self) {
        let users = self
            .items
            .iter()
            .filter(|i| matches!(i, ChatItem::User { .. }));
        let Some(turn) = users.count().checked_sub(1) else {
            return;
        };
        let mut files: Vec<EditedFile> = Vec::new();
        // ponytail: one file is one path as the agent spelled it; two spellings of a file
        // (`/var` and `/private/var`) make two rows.
        for (step, diff) in self.turn_edits(turn).into_iter().enumerate() {
            match files.iter_mut().find(|f| f.path == diff.path) {
                Some(file) => file.steps.push(step),
                None => files.push(EditedFile {
                    path: diff.path.clone(),
                    steps: vec![step],
                    reverted: 0,
                    failed: None,
                }),
            }
        }
        if !files.is_empty() {
            self.push(ChatItem::Edits { turn, files });
        }
    }

    /// The title of the latest tool call still running; none when idle.
    pub fn current_tool(&self) -> Option<&str> {
        if !matches!(self.state, TurnState::Working | TurnState::NeedsYou) {
            return None;
        }
        self.items.iter().rev().find_map(|item| match item {
            ChatItem::Tool(t)
                if matches!(t.status, ToolStatus::Pending | ToolStatus::InProgress) =>
            {
                Some(t.title.as_str())
            }
            _ => None,
        })
    }

    pub fn on_turn_ended(&mut self, reason: &StopReason) {
        self.push_edits();
        self.done = matches!(reason, StopReason::EndTurn);
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
            locations: vec![],
        })
    }

    fn text(s: &str) -> SessionEvent {
        SessionEvent::AgentText(s.into())
    }

    #[test]
    fn plan_is_stored_and_replaced() {
        let entry = |c: &str, s: &str| PlanEntry {
            content: c.into(),
            status: s.into(),
        };
        let mut m = ChatModel::default();
        assert!(
            m.apply(&SessionEvent::Plan(vec![entry("a", "pending")]))
                .is_none()
        );
        m.apply(&SessionEvent::Plan(vec![
            entry("b", "completed"),
            entry("c", "in_progress"),
        ]));
        assert_eq!(
            m.plan,
            vec![entry("b", "completed"), entry("c", "in_progress")]
        );
        assert!(m.items.is_empty());
    }

    #[test]
    fn done_means_the_last_turn_ended_with_end_turn() {
        let mut m = ChatModel::default();
        assert!(!m.done);
        m.push_user("hi");
        m.on_turn_ended(&StopReason::EndTurn);
        assert!(m.done);
        m.push_user("again");
        assert!(!m.done);
        m.on_turn_ended(&StopReason::Cancelled);
        assert!(!m.done);
    }

    #[test]
    fn current_tool_is_the_latest_running_one() {
        let mut m = ChatModel {
            state: TurnState::Working,
            ..Default::default()
        };
        m.apply(&tool("1", ToolStatus::Completed));
        assert_eq!(m.current_tool(), None);
        m.apply(&tool("2", ToolStatus::InProgress));
        assert_eq!(m.current_tool(), Some("t"));
        m.state = TurnState::NeedsYou;
        assert_eq!(m.current_tool(), Some("t"));
        m.state = TurnState::Idle;
        assert_eq!(m.current_tool(), None);
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
            locations: None,
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
            locations: vec![],
            first_sight: vec![],
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

    /// "Show all" stops at 20,000 lines: the rest opens in the editor.
    #[test]
    fn expanded_output_stops_at_20000_lines() {
        let item = ToolItem {
            id: "t".into(),
            title: String::new(),
            kind: String::new(),
            status: ToolStatus::Completed,
            text: "x\n".repeat(25_000),
            diffs: vec![],
            locations: vec![],
            first_sight: vec![],
            expanded: true,
        };
        let (v, hidden) = item.visible_text();
        assert_eq!(v.lines().count(), 20_000);
        assert_eq!(hidden, Some(5_000));
    }

    /// R41: the button never promises more than it shows: past 18,000 hidden lines it says
    /// how many of them it adds.
    #[test]
    fn show_label_promises_what_it_shows() {
        assert_eq!(show_label(3_000), "Show all (3000 more lines)");
        assert_eq!(show_label(18_000), "Show all (18000 more lines)");
        assert_eq!(
            show_label(48_000),
            "Show more (18,000 of 48,000 more lines)"
        );
        assert_eq!(
            show_label(1_234_567),
            "Show more (18,000 of 1,234,567 more lines)"
        );
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

    /// R24: parallel tool calls can open several cards; the session needs the user until the
    /// last one is answered.
    #[test]
    fn needs_you_until_every_card_is_answered() {
        let mut m = ChatModel::default();
        m.on_permission(perm()); // id 7
        m.on_question(QuestionRequest {
            id: 8,
            session: "s".into(),
            message: "Pick".into(),
            fields: vec![],
        });
        m.on_question_answered(8, "Answered: Blue");
        assert_eq!(m.state, TurnState::NeedsYou);
        m.on_question_resolved(8); // the broker reports the answer too
        assert_eq!(m.state, TurnState::NeedsYou);
        m.on_permission_answered(7, "Yes");
        assert_eq!(m.state, TurnState::Working);

        m.on_permission(PermissionRequest { id: 9, ..perm() });
        m.on_question(QuestionRequest {
            id: 10,
            session: "s".into(),
            message: "Pick".into(),
            fields: vec![],
        });
        m.on_permission_resolved(9);
        assert_eq!(m.state, TurnState::NeedsYou);
        m.on_question_resolved(10);
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

    fn edit(
        id: &str,
        path: &str,
        old: Option<&str>,
        new: &str,
        status: ToolStatus,
    ) -> SessionEvent {
        SessionEvent::ToolCall(ToolCallView {
            id: id.into(),
            title: format!("Edit {path}"),
            kind: "edit".into(),
            status,
            content: vec![ToolContent::Diff(FileDiff {
                path: path.into(),
                old_text: old.map(str::to_string),
                new_text: new.into(),
            })],
            locations: vec![],
        })
    }

    #[test]
    fn turn_end_lists_the_turns_edits_by_file() {
        let mut m = ChatModel::default();
        m.push_user("first");
        m.apply(&edit("t0", "/p/x", Some("0"), "1", ToolStatus::Completed));
        m.on_turn_ended(&StopReason::EndTurn);
        m.push_user("second");
        m.apply(&edit("t1", "/p/a", Some("a0"), "a1", ToolStatus::Completed));
        m.apply(&edit("t2", "/p/b", None, "b", ToolStatus::Completed));
        m.apply(&edit("t3", "/p/c", None, "c", ToolStatus::Failed)); // rejected
        m.apply(&edit("t4", "/p/a", Some("a1"), "a2", ToolStatus::Completed));
        m.on_turn_ended(&StopReason::Cancelled);

        let paths = |turn| -> Vec<String> {
            let edits = m.turn_edits(turn);
            edits.iter().map(|d| d.path.display().to_string()).collect()
        };
        assert_eq!(paths(0), ["/p/x"]);
        assert_eq!(paths(1), ["/p/a", "/p/b", "/p/a"]);
        let file = |path: &str, steps: Vec<usize>| EditedFile {
            path: path.into(),
            steps,
            reverted: 0,
            failed: None,
        };
        // The block comes before the turn's closing notice.
        let n = m.items.len();
        assert_eq!(
            m.items[n - 2],
            ChatItem::Edits {
                turn: 1,
                files: vec![file("/p/a", vec![0, 2]), file("/p/b", vec![1])],
            }
        );
        assert!(matches!(&m.items[n - 1], ChatItem::Notice { text, .. } if text == "Cancelled"));

        // A turn without edits ends without a block.
        m.push_user("third");
        m.on_turn_ended(&StopReason::EndTurn);
        assert!(matches!(m.items.last(), Some(ChatItem::User { .. })));
    }

    /// R34: a file counts as created by a call only if wai saw it missing before the call ran.
    #[test]
    fn note_files_records_absence_before_the_call_runs() {
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("old.txt");
        std::fs::write(&existing, "old").unwrap();
        let mut m = ChatModel::default();
        let call = |id: &str, status, diffs: Vec<FileDiff>, locations| {
            SessionEvent::ToolCall(ToolCallView {
                id: id.into(),
                title: "Edit".into(),
                kind: "edit".into(),
                status,
                content: diffs.into_iter().map(ToolContent::Diff).collect(),
                locations,
            })
        };
        let diff = |path: &Path| FileDiff {
            path: path.into(),
            old_text: None,
            new_text: "new".into(),
        };
        let at = |path: &str| Location {
            path: path.into(),
            line: None,
        };

        // Relative paths resolve against the session's folder.
        let pending = vec![diff("new.txt".as_ref()), diff(&existing)];
        m.apply(&call(
            "t1",
            ToolStatus::Pending,
            pending,
            vec![at("loc.txt")],
        ));
        m.note_files(0, dir.path());
        std::fs::write(dir.path().join("new.txt"), "new").unwrap();
        m.apply(&SessionEvent::ToolCallUpdate(ToolCallPatch {
            id: "t1".into(),
            title: None,
            status: Some(ToolStatus::Completed),
            content: None,
            locations: Some(vec![at("new.txt")]),
        }));
        m.note_files(0, dir.path());
        // First seen once it ran: wai can't tell, so it existed.
        m.apply(&call(
            "t2",
            ToolStatus::Completed,
            vec![diff("x.txt".as_ref())],
            vec![],
        ));
        m.note_files(1, dir.path());

        let seen = |ix: usize| match &m.items[ix] {
            ChatItem::Tool(t) => t.first_sight.clone(),
            _ => panic!(),
        };
        let first_sight = vec![
            (PathBuf::from("new.txt"), false),
            (existing.clone(), true),
            (PathBuf::from("loc.txt"), false),
        ];
        assert_eq!(seen(0), first_sight);
        assert_eq!(seen(1), [(PathBuf::from("x.txt"), true)]);
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
