use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::component::IndexPath;
use gpui_kit::component::input::{InputEvent, TextareaState};
use gpui_kit::component::message_scroller::MessageScrollerState;
use gpui_kit::component::select::{SelectEvent, SelectItem, SelectState};
use gpui_kit::component::text::TextViewState;
use gpui_kit::*;
use wai_agent::{
    AgentError, AgentHandle, ConfigCategory, ConfigValue, FileDiff, Location, ModeInfo,
    PermissionChoice, PermissionRequest, QuestionRequest, SessionEvent, SessionStarted, StopReason,
};
use wai_settings::Processing;

use super::edits::open_line;
use super::input::{Attachment, Message};
use super::model::{ChatItem, ChatModel, NoticeKind, TurnState, UNSUPPORTED_QUESTION};
use super::permission::DiffView;
use super::question::QuestionForm;
use crate::agents::AgentRegistry;

/// How often a streaming message takes its new text. Each push costs a parse and a window
/// redraw (~4 ms); pushing every token cost 12–18% of a core at 40 tokens/s.
// ponytail: a fixed rate; lower it for smoother text at the cost of CPU.
const STREAM_FLUSH: Duration = Duration::from_millis(100);

#[derive(Debug)]
pub enum ChatEvent {
    StateChanged,
    TitleChanged,
    /// A prompt's turn ended, with why. Loading a session replays it without one.
    TurnEnded(StopReason),
    /// The agent no longer has the session restored at launch.
    RestoreFailed,
    /// Open in the editor at a 1-based line.
    OpenFile {
        path: PathBuf,
        line: u32,
    },
    /// Revert rewrote (or deleted) this file: open tabs of it catch up.
    Reverted(PathBuf),
    /// R21: a tool call's whole output, to open read-only in the editor.
    OpenOutput {
        tool: String,
        text: String,
    },
    /// R87: the header's ×, to take the pane showing this session off the screen.
    Hide,
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

#[derive(Clone)]
pub(super) struct Choice(ConfigValue);

impl SelectItem for Choice {
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
    /// Sent during a turn to an agent that cannot steer: they go out when it ends.
    pub(super) queued: Vec<Message>,
    /// Queued text going back to the prompt box at the next frame (which has the window).
    pub(super) unsent: Option<String>,
    /// The `/` menu's selected row, its scroll, and whether Esc closed it.
    pub(super) menu_ix: usize,
    pub(super) menu_scroll: ScrollHandle,
    pub(super) menu_closed: bool,
    pub(super) registry: WeakEntity<AgentRegistry>,
    /// Per `Agent`/`Thought` row.
    pub(super) texts: HashMap<usize, Markdown>,
    /// Per `Tool`/`Permission` row.
    pub(super) diffs: HashMap<usize, Vec<DiffView>>,
    pub(super) open_thoughts: HashSet<usize>,
    /// How the steps show (`conversation.processing`).
    pub(super) processing: Processing,
    /// The `Tool` rows unfolded to their output.
    pub(super) open_steps: HashSet<usize>,
    /// The unfolded runs of steps of the grouped mode, by their first row.
    pub(super) open_groups: HashSet<usize>,
    /// While a turn runs: when it started, and the status line's one-second tick.
    pub(super) turn: Option<(Instant, Task<()>)>,
    /// Per open `Question` row, made when it first draws.
    pub(super) questions: HashMap<usize, QuestionForm>,
    pub(super) scroller: Entity<MessageScrollerState>,
    pub(super) modes: Option<Entity<SelectState<Vec<Mode>>>>,
    /// The header's pickers of the model, effort and mode options, in that order.
    pub(super) pickers: Vec<(String, Entity<SelectState<Vec<Choice>>>)>,
    pickers_stale: bool,
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
        // The workspace focuses it (`focus`) when its pane is the focused one.
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |this, _, event: &InputEvent, window, cx| {
                    match event {
                        // Shift+Enter inserted a newline already.
                        InputEvent::PressEnter { shift: false, .. } => this.send(window, cx),
                        // The `/` menu opens again, at its top.
                        InputEvent::Change => {
                            (this.menu_ix, this.menu_closed) = (0, false);
                            cx.notify();
                        }
                        _ => {}
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
            queued: Vec::new(),
            unsent: None,
            menu_ix: 0,
            menu_scroll: ScrollHandle::new(),
            menu_closed: false,
            registry,
            texts: HashMap::new(),
            diffs: HashMap::new(),
            open_thoughts: HashSet::new(),
            processing: Processing::default(),
            open_steps: HashSet::new(),
            open_groups: HashSet::new(),
            turn: None,
            questions: HashMap::new(),
            scroller: cx.new(|cx| MessageScrollerState::new(0, cx)),
            modes: None,
            pickers: Vec::new(),
            pickers_stale: false,
            flush: None,
            _subscriptions: subscriptions,
        };
        this.set_modes(started.modes, started.current_mode, window, cx);
        this.model.config = started.config;
        this.sync_pickers(window, cx);
        this
    }

    /// One picker per model, effort and mode option; reused while the option stays.
    pub(super) fn sync_pickers(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let stale = std::mem::take(&mut self.pickers_stale);
        let mut options: Vec<_> = (self.model.config.iter())
            .filter(|o| o.category != ConfigCategory::Other)
            .cloned()
            .collect();
        options.sort_by_key(|o| match o.category {
            ConfigCategory::Model => 0,
            ConfigCategory::ThoughtLevel => 1,
            _ => 2,
        });
        let mut old = std::mem::take(&mut self.pickers);
        for option in options {
            let items: Vec<Choice> = option.values.iter().cloned().map(Choice).collect();
            let select = match old.iter().position(|(id, _)| *id == option.id) {
                Some(ix) => {
                    let select = old.remove(ix).1;
                    if stale {
                        select.update(cx, |s, cx| s.set_items(items, window, cx));
                    }
                    select
                }
                None => {
                    let selected = items
                        .iter()
                        .position(|c| c.0.id == option.current)
                        .map(IndexPath::new);
                    let select = cx.new(|cx| SelectState::new(items, selected, window, cx));
                    let id = option.id.clone();
                    self._subscriptions.push(cx.subscribe(
                        &select,
                        move |this, _, event: &SelectEvent<Vec<Choice>>, cx| {
                            if let SelectEvent::Confirm(Some(value)) = event {
                                this.set_option(&id, value.clone(), cx);
                            }
                        },
                    ));
                    select
                }
            };
            if select.read(cx).selected_value() != Some(&option.current) {
                select.update(cx, |s, cx| {
                    s.set_selected_value(&option.current, window, cx)
                });
            }
            self.pickers.push((option.id, select));
        }
    }

