use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::component::select::{SelectEvent, SelectItem, SelectState};
use gpui_kit::component::text::TextViewState;
use gpui_kit::*;
use wai_agent::{
    AgentError, AgentHandle, ModeInfo, PermissionChoice, PermissionRequest, SessionEvent,
    SessionStarted, StopReason,
};

use super::input::Attachment;
use super::model::{ChatItem, ChatModel, NoticeKind, TurnState};
use super::permission::DiffView;
use crate::agents::AgentRegistry;

/// How often a streaming message takes its new text. Each push costs a parse and a window
/// redraw (~4 ms); pushing every token cost 12–18% of a core at 40 tokens/s.
// ponytail: a fixed rate; lower it for smoother text at the cost of CPU.
const STREAM_FLUSH: Duration = Duration::from_millis(100);

pub enum ChatEvent {
    StateChanged,
    TitleChanged,
}

/// A message's markdown and how many bytes of it the view already handed over.
pub(super) struct Markdown {
    pub(super) state: Entity<TextViewState>,
    pushed: usize,
}

#[derive(Clone)]
pub(super) struct Mode(ModeInfo);

impl SelectItem for Mode {
    type Value = String;

    fn title(&self) -> SharedString {
        self.0.name.clone().into()
    }

    fn value(&self) -> &String {
        &self.0.id
    }
}

