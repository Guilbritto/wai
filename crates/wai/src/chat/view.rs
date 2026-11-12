use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, Ordering};
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
use crate::git::Head;

/// How often a streaming message takes its new text. Each push costs a parse and a window
/// redraw (~4 ms); pushing every token cost 12–18% of a core at 40 tokens/s.
// ponytail: a fixed rate; lower it for smoother text at the cost of CPU.
const STREAM_FLUSH: Duration = Duration::from_millis(100);

/// What a session shows when the agent gave its id to another open session.
pub(crate) const REUSED: &str =
    "The agent gave this session's id to another session. Restart loads it here again.";

/// Stamps `ChatSession::bound`: the higher stamp holds the id now.
static BINDINGS: AtomicU64 = AtomicU64::new(0);

fn next_binding() -> u64 {
    BINDINGS.fetch_add(1, Ordering::Relaxed) + 1
}

#[derive(Debug)]
pub enum ChatEvent {
    StateChanged,
    TitleChanged,
    /// The agent gave or loaded this session's id.
    Bound,
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
    /// Esc or Stop in a chat whose session has not started: back to the draft.
    StopStart,
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
    /// When the agent last gave or loaded `id`: of two sessions with one id, the higher holds it.
    pub(crate) bound: u64,
    pub cwd: PathBuf,
    /// The branch `cwd` is on, read in the background once the session is open.
    pub head: Option<Head>,
    /// Where the agent works when that is another repository or worktree than `cwd`'s; `None`:
    /// `cwd`.
    pub place: Option<Head>,
    /// Directory the agent touched -> its repository root (`None`: in none), so each is read once.
    roots: HashMap<PathBuf, Option<PathBuf>>,
    /// Counts the `place` reads started: only the last one's answer counts.
    place_read: u64,
    /// While a load replays: the last directory its tool calls named, followed once it ends.
    replayed: Option<PathBuf>,
    pub model: ChatModel,
    pub(super) handle: AgentHandle,
    pub(super) input: Entity<TextareaState>,
    pub(crate) attachments: Vec<Attachment>,
    /// Sent during a turn to an agent that cannot steer: they go out when it ends.
    pub(super) queued: Vec<Message>,
    /// The message of a session that is starting; it goes out once the session exists.
    first: Option<Message>,
    /// Model and mode changes in flight: the first message waits for them.
    settling: usize,
    /// A history resume or restore is replaying: not a turn, whatever the state says.
    loading: bool,
    /// Queued text going back to the prompt box at the next frame (which has the window).
    pub(super) unsent: Option<String>,
    /// The `/` menu's selected row, its scroll, and whether Esc closed it.
    pub(super) menu_ix: usize,
    pub(super) menu_scroll: ScrollHandle,
    pub(super) menu_closed: bool,
    pub(super) registry: WeakEntity<AgentRegistry>,
    /// Per `Agent`/`Thought` row.
    pub(super) texts: HashMap<usize, Markdown>,
    /// Per `Tool` row.
    pub(super) diffs: HashMap<usize, Vec<DiffView>>,
    /// Per waiting `Permission` card, by request id.
    pub(super) card_diffs: HashMap<u64, Vec<DiffView>>,
    pub(super) open_thoughts: HashSet<usize>,
    /// How the steps show (`conversation.processing`).
    pub(super) processing: Processing,
    /// The `Tool` rows unfolded to their output.
    pub(super) open_steps: HashSet<usize>,
    /// The unfolded runs of steps of the grouped mode, by their first row.
    pub(super) open_groups: HashSet<usize>,
    /// While a turn runs: when it started, the status line's tick period and its timer.
    pub(super) turn: Option<(Instant, Duration, Task<()>)>,
    /// When the state last changed; `None` until the first change (a restored session).
    pub since: Option<Instant>,
    /// Per waiting `Question` card, by request id, made when it first draws.
    pub(super) questions: HashMap<u64, QuestionForm>,
    pub(super) scroller: Entity<MessageScrollerState>,
    /// The pinned stack's height at the last paint; 0 while nothing waits.
    pub(super) waiting_height: Pixels,
    pub(super) modes: Option<Entity<SelectState<Vec<Mode>>>>,
    /// The header's pickers of the model, effort and mode options, in that order.
    pub(super) pickers: Vec<(String, Entity<SelectState<Vec<Choice>>>)>,
    pickers_stale: bool,
    /// The subagent whose drawer is open; `None`: `main`.
    pub(super) drawer: Option<String>,
    /// The agent list's keyboard row (0 is `main`) while it has the keys; `None`: the prompt.
    pub(super) agent_cursor: Option<usize>,
    pub(super) agent_scroll: ScrollHandle,
    pub(super) drawer_scroll: ScrollHandle,
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
                .placeholder(format!("Message {}…", wai_settings::agent_label(&agent)));
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
                            // Typing is back in the prompt, out of the agent list.
                            this.agent_cursor = None;
                            cx.notify();
                        }
                        _ => {}
                    }
                }),
            ];
        let mut this = Self {
            model: ChatModel::default(),
            agent,
            bound: if started.id.is_empty() {
                0
            } else {
                next_binding()
            },
            id: started.id,
            cwd,
            head: None,
            place: None,
            roots: HashMap::new(),
            place_read: 0,
            replayed: None,
            handle,
            input,
            attachments: Vec::new(),
            queued: Vec::new(),
            first: None,
            settling: 0,
            loading: false,
            unsent: None,
            menu_ix: 0,
            menu_scroll: ScrollHandle::new(),
            menu_closed: false,
            registry,
            texts: HashMap::new(),
            diffs: HashMap::new(),
            card_diffs: HashMap::new(),
            open_thoughts: HashSet::new(),
            processing: Processing::default(),
            open_steps: HashSet::new(),
            open_groups: HashSet::new(),
            turn: None,
            since: None,
            questions: HashMap::new(),
            scroller: cx.new(|cx| MessageScrollerState::new(0, cx)),
            waiting_height: px(0.),
            modes: None,
            pickers: Vec::new(),
            pickers_stale: false,
            drawer: None,
            agent_cursor: None,
            agent_scroll: ScrollHandle::new(),
            drawer_scroll: ScrollHandle::new(),
            flush: None,
            _subscriptions: subscriptions,
        };
        this.set_modes(started.modes, started.current_mode, window, cx);
        this.model.config = started.config;
        this.sync_pickers(window, cx);
        this
    }

    /// A chat for a session that has not started: it shows `text` at once and prompts it when
    /// `started` gives the session its id.
    #[allow(clippy::too_many_arguments)]
    pub fn pending(
        agent: String,
        cwd: PathBuf,
        handle: AgentHandle,
        registry: WeakEntity<AgentRegistry>,
        text: String,
        attachments: Vec<Attachment>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let none = SessionStarted {
            id: String::new(),
            modes: Vec::new(),
            current_mode: None,
            config: Vec::new(),
        };
        let mut this = Self::new(agent, none, cwd, handle, registry, window, cx);
        let message = Message { text, attachments };
        let ix = this.update_model(cx, |m| m.push_user(&message.shown()));
        this.first = Some(message);
        this.sync_rows(Some(ix), cx);
        this
    }

    /// Whether the agent has not given this session an id yet.
    pub fn is_starting(&self) -> bool {
        self.id.is_empty()
    }

    /// Adds a step line to the transcript; `end_step` closes it.
    pub fn step(&mut self, title: String, cx: &mut Context<Self>) -> usize {
        let ix = self.update_model(cx, |m| m.push_step(title));
        self.sync_rows(Some(ix), cx);
        cx.notify();
        ix
    }

    pub fn end_step(&mut self, ix: usize, title: String, failed: bool, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.end_step(ix, title, failed));
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }

    /// The agent started the session: it takes the id, modes and options (its first message goes
    /// out with `send_first`).
    pub fn started(
        &mut self,
        started: SessionStarted,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.id = started.id.clone();
        self.bound = next_binding();
        self.take_started(started, window, cx);
        cx.emit(ChatEvent::Bound);
        cx.notify();
    }

    /// Another open session holds this id now: the turn ends here and Restart shows.
    pub fn lose_id(&mut self, cx: &mut Context<Self>) {
        self.update_model(cx, |m| {
            m.wind_down();
            m.state = TurnState::Error(REUSED.into());
            m.notice(REUSED.into(), NoticeKind::Error);
        });
        self.sync_rows(None, cx);
        self.release_queue(false, cx);
        cx.notify();
    }

    /// Sends the first message (its bubble is already in the list), once the picked model and
    /// mode are in place.
    pub fn send_first(&mut self, cx: &mut Context<Self>) {
        if self.settling > 0 || self.id.is_empty() {
            return;
        }
        if let Some(message) = self.first.take() {
            let blocks = self.blocks(&message, cx);
            self.handle.prompt(self.id.clone(), blocks);
        }
    }

    /// What the user typed here while the session started, and the messages queued, with the
    /// attachments of the chat and then of the queued messages: it goes back to the draft.
    pub fn leftover(&self, cx: &App) -> (String, Vec<Attachment>) {
        let typed = self.input.read(cx).value().trim().to_string();
        let queued = self.queued.iter().map(|m| m.text.clone());
        let parts: Vec<String> = std::iter::once(typed)
            .chain(queued)
            .filter(|t| !t.is_empty())
            .collect();
        let files = (self.attachments.iter())
            .chain(self.queued.iter().flat_map(|m| &m.attachments))
            .cloned()
            .collect();
        (parts.join("\n\n"), files)
    }

    /// Sends `text` as a prompt once the picked model and mode are in place (an extension's
    /// `sessions/open` with `send`). `false` when the session is not idle or `text` is empty.
    pub fn send_settled(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let text = text.trim();
        if self.model.state != TurnState::Idle || text.is_empty() {
            return false;
        }
        let ix = self.update_model(cx, |m| m.push_user(text));
        self.first = Some(Message {
            text: text.into(),
            attachments: Vec::new(),
        });
        self.sync_rows(Some(ix), cx);
        self.send_first(cx);
        cx.notify();
        true
    }

    /// Takes what a started or loaded session reports, unless the pane already has it (a
    /// history row learns its modes and options here).
    fn take_started(
        &mut self,
        started: SessionStarted,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.modes.is_none() {
            self.set_modes(started.modes, started.current_mode, window, cx);
        }
        if self.model.config.is_empty() {
            self.model.config = started.config;
            self.pickers_stale = true;
        }
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
        self.settling += 1;
        cx.spawn(async move |this, cx| {
            let set = handle.set_config(id, option.clone(), value).await;
            this.update(cx, |this, cx| {
                this.settling -= 1;
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
                this.send_first(cx);
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

    /// Records the pinned stack's height: the list's bottom inset follows it.
    pub(super) fn set_waiting_height(&mut self, height: Pixels, cx: &mut Context<Self>) {
        if self.waiting_height == height {
            return;
        }
        self.waiting_height = height;
        let last = self.model.items.len();
        let following = self.scroller.read(cx).is_following_tail();
        self.remeasure(last.saturating_sub(1)..last, cx);
        if following {
            self.scroller.update(cx, |list, cx| list.scroll_to_end(cx));
        }
        cx.notify();
    }

    pub fn status(&self) -> &TurnState {
        &self.model.state
    }

    /// Puts the caret in the prompt box.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    /// Whether a turn is running or waits on the user.
    pub fn busy(&self) -> bool {
        !self.loading && matches!(self.model.state, TurnState::Working | TurnState::NeedsYou)
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
        let located = match ev {
            SessionEvent::ToolCall(_) => true,
            SessionEvent::ToolCallUpdate(p) => p.locations.is_some(),
            _ => false,
        };
        if located && let Some(ChatItem::Tool(t)) = self.model.items.get(ix) {
            let path = t.locations.last().map(|l| l.path.clone());
            if let (None, Some(path)) = (&t.subagent, path) {
                self.follow(&path, cx);
            }
        }
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
        let diffs = self.permission_diffs(&req);
        self.card_diffs.insert(req.id, diffs);
        self.update_model(cx, |m| m.on_permission(req));
        cx.notify();
    }

    /// Resolutions reach every session of the agent; the model ignores ids it does not hold.
    pub fn handle_permission_resolved(&mut self, id: u64, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.on_permission_resolved(id));
        self.card_diffs.remove(&id);
        self.sync_rows(None, cx);
        cx.notify();
    }

    pub fn handle_question(&mut self, req: QuestionRequest, cx: &mut Context<Self>) {
        self.update_model(cx, |m| m.on_question(req));
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
        self.questions.remove(&id);
        self.sync_rows(None, cx);
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
        self.reread_branch(cx);
        cx.emit(ChatEvent::TurnEnded(reason.clone()));
        self.release_queue(*reason == StopReason::EndTurn, cx);
        cx.notify();
    }

    /// Sets the session's own folder head; the agent being there again drops `place`.
    pub fn set_home(&mut self, head: Option<Head>, cx: &mut Context<Self>) {
        let same = |a: &Option<Head>, b: &Head| a.as_ref().is_some_and(|a| a.root == b.root);
        if let Some(h) = &head
            && same(&self.place, h)
        {
            self.place = None;
        }
        self.head = head;
        cx.notify();
    }

    /// The agent touched `path`: the strip moves to that file's repository and worktree.
    fn follow(&mut self, path: &Path, cx: &mut Context<Self>) {
        let path = self.cwd.join(path);
        let dir = match path.is_dir() {
            true => Some(path.as_path()),
            false => path.parent(),
        };
        let Some(dir) = dir.map(Path::to_path_buf) else {
            return;
        };
        if self.loading {
            self.replayed = Some(dir);
            return;
        }
        let current = self.place.as_ref().or(self.head.as_ref()).map(|h| &h.root);
        match self.roots.get(&dir) {
            Some(None) => return,
            Some(Some(root)) if Some(root) == current => {
                self.place_read += 1;
                return;
            }
            _ => {}
        }
        self.place_read += 1;
        let n = self.place_read;
        let found = cx.background_spawn({
            let dir = dir.clone();
            async move { crate::git::head(&dir) }
        });
        cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                this.roots
                    .insert(dir, found.as_ref().map(|h| h.root.clone()));
                let Some(found) = found.filter(|_| n == this.place_read) else {
                    return;
                };
                match this.head.as_ref().is_some_and(|h| h.root == found.root) {
                    true => {
                        this.place = None;
                        this.head = Some(found);
                    }
                    false => this.place = Some(found),
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// A turn ended: the branch may have changed; read it again where the agent works.
    fn reread_branch(&mut self, cx: &mut Context<Self>) {
        let Some(root) = self
            .place
            .as_ref()
            .or(self.head.as_ref())
            .map(|h| h.root.clone())
        else {
            return;
        };
        let found = cx.background_spawn(async move { crate::git::head(&root) });
        cx.spawn(async move |this, cx| {
            let Some(found) = found.await else { return };
            this.update(cx, |this, cx| {
                for slot in [&mut this.place, &mut this.head] {
                    if slot.as_ref().is_some_and(|h| h.root == found.root) {
                        *slot = Some(found.clone());
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
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

    /// When the running turn started, for the sidebar.
    pub fn turn_started(&self) -> Option<Instant> {
        self.turn.as_ref().map(|(at, ..)| *at)
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
            self.since = Some(Instant::now());
            cx.emit(ChatEvent::StateChanged);
            self.sync_clock(cx);
        }
        if self.model.title != title {
            cx.emit(ChatEvent::TitleChanged);
        }
        result
    }

    /// The status line's clock: ticks at `tick_period` while a turn runs, restarting only when
    /// the period changes; no timer otherwise.
    fn sync_clock(&mut self, cx: &mut Context<Self>) {
        let Some(period) = tick_period(&self.model.state) else {
            self.turn = None;
            return;
        };
        if matches!(&self.turn, Some((_, current, _)) if *current == period) {
            return;
        }
        let started = self.turn.as_ref().map_or_else(Instant::now, |(at, ..)| *at);
        let tick = cx.spawn(async move |this, cx| {
            loop {
                let wait = until_next_tick(period, EPOCH.elapsed());
                cx.background_executor().timer(wait).await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        });
        self.turn = Some((started, period, tick));
    }

    /// Appends new rows to the list and remeasures `changed`.
    pub(super) fn sync_rows(&mut self, changed: Option<usize>, cx: &mut Context<Self>) {
        let model = &self.model;
        let rows = model.items.len();
        self.scroller.update(cx, |list, cx| {
            let known = list.item_count();
            if rows > known {
                list.append(rows - known, cx);
                // The card's last row may stop being last: its edges change.
                if known > 0 && model.items[known - 1].in_card() {
                    let run = model.run(known - 1, ChatItem::in_card);
                    list.remeasure_items(run.start..known, cx);
                }
            }
            if let Some(ix) = changed.filter(|ix| *ix < known) {
                // A changed row can change its card's edges, and the header moves with them:
                // remeasure from its card to the end of its turn.
                let (start, end) = match model.items[ix].in_card() {
                    true => {
                        let turn = (model.items[ix..].iter())
                            .position(|i| {
                                matches!(i, ChatItem::User { .. } | ChatItem::Steered { .. })
                            })
                            .map_or(rows, |n| ix + n);
                        (model.run(ix, ChatItem::in_card).start, turn)
                    }
                    false => (ix, ix + 1),
                };
                list.remeasure_items(start..end.min(known), cx);
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
                // A subagent's answer is shown as Markdown in its drawer.
                if tool.kind == "subagent" && !tool.text.is_empty() {
                    let text = &tool.text;
                    match self.texts.get_mut(&ix) {
                        Some(md) if text.len() >= md.pushed && text.is_char_boundary(md.pushed) => {
                            let delta = &text[md.pushed..];
                            md.state.update(cx, |state, cx| state.push_str(delta, cx));
                            md.pushed = text.len();
                        }
                        _ => {
                            let state =
                                cx.new(|cx| TextViewState::markdown(text, cx).selectable(true));
                            let pushed = text.len();
                            self.texts.insert(ix, Markdown { state, pushed });
                        }
                    }
                }
            }
            ChatItem::Permission { .. } => {}
            ChatItem::User { .. }
            | ChatItem::Steered { .. }
            | ChatItem::Question { .. }
            | ChatItem::Notice { .. }
            | ChatItem::Edits { .. } => {}
        }
    }

    /// R5: the request's own diff, else the diffs of its tool call.
    fn permission_diffs(&self, request: &PermissionRequest) -> Vec<DiffView> {
        let tool = self.model.items.iter().rev().find_map(|item| match item {
            ChatItem::Tool(t) if t.id == request.tool_call_id => Some(t),
            _ => None,
        });
        let locations = tool.map_or(&[][..], |t| &t.locations[..]);
        let diffs = match &request.diff {
            Some(diff) => std::slice::from_ref(diff),
            None => tool.map_or(&[][..], |t| &t.diffs[..]),
        };
        diff_views(diffs, locations, &self.cwd)
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
        self.card_diffs.remove(&request);
        self.sync_rows(None, cx);
        cx.notify();
    }

    pub(crate) fn set_mode(&mut self, mode: String, cx: &mut Context<Self>) {
        let previous = self.model.current_mode.replace(mode.clone());
        let (handle, id) = (self.handle.clone(), self.id.clone());
        self.settling += 1;
        cx.spawn(async move |this, cx| {
            let set = handle.set_mode(id, mode).await;
            this.update(cx, |this, cx| {
                this.settling -= 1;
                if let Err(e) = set {
                    this.model.current_mode = previous;
                    this.model
                        .notice(format!("Could not switch mode: {e}"), NoticeKind::Error);
                    this.sync_rows(None, cx);
                }
                this.send_first(cx);
                cx.notify();
            })
            .ok();
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
        self.bound = next_binding();
        cx.emit(ChatEvent::Bound);
        self.model.items.clear();
        self.model.subagents.clear();
        self.model.waiting.clear();
        self.model.steering.clear();
        self.model.usage = None;
        self.texts.clear();
        self.diffs.clear();
        self.card_diffs.clear();
        self.open_thoughts.clear();
        self.open_steps.clear();
        self.open_groups.clear();
        self.questions.clear();
        self.model.subagents.clear();
        self.drawer = None;
        self.agent_cursor = None;
        self.scroller.update(cx, |list, cx| list.reset(0, cx));
        self.update_model(cx, |m| m.state = TurnState::Working);
        // Loading is no change of the session's own: its row shows no time until one.
        self.since = None;
        self.loading = true;
        let (id, cwd, registry) = (self.id.clone(), self.cwd.clone(), self.registry.clone());
        cx.spawn_in(window, async move |this, cx| {
            let loaded = handle.load_session(id, cwd, mcp).await;
            this.update_in(cx, |this, window, cx| {
                this.loading = false;
                if let Some(dir) = this.replayed.take() {
                    this.follow(&dir, cx);
                }
                let agent = this.agent.clone();
                match loaded {
                    Ok(started) => {
                        registry.update(cx, |r, cx| r.mark_ready(&agent, cx)).ok();
                        this.take_started(started, window, cx);
                        this.update_model(cx, |m| m.state = TurnState::Idle);
                        this.since = None;
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

/// The clock every session's ticks align to, so they all land in the same frame.
static EPOCH: LazyLock<Instant> = LazyLock::new(Instant::now);

/// The wait to the next multiple of `period` on the `EPOCH` clock, `since` being its reading.
fn until_next_tick(period: Duration, since: Duration) -> Duration {
    let period = period.as_nanos().max(1);
    Duration::from_nanos((period - since.as_nanos() % period) as u64)
}

/// How often the status line's clock ticks: fast enough to turn its ring while the agent works,
/// once a second while a card waits for you, never when no turn runs.
pub(super) fn tick_period(state: &TurnState) -> Option<Duration> {
    match state {
        TurnState::Working => Some(Duration::from_millis(125)),
        TurnState::NeedsYou => Some(Duration::from_secs(1)),
        _ => None,
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
    use std::time::{Duration, Instant};

    use super::{tick_period, until_next_tick};
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{
        ElementId, Entity, InputEvent, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent,
        TestAppContext, VisualTestContext, point, px, size,
    };
    use wai_agent::{
        ChoiceKind, Location, PermissionChoice, PermissionRequest, SessionEvent, StopReason,
        SubagentPatch, SubagentState, ToolCallView, ToolStatus,
    };
    use wai_settings::{AppState, Change, OpenSession, Processing, Settings};

    use super::ChatSession;
    use crate::agents::RegistryEvent;
    use crate::chat::input::{Attachment, Message};
    use crate::chat::model::{
        ChatItem, EditedFile, NoticeKind, Subagent, TurnState, UNSUPPORTED_QUESTION,
    };
    use crate::git::tests::{git, repo_with, submodule_worktree};
    use crate::notify;
    use crate::sidebar::canonical;
    use crate::workspace::tests::{
        click, fake_live, fake_settings, history_settings, launch, live, load_history, open,
        open_at, reveal, start_now, wait_until,
    };
    use crate::workspace::{Pane, Workspace};

    /// The session that lands in pane 0.
    fn start_session(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ChatSession> {
        start_now(workspace, cx)
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
    /// once the turn reads it, the turn's reply takes it in, and no second prompt goes out.
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

    /// A message steered while a card waits stays pinned until the agent reads it, then joins
    /// the chat after the answered card and before the reply to it.
    #[gpui_kit::test]
    fn steered_message_stays_pinned_until_read(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--steering --ask-permission"));
        let session = start_session(&workspace, cx);
        send("edit", cx);
        wait_until(cx, "the permission card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
        });
        send("also", cx);
        wait_until(cx, "the steer taken", |cx| {
            session.read_with(cx, |s, _| {
                s.model.steering.first().is_some_and(|s| s.injected)
            })
        });
        let also = ChatItem::Steered {
            text: "also".into(),
        };
        assert!(cx.update(|w, cx| {
            w.render_frame(cx);
            w.within("waiting").try_find(("steering", 0usize)).is_some()
        }));
        assert!(!session.read_with(cx, |s, _| s.model.items.contains(&also)));
        click_edits(None, "perm-allow_once", cx);
        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        let items = session.read_with(cx, |s, _| s.model.items.clone());
        let at = |f: &dyn Fn(&ChatItem) -> bool| items.iter().position(f).unwrap();
        let card = at(&|i| matches!(i, ChatItem::Permission { .. }));
        let steer = at(&|i| *i == also);
        let reply = at(&|i| matches!(i, ChatItem::Agent { .. }));
        assert!(card < steer && steer < reply, "{items:?}");
        assert_eq!(
            items[reply],
            ChatItem::Agent {
                text: "chose:allow-once Steered: also".into()
            }
        );
        assert!(!shows(("steering", 0usize), cx));
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
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(
                window
                    .within("waiting")
                    .try_find(("queued", 2usize))
                    .is_some()
            );
            let (waiting, input) = (
                window.find("waiting").bounds(),
                window.within("prompt-box").find("input-state").bounds(),
            );
            assert!(waiting.bottom() <= input.top());
        });
        click_edits(None, ("queued-remove", 1usize), cx);
        cx.update(|window, cx| window.render_frame(cx));
        assert_eq!(typed(&session, cx), "second", "back in the prompt box");
        assert!(!shows(("queued", 2usize), cx));
        assert!(shows(("queued", 1usize), cx));
        // Review focus 2: a long message shows at most 3 lines.
        let long: Vec<String> = (1..=10).map(|i| format!("line {i}")).collect();
        session.update(cx, |s, cx| {
            s.queued.push(Message {
                text: long.join("\n"),
                attachments: Vec::new(),
            });
            cx.notify()
        });
        let height = |ix: usize, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.find(("queued", ix)).bounds().size.height
            })
        };
        assert!(
            height(2, cx) < height(0, cx) * 3.,
            "the long balloon is not clamped"
        );
        session.update(cx, |s, cx| {
            s.queued.pop();
            cx.notify()
        });

        wait_until(cx, "the queued prompt's reply", |cx| {
            state(&session, cx) == TurnState::Idle && user_texts(&session, cx).len() == 2
        });
        assert_eq!(user_texts(&session, cx), ["go", "first\n\nthird"]);
        assert!(!shows("waiting", cx));
        assert_eq!(typed(&session, cx), "second");
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

    /// A subagent shows as one card (kind, task, pill); its tool call leaves the transcript and
    /// shows, with the answer, in its drawer; its plan does not replace the session's.
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
        let row = |id: &str, cx: &mut VisualTestContext| {
            session.read_with(cx, |s, _| {
                s.model
                    .items
                    .iter()
                    .enumerate()
                    .find_map(|(ix, item)| match item {
                        ChatItem::Tool(t) if t.id == id => Some((ix, t.clone())),
                        _ => None,
                    })
            })
        };
        let (card_ix, card) = row(&sub, cx).expect("the subagent's card");
        assert_eq!(card.status, ToolStatus::Completed);
        assert_eq!(card.title, "Explore the code");
        assert_eq!(card.text, "found it");
        let (read_ix, read) = row("sub-read", cx).expect("the subagent's call");
        assert_eq!(read.subagent.as_deref(), Some(sub.as_str()));
        assert_eq!(session.read_with(cx, |s, _| s.model.plan.len()), 3);
        assert_eq!(
            last_agent_text(&session, cx).as_deref(),
            Some("Hello from fake agent")
        );

        assert!(shows(("subagent-card", card_ix), cx));
        assert!(
            !shows(("tool", read_ix), cx),
            "the call left the transcript"
        );
        assert!(!shows("subagent-drawer", cx));
        click_edits(None, ("subagent-card", card_ix), cx);
        assert_eq!(agents_at(&session, cx), (None, Some(sub)));
        assert!(shows("subagent-drawer", cx));
        cx.update(|window, cx| {
            window.render_frame(cx);
            let drawer = window.within("subagent-drawer");
            assert!(
                drawer.try_find(("tool", read_ix)).is_some(),
                "the call's line"
            );
            assert!(drawer.try_find("subagent-answer").is_some(), "the answer");
        });
    }

    /// With the drawer open, Esc closes it and the turn runs on; the next Esc stops the turn.
    #[gpui_kit::test]
    fn esc_closes_the_drawer_not_the_turn(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(
            cx,
            fake_settings("--subagent --background --chunk-delay-ms 300"),
        );
        let session = start_session(&workspace, cx);
        send("go", cx);
        wait_until(cx, "the subagent", |cx| {
            session.read_with(cx, |s, _| !s.model.subagents.is_empty())
        });
        click_edits(None, ("agent-row", 0usize), cx);
        assert!(shows("subagent-drawer", cx));
        assert_eq!(state(&session, cx), TurnState::Working);

        cx.simulate_keystrokes("escape");
        assert!(!shows("subagent-drawer", cx));
        assert_eq!(state(&session, cx), TurnState::Working);

        cx.simulate_keystrokes("escape");
        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle
        });
    }

    /// Review focus 3: a restart clears the subagents, since the agent re-announces its own.
    #[gpui_kit::test]
    fn restart_clears_the_subagents(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent"));
        let session = start_session(&workspace, cx);
        send("hi", cx);
        wait_until(cx, "the subagent", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle && !s.model.subagents.is_empty()
            })
        });
        cx.update(|window, cx| session.update(cx, |s, cx| s.load(false, window, cx)));
        wait_until(cx, "the restart", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle && s.model.subagents.is_empty()
            })
        });
    }

    /// Review focus 1: the drawer closes with its subject: another session shown in the pane,
    /// the subagent gone from the model, the session ended.
    #[gpui_kit::test]
    fn drawer_closes_when_its_subject_goes(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent"));
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        let sub = session.read_with(cx, |s, _| format!("{}-sub", s.id));
        let open_it = |cx: &mut VisualTestContext| {
            session.update(cx, |s, cx| s.open_drawer(Some(sub.clone()), cx));
            assert!(shows("subagent-drawer", cx));
        };

        open_it(cx);
        let other = start_session(&workspace, cx);
        assert_eq!(agents_at(&session, cx), (None, None), "(a) another session");
        assert!(!other.read_with(cx, |s, _| s.drawer.is_some()));
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.show(session.clone(), false, window, cx))
        });

        open_it(cx);
        session.update(cx, |s, _| s.model.subagents.clear());
        assert!(!shows("subagent-drawer", cx), "(b) no subagent");
        assert_eq!(agents_at(&session, cx), (None, None));

        let ix = workspace.read_with(cx, |w, _| w.sessions.iter().position(|s| s == &session));
        reveal(("open-close", ix.unwrap()), cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(("open-close", ix.unwrap()), cx);
        });
        cx.run_until_parked();
        assert!(
            workspace.read_with(cx, |w, _| !w.sessions.contains(&session)),
            "(c) ended"
        );
    }

    /// A draft shown over a session closes that session's drawer, as another session does.
    #[gpui_kit::test]
    fn a_draft_shown_over_a_session_closes_its_drawer(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        cx.simulate_keystrokes("secondary-n");
        let draft = workspace.read_with(cx, |w, _| w.drafts[0].clone());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.show(session.clone(), false, window, cx))
        });
        session.update(cx, |s, cx| {
            let started = Instant::now();
            s.model.subagents.push(Subagent {
                id: "old".into(),
                title: "Earlier task".into(),
                kind: None,
                state: SubagentState::Done,
                used: None,
                started,
                ended: Some(started),
                turn: s.model.turn,
                said: Vec::new(),
                plan: Vec::new(),
                can_stop: false,
            });
            s.open_drawer(Some("old".into()), cx);
        });
        assert!(shows("subagent-drawer", cx));

        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.show_draft(draft, false, window, cx))
        });
        assert_eq!(agents_at(&session, cx), (None, None));
    }

    /// The agent's exit parks a live subagent: its clock stops and it shows stopped, not running.
    /// A Restart starts over with no subagent, drawer or list cursor.
    #[gpui_kit::test]
    fn exit_parks_subagents_and_restart_clears_them(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        session.update(cx, |s, cx| {
            let patch = SubagentPatch {
                id: "bg".into(),
                title: Some("Background task".into()),
                state: Some(SubagentState::Running),
                ..Default::default()
            };
            s.handle_event(&SessionEvent::Subagent(patch), cx);
            s.open_drawer(Some("bg".into()), cx);
            s.agent_cursor = Some(1);
        });
        session.update(cx, |s, cx| s.handle_agent_exited("gone", cx));
        let ended = session.read_with(cx, |s, _| s.model.subagents[0].ended.is_some());
        assert!(ended, "the clock stops");
        let label = cx.update(|window, cx| {
            window.render_frame(cx);
            let row = window.find(("agent-row", 0usize));
            row.label().map(str::to_owned).unwrap_or_default()
        });
        assert!(label.contains("stopped"), "{label}");

        cx.update(|window, cx| session.update(cx, |s, cx| s.load(false, window, cx)));
        assert!(session.read_with(cx, |s, _| s.model.subagents.is_empty()));
        assert_eq!(agents_at(&session, cx), (None, None));
    }

    /// A permission card stays above the drawer and answerable.
    #[gpui_kit::test]
    fn waiting_cards_stay_above_the_drawer(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent --ask-permission"));
        let session = start_session(&workspace, cx);
        send("edit", cx);
        wait_until(cx, "the permission card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
                && session.read_with(cx, |s, _| !s.model.subagents.is_empty())
        });
        click_edits(None, ("agent-row", 0usize), cx);
        assert!(shows("subagent-drawer", cx));
        cx.update(|window, cx| {
            window.render_frame(cx);
            let (waiting, drawer) = (
                window.find("waiting").bounds(),
                window.find("subagent-drawer").bounds(),
            );
            assert!(
                drawer.bottom() <= waiting.top() + px(1.),
                "{drawer:?} {waiting:?}"
            );
            window.click("perm-allow_once", cx);
        });
        wait_until(cx, "the answer", |cx| {
            state(&session, cx) != TurnState::NeedsYou
        });
    }

    /// In a short pane a tall waiting card leaves the drawer only the room above it: the drawer
    /// never rises over the pane's header.
    #[gpui_kit::test]
    fn drawer_fits_above_a_tall_waiting_card(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent --ask-permission"));
        let session = start_session(&workspace, cx);
        cx.simulate_resize(size(px(900.), px(500.)));
        send("edit", cx);
        wait_until(cx, "the permission card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
                && session.read_with(cx, |s, _| !s.model.subagents.is_empty())
        });
        click_edits(None, ("agent-row", 0usize), cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            let (transcript, waiting, drawer) = (
                window.find("transcript").bounds(),
                window.find("waiting").bounds(),
                window.find("subagent-drawer").bounds(),
            );
            assert!(
                drawer.top() >= transcript.top(),
                "{drawer:?} {transcript:?} {waiting:?}"
            );
            assert!(drawer.bottom() <= waiting.top() + px(1.));
            assert!(
                drawer.size.height > px(0.),
                "the drawer still shows: {drawer:?}"
            );
        });
    }

    /// The agent list under the prompt: not drawn until a subagent shows, then `main` and a row
    /// per subagent, named by its kind, with its title as the task.
    #[gpui_kit::test]
    fn agent_list_shows_main_and_subagents(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent"));
        let session = start_session(&workspace, cx);
        assert!(!shows("agent-list", cx));

        turn(&session, cx);
        assert!(shows("agent-main", cx));
        assert!(!shows("agent-hint", cx));
        let label = cx.update(|window, cx| {
            window.render_frame(cx);
            window
                .find(("agent-row", 0usize))
                .label()
                .map(str::to_owned)
        });
        let label = label.unwrap_or_default();
        assert!(
            label.starts_with("Explore") && label.contains("Explore the code"),
            "{label}"
        );
    }

    /// Review focus 2: past six rows the list keeps six rows' height and scrolls.
    #[gpui_kit::test]
    fn agent_list_caps_at_six_rows(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        session.update(cx, |s, cx| {
            for i in 0..8 {
                s.model.subagents.push(Subagent {
                    id: format!("s{i}"),
                    title: format!("Task {i}"),
                    kind: Some("Explore".into()),
                    state: SubagentState::Running,
                    used: None,
                    started: Instant::now(),
                    ended: None,
                    turn: 0,
                    said: Vec::new(),
                    plan: Vec::new(),
                    can_stop: false,
                });
            }
            cx.notify();
        });
        let bounds = |id: ElementId, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.find(id).bounds()
            })
        };
        let list = bounds("agent-list".into(), cx);
        let (first, sixth) = (
            bounds("agent-main".into(), cx),
            bounds(("agent-row", 4usize).into(), cx),
        );
        assert!(
            (list.top() - first.top()).abs() < px(0.5),
            "{list:?} {first:?}"
        );
        assert!(
            (list.bottom() - sixth.bottom()).abs() < px(0.5),
            "{list:?} {sixth:?}"
        );
        let last = ElementId::from(("agent-row", 7usize));
        assert!(bounds(last.clone(), cx).top() >= list.bottom());
        cx.update(|window, cx| {
            let delta = gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-400.)));
            window.scroll("agent-list", delta, cx);
        });
        let row = bounds(last, cx);
        assert!(
            row.bottom() <= list.bottom() + px(0.5) && row.top() >= list.top(),
            "the list did not scroll: {row:?} {list:?}"
        );
    }

    fn agents_at(
        session: &Entity<ChatSession>,
        cx: &mut VisualTestContext,
    ) -> (Option<usize>, Option<String>) {
        session.read_with(cx, |s, _| (s.agent_cursor, s.drawer.clone()))
    }

    /// From an empty prompt ↓ enters the list at `main`, ↓/↑ move, Enter opens the row's drawer
    /// without sending, ↑ from `main` goes back to the prompt and Esc leaves the list; with text
    /// in the prompt ↓ stays in it.
    #[gpui_kit::test]
    fn arrow_down_enters_the_list(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent"));
        let session = start_session(&workspace, cx);
        let sub = session.read_with(cx, |s, _| format!("{}-sub", s.id));
        turn(&session, cx);

        cx.simulate_keystrokes("down");
        assert_eq!(agents_at(&session, cx), (Some(0), None));
        assert!(shows("agent-hint", cx));
        cx.simulate_keystrokes("down down");
        assert_eq!(
            agents_at(&session, cx),
            (Some(1), None),
            "past the last row"
        );
        cx.simulate_keystrokes("enter");
        assert_eq!(agents_at(&session, cx), (Some(1), Some(sub.clone())));
        assert_eq!(user_texts(&session, cx), ["go"], "Enter sent a prompt");
        cx.simulate_keystrokes("up up");
        assert_eq!(agents_at(&session, cx), (None, Some(sub)));
        assert!(!shows("agent-hint", cx));
        cx.simulate_keystrokes("escape");
        assert_eq!(agents_at(&session, cx), (None, None));

        cx.simulate_input("x");
        cx.simulate_keystrokes("down");
        assert_eq!(agents_at(&session, cx), (None, None));
        assert_eq!(typed(&session, cx), "x");
    }

    /// A click on a subagent's row opens its drawer, a second click closes it, and so does a
    /// click on `main`.
    #[gpui_kit::test]
    fn clicks_open_and_close_the_drawer(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--subagent"));
        let session = start_session(&workspace, cx);
        let sub = session.read_with(cx, |s, _| format!("{}-sub", s.id));
        turn(&session, cx);
        let row = ("agent-row", 0usize);

        click_edits(None, row, cx);
        assert_eq!(agents_at(&session, cx), (None, Some(sub.clone())));
        click_edits(None, row, cx);
        assert_eq!(agents_at(&session, cx), (None, None));
        click_edits(None, row, cx);
        assert_eq!(agents_at(&session, cx), (None, Some(sub)));
        click_edits(None, "agent-main", cx);
        assert_eq!(agents_at(&session, cx), (None, None));
    }

    /// A drawer still showing (its subagent is in the model, though from an earlier turn) is closed by
    /// the first Esc, which keeps the turn; the second stops it.
    #[gpui_kit::test]
    fn escape_closes_a_visible_drawer_then_stops_the_turn(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(&slow_reply(200, 20)));
        let session = start_session(&workspace, cx);
        session.update(cx, |s, cx| {
            let started = Instant::now();
            s.model.subagents.push(Subagent {
                id: "old".into(),
                title: "Earlier task".into(),
                kind: Some("Explore".into()),
                state: SubagentState::Done,
                used: None,
                started,
                ended: Some(started),
                turn: s.model.turn,
                said: Vec::new(),
                plan: Vec::new(),
                can_stop: false,
            });
            s.open_drawer(Some("old".into()), cx);
        });

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        cx.simulate_keystrokes("escape");
        assert_eq!(agents_at(&session, cx), (None, None));
        assert_eq!(
            state(&session, cx),
            TurnState::Working,
            "the first Esc kept the turn"
        );
        cx.simulate_keystrokes("escape");
        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        let cancelled = ChatItem::Notice {
            text: "Cancelled".into(),
            kind: NoticeKind::Info,
        };
        let last = session.read_with(cx, |s, _| s.model.items.last().cloned());
        assert_eq!(last, Some(cancelled), "the second Esc stops the turn");
        assert_eq!(agents_at(&session, cx), (None, None));
    }

    /// Esc in the agent list only leaves it: the turn runs on to its end.
    #[gpui_kit::test]
    fn escape_leaves_the_agent_list_before_stopping(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let flags = format!("--subagent {}", slow_reply(20, 50));
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);

        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        cx.simulate_keystrokes("down");
        assert_eq!(agents_at(&session, cx), (Some(0), None));
        cx.simulate_keystrokes("escape");
        assert_eq!(agents_at(&session, cx), (None, None));
        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        let words: Vec<String> = (0..20).map(|i| format!("w{i}")).collect();
        assert_eq!(last_agent_text(&session, cx), Some(words.join(" ")));
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
        assert!(session.read_with(cx, |s, _| {
            !s.model
                .items
                .iter()
                .any(|i| matches!(i, ChatItem::Permission { .. }))
        }));
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(
                window
                    .within("waiting")
                    .try_find("perm-allow_once")
                    .is_some(),
                "the card is pinned"
            );
            let input = window.within("prompt-box").find("input-state").bounds();
            let bounds = |id: &'static str| window.find(id).bounds();
            let (waiting, list) = (bounds("waiting"), bounds("transcript"));
            // O1: the stack lies over the list's bottom, inside its area, not below it.
            assert!(
                list.top() < waiting.top() && waiting.bottom() <= list.bottom(),
                "{waiting:?} is not over {list:?}"
            );
            assert!(
                list.bottom() - waiting.bottom() <= px(9.),
                "{waiting:?} does not sit at the list's bottom"
            );
            assert!(
                waiting.bottom() <= input.top(),
                "{waiting:?} is not above {input:?}"
            );
        });
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click("perm-allow_once", cx);
        });

        wait_until(cx, "the answer", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        let reply = last_agent_text(&session, cx).unwrap_or_default();
        assert!(reply.starts_with("chose:"), "{reply}");
        let items = session.read_with(cx, |s, _| s.model.items.clone());
        let card = items
            .iter()
            .position(|i| matches!(i, ChatItem::Permission { .. }))
            .unwrap();
        assert!(matches!(&items[card + 1], ChatItem::Agent { text } if text.starts_with("chose:")));
        assert!(shows(("permission", card), cx), "the closed line");
        assert!(!shows("waiting", cx));
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

    /// O1: while a card waits, the list's last row scrolls clear of the stack (the list's bottom
    /// inset follows the stack's height); once nothing waits, the inset goes.
    #[gpui_kit::test]
    fn list_scrolls_clear_of_the_pinned_stack(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let reply = dir.path().join("long.md");
        let lines: Vec<String> = (1..=80).map(|i| format!("line {i}")).collect();
        fs::write(&reply, lines.join("\n\n")).unwrap();
        // With --ask-permission the reply streams before the tool call: the list overflows.
        let flags = format!("--ask-permission --reply-file {}", wai_agent::quote(&reply));
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);
        send("edit", cx);
        wait_until(cx, "the permission card", |cx| {
            state(&session, cx) == TurnState::NeedsYou
        });
        // The canvas reports the height in prepaint: the inset lands on the next frame.
        let frame = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.render_frame(cx);
            });
        };
        frame(cx);
        let last = session.read_with(cx, |s, _| s.model.items.len() - 1); // the edit's tool row
        let clear = |cx: &mut VisualTestContext| {
            cx.update(|window, _| {
                let (row, stack) = (
                    window.find(("tool", last)).bounds(),
                    window.find("waiting").bounds(),
                );
                (row.bottom() <= stack.top() + px(0.5), stack.size.height)
            })
        };
        let height = session.read_with(cx, |s, _| s.waiting_height);
        let (is_clear, stack) = clear(cx);
        assert!(
            (height - stack).abs() < px(0.5),
            "inset {height:?} vs stack {stack:?}"
        );
        assert!(is_clear, "at the tail, the last row sits above the stack");
        // Scrolled up and back: the last row is still reachable, clear of the stack.
        cx.update(|window, cx| {
            let delta = gpui_kit::ScrollDelta::Pixels(gpui_kit::point(px(0.), px(600.)));
            window.scroll("transcript", delta, cx);
        });
        session.update(cx, |s, cx| {
            s.scroller.update(cx, |list, cx| list.scroll_to_end(cx))
        });
        frame(cx);
        assert!(clear(cx).0, "the last row scrolls clear of the stack");
        // Nothing waits: no inset.
        click_edits(None, "perm-allow_once", cx);
        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle
        });
        frame(cx);
        assert!(!shows("waiting", cx));
        assert_eq!(session.read_with(cx, |s, _| s.waiting_height), px(0.));
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
        assert!(cx.update(|w, cx| {
            w.render_frame(cx);
            w.within("waiting").try_find("question-submit").is_some()
        }));
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

    /// Each agent message copies its Markdown; each code block, its code.
    #[gpui_kit::test]
    fn copy_buttons_copy_the_message_and_its_code(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let reply = dir.path().join("reply.md");
        fs::write(&reply, "Run this:\n\n```sh\ncargo test\n```\n").unwrap();
        let flags = format!("--reply-file {}", wai_agent::quote(&reply));
        let (workspace, cx) = open(cx, fake_settings(&flags));
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        wait_until(cx, "the markdown", |cx| {
            session.read_with(cx, |s, cx| {
                s.texts.get(&1).is_some_and(|md| {
                    md.state
                        .read(cx)
                        .rendered_text()
                        .as_str()
                        .contains("cargo test")
                })
            })
        });
        let message = last_agent_text(&session, cx).unwrap();
        click_edits(None, ("copy-message", 1usize), cx);
        let copied = cx.read_from_clipboard().and_then(|c| c.text());
        assert_eq!(copied.unwrap(), message);
        click_edits(None, "copy-code", cx);
        let code = cx.read_from_clipboard().and_then(|c| c.text());
        assert_eq!(code.unwrap().trim_end(), "cargo test");
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

    /// Every card segment of a turn touches the next.
    fn assert_touching(session: &Entity<ChatSession>, cx: &mut VisualTestContext) {
        let last = session.read_with(cx, |s, _| s.model.items.len() - 1);
        for k in 1..last {
            let (a, b) = (card_bounds(k, cx), card_bounds(k + 1, cx));
            assert!((b.top() - a.bottom()).abs() < px(0.5), "{k}: {a:?} {b:?}");
        }
    }

    #[test]
    fn tick_period_follows_the_state() {
        assert_eq!(
            tick_period(&TurnState::Working),
            Some(Duration::from_millis(125))
        );
        assert_eq!(
            tick_period(&TurnState::NeedsYou),
            Some(Duration::from_secs(1))
        );
        assert_eq!(tick_period(&TurnState::Idle), None);
        assert_eq!(tick_period(&TurnState::Error(String::new())), None);
    }

    /// The clock switches period with the state and keeps the turn's start.
    #[gpui_kit::test]
    fn clock_follows_the_state_and_keeps_its_start(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        let step = |state: TurnState, cx: &mut VisualTestContext| {
            session.update(cx, |s, cx| {
                s.model.state = state;
                s.sync_clock(cx);
                s.turn.as_ref().map(|(at, period, _)| (*at, *period))
            })
        };
        let (at, period) = step(TurnState::Working, cx).unwrap();
        assert_eq!(period, Duration::from_millis(125));
        let (waiting_at, period) = step(TurnState::NeedsYou, cx).unwrap();
        assert_eq!((waiting_at, period), (at, Duration::from_secs(1)));
        let (again_at, period) = step(TurnState::Working, cx).unwrap();
        assert_eq!((again_at, period), (at, Duration::from_millis(125)));
        assert!(step(TurnState::Idle, cx).is_none());
    }

    #[test]
    fn ticks_align_to_the_period() {
        let p = Duration::from_millis(125);
        let wait = |ms| until_next_tick(p, Duration::from_millis(ms));
        assert_eq!(wait(0), p);
        assert_eq!(wait(100), Duration::from_millis(25));
        assert_eq!(wait(125), p);
    }

    /// A turn is one card: its segments touch, and only its first row carries the header.
    #[gpui_kit::test]
    fn turn_is_one_card(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--steps 2"));
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        let last = session.read_with(cx, |s, _| s.model.items.len() - 1);
        assert!(shows(("reply-head", 1usize), cx));
        assert!(!shows(("reply-head", last), cx));
        assert_touching(&session, cx);
    }

    /// Hidden, the reply's text is the card's first segment and carries the header.
    #[gpui_kit::test]
    fn hidden_steps_leave_the_text_as_the_card(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let settings = processing_settings("--steps 2", Processing::Hidden);
        let (workspace, cx) = open(cx, settings);
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        let last = session.read_with(cx, |s, _| s.model.items.len() - 1);
        assert!(shows(("reply-head", last), cx));
        assert!(!shows(("card", 2usize), cx));
    }

    /// Grouped, opening the group keeps the segments touching.
    #[gpui_kit::test]
    fn grouped_card_stays_contiguous(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let settings = processing_settings("--steps 2", Processing::Grouped);
        let (workspace, cx) = open(cx, settings);
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        let last = session.read_with(cx, |s, _| s.model.items.len() - 1);
        assert!(shows(("reply-head", 1usize), cx));
        assert!(!shows(("reply-head", last), cx));
        assert!(!shows(("card", 2usize), cx), "folded: only the head");
        let (head, text) = (card_bounds(1, cx), card_bounds(last, cx));
        assert!(
            (text.top() - head.bottom()).abs() < px(0.5),
            "{head:?} {text:?}"
        );
        click_edits(None, ("group", 1usize), cx);
        assert_touching(&session, cx);
    }

    fn card_bounds(ix: usize, cx: &mut VisualTestContext) -> gpui_kit::Bounds<gpui_kit::Pixels> {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(("card", ix)).bounds()
        })
    }

    /// A step is one compact line of its own, touching the next; unfolded, its output sits in a
    /// box under the line; a Thought opens the same way.
    #[gpui_kit::test]
    fn step_lines_are_compact_and_open_below(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--steps 2"));
        let session = start_session(&workspace, cx);
        turn(&session, cx);
        let bounds = |id: (&'static str, usize), cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.find(id).bounds()
            })
        };
        let (tool, line, next) = (
            bounds(("tool", 2), cx),
            bounds(("step", 2), cx),
            bounds(("step", 3), cx),
        );
        assert_eq!(line.top(), tool.top(), "the line is the step's own box");
        assert!(
            (next.top() - line.bottom()).abs() < px(0.5),
            "{line:?} {next:?}"
        );
        click_edits(None, ("step", 2usize), cx);
        let (line, output) = (bounds(("step", 2), cx), bounds(("step-output", 2), cx));
        let gap = cx.update(|window, _| window.rem_size() * 0.25); // `gap_1`
        assert!(
            (output.top() - line.bottom() - gap).abs() < px(0.5),
            "{output:?} is not {gap:?} under {line:?}"
        );
        click_edits(None, ("thought", 1usize), cx);
        let (line, output) = (
            bounds(("thought", 1), cx),
            bounds(("thought-output", 1), cx),
        );
        assert!(
            (output.top() - line.bottom() - gap).abs() < px(0.5),
            "{output:?} {line:?}"
        );
    }

    /// D9: an × drawn before the queue changed does nothing (two clicks in one frame).
    #[gpui_kit::test]
    fn stale_queue_clicks_do_nothing(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(&slow_reply(200, 20)));
        let session = start_session(&workspace, cx);
        send("go", cx);
        wait_until(cx, "the first chunk", |cx| {
            last_agent_text(&session, cx).is_some()
        });
        for message in ["first", "second", "third"] {
            send(message, cx);
        }
        // Both clicks land in one frame: `window.click` redraws between them, so send raw events.
        cx.update(|window, cx| {
            window.render_frame(cx);
            let centres: Vec<_> = (0usize..2)
                .map(|i| window.find(("queued-remove", i)).bounds().center())
                .collect();
            for position in centres {
                let none = Modifiers::none();
                for event in [
                    MouseDownEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: none,
                        click_count: 1,
                        first_mouse: false,
                    }
                    .to_platform_input(),
                    MouseUpEvent {
                        button: MouseButton::Left,
                        position,
                        modifiers: none,
                        click_count: 1,
                    }
                    .to_platform_input(),
                ] {
                    window.dispatch_event(event, cx);
                }
            }
        });
        cx.update(|window, cx| window.render_frame(cx));
        let queued = session.read_with(cx, |s, _| {
            s.queued.iter().map(|m| m.text.clone()).collect::<Vec<_>>()
        });
        assert_eq!(queued, ["second", "third"]);
        assert_eq!(typed(&session, cx), "first");
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

    /// Grouped, a run that starts with a subagent's call (shown only in its drawer) is headed by
    /// main's first step, and its label counts and names main's steps only.
    #[gpui_kit::test]
    fn grouped_run_led_by_a_subagent_call_heads_on_main(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, processing_settings("", Processing::Grouped));
        let session = start_session(&workspace, cx);
        let call = |id: &str, title: &str, subagent: Option<&str>| {
            SessionEvent::ToolCall(ToolCallView {
                id: id.into(),
                title: title.into(),
                kind: "read".into(),
                status: ToolStatus::Completed,
                content: vec![],
                locations: vec![],
                subagent: subagent.map(str::to_string),
            })
        };
        session.update(cx, |s, cx| {
            s.handle_event(&call("sub-read", "Read notes.md", Some("bg")), cx);
            s.handle_event(&call("main-read", "Read README.md", None), cx);
        });
        assert!(shows(("group", 0usize), cx), "main's step heads the run");
        let label = session.read_with(cx, |s, _| s.model.run_label(0..2));
        assert_eq!(label, "1 step · Read README.md");
        assert!(!shows(("step", 1usize), cx));
        click_edits(None, ("group", 0usize), cx);
        assert!(shows(("step", 1usize), cx));
        assert!(
            !shows(("step", 0usize), cx),
            "the subagent's call stays out"
        );
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
        // Rows: the prompt, the thought, the Read call, the edit
        // (its card waits above the prompt box).
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
        let (line, time) = cx.update(|window, _| {
            (
                window.find("turn-status").bounds(),
                window.find("turn-status-time").bounds(),
            )
        });
        assert!(
            line.right() - time.right() <= px(13.),
            "the time sits at the line's end"
        );
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

    #[gpui_kit::test]
    fn since_follows_the_state(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = live(cx, fake_settings("--steps 1 --chunk-delay-ms 300"), None);
        let session = start_session(&workspace, cx);
        assert!(session.read_with(cx, |s, _| s.since.is_none()));
        send("go", cx);
        wait_until(cx, "the turn", |cx| {
            session.read_with(cx, |s, _| s.since.is_some())
        });
        let working = session.read_with(cx, |s, _| s.since).unwrap();
        wait_until(cx, "the turn's end", |cx| {
            state(&session, cx) == TurnState::Idle && last_agent_text(&session, cx).is_some()
        });
        assert!(session.read_with(cx, |s, _| s.since).unwrap() > working);
    }

    fn typed(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> String {
        session.read_with(cx, |s, cx| s.input.read(cx).value().to_string())
    }

    /// Review focus 1: with the prompt box empty, 1–9 answer the top card, the oldest; a click
    /// answers the card it is on; with text in the box, a digit types.
    #[gpui_kit::test]
    fn number_keys_answer_the_top_permission_card(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        let request = |id: u64, session: String| PermissionRequest {
            id,
            session,
            tool_call_id: "t".into(),
            title: format!("Run {id}"),
            diff: None,
            subagent: None,
            options: vec![
                PermissionChoice {
                    id: "y".into(),
                    name: "Yes".into(),
                    kind: ChoiceKind::AllowOnce,
                },
                PermissionChoice {
                    id: "n".into(),
                    name: "No".into(),
                    kind: ChoiceKind::RejectOnce,
                },
            ],
        };
        // The broker never asked these: an answer fails, and the card closes as Cancelled.
        let closed = |cx: &mut VisualTestContext| {
            session.read_with(cx, |s, _| {
                (s.model.items.iter())
                    .filter_map(|i| match i {
                        ChatItem::Permission { request, .. } => Some(request.id),
                        _ => None,
                    })
                    .collect::<Vec<_>>()
            })
        };
        session.update(cx, |s, cx| {
            let id = s.id.clone();
            s.handle_permission(request(100, id.clone()), cx);
            s.handle_permission(request(101, id), cx);
        });
        cx.simulate_keystrokes("2");
        assert_eq!(closed(cx), [100]);
        assert_eq!(typed(&session, cx), "");
        cx.update(|window, cx| {
            window.render_frame(cx);
            window
                .within(("waiting-permission", 101u64))
                .click("perm-allow_once", cx);
        });
        assert_eq!(closed(cx), [100, 101]);
        session.update(cx, |s, cx| {
            let id = s.id.clone();
            s.handle_permission(request(102, id), cx)
        });
        cx.simulate_input("a");
        cx.simulate_keystrokes("1");
        assert_eq!(typed(&session, cx), "a1");
        assert_eq!(closed(cx), [100, 101]);
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
            let input = window.within("prompt-box").find("input-state").bounds();
            (window.find("commands").bounds(), input)
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
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.start_session("fake".into(), None, false, None, window, cx)
            })
        });
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
            let pane = window.within(ElementId::View(session.entity_id()));
            let inputs: Vec<_> = (pane.find_all("input-state").iter())
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
        reveal(("open-close", 1usize), cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(("open-close", 1usize), cx);
        });
        click("confirm", cx);

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

    /// `a` loses its process (killed once it has replied); a new process starts `b` with the same id.
    fn two_sessions_one_id(
        cx: &mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<ChatSession>,
        Entity<ChatSession>,
        &mut VisualTestContext,
        tempfile::TempDir,
    ) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let flag = format!("--pid-file '{}'", dir.path().join("agent.pid").display());
        let (workspace, cx) = open(cx, fake_settings(&flag));
        let a = start_session(&workspace, cx);
        a.update(cx, |s, cx| s.send_text("hi", cx));
        wait_until(cx, "a's reply", |cx| {
            last_agent_text(&a, cx).as_deref() == Some("Hello from fake agent")
        });
        let pid = std::fs::read_to_string(dir.path().join("agent.pid")).unwrap();
        // a process this test spawned
        std::process::Command::new("kill")
            .arg(pid.trim())
            .status()
            .unwrap();
        wait_until(cx, "the crash", |cx| {
            matches!(state(&a, cx), TurnState::Error(_))
        });
        let b = start_session(&workspace, cx);
        let ids = (
            a.read_with(cx, |s, _| s.id.clone()),
            b.read_with(cx, |s, _| s.id.clone()),
        );
        assert_eq!(ids, ("fake-1".to_string(), "fake-1".to_string()));
        (workspace, a, b, cx, dir)
    }

    fn notices(session: &Entity<ChatSession>, text: &str, cx: &mut VisualTestContext) -> bool {
        session.read_with(cx, |s, _| {
            (s.model.items.iter())
                .any(|i| matches!(i, ChatItem::Notice { text: t, .. } if t == text))
        })
    }

    #[gpui_kit::test]
    fn an_id_the_agent_reuses_goes_to_its_new_session(cx: &mut TestAppContext) {
        let (_workspace, a, b, cx, _dir) = two_sessions_one_id(cx);
        let count = |cx: &mut VisualTestContext| {
            a.read_with(cx, |s, _| {
                (s.model.items.iter())
                    .filter(|i| matches!(i, ChatItem::Agent { .. }))
                    .count()
            })
        };
        let before = count(cx);
        b.update(cx, |s, cx| s.send_text("go", cx));
        wait_until(cx, "b's reply", |cx| {
            last_agent_text(&b, cx).as_deref() == Some("Hello from fake agent")
        });
        assert_eq!(count(cx), before, "a keeps only its own reply");
        assert!(notices(&a, super::REUSED, cx));
    }

    #[gpui_kit::test]
    fn restart_takes_a_reused_id_back(cx: &mut TestAppContext) {
        let (_workspace, a, b, cx, _dir) = two_sessions_one_id(cx);
        cx.update(|window, cx| a.update(cx, |s, cx| s.load(false, window, cx)));
        wait_until(cx, "the replay", |cx| state(&a, cx) == TurnState::Idle);
        let replay = [
            ChatItem::User {
                text: "previous prompt".into(),
            },
            ChatItem::Agent {
                text: "previous answer".into(),
            },
        ];
        assert_eq!(a.read_with(cx, |s, _| s.model.items.clone()), replay);
        let b_items = b.read_with(cx, |s, _| s.model.items.clone());
        assert!(replay.iter().all(|i| !b_items.contains(i)));
        assert!(notices(&b, super::REUSED, cx));
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
    fn bar_shows_model_effort_and_mode_pickers(cx: &mut TestAppContext) {
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
        let ids = cx.update(|window, cx| {
            window.render_frame(cx);
            ["bar-model", "bar-effort", "bar-mode"].map(|id| window.try_find(id).is_some())
        });
        assert_eq!(ids, [true; 3]);
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
    fn extension_prompt_waits_for_the_default_model(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, models_settings(Some("fake-large")));
        workspace.update_in(cx, |w, window, cx| {
            w.start_session("fake".into(), Some("hi".into()), true, None, window, cx)
        });
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 1)
        });
        let session = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        wait_until(cx, "the reply", |cx| {
            last_agent_text(&session, cx).as_deref() == Some("model:fake-large")
        });
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

    /// Stops the project's watcher before its folder goes.
    fn leave(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project("/nonexistent".into(), window, cx))
        });
    }

    fn pill(id: &'static str, cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window
                .try_find(id)
                .and_then(|e| e.label().map(str::to_owned))
        })
    }

    /// 2.4: the header shows what the conversation cost, as the agent reports it.
    #[gpui_kit::test]
    fn header_shows_the_conversation_cost(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        let usage = |cost_usd| SessionEvent::Usage {
            used: 43_000,
            size: 1_000_000,
            cost_usd,
            rate_limit: None,
        };
        assert_eq!(pill("session-cost", cx), None, "no cost reported yet");
        session.update(cx, |s, cx| s.handle_event(&usage(Some(0.4237)), cx));
        assert_eq!(pill("session-cost", cx).as_deref(), Some("$0.42"));
        session.update(cx, |s, cx| s.handle_event(&usage(None), cx));
        assert_eq!(pill("session-cost", cx).as_deref(), Some("$0.42"));
    }

    #[gpui_kit::test]
    fn strip_shows_folder_branch_and_agent(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &["feature"]);
        let (workspace, cx, _user) = fake_live(cx, "--models");
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        let session = start_session(&workspace, cx);
        wait_until(cx, "the branch", |cx| {
            session.read_with(cx, |s, _| s.head.is_some())
        });
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        assert_eq!(pill("pill-folder", cx), Some(name));
        assert_eq!(pill("pill-branch", cx).as_deref(), Some("feature"));
        assert_eq!(pill("pill-agent", cx).as_deref(), Some("fake"));
        let placeholder = cx.update(|window, _| {
            let input = window.within("prompt-box").find("input-state");
            input.placeholder().map(str::to_owned)
        });
        assert_eq!(placeholder.as_deref(), Some("Message fake…"));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn restored_session_shows_its_strip(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &[]);
        let path = dir.path().join("state.json");
        let state = AppState {
            open_sessions: vec![OpenSession {
                agent: "fake".into(),
                session_id: "abc".into(),
                cwd: root.clone(),
                pane: 0,
                name: None,
            }],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path));
        cx.deactivate_window();

        launch(&workspace, cx);

        wait_until(cx, "the branch", |cx| {
            workspace.read_with(cx, |w, cx| {
                w.sessions
                    .first()
                    .is_some_and(|s| s.read(cx).head.is_some())
            })
        });
        assert_eq!(pill("pill-branch", cx).as_deref(), Some("main"));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn no_branch_pill_outside_a_repository(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        // Let the (empty) read land before looking.
        cx.run_until_parked();
        assert!(session.read_with(cx, |s, _| s.head.is_none()));
        assert!(pill("pill-folder", cx).is_some());
        assert_eq!(pill("pill-branch", cx), None);
    }

    /// A superproject opened in a workspace whose fake agent reads `reads` (paths) each turn.
    fn follow_setup(
        cx: &mut TestAppContext,
        reads: impl FnOnce(&std::path::Path, &std::path::Path) -> String,
    ) -> (
        Entity<Workspace>,
        &mut VisualTestContext,
        (tempfile::TempDir, tempfile::TempDir),
        (PathBuf, PathBuf),
    ) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let (sup, wt) = submodule_worktree(dir.path());
        let (workspace, cx, user) = fake_live(cx, &reads(&sup, &wt));
        let root = sup.clone();
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.set_project(root, window, cx)));
        (workspace, cx, (dir, user), (sup, wt))
    }

    fn head_landed(session: &Entity<ChatSession>, cx: &mut VisualTestContext) {
        wait_until(cx, "the branch", |cx| {
            session.read_with(cx, |s, _| s.head.is_some())
        });
    }

    #[gpui_kit::test]
    fn strip_follows_the_agent_into_a_submodule_worktree(cx: &mut TestAppContext) {
        let (workspace, cx, _dir, (_sup, wt)) =
            follow_setup(cx, |_, wt| format!("--read {}/a.txt", wt.display()));
        let session = start_session(&workspace, cx);
        head_landed(&session, cx);
        send("go", cx);
        wait_until(cx, "the place", |cx| {
            session.read_with(cx, |s, _| s.place.is_some())
        });
        assert_eq!(pill("pill-folder", cx).as_deref(), Some("inventories"));
        assert_eq!(pill("pill-branch", cx).as_deref(), Some("feat-x"));
        assert!(session.read_with(cx, |s, _| s.place.as_ref().unwrap().worktree));
        assert_eq!(wt.file_name().unwrap(), "x");
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn strip_comes_back_with_the_agent(cx: &mut TestAppContext) {
        let (workspace, cx, _dir, _) = follow_setup(cx, |sup, wt| {
            format!(
                "--read {}/a.txt --read {}/b.txt",
                wt.display(),
                sup.display()
            )
        });
        let session = start_session(&workspace, cx);
        head_landed(&session, cx);
        send("go", cx);
        wait_until(cx, "both reads", |cx| {
            session.read_with(cx, |s, _| {
                s.roots.len() == 2 && s.model.state == TurnState::Idle
            })
        });
        assert!(session.read_with(cx, |s, _| s.place.is_none()));
        assert_eq!(pill("pill-folder", cx).as_deref(), Some("super"));
        assert_eq!(pill("pill-branch", cx).as_deref(), Some("main"));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn a_cached_read_back_home_drops_an_earlier_read_in_flight(cx: &mut TestAppContext) {
        let (workspace, cx, _dir, (sup, _)) = follow_setup(cx, |sup, wt| {
            format!(
                "--read {}/a.txt --read {}/b.txt",
                wt.display(),
                sup.display()
            )
        });
        let session = start_session(&workspace, cx);
        head_landed(&session, cx);
        session.update(cx, |s, _| {
            let root = s.head.as_ref().unwrap().root.clone();
            s.roots.insert(sup, Some(root));
        });
        send("go", cx);
        wait_until(cx, "the turn", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::Idle)
        });
        cx.run_until_parked();
        assert!(session.read_with(cx, |s, _| s.place.is_none()));
        assert_eq!(pill("pill-folder", cx).as_deref(), Some("super"));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn outside_a_repository_the_strip_stays(cx: &mut TestAppContext) {
        let plain = tempfile::tempdir().unwrap();
        std::fs::create_dir(plain.path().join("plain")).unwrap();
        let read = plain.path().join("plain/n.txt");
        let (workspace, cx, _dir, _) =
            follow_setup(cx, |_, _| format!("--read {}", read.display()));
        let session = start_session(&workspace, cx);
        head_landed(&session, cx);
        let before = (pill("pill-folder", cx), pill("pill-branch", cx));
        send("go", cx);
        wait_until(cx, "the read", |cx| {
            session.read_with(cx, |s, _| s.roots.len() == 1)
        });
        assert!(session.read_with(cx, |s, _| s.place.is_none()));
        assert_eq!((pill("pill-folder", cx), pill("pill-branch", cx)), before);
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn subagent_locations_do_not_move_the_strip(cx: &mut TestAppContext) {
        let (workspace, cx, _dir, (_sup, wt)) = follow_setup(cx, |_, _| String::new());
        let session = start_session(&workspace, cx);
        head_landed(&session, cx);
        let call = SessionEvent::ToolCall(ToolCallView {
            id: "c".into(),
            title: "Read a.txt".into(),
            kind: "read".into(),
            status: ToolStatus::Completed,
            content: Vec::new(),
            locations: vec![Location {
                path: wt.join("a.txt"),
                line: None,
            }],
            subagent: Some("child".into()),
        });
        session.update(cx, |s, cx| s.handle_event(&call, cx));
        cx.run_until_parked();
        assert!(session.read_with(cx, |s, _| s.roots.is_empty() && s.place.is_none()));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn detached_submodule_shows_no_branch_chip(cx: &mut TestAppContext) {
        let (workspace, cx, _dir, (sup, _wt)) = follow_setup(cx, |sup, _| {
            format!("--read {}/inventories/a.txt", sup.display())
        });
        git(&sup.join("inventories"), &["checkout", "--detach"]);
        let session = start_session(&workspace, cx);
        head_landed(&session, cx);
        send("go", cx);
        wait_until(cx, "the place", |cx| {
            session.read_with(cx, |s, _| s.place.is_some())
        });
        assert_eq!(pill("pill-folder", cx).as_deref(), Some("inventories"));
        assert_eq!(pill("pill-branch", cx), None);
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn resumed_session_shows_where_the_agent_worked(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let (sup, wt) = submodule_worktree(dir.path());
        let path = dir.path().join("state.json");
        let state = AppState {
            open_sessions: vec![OpenSession {
                agent: "fake".into(),
                session_id: "abc".into(),
                cwd: sup.clone(),
                pane: 0,
                name: None,
            }],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let flag = format!("--read {}/a.txt", wt.display());
        let (workspace, cx) = open_at(cx, fake_settings(&flag), Some(path));
        cx.deactivate_window();

        launch(&workspace, cx);

        wait_until(cx, "the place", |cx| {
            workspace.read_with(cx, |w, cx| {
                w.sessions
                    .first()
                    .is_some_and(|s| s.read(cx).place.is_some())
            })
        });
        assert_eq!(pill("pill-folder", cx).as_deref(), Some("inventories"));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn strip_reads_the_branch_again_after_a_turn(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &["feature"]);
        let (workspace, cx, _user) = fake_live(cx, "");
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        let session = start_session(&workspace, cx);
        head_landed(&session, cx);
        git(&root, &["checkout", "-b", "other"]);
        send("go", cx);
        wait_until(cx, "the new branch", |cx| {
            session.read_with(cx, |s, _| {
                s.head.as_ref().and_then(|h| h.branch.as_deref()) == Some("other")
            })
        });
        assert_eq!(pill("pill-branch", cx).as_deref(), Some("other"));
        leave(&workspace, cx);
    }

    /// The strip sits on the prompt box's top edge, not in the header, and stays inside it.
    #[gpui_kit::test]
    fn composer_strip_sits_on_the_prompt_box(cx: &mut TestAppContext) {
        strip_on_box(cx, false);
    }

    #[gpui_kit::test]
    fn composer_strip_stays_in_a_narrow_pane(cx: &mut TestAppContext) {
        strip_on_box(cx, true);
    }

    fn strip_on_box(cx: &mut TestAppContext, narrow: bool) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let long = "chore-ec-9688-energy-dashboard-with-a-very-long-name";
        let root = repo_with(&dir.path().join("a-long-project-folder-name"), &[long]);
        let (workspace, cx, _user) = fake_live(cx, "--models");
        if narrow {
            cx.simulate_resize(gpui_kit::size(px(860.), px(700.)));
        }
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        let session = start_session(&workspace, cx);
        wait_until(cx, "the branch", |cx| {
            session.read_with(cx, |s, _| s.head.is_some())
        });
        send("hello", cx);
        wait_until(cx, "the reply", |cx| state(&session, cx) == TurnState::Idle);
        cx.update(|window, cx| {
            window.render_frame(cx);
            let bounds = |id: &'static str| window.find(id).bounds();
            let (strip, field, list) = (
                bounds("composer-strip"),
                bounds("prompt-box"),
                bounds("transcript"),
            );
            assert!(
                (strip.bottom() - field.top()).abs() < px(0.5),
                "{strip:?} {field:?}"
            );
            assert!(((strip.left() - field.left()) - px(18.)).abs() < px(0.5));
            assert!(
                strip.right() <= field.right() + px(0.5),
                "{strip:?} {field:?}"
            );
            // Wide, the tab is as wide as its chips; narrow, the chips shrank to fill the box
            // and stay inside the tab.
            let gap = field.right() - strip.right();
            match narrow {
                true => assert!(
                    gap < px(1.),
                    "the chips did not shrink: {strip:?} {field:?}"
                ),
                false => assert!(gap > px(100.), "{strip:?} {field:?}"),
            }
            for id in ["pill-folder", "pill-branch", "pill-agent"] {
                let chip = bounds(id);
                assert!(chip.bottom() <= field.top() + px(0.5), "{id} {chip:?}");
                assert!(
                    chip.top() > list.bottom() - px(0.5),
                    "{id} is in the header"
                );
                assert!(chip.right() <= strip.right() + px(0.5), "{id} {chip:?}");
            }
        });
        leave(&workspace, cx);
    }

    /// An agent with no modes and no model options: the bar holds only attach and send.
    #[gpui_kit::test]
    fn bar_has_no_empty_chips_without_pickers(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_session(&workspace, cx);
        cx.run_until_parked();
        assert!(session.read_with(cx, |s, _| s.pickers.is_empty()));
        // The fake agent always reports modes, so its legacy mode chip is the only one.
        let legacy = session.read_with(cx, |s, _| s.legacy_mode_picker().is_some());
        let found = cx.update(|window, cx| {
            window.render_frame(cx);
            ["bar-model", "bar-effort", "bar-mode"].map(|id| window.try_find(id).is_some())
        });
        assert_eq!(found, [false, false, legacy]);
    }

    /// With a card waiting, a digit typed while a bar picker (not the prompt) has focus does
    /// not answer it.
    #[gpui_kit::test]
    fn digits_do_not_answer_a_card_from_outside_the_prompt(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--models"));
        let session = start_session(&workspace, cx);
        session.update(cx, |s, cx| {
            let id = s.id.clone();
            let request = PermissionRequest {
                id: 100,
                session: id,
                tool_call_id: "t".into(),
                title: "Run".into(),
                diff: None,
                subagent: None,
                options: vec![PermissionChoice {
                    id: "y".into(),
                    name: "Yes".into(),
                    kind: ChoiceKind::AllowOnce,
                }],
            };
            s.handle_permission(request, cx);
        });
        let waiting =
            |cx: &mut VisualTestContext| session.read_with(cx, |s, _| !s.model.waiting.is_empty());
        cx.update(|window, cx| window.blur(cx));
        cx.simulate_keystrokes("1");
        assert!(waiting(cx), "a digit outside the prompt answered the card");
    }
}