    /// Sets a config option of this session only; an error puts the old value back.
    pub(crate) fn set_option(&mut self, option: &str, value: String, cx: &mut Context<Self>) {
        let Some(found) = self.model.config.iter_mut().find(|o| o.id == option) else {
            return;
        };
        if found.current == value {
            return;
        }
        let previous = std::mem::replace(&mut found.current, value.clone());
        let name = found.name.clone();
        let (handle, id, option) = (self.handle.clone(), self.id.clone(), option.to_string());
        cx.spawn(async move |this, cx| {
            let set = handle.set_config(id, option.clone(), value).await;
            this.update(cx, |this, cx| {
                match set {
                    Ok(options) => this.model.config = options,
                    Err(e) => {
                        if let Some(o) = this.model.config.iter_mut().find(|o| o.id == option) {
                            o.current = previous;
                        }
                        this.model
                            .notice(format!("Could not switch {name}: {e}"), NoticeKind::Error);
                        this.sync_rows(None, cx);
                    }
                }
                this.pickers_stale = true;
                cx.notify();
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// The default model of a brand-new session: the agent's own when it no longer offers it.
    pub fn apply_default_model(&mut self, model: &str, cx: &mut Context<Self>) {
        let Some(option) = (self.model.config.iter())
            .find(|o| o.category == ConfigCategory::Model)
            .cloned()
        else {
            return;
        };
        if option.values.iter().any(|v| v.id == model) {
            return self.set_option(&option.id, model.to_string(), cx);
        }
        let current = (option.values.iter())
            .find(|v| v.id == option.current)
            .map_or(&option.current, |v| &v.name);
        let label = wai_settings::agent_label(&self.agent);
        self.model.notice(
            format!("{label} no longer offers the model {model}; using {current}."),
            NoticeKind::Info,
        );
        self.sync_rows(None, cx);
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

    /// Shows the steps as `processing` says, from now on and for the rows already there.
    pub fn set_processing(&mut self, processing: Processing, cx: &mut Context<Self>) {
        if self.processing == processing {
            return;
        }
        self.processing = processing;
        self.remeasure(0..self.model.items.len(), cx);
        cx.notify();
    }

    /// Rows `rows` changed height.
    pub(super) fn remeasure(&mut self, rows: std::ops::Range<usize>, cx: &mut Context<Self>) {
        self.scroller.update(cx, |list, cx| {
            let known = list.item_count();
            list.remeasure_items(rows.start.min(known)..rows.end.min(known), cx);
        });
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
        self.pickers_stale |= matches!(ev, SessionEvent::ConfigChanged(_));
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
        // R34: before the agent can write, note which of its files exist.
        self.model.note_files(ix, &self.cwd);
        self.sync_row(ix, cx);
        self.sync_rows(Some(ix), cx);
        // An edit that reached the disk leaves its run of steps: the rows around it follow.
        if self.processing == Processing::Grouped {
            let around = |item: &ChatItem| item.is_step() || item.is_edit();
            if around(&self.model.items[ix]) {
                self.remeasure(self.model.run(ix, around), cx);
            }
        }
        cx.notify();
    }

    pub fn handle_permission(&mut self, req: PermissionRequest, cx: &mut Context<Self>) {
        let ix = self.update_model(cx, |m| m.on_permission(req));
        self.sync_row(ix, cx);
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }

    /// Resolutions reach every session of the agent; the model ignores ids it does not hold.
    pub fn handle_permission_resolved(&mut self, id: u64, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.on_permission_resolved(id));
        cx.notify();
    }

    pub fn handle_question(&mut self, req: QuestionRequest, cx: &mut Context<Self>) {
        let ix = self.update_model(cx, |m| m.on_question(req));
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }

    /// The agent's form had a field wai cannot show; wai-agent already declined it.
    pub fn handle_question_unsupported(&mut self, cx: &mut Context<Self>) {
        self.model
            .notice(UNSUPPORTED_QUESTION.into(), NoticeKind::Info);
        self.sync_rows(None, cx);
        cx.notify();
    }

    /// Like permissions, resolutions reach every session of the agent.
    pub fn handle_question_resolved(&mut self, id: u64, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.on_question_resolved(id));
        let items = &self.model.items;
        self.questions.retain(|ix, _| {
            matches!(
                items.get(*ix),
                Some(ChatItem::Question { answered: None, .. })
            )
        });
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
        cx.emit(ChatEvent::TurnEnded(reason.clone()));
        self.release_queue(*reason == StopReason::EndTurn, cx);
        cx.notify();
    }

    /// The agent process died: every session on it shows the error and a Restart button.
    pub fn handle_agent_exited(&mut self, error: &str, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.on_exited(Some(error)));
        self.sync_rows(None, cx);
        self.release_queue(false, cx);
        cx.notify();
    }

    /// The user's name for the session (`None`: the agent's title again). Never sent to the
    /// agent.
    pub fn rename(&mut self, name: Option<String>, cx: &mut Context<Self>) {
        self.model.name = name;
        cx.emit(ChatEvent::TitleChanged);
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
            self.sync_clock(cx);
        }
        if self.model.title != title {
            cx.emit(ChatEvent::TitleChanged);
        }
        result
    }

    /// The status line's clock: one tick a second while a turn runs, no timer otherwise.
    fn sync_clock(&mut self, cx: &mut Context<Self>) {
        if !matches!(self.model.state, TurnState::Working | TurnState::NeedsYou) {
            self.turn = None;
        } else if self.turn.is_none() {
            let tick = cx.spawn(async move |this, cx| {
                loop {
                    cx.background_executor().timer(Duration::from_secs(1)).await;
                    if this.update(cx, |_, cx| cx.notify()).is_err() {
                        break;
                    }
                }
            });
            self.turn = Some((Instant::now(), tick));
        }
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
                let diffs = diff_views(&tool.diffs, &tool.locations, &self.cwd);
                self.diffs.insert(ix, diffs);
            }
            // R5: the request's own diff, else the diffs of its tool call.
            ChatItem::Permission { request, .. } => {
                let tool = self.model.items.iter().rev().find_map(|item| match item {
                    ChatItem::Tool(t) if t.id == request.tool_call_id => Some(t),
                    _ => None,
                });
                let locations = tool.map_or(&[][..], |t| &t.locations[..]);
                let diffs = match &request.diff {
                    Some(diff) => std::slice::from_ref(diff),
                    None => tool.map_or(&[][..], |t| &t.diffs[..]),
                };
                let diffs = diff_views(diffs, locations, &self.cwd);
                self.diffs.insert(ix, diffs);
            }
            ChatItem::User { .. }
            | ChatItem::Steered { .. }
            | ChatItem::Question { .. }
            | ChatItem::Notice { .. }
            | ChatItem::Edits { .. } => {}
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
    /// empty list: Restart after a crash, a history row, and with `restore` a session of the last
    /// run. The replay arrives as events while `load_session` is pending, so the session must
    /// already be routed.
    pub fn load(&mut self, restore: bool, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self.agent.clone();
        let started = self
            .registry
            .update(cx, |r, cx| Some((r.handle(&agent, cx)?, r.mcp_servers())));
        let Ok(Some((handle, mcp))) = started else {
            return;
        };
        self.handle = handle.clone();
        self.model.items.clear();
        self.texts.clear();
        self.diffs.clear();
        self.open_thoughts.clear();
        self.open_steps.clear();
        self.open_groups.clear();
        self.questions.clear();
        self.scroller.update(cx, |list, cx| list.reset(0, cx));
        self.update_model(cx, |m| m.state = TurnState::Working);
        let (id, cwd, registry) = (self.id.clone(), self.cwd.clone(), self.registry.clone());
        cx.spawn_in(window, async move |this, cx| {
            let loaded = handle.load_session(id, cwd, mcp).await;
            this.update_in(cx, |this, window, cx| {
                let agent = this.agent.clone();
                match loaded {
                    Ok(started) => {
                        registry.update(cx, |r, cx| r.mark_ready(&agent, cx)).ok();
                        // A history row learns its modes here.
                        if this.modes.is_none() {
                            this.set_modes(started.modes, started.current_mode, window, cx);
                        }
                        if this.model.config.is_empty() {
                            this.model.config = started.config;
                            this.pickers_stale = true;
                        }
                        this.update_model(cx, |m| m.state = TurnState::Idle);
                    }
                    Err(AgentError::AuthRequired) => {
                        this.update_model(cx, |m| m.on_auth_required(&agent));
                    }
                    // A death was already reported by the registry.
                    Err(_) if matches!(this.model.state, TurnState::Error(_)) => {}
                    // The agent answered: it no longer has the session. The pane says so and
                    // takes no prompt; the workspace forgets the session.
                    Err(AgentError::Protocol(e)) if restore => {
                        eprintln!("wai: could not restore session {}: {e}", this.id);
                        let name = this.model.shown_title().unwrap_or(&this.id);
                        let text = format!("Could not restore session {name}");
                        this.update_model(cx, |m| {
                            m.state = TurnState::Error(text.clone());
                            m.notice(text, NoticeKind::Info);
                        });
                        cx.emit(ChatEvent::RestoreFailed);
                    }
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

/// A tool call's diffs, each opening at its line (`open_line`).
fn diff_views(diffs: &[FileDiff], locations: &[Location], cwd: &Path) -> Vec<DiffView> {
    let view = |k| DiffView::new(&diffs[k], cwd, open_line(locations, diffs, k));
    (0..diffs.len()).map(view).collect()
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::fs;
    use std::path::PathBuf;
    use std::rc::Rc;

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, Entity, Modifiers, TestAppContext, VisualTestContext, point, px};
    use wai_agent::{StopReason, SubagentState, ToolStatus};
    use wai_settings::{Change, Processing, Settings};

    use super::ChatSession;
    use crate::agents::RegistryEvent;
    use crate::chat::input::{Attachment, Message};
    use crate::chat::model::{ChatItem, EditedFile, NoticeKind, TurnState, UNSUPPORTED_QUESTION};
    use crate::notify;
    use crate::sidebar::canonical;
    use crate::workspace::tests::{
        fake_settings, history_settings, live, load_history, open, wait_until,
    };
    use crate::workspace::{Pane, Workspace};

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

    /// The fake agent's flags for a reply of `n` words, one every `delay` ms: the turn runs that
    /// long.
    fn slow_reply(n: usize, delay: u32) -> String {
        let words: Vec<String> = (0..n).map(|i| format!("w{i}")).collect();
        format!("--reply '{}' --chunk-delay-ms {delay}", words.join(" "))
    }

    fn user_texts(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> Vec<String> {
        session.read_with(cx, |s, _| {
            (s.model.items.iter())
                .filter_map(|item| match item {
                    ChatItem::User { text } => Some(text.clone()),
                    _ => None,
                })
                .collect()
        })
    }

    /// All the agent's text, every message joined.
    fn agent_texts(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> String {
        session.read_with(cx, |s, _| {
            (s.model.items.iter())
                .filter_map(|item| match item {
                    ChatItem::Agent { text } => Some(text.as_str()),
                    _ => None,
                })
                .collect()
        })
    }

    /// With an agent that steers, a message sent during a turn joins it: it shows as the user's
    /// at once, the turn's reply takes it in, and no second prompt goes out.
    #[gpui_kit::test]
    fn message_sent_during_a_turn_steers_it(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let flags = format!("--steering {}", slow_reply(20, 50));
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        send("also this", cx);
        let steered = ChatItem::Steered {
            text: "also this".into(),
        };
        wait_until(cx, "the steered message", |cx| {
            session.read_with(cx, |s, _| s.model.items.contains(&steered))
        });
        assert_eq!(typed(&session, cx), "");
        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        assert_eq!(user_texts(&session, cx), ["go"], "a second prompt went out");
        let words: Vec<String> = (0..20).map(|i| format!("w{i}")).collect();
        let reply = format!("{} Steered: also this", words.join(" "));
        assert_eq!(agent_texts(&session, cx), reply);
    }

    /// A steer that finds no turn running (it ended meanwhile) goes out as a prompt.
    #[gpui_kit::test]
    fn steer_without_a_turn_sends_a_prompt(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--steering"));
        let session = start_session(&workspace, cx);

        let late = Message {
            text: "late".into(),
            attachments: Vec::new(),
        };
        session.update(cx, |s, cx| s.steer(late, cx));
        wait_until(cx, "the reply", |cx| {
            state(&session, cx) == TurnState::Idle && last_agent_text(&session, cx).is_some()
        });
        let items = session.read_with(cx, |s, _| s.model.items.clone());
        assert_eq!(
            items,
            [
                ChatItem::User {
                    text: "late".into()
                },
                ChatItem::Agent {
                    text: "Hello from fake agent".into()
                },
            ]
        );
    }

    /// An agent that cannot steer: messages sent during a turn wait above the prompt box, × drops
    /// one, and at the turn's end the rest go out as one prompt.
    #[gpui_kit::test]
    fn messages_sent_during_a_turn_queue_until_it_ends(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(&slow_reply(40, 50)));
        let session = start_session(&workspace, cx);

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        for message in ["first", "second", "third"] {
            send(message, cx);
        }
        assert_eq!(typed(&session, cx), "");
        assert!(shows(("queued", 2usize), cx));
        click_edits(None, ("queued-remove", 1usize), cx);
        assert!(!shows(("queued", 2usize), cx));
        assert!(shows(("queued", 1usize), cx));

        wait_until(cx, "the queued prompt's reply", |cx| {
            state(&session, cx) == TurnState::Idle && user_texts(&session, cx).len() == 2
        });
        assert_eq!(user_texts(&session, cx), ["go", "first\n\nthird"]);
        assert!(!shows(("queued", 0usize), cx));
    }

    /// Stop with messages queued: nothing goes out; their text and attachments go back into the
    /// prompt box, ahead of what was typed since.
    #[gpui_kit::test]
    fn stop_puts_queued_messages_back_in_the_prompt_box(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(&slow_reply(200, 20)));
        let session = start_session(&workspace, cx);
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.md");
        fs::write(&file, "x").unwrap();

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        session.update(cx, |s, _| s.attachments.push(Attachment::new(&file)));
        send("held", cx);
        assert!(session.read_with(cx, |s, _| s.attachments.is_empty()));
        cx.simulate_input("draft");
        cx.simulate_keystrokes("escape");

        let cancelled = ChatItem::Notice {
            text: "Cancelled".into(),
            kind: NoticeKind::Info,
        };
        wait_until(cx, "Cancelled", |cx| {
            session.read_with(cx, |s, _| s.model.items.last() == Some(&cancelled))
        });
        cx.update(|window, cx| window.render_frame(cx));
        assert_eq!(typed(&session, cx), "held\n\ndraft");
        let names = session.read_with(cx, |s, _| {
            (s.attachments.iter())
                .map(|a| a.name.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(names, ["notes.md"]);
        assert!(!shows(("queued", 0usize), cx));
        assert_eq!(user_texts(&session, cx), ["go"]);
    }

    /// While a turn runs, Send stands next to Stop, enabled once there is something to send.
    #[gpui_kit::test]
    fn send_stays_next_to_stop_during_a_turn(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(&slow_reply(200, 20)));
        let session = start_session(&workspace, cx);
        let send_disabled = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.find("send").disabled() == Some(true)
            })
        };

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        assert!(shows("stop", cx));
        assert!(send_disabled(cx), "nothing to send yet");
        cx.simulate_input("more");
        assert!(!send_disabled(cx));
        assert!(shows("stop", cx));
    }

    /// ⌘V of an image or of files copied in Finder attaches them; text pastes as text.
    #[gpui_kit::test]
    fn paste_attaches_images_and_files(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        let names = |cx: &mut VisualTestContext| {
            session.read_with(cx, |s, _| {
                s.attachments
                    .iter()
                    .map(|a| a.name.clone())
                    .collect::<Vec<_>>()
            })
        };
        let paste = |item: gpui_kit::ClipboardItem, cx: &mut VisualTestContext| {
            cx.write_to_clipboard(item);
            cx.simulate_keystrokes("secondary-v");
            cx.run_until_parked();
        };

        let image = gpui_kit::Image::from_bytes(gpui_kit::ImageFormat::Png, vec![0x89, 1]);
        paste(
            gpui_kit::ClipboardItem {
                entries: vec![gpui_kit::ClipboardEntry::Image(image)],
            },
            cx,
        );
        assert_eq!(names(cx), ["Pasted image.png"]);
        assert_eq!(session.read_with(cx, |s, cx| s.draft(cx)), "");

        paste(gpui_kit::ClipboardItem::new_string("some text".into()), cx);
        assert_eq!(session.read_with(cx, |s, cx| s.draft(cx)), "some text");
        assert_eq!(names(cx).len(), 1);

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("notes.pdf");
        fs::write(&file, "x").unwrap();
        paste(
            gpui_kit::ClipboardItem {
                entries: vec![
                    gpui_kit::ClipboardEntry::ExternalPaths(gpui_kit::ExternalPaths(
                        vec![file.clone()].into(),
                    )),
                    gpui_kit::ClipboardEntry::String(gpui_kit::ClipboardString::new(
                        file.display().to_string(),
                    )),
                ],
            },
            cx,
        );
        assert_eq!(names(cx), ["Pasted image.png", "notes.pdf"]);
        assert_eq!(session.read_with(cx, |s, cx| s.draft(cx)), "some text");

        cx.simulate_keystrokes("enter");
        wait_until(cx, "the prompt", |cx| {
            session.read_with(cx, |s, _| !s.model.items.is_empty())
        });
        let first = session.read_with(cx, |s, _| s.model.items[0].clone());
        assert_eq!(
            first,
            ChatItem::User {
                text: "some text\nAttached: Pasted image.png, notes.pdf".into()
            }
        );
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

    /// A subagent shows as a card with its title, status and answer; its tool call stays inline,
    /// tagged with it; its plan does not replace the session's.
    #[gpui_kit::test]
    fn subagent_shows_in_the_chat(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent --plan"));
        let session = start_session(&workspace, cx);
        let sub = session.read_with(cx, |s, _| format!("{}-sub", s.id));

        send("hi", cx);
        wait_until(cx, "the subagent to finish", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle
                    && s.model
                        .subagents
                        .iter()
                        .any(|a| a.id == sub && a.state == SubagentState::Done)
            })
        });
        let tool = |id: &str, cx: &mut VisualTestContext| {
            session.read_with(cx, |s, _| {
                s.model.items.iter().find_map(|item| match item {
                    ChatItem::Tool(t) if t.id == id => Some(t.clone()),
                    _ => None,
                })
            })
        };
        let card = tool(&sub, cx).expect("the subagent's card");
        assert_eq!(card.status, ToolStatus::Completed);
        assert_eq!(card.title, "Explore the code");
        assert_eq!(card.text, "found it");
        let read = tool("sub-read", cx).expect("the subagent's call");
        assert_eq!(read.subagent.as_deref(), Some(sub.as_str()));
        assert_eq!(session.read_with(cx, |s, _| s.model.plan.len()), 3);
        assert_eq!(
            last_agent_text(&session, cx).as_deref(),
            Some("Hello from fake agent")
        );
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

    /// The choice rows and the icon-only Stop are named for assistive tech: the rows' text is a
    /// child, not a button label.
    #[gpui_kit::test]
    fn permission_choices_have_names(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-permission"));
        let session = start_session(&workspace, cx);

        send("edit", cx);
        wait_until(cx, "the permission card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
        });
        cx.update(|window, cx| {
            window.render_frame(cx);
            let label = |id: &'static str| window.find(id).label().map(str::to_owned);
            assert_eq!(label("perm-allow_once").as_deref(), Some("Yes"));
            assert_eq!(label("perm-reject_once").as_deref(), Some("No"));
            assert_eq!(label("stop").as_deref(), Some("Stop"));
        });
    }

    /// Submit waits for the required choice; then the answer reaches the agent.
    #[gpui_kit::test]
    fn question_card_submits_choice(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-question"));
        let session = start_session(&workspace, cx);

        send("ask", cx);
        wait_until(cx, "the question card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
        });
        cx.update(|window, cx| window.click("question-submit", cx));
        assert_eq!(
            state(&session, cx),
            TurnState::NeedsYou,
            "submitted unanswered"
        );
        cx.update(|window, cx| window.within("field-color").click(1usize, cx)); // blue
        cx.update(|window, cx| window.click("question-submit", cx));

        wait_until(cx, "the answer", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        let reply = last_agent_text(&session, cx).unwrap_or_default();
        assert!(
            reply.starts_with("answered:") && reply.contains(r#""color":"blue""#),
            "{reply}"
        );
        let answered = session.read_with(cx, |s, _| {
            s.model.items.iter().find_map(|item| match item {
                ChatItem::Question { answered, .. } => answered.clone(),
                _ => None,
            })
        });
        assert_eq!(answered.as_deref(), Some("Answered: Blue"));
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find("question-submit").is_none(), "form stays");
        });
    }

    /// R23: a form with a field wai cannot show is declined, and the chat says so.
    #[gpui_kit::test]
    fn unsupported_question_is_declined_with_notice(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-unsupported-question"));
        let session = start_session(&workspace, cx);

        send("ask", cx);
        wait_until(cx, "the reply", |cx| {
            state(&session, cx) == TurnState::Idle && last_agent_text(&session, cx).is_some()
        });
        assert_eq!(
            last_agent_text(&session, cx).as_deref(),
            Some("answered:declined")
        );
        let items = session.read_with(cx, |s, _| s.model.items.clone());
        let notice = ChatItem::Notice {
            text: UNSUPPORTED_QUESTION.into(),
            kind: NoticeKind::Info,
        };
        assert!(items.contains(&notice), "{items:?}");
        assert!(
            !items
                .iter()
                .any(|item| matches!(item, ChatItem::Question { .. })),
            "{items:?}"
        );
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

    /// Clicks `id` in the Edits block at row `ix` (or anywhere with `None`), then lets it settle.
    fn click_edits(ix: Option<usize>, id: impl Into<ElementId>, cx: &mut VisualTestContext) {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            match ix {
                Some(ix) => window.within(("edits", ix)).click(id, cx),
                None => window.click(id, cx),
            }
        });
        cx.run_until_parked();
    }

    /// Sends `edit` and allows the fake agent's `n` edits, one permission card at a time (it
    /// writes each file only then), until the turn ends with its Edits block: its row and files.
    fn run_edits(
        session: &Entity<ChatSession>,
        n: usize,
        cx: &mut VisualTestContext,
    ) -> (usize, Vec<EditedFile>) {
        send("edit", cx);
        for _ in 0..n {
            wait_until(cx, "an edit's permission card", |cx| {
                state(session, cx) == TurnState::NeedsYou
            });
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.click("perm-allow_once", cx);
            });
        }
        wait_until(cx, "the Edits block", |cx| {
            state(session, cx) == TurnState::Idle
                && session.read_with(cx, |s, _| {
                    matches!(s.model.items.last(), Some(ChatItem::Edits { .. }))
                })
        });
        session.read_with(cx, |s, _| match s.model.items.last() {
            Some(ChatItem::Edits { files, .. }) => (s.model.items.len() - 1, files.clone()),
            _ => unreachable!(),
        })
    }