/// One conversation with an agent: the message list, the header and the prompt box.
pub struct ChatSession {
    pub agent: String,
    pub id: String,
    pub cwd: PathBuf,
    pub model: ChatModel,
    pub(super) handle: AgentHandle,
    pub(super) input: Entity<TextareaState>,
    pub(super) attachments: Vec<Attachment>,
    registry: WeakEntity<AgentRegistry>,
    /// Per `Agent`/`Thought` row.
    pub(super) texts: HashMap<usize, Markdown>,
    /// Per `Tool`/`Permission` row.
    pub(super) diffs: HashMap<usize, Vec<DiffView>>,
    pub(super) open_thoughts: HashSet<usize>,
    pub(super) scroller: Entity<MessageScrollerState>,
    pub(super) modes: Option<Entity<SelectState<Vec<Mode>>>>,
    /// Pending while streamed text waits for `STREAM_FLUSH`.
    flush: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<ChatEvent> for ChatSession {}

impl ChatSession {
    pub fn new(
        agent: String,
        started: SessionStarted,
        cwd: PathBuf,
        handle: AgentHandle,
        registry: WeakEntity<AgentRegistry>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx)
                .auto_grow(1, 8)
                .placeholder(format!("Message {agent}…"));
            input.set_submit_on_enter(true, cx);
            input
        });
        input.update(cx, |input, cx| input.focus(window, cx));
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    // Shift+Enter inserted a newline already.
                    if let InputEvent::PressEnter { shift: false, .. } = event {
                        this.send(window, cx);
                    }
                }),
            ];
        let mut this = Self {
            model: ChatModel::default(),
            agent,
            id: started.id,
            cwd,
            handle,
            input,
            attachments: Vec::new(),
            registry,
            texts: HashMap::new(),
            diffs: HashMap::new(),
            open_thoughts: HashSet::new(),
            scroller: cx.new(|cx| MessageScrollerState::new(0, cx)),
            modes: None,
            flush: None,
            _subscriptions: subscriptions,
        };
        this.set_modes(started.modes, started.current_mode, window, cx);
        this
    }

    /// The agent's modes and the header's mode picker, when it has modes.
    fn set_modes(
        &mut self,
        modes: Vec<ModeInfo>,
        current: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !modes.is_empty() {
            let items: Vec<Mode> = modes.iter().cloned().map(Mode).collect();
            let selected = items
                .iter()
                .position(|m| Some(&m.0.id) == current.as_ref())
                .map(IndexPath::new);
            let select = cx.new(|cx| SelectState::new(items, selected, window, cx));
            self._subscriptions.push(cx.subscribe(
                &select,
                |this, _, event: &SelectEvent<Vec<Mode>>, cx| {
                    let SelectEvent::Confirm(Some(mode)) = event else {
                        return;
                    };
                    this.set_mode(mode.clone(), cx);
                },
            ));
            self.modes = Some(select);
        }
        self.model.modes = modes;
        self.model.current_mode = current;
    }

    pub fn status(&self) -> &TurnState {
        &self.model.state
    }

    /// Puts the caret in the prompt box.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Cancels a running turn: a closed session must not keep working out of sight.
    pub fn stop(&self) {
        if matches!(self.model.state, TurnState::Working | TurnState::NeedsYou) {
            self.handle.cancel(self.id.clone());
        }
    }

    pub fn handle_event(&mut self, ev: &SessionEvent, cx: &mut Context<Self>) {
        let rows = self.model.items.len();
        let Some(ix) = self.update_model(cx, |m| m.apply(ev)) else {
            return cx.notify(); // header: title, usage, mode
        };
        // A growing message takes its text on the next flush, which also repaints it.
        if ix < rows
            && matches!(
                ev,
                SessionEvent::AgentText(_) | SessionEvent::AgentThought(_)
            )
        {
            return self.schedule_flush(cx);
        }
        self.sync_row(ix, cx);
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }

    pub fn handle_permission(&mut self, req: PermissionRequest, cx: &mut Context<Self>) {
        let ix = self.update_model(cx, |m| m.on_permission(req));
        self.sync_row(ix, cx);
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }

    /// Resolutions are broadcast to every session; the model ignores ids it does not hold.
    pub fn handle_permission_resolved(&mut self, id: u64, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.on_permission_resolved(id));
        cx.notify();
    }

    pub fn handle_turn_ended(&mut self, reason: &StopReason, cx: &mut Context<Self>) {
        let agent = self.agent.clone();
        self.update_model(cx, |m| {
            m.on_turn_ended(reason);
            if matches!(reason, StopReason::Other(r) if r == "auth_required") {
                m.on_auth_required(&agent);
            }
        });
        self.sync_rows(None, cx);
        cx.notify();
    }

    /// The agent process died: every session on it shows the error and a Restart button.
    pub fn handle_agent_exited(&mut self, error: &str, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.on_exited(Some(error)));
        self.sync_rows(None, cx);
        cx.notify();
    }

    /// Runs `f` on the model and reports state and title changes.
    pub(super) fn update_model<R>(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut ChatModel) -> R,
    ) -> R {
        let (state, title) = (self.model.state.clone(), self.model.title.clone());
        let result = f(&mut self.model);
        if self.model.state != state {
            cx.emit(ChatEvent::StateChanged);
        }
        if self.model.title != title {
            cx.emit(ChatEvent::TitleChanged);
        }
        result
    }

    /// Appends new rows to the list and remeasures `changed`.
    pub(super) fn sync_rows(&mut self, changed: Option<usize>, cx: &mut Context<Self>) {
        let rows = self.model.items.len();
        self.scroller.update(cx, |list, cx| {
            let known = list.item_count();
            if rows > known {
                list.append(rows - known, cx);
            }
            if let Some(ix) = changed.filter(|ix| *ix < known) {
                list.remeasure_items(ix..ix + 1, cx);
            }
        });
    }

    /// Hands streamed text to the markdown views once per `STREAM_FLUSH`: one parse and one
    /// redraw for all the tokens that arrived meanwhile. No timer runs while nothing streams.
    fn schedule_flush(&mut self, cx: &mut Context<Self>) {
        if self.flush.is_some() {
            return;
        }
        self.flush = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(STREAM_FLUSH).await;
            this.update(cx, |this, cx| {
                this.flush = None;
                let rows: Vec<usize> = this.texts.keys().copied().collect();
                for ix in rows {
                    this.sync_row(ix, cx);
                }
            })
            .ok();
        }));
    }

    /// Brings a row's derived state up to date: the markdown delta, or the diffs.
    fn sync_row(&mut self, ix: usize, cx: &mut Context<Self>) {
        match &self.model.items[ix] {
            ChatItem::Agent { text } | ChatItem::Thought { text } => {
                match self.texts.get_mut(&ix) {
                    Some(md) => {
                        let delta = &text[md.pushed..];
                        md.state.update(cx, |state, cx| state.push_str(delta, cx));
                        md.pushed = text.len();
                    }
                    None => {
                        let state = cx.new(|cx| TextViewState::markdown(text, cx).selectable(true));
                        let pushed = text.len();
                        self.texts.insert(ix, Markdown { state, pushed });
                    }
                }
            }
            ChatItem::Tool(tool) => {
                let diffs = tool.diffs.iter().map(DiffView::new).collect();
                self.diffs.insert(ix, diffs);
            }
            // R5: the request's own diff, else the diffs of its tool call.
            ChatItem::Permission { request, .. } => {
                let diffs = match &request.diff {
                    Some(diff) => vec![DiffView::new(diff)],
                    None => self
                        .model
                        .items
                        .iter()
                        .rev()
                        .find_map(|item| match item {
                            ChatItem::Tool(t) if t.id == request.tool_call_id => Some(&t.diffs),
                            _ => None,
                        })
                        .map(|diffs| diffs.iter().map(DiffView::new).collect())
                        .unwrap_or_default(),
                };
                self.diffs.insert(ix, diffs);
            }
            ChatItem::User { .. } | ChatItem::Notice { .. } => {}
        }
    }

    pub(super) fn answer(
        &mut self,
        request: u64,
        choice: &PermissionChoice,
        cx: &mut Context<Self>,
    ) {
        let sent = self
            .handle
            .answer_permission(request, Some(choice.id.clone()));
        self.update_model(cx, |m| match sent {
            Ok(()) => m.on_permission_answered(request, &choice.name),
            // Already resolved on the agent's side (cancelled, say).
            Err(_) => m.on_permission_resolved(request),
        });
        cx.notify();
    }

    fn set_mode(&mut self, mode: String, cx: &mut Context<Self>) {
        let previous = self.model.current_mode.replace(mode.clone());
        let (handle, id) = (self.handle.clone(), self.id.clone());
        cx.spawn(async move |this, cx| {
            if let Err(e) = handle.set_mode(id, mode).await {
                this.update(cx, |this, cx| {
                    this.model.current_mode = previous;
                    this.model
                        .notice(format!("Could not switch mode: {e}"), NoticeKind::Error);
                    this.sync_rows(None, cx);
                    cx.notify();
                })
                .ok();
            }
        })
        .detach();
        cx.notify();
    }

    /// Respawns the agent through the registry if it died, then loads this session into an
    /// empty list: Restart after a crash, and a history row. The replay arrives as events while
    /// `load_session` is pending, so the session must already be routed.
    pub fn load(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self.agent.clone();
        let Ok(Some(handle)) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            return;
        };
        self.handle = handle.clone();
        self.model.items.clear();
        self.texts.clear();
        self.diffs.clear();
        self.open_thoughts.clear();
        self.scroller.update(cx, |list, cx| list.reset(0, cx));
        self.update_model(cx, |m| m.state = TurnState::Working);
        let (id, cwd, registry) = (self.id.clone(), self.cwd.clone(), self.registry.clone());
        cx.spawn_in(window, async move |this, cx| {
            let loaded = handle.load_session(id, cwd).await;
            this.update_in(cx, |this, window, cx| {
                let agent = this.agent.clone();
                match loaded {
                    Ok(started) => {
                        registry.update(cx, |r, cx| r.mark_ready(&agent, cx)).ok();
                        // A history row learns its modes here.
                        if this.modes.is_none() {
                            this.set_modes(started.modes, started.current_mode, window, cx);
                        }
                        this.update_model(cx, |m| m.state = TurnState::Idle);
                    }
                    Err(AgentError::AuthRequired) => {
                        this.update_model(cx, |m| m.on_auth_required(&agent));
                    }
                    // A death was already reported by the registry.
                    Err(_) if matches!(this.model.state, TurnState::Error(_)) => {}
                    Err(e) => this.update_model(cx, |m| {
                        let error = format!("Could not load the session: {e}");
                        m.state = TurnState::Error(error.clone());
                        m.notice(error, NoticeKind::Error);
                    }),
                }
                this.sync_rows(None, cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    use super::ChatSession;
    use crate::chat::model::{ChatItem, NoticeKind, TurnState};
    use crate::workspace::Workspace;
    use crate::workspace::tests::{
        fake_settings, history_settings, load_history, open, wait_until,
    };

    /// ⌘N, then the session that lands in pane 0.
    fn start_session(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ChatSession> {
        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "a session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        workspace.read_with(cx, |w, _| w.panes[0].session().cloned().unwrap())
    }

    fn send(prompt: &str, cx: &mut VisualTestContext) {
        cx.simulate_input(prompt);
        cx.simulate_keystrokes("enter");
    }

    fn last_agent_text(
        session: &Entity<ChatSession>,
        cx: &mut VisualTestContext,
    ) -> Option<String> {
        session.read_with(cx, |s, _| {
            s.model.items.iter().rev().find_map(|item| match item {
                ChatItem::Agent { text } => Some(text.clone()),
                _ => None,
            })
        })
    }

    fn state(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> TurnState {
        session.read_with(cx, |s, _| s.model.state.clone())
    }

    #[gpui_kit::test]
    fn sending_prompt_streams_reply(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);

        send("hi", cx);

        wait_until(cx, "the reply", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle
                    && matches!(s.model.items.last(), Some(ChatItem::Agent { text }) if text == "Hello from fake agent")
            })
        });
        let first = session.read_with(cx, |s, _| s.model.items[0].clone());
        assert_eq!(first, ChatItem::User { text: "hi".into() });
        // Each chunk reached the markdown once: the deltas add up to the message.
        wait_until(cx, "the markdown", |cx| {
            session.read_with(cx, |s, cx| {
                let md = &s.texts[&1].state;
                md.read(cx).rendered_text().as_str().trim() == "Hello from fake agent"
            })
        });
    }

    #[gpui_kit::test]
    fn permission_card_buttons_answer(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-permission"));
        let session = start_session(&workspace, cx);

        send("edit", cx);
        wait_until(cx, "the permission card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
        });
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("perm-allow_once").is_some(), "no card");
            window.click("perm-allow_once", cx);
        });

        wait_until(cx, "the answer", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        let reply = last_agent_text(&session, cx).unwrap_or_default();
        assert!(reply.starts_with("chose:"), "{reply}");
        let answered = session.read_with(cx, |s, _| {
            s.model.items.iter().find_map(|item| match item {
                ChatItem::Permission { answered, .. } => answered.clone(),
                _ => None,
            })
        });
        assert_eq!(answered.as_deref(), Some("Yes"));
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("perm-allow_once").is_none(), "buttons stay");
        });
    }

    #[gpui_kit::test]
    fn escape_cancels_turn(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let words: Vec<String> = (0..200).map(|i| format!("w{i}")).collect();
        let reply = words.join(" ");
        let flags = format!("--reply '{reply}' --chunk-delay-ms 20");
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        cx.simulate_keystrokes("escape");

        let cancelled = ChatItem::Notice {
            text: "Cancelled".into(),
            kind: NoticeKind::Info,
        };
        wait_until(cx, "Cancelled", |cx| {
            session.read_with(cx, |s, _| s.model.items.last() == Some(&cancelled))
        });
        assert_eq!(state(&session, cx), TurnState::Idle);
        assert!(last_agent_text(&session, cx).unwrap().len() < reply.len());
    }

    /// The agent replays a loaded session before `load_session` returns, so the session must
    /// be routed by then.
    #[gpui_kit::test]
    fn history_row_resumes_with_replay(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(3));
        let rows = load_history(&workspace, 3, cx);

        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));
        let session = workspace
            .read_with(cx, |w, _| w.panes[0].session().cloned())
            .expect("the focused pane shows it");

        wait_until(cx, "the replay", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle && s.model.items.len() == 2
            })
        });
        session.read_with(cx, |s, _| {
            assert_eq!(s.id, "session-0");
            assert_eq!(s.model.title.as_deref(), Some("Session 0"));
            assert_eq!(
                s.model.items,
                [
                    ChatItem::User {
                        text: "previous prompt".into()
                    },
                    ChatItem::Agent {
                        text: "previous answer".into()
                    },
                ]
            );
        });

        // Clicked again, it shows the open session instead of loading a second copy.
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), 1);
    }

    #[gpui_kit::test]
    fn restart_reloads_session_after_crash(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--crash-after 1"));
        let session = start_session(&workspace, cx);

        send("hi", cx);
        wait_until(cx, "the error", |cx| {
            matches!(state(&session, cx), TurnState::Error(_))
        });
        cx.update(|window, cx| window.click("restart", cx));

        // A new process replays the conversation into the emptied list.
        wait_until(cx, "the replay", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        let items = session.read_with(cx, |s, _| s.model.items.clone());
        assert_eq!(
            items,
            [
                ChatItem::User {
                    text: "previous prompt".into()
                },
                ChatItem::Agent {
                    text: "previous answer".into()
                },
            ]
        );
    }
}