    /// The editor tab of `path` after Open: its path and the caret's (0-based) line.
    fn opened(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> (PathBuf, u32) {
        let tab = workspace.read_with(cx, |w, cx| {
            w.panes.iter().find_map(|pane| match pane {
                Pane::Editor(pane) => {
                    let pane = pane.read(cx);
                    pane.tabs.get(pane.active).cloned()
                }
                _ => None,
            })
        });
        let tab = tab.expect("an editor tab");
        tab.read_with(cx, |t, cx| {
            (t.path.clone(), t.editor.read(cx).cursor_position().line)
        })
    }

    fn failed(
        session: &Entity<ChatSession>,
        ix: usize,
        file: usize,
        cx: &mut VisualTestContext,
    ) -> Option<String> {
        session.read_with(cx, |s, _| match &s.model.items[ix] {
            ChatItem::Edits { files, .. } => files[file].failed.clone(),
            _ => None,
        })
    }

    /// Revert, confirmed in a dialog whose text says what happens: `revert-delete`,
    /// `revert-whole` (the whole file goes back) or `revert-snippet` (only the edit's lines do).
    fn revert(ix: usize, file: usize, says: &'static str, cx: &mut VisualTestContext) {
        click_edits(Some(ix), ("edit-revert", file), cx);
        assert!(
            cx.update(|window, cx| window.has_active_dialog(cx)),
            "no confirmation"
        );
        cx.update(|window, cx| window.render_frame(cx));
        assert!(
            cx.debug_bounds(says).is_some(),
            "the dialog does not say {says}"
        );
        click_edits(None, "revert-confirm", cx);
    }

    /// R38: a Revert that would refuse says why at once, without asking first.
    fn refused(ix: usize, file: usize, cx: &mut VisualTestContext) {
        click_edits(Some(ix), ("edit-revert", file), cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    }

    /// A finished turn lists the files it edited, each with Open and Revert; Revert asks first,
    /// undoes a file's steps newest first and refuses a file changed since (Review Focus 2).
    #[gpui_kit::test]
    fn edits_block_lists_turn_files(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let (a, b) = (dir.path().join("a.txt"), dir.path().join("b.txt"));
        fs::write(&a, "x\n").unwrap();
        // The fake agent edits a, creates b, then edits a again.
        let (qa, qb) = (wai_agent::quote(&a), wai_agent::quote(&b));
        let flags = format!("--edit {qa} --edit {qb} --edit {qa}");
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);
        let edited = "edited by the fake agent\n";

        let (ix, files) = run_edits(&session, 3, cx);
        let paths: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths, [a.clone(), b.clone()]);
        assert_eq!(files[0].steps, [0, 2]);
        cx.update(|window, cx| {
            window.render_frame(cx);
            let block = window.within(("edits", ix));
            assert!(block.try_find(("edit-open", 1usize)).is_some());
            assert!(block.try_find(("edit-open", 2usize)).is_none());
        });

        // Open: at the first line the latest edit changed (the third).
        click_edits(Some(ix), ("edit-open", 0usize), cx);
        let tab = workspace.read_with(cx, |w, cx| {
            w.panes.iter().find_map(|pane| match pane {
                Pane::Editor(pane) => pane.read(cx).tabs.first().cloned(),
                _ => None,
            })
        });
        let tab = tab.expect("an editor tab");
        tab.read_with(cx, |t, cx| {
            assert_eq!(t.path, canonical(&a));
            assert_eq!(t.editor.read(cx).cursor_position().line, 2);
        });
        let text = |cx: &mut VisualTestContext| {
            tab.read_with(cx, |t, cx| t.editor.read(cx).text().to_string())
        };

        // Cancel writes nothing.
        click_edits(Some(ix), ("edit-revert", 0usize), cx);
        click_edits(None, "cancel", cx);
        assert_eq!(
            fs::read_to_string(&a).unwrap(),
            format!("x\n{edited}{edited}")
        );

        // Newest step first; the open tab follows.
        revert(ix, 0, "revert-whole", cx);
        assert_eq!(fs::read_to_string(&a).unwrap(), format!("x\n{edited}"));
        assert_eq!(text(cx), format!("x\n{edited}"));
        revert(ix, 0, "revert-whole", cx);
        assert_eq!(fs::read_to_string(&a).unwrap(), "x\n");
        cx.update(|window, cx| {
            window.render_frame(cx);
            let block = window.within(("edits", ix));
            let revert = block.try_find(("edit-revert", 0usize));
            assert!(revert.is_none(), "all reverted");
        });

        // A file changed since the edit stays as it is, and the row says why.
        fs::write(&b, "mine\n").unwrap();
        refused(ix, 1, cx);
        assert_eq!(fs::read_to_string(&b).unwrap(), "mine\n");
        assert_eq!(
            failed(&session, ix, 1, cx).as_deref(),
            Some("File changed since this edit — open it to compare")
        );

        // A file the edit created is deleted.
        fs::write(&b, edited).unwrap();
        revert(ix, 1, "revert-delete", cx);
        assert!(!b.exists());
    }

    /// R32, R33, R35, R34 end to end, with edits shaped like claude-agent-acp's: an Edit reported
    /// as a hunk with its location opens at the changed line and reverts just that hunk; Revert
    /// waits while a turn runs; a Write over an existing file (no old text) is never reverted.
    #[gpui_kit::test]
    fn snippet_reverts_and_write_over_existing_refuses(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let (lib, keep) = (dir.path().join("lib.rs"), dir.path().join("keep.txt"));
        let original: String = (1..=9).map(|i| format!("fn f{i}() {{}}\n")).collect();
        fs::write(&lib, &original).unwrap();
        fs::write(&keep, "mine\n").unwrap();
        let flags = format!(
            "--edit-hunk {} --write {}",
            wai_agent::quote(&lib),
            wai_agent::quote(&keep)
        );
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);

        let (ix, files) = run_edits(&session, 2, cx);
        let paths: Vec<_> = files.iter().map(|f| f.path.clone()).collect();
        assert_eq!(paths, [lib.clone(), keep.clone()]);
        let edited = original.replace("fn f5() {}", "fn f5() {} // edited");
        assert_eq!(fs::read_to_string(&lib).unwrap(), edited);

        // The hunk starts at line 2 (its location); its change is on its 4th line: line 5.
        click_edits(Some(ix), ("edit-open", 0usize), cx);
        assert_eq!(opened(&workspace, cx), (canonical(&lib), 4));

        // R35: no Revert while a turn runs.
        let set_state = |state: TurnState, cx: &mut VisualTestContext| {
            session.update(cx, |s, cx| s.update_model(cx, |m| m.state = state));
        };
        set_state(TurnState::Working, cx);
        click_edits(Some(ix), ("edit-revert", 0usize), cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
        set_state(TurnState::Idle, cx);

        // Only the hunk goes back: a change elsewhere in the file stays.
        let mine = edited.replace("fn f1() {}", "fn f1() { mine() }");
        fs::write(&lib, &mine).unwrap();
        revert(ix, 0, "revert-snippet", cx);
        assert_eq!(
            fs::read_to_string(&lib).unwrap(),
            original.replace("fn f1() {}", "fn f1() { mine() }")
        );
        assert_eq!(failed(&session, ix, 0, cx), None);

        // R34: the Write had no old text and keep.txt was there before it: it stays.
        refused(ix, 1, cx);
        assert_eq!(
            fs::read_to_string(&keep).unwrap(),
            "written by the fake agent\n"
        );
        assert_eq!(
            failed(&session, ix, 1, cx).as_deref(),
            Some("Can't revert: the file existed before this edit.")
        );
    }

    /// The fake agent with `flag`, its steps shown as `processing` says.
    fn processing_settings(flag: &str, processing: Processing) -> Settings {
        let mut settings = fake_settings(flag);
        settings.conversation.processing = processing;
        settings
    }

    /// Sends a prompt and waits for the turn's reply.
    fn turn(session: &Entity<ChatSession>, cx: &mut VisualTestContext) {
        send("go", cx);
        wait_until(cx, "the reply", |cx| {
            state(session, cx) == TurnState::Idle && last_agent_text(session, cx).is_some()
        });
    }

    /// Whether the next frame draws `id`.
    fn shows(id: impl Into<ElementId>, cx: &mut VisualTestContext) -> bool {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
    }

    /// `--steps 2` is a thought (row 1) and two Read calls (rows 2 and 3). Step by step, each is
    /// one folded line; a click unfolds a call's output.
    #[gpui_kit::test]
    fn steps_are_folded_lines_that_unfold(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--steps 2"));
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        session.read_with(cx, |s, _| {
            assert!(matches!(s.model.items[1], ChatItem::Thought { .. }));
            assert!(matches!(s.model.items[2], ChatItem::Tool(_)));
            assert!(matches!(s.model.items[3], ChatItem::Tool(_)));
        });

        assert!(shows(("thought", 1usize), cx));
        assert!(shows(("step", 2usize), cx) && shows(("step", 3usize), cx));
        assert!(!shows(("step-output", 2usize), cx), "starts folded");
        click_edits(None, ("step", 2usize), cx);
        assert!(shows(("step-output", 2usize), cx));
        assert!(!shows(("step-output", 3usize), cx));
        click_edits(None, ("step", 2usize), cx);
        assert!(!shows(("step-output", 2usize), cx));
    }

    /// Grouped: the run of steps between two messages is one folded block that says how many;
    /// unfolded, it lists the step lines, each still folded.
    #[gpui_kit::test]
    fn grouped_mode_folds_a_run_of_steps_into_one_block(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let settings = processing_settings("--steps 2", Processing::Grouped);
        let (workspace, cx) = open(cx, settings);
        let session = start_session(&workspace, cx);
        turn(&session, cx);

        assert!(shows(("group", 1usize), cx));
        assert!(!shows(("group", 2usize), cx), "one block for the run");
        for hidden in [("thought", 1usize), ("step", 2), ("step", 3)] {
            assert!(!shows(hidden, cx), "{hidden:?}");
        }
        let label = session.read_with(cx, |s, _| s.model.run_label(1..4));
        assert_eq!(label, "3 steps · Read README.md");
        assert!(last_agent_text(&session, cx).is_some());

        click_edits(None, ("group", 1usize), cx);
        for listed in [("thought", 1usize), ("step", 2), ("step", 3)] {
            assert!(shows(listed, cx), "{listed:?}");
        }
        assert!(!shows(("step-output", 2usize), cx), "each still folded");
        click_edits(None, ("step", 2usize), cx);
        assert!(shows(("step-output", 2usize), cx));
    }

    /// Hidden: no step shows, but what needs the user and what changed their files does: the
    /// permission card, the edit's own line, and the Edits block, whose Revert still works.
    #[gpui_kit::test]
    fn hidden_mode_keeps_cards_and_edits(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        let flags = format!("--steps 1 --edit {}", wai_agent::quote(&file));
        let settings = processing_settings(&flags, Processing::Hidden);
        let (workspace, cx) = open(cx, settings);
        let session = start_session(&workspace, cx);

        send("edit", cx);
        wait_until(cx, "the permission card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
        });
        assert!(shows("perm-allow_once", cx), "the card shows");
        // Rows: the prompt, the thought, the Read call, the edit, its card.
        assert!(!shows(("edited", 3usize), cx), "not written yet");
        click_edits(None, "perm-allow_once", cx);
        wait_until(cx, "the Edits block", |cx| {
            state(&session, cx) == TurnState::Idle
                && session.read_with(cx, |s, _| {
                    matches!(s.model.items.last(), Some(ChatItem::Edits { .. }))
                })
        });
        let ix = session.read_with(cx, |s, _| s.model.items.len() - 1);

        for hidden in [("thought", 1usize), ("step", 2), ("step", 3), ("group", 1)] {
            assert!(!shows(hidden, cx), "{hidden:?}");
        }
        assert!(shows(("edited", 3usize), cx), "the edit's line");
        assert!(last_agent_text(&session, cx).is_some());
        assert!(file.exists());
        revert(ix, 0, "revert-delete", cx);
        assert!(!file.exists());
    }

    /// While a turn runs, a line above the prompt says what the agent does and for how long; it
    /// goes with the turn, and so does its tick.
    #[gpui_kit::test]
    fn status_line_follows_the_turn(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--steps 1 --chunk-delay-ms 300"));
        let session = start_session(&workspace, cx);
        assert!(!shows("turn-status", cx));
        assert!(session.read_with(cx, |s, _| s.turn.is_none()));

        send("go", cx);
        wait_until(cx, "the Read call", |cx| {
            session.read_with(cx, |s, _| {
                s.model.activity().as_deref() == Some("Reading README.md…")
            })
        });
        assert!(shows("turn-status", cx));
        assert!(session.read_with(cx, |s, _| s.turn.is_some()));
        wait_until(cx, "the reply's first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        let activity = session.read_with(cx, |s, _| s.model.activity());
        assert_eq!(activity.as_deref(), Some("Thinking…"));
        assert!(shows("turn-status", cx));

        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        assert!(!shows("turn-status", cx));
        assert!(session.read_with(cx, |s, _| s.turn.is_none()), "no tick");
    }

    /// A change of the mode, from the screen or in the file, reaches the sessions already open.
    #[gpui_kit::test]
    fn processing_change_applies_to_open_sessions(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = live(cx, fake_settings("--steps 1"), None);
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        assert!(shows(("step", 2usize), cx));

        workspace.update_in(cx, |w, window, cx| {
            w.change_setting(Change::Processing(Processing::Grouped), window, cx)
        });
        wait_until(cx, "the grouped mode", |cx| {
            session.read_with(cx, |s, _| s.processing == Processing::Grouped)
        });
        assert!(shows(("group", 1usize), cx));
        assert!(!shows(("step", 2usize), cx));

        let file = "[conversation]\nprocessing = \"hidden\"\n";
        fs::write(dir.path().join("settings.toml"), file).unwrap();
        wait_until(cx, "the hidden mode", |cx| {
            session.read_with(cx, |s, _| s.processing == Processing::Hidden)
        });
        assert!(!shows(("group", 1usize), cx));
        assert!(!shows(("step", 2usize), cx));
        assert!(last_agent_text(&session, cx).is_some());
    }

    fn typed(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> String {
        session.read_with(cx, |s, cx| s.input.read(cx).value().to_string())
    }

    /// ⌘N, then the session once its agent's commands arrived.
    fn commands_session(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ChatSession> {
        let session = start_session(workspace, cx);
        wait_until(cx, "the agent's commands", |cx| {
            session.read_with(cx, |s, _| !s.model.commands.is_empty())
        });
        session
    }

    /// The `/` menu's rows as `/name`; `None` while it is closed.
    fn menu(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> Option<Vec<String>> {
        session.read_with(cx, |s, cx| {
            let rows = s.menu(cx)?;
            Some(rows.iter().map(|c| format!("/{}", c.name)).collect())
        })
    }

    /// `/` first in the prompt box opens the agent's commands above it, filtered by what follows;
    /// a space after the name closes it.
    #[gpui_kit::test]
    fn slash_opens_the_command_menu(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = commands_session(&workspace, cx);
        assert!(!shows("commands", cx));

        cx.simulate_input("/");
        let all = ["/init", "/review", "/compact", "/pr-review"];
        assert_eq!(menu(&session, cx).unwrap(), all);
        assert!(shows("commands", cx) && shows(("command", 3usize), cx));
        let (list, input) = cx.update(|window, _| {
            let bounds = |id: &'static str| window.find(id).bounds();
            (bounds("commands"), bounds("input-state"))
        });
        assert!(
            list.bottom() < input.top(),
            "{list:?} is not above {input:?}"
        );
        // The input hint shows on its command's row.
        assert!(shows(("command-hint", 2usize), cx));
        assert!(!shows(("command-hint", 0usize), cx));

        cx.simulate_input("re");
        assert_eq!(menu(&session, cx).unwrap(), ["/review", "/pr-review"]);
        assert!(!shows(("command", 2usize), cx));
        cx.simulate_input("view ");
        assert_eq!(menu(&session, cx), None);
        assert!(!shows("commands", cx));
    }

    /// ↓/↑ move the selection and Enter puts `/name ` in the box, caret at its end, sending
    /// nothing; Tab completes too.
    #[gpui_kit::test]
    fn keys_complete_a_command(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = commands_session(&workspace, cx);

        cx.simulate_input("/");
        cx.simulate_keystrokes("down down up enter");
        assert_eq!(typed(&session, cx), "/review ");
        assert_eq!(menu(&session, cx), None);
        cx.simulate_input("x");
        assert_eq!(typed(&session, cx), "/review x");
        assert!(
            session.read_with(cx, |s, _| s.model.items.is_empty()),
            "sent"
        );

        session.update_in(cx, |s, window, cx| s.prefill("", window, cx));
        cx.simulate_input("/co");
        cx.simulate_keystrokes("tab");
        assert_eq!(typed(&session, cx), "/compact ");
    }

    /// A click on a row completes its command.
    #[gpui_kit::test]
    fn click_completes_a_command(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = commands_session(&workspace, cx);

        cx.simulate_input("/");
        click_edits(None, ("command", 2usize), cx);
        assert_eq!(typed(&session, cx), "/compact ");
        cx.simulate_input("x");
        assert_eq!(typed(&session, cx), "/compact x");
    }

    /// Esc closes the menu and keeps the text; typing opens it again. With it closed, Enter
    /// sends.
    #[gpui_kit::test]
    fn escape_closes_the_command_menu(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = commands_session(&workspace, cx);

        cx.simulate_input("/in");
        cx.simulate_keystrokes("escape");
        assert_eq!(menu(&session, cx), None);
        assert_eq!(typed(&session, cx), "/in");
        cx.simulate_input("i");
        assert_eq!(menu(&session, cx).unwrap(), ["/init"]);
        cx.simulate_keystrokes("escape enter");
        wait_until(cx, "the reply", |cx| {
            state(&session, cx) == TurnState::Idle && last_agent_text(&session, cx).is_some()
        });
        assert_eq!(user_texts(&session, cx), ["/ini"]);
    }

    /// Before the agent sent its commands, the menu says so in a single row.
    #[gpui_kit::test]
    fn command_menu_without_commands_says_so(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = commands_session(&workspace, cx);
        session.update(cx, |s, _| s.model.commands.clear());

        cx.simulate_input("/");
        assert_eq!(menu(&session, cx), Some(vec![]));
        let label = cx.update(|window, cx| {
            window.render_frame(cx);
            window.find("commands-empty").label().map(str::to_owned)
        });
        assert_eq!(label.as_deref(), Some("No commands from fake yet"));
    }

    /// A session that lands after ⌘N must not pull the caret out of the pane the user is in.
    #[gpui_kit::test]
    fn landing_session_keeps_focus_on_focused_pane(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(1));
        let rows = load_history(&workspace, 1, cx);

        // Pane 0 prepares a session; meanwhile a history row opens to the side and takes focus.
        cx.simulate_keystrokes("secondary-n");
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], true, window, cx)));
        let side = workspace.read_with(cx, |w, _| w.panes[1].session().cloned().unwrap());

        wait_until(cx, "the new session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        let landed = workspace.read_with(cx, |w, _| w.panes[0].session().cloned().unwrap());
        cx.simulate_input("typed");

        assert_eq!(workspace.read_with(cx, |w, _| w.focused_pane), 1);
        assert_eq!(typed(&side, cx), "typed");
        assert_eq!(typed(&landed, cx), "");
    }

    /// R83: a press anywhere on the prompt pill, its padding included, puts the caret in it, even
    /// from another field of the same pane (the question card's note here).
    #[gpui_kit::test]
    fn press_on_prompt_pill_focuses_input(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-question"));
        let session = start_session(&workspace, cx);
        send("ask", cx);
        wait_until(cx, "the question card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
        });

        cx.run_until_parked();
        let (inputs, send) = cx.update(|window, cx| {
            window.render_frame(cx);
            let inputs: Vec<_> = (window.find_all("input-state").iter())
                .map(|input| input.bounds())
                .collect();
            (inputs, window.find("send").bounds())
        });
        assert_eq!(inputs.len(), 2, "the note and the prompt");
        cx.simulate_click(inputs[0].center(), Modifiers::none());
        cx.simulate_input("n");
        assert_eq!(typed(&session, cx), "", "the note has the caret");

        // The pill's right padding, past the send button.
        let padding = point(send.right() + px(3.), send.center().y);
        cx.simulate_click(padding, Modifiers::none());
        cx.simulate_input("typed");

        assert_eq!(typed(&session, cx), "typed");
    }

    /// Clicking into a pane moves the caret there, so typing and ⌘W/⌘N act on the same pane.
    #[gpui_kit::test]
    fn click_moves_focus_to_pane(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        for row in load_history(&workspace, 2, cx) {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&row, true, window, cx)));
        }
        let pane = |ix: usize, cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| w.panes[ix].session().cloned().unwrap())
        };
        let (left, right) = (pane(0, cx), pane(1, cx));
        assert_eq!(workspace.read_with(cx, |w, _| w.focused_pane), 1);

        cx.run_until_parked();
        let bounds = cx.debug_bounds("pane-0").expect("pane 0 drawn");
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.simulate_input("typed");

        assert_eq!(workspace.read_with(cx, |w, _| w.focused_pane), 0);
        assert_eq!(typed(&left, cx), "typed");
        assert_eq!(typed(&right, cx), "");
    }

    /// Clicking blank space in the pane that already is the focused one gives its prompt the
    /// caret back when the focus went elsewhere (the terminal here).
    #[gpui_kit::test]
    fn click_in_focused_pane_refocuses_its_prompt(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent and the shell are real processes
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        cx.simulate_keystrokes("ctrl-`");
        assert_eq!(workspace.read_with(cx, |w, _| w.focused_pane), 0);

        cx.run_until_parked();
        let bounds = cx.debug_bounds("pane-0").expect("pane 0 drawn");
        cx.simulate_click(bounds.center(), Modifiers::none());
        cx.simulate_input("typed");

        assert_eq!(typed(&session, cx), "typed");
    }

    /// R21: past 20,000 lines, "Open full output in editor" writes the whole output to
    /// `<config dir>/scratch/<session>-<tool>.txt` and opens it read-only.
    #[gpui_kit::test]
    fn full_output_opens_read_only_in_editor(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let config = tempfile::tempdir().unwrap();
        // 25,000 lines of `x`.
        let (workspace, cx) = open(cx, fake_settings("--flood 50000"));
        workspace.update(cx, |w, _| w.user_dir = config.path().to_path_buf());
        let session = start_session(&workspace, cx);
        send("go", cx);
        wait_until(cx, "the reply", |cx| {
            state(&session, cx) == TurnState::Idle && last_agent_text(&session, cx).is_some()
        });
        let ix = session.read_with(cx, |s, _| {
            s.model
                .items
                .iter()
                .position(|i| matches!(i, ChatItem::Tool(_)))
        });
        let ix = ix.expect("the flood's tool call");

        click_edits(None, ("step", ix), cx);
        click_edits(None, ("show-all", ix), cx);
        click_edits(None, ("open-output", ix), cx);

        wait_until(cx, "the output's tab", |cx| {
            workspace.read_with(cx, |w, _| {
                w.panes.iter().any(|p| matches!(p, Pane::Editor(_)))
            })
        });
        let tab = workspace.read_with(cx, |w, cx| {
            w.panes.iter().find_map(|pane| match pane {
                Pane::Editor(pane) => pane.read(cx).tabs.first().cloned(),
                _ => None,
            })
        });
        let path = canonical(config.path()).join("scratch/fake-1-flood.txt");
        tab.expect("a tab").read_with(cx, |t, cx| {
            assert_eq!(t.path, path);
            assert!(t.read_only);
            assert_eq!(t.editor.read(cx).text().len(), 50_000);
        });
    }

    /// R12: the Open row's × on a session waiting for a permission ends it and drops its pane,
    /// and the agent's turn ends instead of waiting for an answer nobody can give.
    #[gpui_kit::test]
    fn close_drops_session_and_pane_and_ends_its_turn(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let cwd = std::env::temp_dir();
        let flags = format!(
            "--ask-permission --sessions 1 --sessions-cwd {}",
            wai_agent::quote(&cwd)
        );
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let rows = load_history(&workspace, 1, cx);
        let first = start_session(&workspace, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], true, window, cx)));
        let side = workspace.read_with(cx, |w, _| w.panes[1].session().cloned().unwrap());
        wait_until(cx, "the replay", |cx| state(&side, cx) == TurnState::Idle);

        let registry = workspace.read_with(cx, |w, _| w.registry.clone());
        let ended = Rc::new(RefCell::new(Vec::new()));
        cx.update(|_, cx| {
            let ended = ended.clone();
            cx.subscribe(&registry, move |_, event, _| {
                if let RegistryEvent::TurnEnded {
                    session, reason, ..
                } = event
                {
                    ended.borrow_mut().push((session.clone(), reason.clone()));
                }
            })
            .detach();
        });

        send("edit", cx); // into the focused side pane
        wait_until(cx, "the permission card", |cx| {
            state(&side, cx) == TurnState::NeedsYou
        });
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(("open-close", 1usize), cx);
        });

        workspace.read_with(cx, |w, _| {
            assert!(
                w.sessions == [first.clone()],
                "only the first session stays open"
            );
            assert_eq!(w.panes.len(), 1);
            assert_eq!(w.focused_pane, 0);
            assert!(w.panes[0].session() == Some(&first));
        });
        wait_until(cx, "the closed session's turn to end", |_| {
            ended
                .borrow()
                .contains(&("session-0".into(), StopReason::Cancelled))
        });
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

    /// Puts the window in the background, as when the user switched to another app.
    fn deactivate(cx: &mut VisualTestContext) {
        cx.deactivate_window();
        assert!(!cx.update(|window, _| window.is_window_active()));
    }

    /// R13: a turn that finishes (`EndTurn`) in the background says so.
    #[gpui_kit::test]
    fn finished_turn_notifies_in_background(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        deactivate(cx);

        send("hi", cx);
        wait_until(cx, "the reply", |cx| {
            state(&session, cx) == TurnState::Idle && last_agent_text(&session, cx).is_some()
        });
        let sent = notify::sent();
        assert_eq!(sent.len(), 1, "{sent:?}");
        assert!(sent[0].0.ends_with(" is done"), "{sent:?}");
        assert_eq!(sent[0].1, "Hello from fake agent");
    }

    /// R13: a cancelled turn is not "done".
    #[gpui_kit::test]
    fn cancelled_turn_does_not_notify(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let reply: Vec<String> = (0..200).map(|i| format!("w{i}")).collect();
        let flags = format!("--reply '{}' --chunk-delay-ms 20", reply.join(" "));
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);
        deactivate(cx);

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        cx.simulate_keystrokes("escape");
        wait_until(cx, "the cancel", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        assert_eq!(notify::sent(), []);
    }

    /// R13: loading a session (a history row now, a restore at launch in 1.9) replays it; that
    /// is not a finished turn.
    #[gpui_kit::test]
    fn loaded_session_does_not_notify(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(1));
        let rows = load_history(&workspace, 1, cx);
        deactivate(cx);

        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));
        let session = workspace.read_with(cx, |w, _| w.panes[0].session().cloned().unwrap());
        wait_until(cx, "the replay", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle && s.model.items.len() == 2
            })
        });
        assert_eq!(notify::sent(), []);
    }

    fn models_settings(default: Option<&str>) -> wai_settings::Settings {
        let mut settings = fake_settings("--models");
        if let Some(model) = default {
            settings.models.insert("fake".into(), model.into());
        }
        settings
    }

    fn reply(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> String {
        send("which?", cx);
        wait_until(cx, "the reply", |cx| {
            state(session, cx) == TurnState::Idle
                && last_agent_text(session, cx).is_some_and(|t| t.starts_with("model:"))
        });
        last_agent_text(session, cx).unwrap()
    }

    #[gpui_kit::test]
    fn header_shows_model_and_effort_pickers(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, models_settings(None));
        let session = start_session(&workspace, cx);
        let pickers = session.read_with(cx, |s, _| {
            s.pickers
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(pickers, ["model", "effort"]);
        assert!(session.read_with(cx, |s, _| s.legacy_mode_picker().is_some()));
    }

    #[gpui_kit::test]
    fn switching_model_applies_to_this_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, models_settings(None));
        let session = start_session(&workspace, cx);
        session.update(cx, |s, cx| s.set_option("model", "fake-large".into(), cx));
        assert_eq!(reply(&session, cx), "model:fake-large");

        workspace.update_in(cx, |w, window, cx| {
            w.start_session("fake".into(), None, false, None, window, cx)
        });
        wait_until(cx, "a second session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 2)
        });
        let second = workspace.read_with(cx, |w, _| w.sessions.last().cloned().unwrap());
        assert_eq!(reply(&second, cx), "model:fake-small");
    }

    #[gpui_kit::test]
    fn default_model_applies_to_new_sessions_only(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, models_settings(Some("fake-large")));
        let session = start_session(&workspace, cx);
        assert_eq!(reply(&session, cx), "model:fake-large");
    }

    #[gpui_kit::test]
    fn missing_default_model_shows_a_notice(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, models_settings(Some("gone")));
        let session = start_session(&workspace, cx);
        let notice = ChatItem::Notice {
            text: "fake no longer offers the model gone; using Fake Small.".into(),
            kind: NoticeKind::Info,
        };
        assert!(session.read_with(cx, |s, _| s.model.items.contains(&notice)));
    }

    #[gpui_kit::test]
    fn mode_option_replaces_the_legacy_picker(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--models --config-mode"));
        let session = start_session(&workspace, cx);
        let pickers = session.read_with(cx, |s, _| {
            s.pickers
                .iter()
                .map(|(id, _)| id.clone())
                .collect::<Vec<_>>()
        });
        assert_eq!(pickers, ["model", "effort", "mode"]);
        assert!(session.read_with(cx, |s, _| s.modes.is_some()
            && s.legacy_mode_picker().is_none()));
    }

    #[gpui_kit::test]
    fn failed_switch_reverts_and_says_so(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--models --reject-config"));
        let session = start_session(&workspace, cx);
        session.update(cx, |s, cx| s.set_option("model", "fake-large".into(), cx));
        wait_until(cx, "the notice", |cx| {
            session.read_with(cx, |s, _| {
                s.model.items.iter().any(|i| {
                    matches!(i, ChatItem::Notice { text, kind: NoticeKind::Error }
                        if text.starts_with("Could not switch Model: "))
                })
            })
        });
        cx.run_until_parked();
        let current = session.read_with(cx, |s, _| s.model.config[0].current.clone());
        assert_eq!(current, "fake-small");
        let picked = session.read_with(cx, |s, cx| {
            s.pickers[0].1.read(cx).selected_value().cloned()
        });
        assert_eq!(picked.as_deref(), Some("fake-small"));
    }
}
