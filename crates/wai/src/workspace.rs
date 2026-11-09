use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, h_resizable, resizable_panel, v_flex, v_resizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::{
    AgentError, AgentHandle, BrokerError, QuestionAnswer, SessionEvent, SessionStarted,
    SessionSummary, StopReason,
};
use wai_project::{FsChange, ProjectWatcher};
use wai_settings::{AppState, OpenSession, Settings, Template, config_dir, load_templates};

use crate::agents::{AgentRegistry, AgentStatus, RegistryEvent};
use crate::agents_view::{AgentsView, ShowSession};
use crate::app::{CloseSession, FocusNextPane, NewSession, OpenFolder, Quit, Save, ToggleTerminal};
use crate::chat::{ChatEvent, ChatSession, signed_out};
use crate::editor::{self, EditorPane, Emptied, FileEditor};
use crate::ext_host::ExtHost;
use crate::extensions_view::ExtensionsView;
use crate::files::{FileTree, OpenFile};
use crate::notify::{self, should_notify};
use crate::search_view::{OpenMatch, SearchView};
use crate::sidebar::{SidebarTab, canonical};
use crate::terminal_view::TerminalView;

const SIDEBAR_WIDTH: f32 = 260.;
const TERMINAL_HEIGHT: f32 = 260.;
pub const MAX_PANES: usize = 4;
/// Events kept per session id that is not open yet (see `Workspace::early`).
const EARLY_EVENTS: usize = 1_000;
const START_HINT: &str = if cfg!(target_os = "macos") {
    "Start a session (⌘N)"
} else {
    "Start a session (Ctrl+N)"
};

pub enum Pane {
    /// The start hint, and why the last start failed, if it did.
    Empty(Option<String>),
    /// Reserved while `new_session` runs; `ticket` tells this request's placeholder apart.
    Preparing {
        agent: String,
        ticket: u64,
    },
    Session(Entity<ChatSession>),
    /// Open files in tabs; one pane at most. Not kept between runs.
    Editor(Entity<EditorPane>),
}

impl Pane {
    pub fn session(&self) -> Option<&Entity<ChatSession>> {
        match self {
            Pane::Session(session) => Some(session),
            _ => None,
        }
    }
}

/// What waits on the question about unsaved edits.
#[derive(Clone)]
pub(crate) enum Leave {
    Quit,
    Close,
    Open(PathBuf),
}

/// The window's root view: sidebar | chat panes over the terminal panel.
pub struct Workspace {
    /// Canonical, so history cwds compare with it (R3).
    pub project: PathBuf,
    pub settings: Settings,
    /// The "New from template" menu's entries; empty hides the menu.
    pub templates: Vec<Template>,
    /// The config dir: the user's `templates.toml` (re-read on a project switch) and `scratch/`,
    /// where tool outputs open from (R21; cleared at launch).
    pub user_dir: PathBuf,
    pub registry: Entity<AgentRegistry>,
    /// Every open session, shown in a pane or not, in the order they opened.
    pub sessions: Vec<Entity<ChatSession>>,
    pub panes: Vec<Pane>,
    pub focused_pane: usize,
    pub terminal_visible: bool,
    /// Created on the first toggle, in the project folder.
    pub terminal: Option<Entity<TerminalView>>,
    /// The default agent's sessions, each with its cwd canonicalized for the project filter.
    pub history: Vec<(PathBuf, SessionSummary)>,
    pub all_projects: bool,
    pub(crate) history_task: Option<Task<()>>,
    pub sidebar_tab: SidebarTab,
    pub files: Entity<FileTree>,
    pub search: Entity<SearchView>,
    pub(crate) agents: Entity<AgentsView>,
    /// Created on first use (the sidebar's Extensions button, a project's suggestion): it reads
    /// `user_dir`'s extensions.
    pub(crate) extensions: Option<Entity<ExtensionsView>>,
    /// The enabled extensions' processes.
    pub(crate) ext_host: Entity<ExtHost>,
    /// The project's one watcher and the task that hands its changes out; `None` when the
    /// project cannot be watched.
    watch: Option<(ProjectWatcher, Task<()>)>,
    next_ticket: u64,
    /// Tickets whose "Preparing…" pane ⌘W closed: their session never opens.
    cancelled: HashSet<u64>,
    /// `new_session` calls in flight, per agent.
    starting: HashMap<String, usize>,
    /// Events for session ids not open yet, per agent: an agent may describe a session (title,
    /// usage, commands) before `new_session` returns. Kept, `EARLY_EVENTS` per id, only while a
    /// session of that agent is starting.
    early: HashMap<(String, String), Vec<SessionEvent>>,
    /// Where the open sessions, the project and the terminal's visibility are kept between runs;
    /// `None` keeps nothing (tests).
    state_path: Option<PathBuf>,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        project: PathBuf,
        settings: Settings,
        state_path: Option<PathBuf>,
        global_ignores: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let registry = cx.new(|_| AgentRegistry::new(&settings));
        let routing = cx.subscribe(&registry, |this, _, event, cx| this.route(event, cx));
        // The process ends ~200 ms after the quit hooks: nothing it started may outlive it. The
        // agents are signalled and the extensions killed first: stopping a shell that ignores
        // SIGHUP blocks for 200 ms.
        let quit = cx.on_app_quit(|this, cx| {
            let agents = this.registry.read(cx).shutdown_all();
            this.ext_host.update(cx, |host, _| host.stop_all());
            this.save_state(cx);
            if let Some(terminal) = &this.terminal {
                terminal.update(cx, |t, _| t.stop());
            }
            agents
        });
        // R28: quitting or closing the window asks about unsaved edits first. Deferred: a key
        // dispatch holds the window until it ends.
        let (this, handle) = (cx.weak_entity(), window.window_handle());
        App::on_action(cx, move |_: &Quit, cx| {
            let this = this.clone();
            cx.defer(move |cx| {
                let leave = |_: AnyView, window: &mut Window, cx: &mut App| {
                    this.update(cx, |this, cx| this.leave(Leave::Quit, window, cx))
                };
                if !matches!(handle.update(cx, leave), Ok(Ok(()))) {
                    cx.quit();
                }
            });
        });
        let this = cx.weak_entity();
        window.on_window_should_close(cx, move |window, cx| {
            let ask = |this: &mut Self, cx: &mut Context<Self>| {
                let clean = this.dirty_tabs(cx).is_empty();
                if !clean {
                    this.leave(Leave::Close, window, cx);
                }
                clean
            };
            this.update(cx, ask).unwrap_or(true)
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        window.set_window_title(&title(&project));
        let project = canonical(&project);
        let ext_host = cx.new(|_| ExtHost::new(project.clone()));
        let files = cx.new(|cx| FileTree::new(project.clone(), global_ignores, cx));
        let open_file = cx.subscribe_in(&files, window, |this, _, OpenFile(path), window, cx| {
            this.open_file(path.clone(), None, window, cx)
        });
        let search = cx.new(|cx| SearchView::new(project.clone(), global_ignores, window, cx));
        let open_match = cx.subscribe_in(
            &search,
            window,
            |this, _, OpenMatch { path, line }, window, cx| {
                this.open_file(path.clone(), Some(*line), window, cx)
            },
        );
        let agents = cx.new(|_| AgentsView::new());
        let open_session = cx.subscribe_in(
            &agents,
            window,
            |this, _, ShowSession(session), window, cx| {
                this.show(session.clone(), false, window, cx)
            },
        );
        Self {
            project,
            settings,
            templates: Vec::new(),
            user_dir: config_dir(),
            registry,
            agents,
            sessions: Vec::new(),
            panes: vec![Pane::Empty(None)],
            focused_pane: 0,
            terminal_visible: false,
            terminal: None,
            history: Vec::new(),
            all_projects: false,
            history_task: None,
            sidebar_tab: SidebarTab::default(),
            files,
            search,
            extensions: None,
            ext_host,
            watch: None,
            next_ticket: 0,
            cancelled: HashSet::new(),
            starting: HashMap::new(),
            early: HashMap::new(),
            state_path,
            focus,
            _subscriptions: vec![routing, quit, open_file, open_match, open_session],
        }
    }

    /// Starts the project's watcher, replacing the last project's: once the window is open, like
    /// the agent (tests that need it start it), and again on each new project. Its changes reach
    /// the open files and the tree in batches (at most one per 100 ms), each wake-up drained
    /// before one update.
    pub fn watch_project(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // ponytail: on Linux, inotify walks the whole tree here (node_modules too) on the UI
        // thread; move the start to the background executor if Linux matters.
        let (watcher, changes) = match ProjectWatcher::start(&self.project) {
            Ok(started) => started,
            Err(e) => {
                self.watch = None;
                return eprintln!("wai: cannot watch {}: {e}", self.project.display());
            }
        };
        let task = cx.spawn_in(window, async move |this, cx| {
            while let Ok(change) = changes.recv().await {
                let mut batch = vec![change];
                while let Ok(more) = changes.try_recv() {
                    batch.push(more);
                }
                let applied = this.update_in(cx, |this, window, cx| {
                    this.disk_changed(&batch, window, cx);
                    this.files.update(cx, |files, cx| files.apply(batch, cx))
                });
                if applied.is_err() {
                    break;
                }
            }
        });
        self.watch = Some((watcher, task));
    }

    /// Hands a watcher batch to the open files it touches.
    fn disk_changed(&self, batch: &[FsChange], window: &mut Window, cx: &mut Context<Self>) {
        let touched = |path: &Path| {
            batch.iter().any(|change| match change {
                FsChange::Rescan => true,
                FsChange::Changed(paths) => paths.iter().any(|p| p == path),
            })
        };
        for pane in &self.panes {
            if let Pane::Editor(pane) = pane {
                for tab in pane.read(cx).tabs.clone() {
                    if touched(&tab.read(cx).path) {
                        tab.update(cx, |tab, cx| tab.disk_event(window, cx));
                    }
                }
            }
        }
    }

    /// Starts the default agent, so the history fills in once it is ready.
    pub fn start_agent(&mut self, cx: &mut Context<Self>) {
        let agent = self.settings.default_agent.clone();
        self.registry.update(cx, |r, cx| r.handle(&agent, cx));
    }

    /// Reopens the last run's sessions, which `load_session` replays, and its terminal. A session
    /// goes back to its pane: the first one saved for a pane takes it, panes left empty close, and
    /// one saved in no pane only joins the open list. Sessions of agents the settings no longer
    /// have are dropped.
    pub fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = &self.state_path else {
            return;
        };
        let state = AppState::load(path);
        let mut panes: [Option<Entity<ChatSession>>; MAX_PANES] = Default::default();
        for saved in state.open_sessions {
            let (agent, id, cwd) = (saved.agent, saved.session_id, saved.cwd);
            let Some(session) = self.loading_session(agent, id, cwd, window, cx) else {
                continue;
            };
            if let Some(slot @ None) = panes.get_mut(saved.pane) {
                *slot = Some(session.clone());
            }
            session.update(cx, |s, cx| s.load(true, window, cx));
        }
        let panes: Vec<_> = panes.into_iter().flatten().map(Pane::Session).collect();
        if !panes.is_empty() {
            self.panes = panes;
        }
        if state.terminal_visible {
            self.toggle_terminal(&ToggleTerminal, window, cx);
        }
        self.focus_pane(0, window, cx);
        self.save_state(cx);
    }

    /// Writes what the next launch restores: on each structural change and at quit, never on a
    /// timer. A session in no pane is saved with pane `MAX_PANES`.
    fn save_state(&self, cx: &App) {
        let Some(path) = &self.state_path else {
            return;
        };
        let pane = |session| self.panes.iter().position(|p| p.session() == Some(session));
        let open_sessions = self.sessions.iter().map(|session| {
            let s = session.read(cx);
            OpenSession {
                agent: s.agent.clone(),
                session_id: s.id.clone(),
                cwd: s.cwd.clone(),
                pane: pane(session).unwrap_or(MAX_PANES),
            }
        });
        let state = AppState {
            project: Some(self.project.clone()),
            open_sessions: open_sessions.collect(),
            terminal_visible: self.terminal_visible,
            // ponytail: the sidebar does not resize yet; save its width once it does.
            sidebar_width: SIDEBAR_WIDTH,
        };
        // ponytail: a few hundred bytes on the UI thread; move to the background executor if
        // it ever shows in a frame.
        if let Err(e) = state.save(path) {
            eprintln!("wai: could not save {}: {e}", path.display());
        }
    }

    fn session(&self, agent: &str, id: &str, cx: &App) -> Option<Entity<ChatSession>> {
        self.sessions
            .iter()
            .find(|session| {
                let session = session.read(cx);
                session.agent == agent && session.id == id
            })
            .cloned()
    }

    fn route(&mut self, event: &RegistryEvent, cx: &mut Context<Self>) {
        match event {
            RegistryEvent::Session {
                agent,
                session,
                event,
            } => match self.session(agent, session, cx) {
                Some(s) => s.update(cx, |s, cx| s.handle_event(event, cx)),
                None if self.starting.get(agent).is_some_and(|n| *n > 0) => {
                    let early = (self.early)
                        .entry((agent.clone(), session.clone()))
                        .or_default();
                    if early.len() < EARLY_EVENTS {
                        early.push(event.clone());
                    }
                }
                None => {}
            },
            RegistryEvent::Permission { agent, request } => {
                match self.session(agent, &request.session, cx) {
                    Some(s) => s.update(cx, |s, cx| s.handle_permission(request.clone(), cx)),
                    None => self.unrouted(agent, cx, |h| h.answer_permission(request.id, None)),
                }
            }
            RegistryEvent::Question { agent, request } => {
                match self.session(agent, &request.session, cx) {
                    Some(s) => s.update(cx, |s, cx| s.handle_question(request.clone(), cx)),
                    None => self.unrouted(agent, cx, |h| {
                        h.answer_question(request.id, QuestionAnswer::Cancel)
                    }),
                }
            }
            RegistryEvent::TurnEnded {
                agent,
                session,
                reason,
            } => {
                if let Some(s) = self.session(agent, session, cx) {
                    s.update(cx, |s, cx| s.handle_turn_ended(reason, cx));
                }
                if !matches!(reason, StopReason::Other(r) if r == "auth_required") {
                    self.registry.update(cx, |r, cx| r.mark_ready(agent, cx));
                }
                self.refresh_history(cx);
            }
            // R7: carries no session id; the agent's sessions ignore ids they do not hold. R41:
            // ids restart at 1 per agent, so other agents' sessions never see it.
            RegistryEvent::PermissionResolved { agent, id } => {
                for s in &self.sessions {
                    if s.read(cx).agent == *agent {
                        s.update(cx, |s, cx| s.handle_permission_resolved(*id, cx));
                    }
                }
            }
            RegistryEvent::QuestionUnsupported { agent, session } => {
                if let Some(s) = self.session(agent, session, cx) {
                    s.update(cx, |s, cx| s.handle_question_unsupported(cx));
                }
            }
            RegistryEvent::QuestionResolved { agent, id } => {
                for s in &self.sessions {
                    if s.read(cx).agent == *agent {
                        s.update(cx, |s, cx| s.handle_question_resolved(*id, cx));
                    }
                }
            }
            RegistryEvent::StatusChanged(agent) => {
                match self.registry.read(cx).status(agent).cloned() {
                    Some(AgentStatus::Failed(error)) => {
                        for s in &self.sessions {
                            if s.read(cx).agent == *agent {
                                s.update(cx, |s, cx| s.handle_agent_exited(&error, cx));
                            }
                        }
                    }
                    Some(AgentStatus::Ready(_)) => self.refresh_history(cx),
                    _ => {}
                }
                // Empty panes show the default agent's problem, if any.
                cx.notify();
            }
        }
    }

    /// Cancels a request no open session can show, so the agent's turn goes on. A dead agent's
    /// requests died with it (and `handle` would respawn it).
    fn unrouted(
        &mut self,
        agent: &str,
        cx: &mut Context<Self>,
        cancel: impl FnOnce(&AgentHandle) -> Result<(), BrokerError>,
    ) {
        if matches!(
            self.registry.read(cx).status(agent),
            Some(AgentStatus::Failed(_))
        ) {
            return;
        }
        if let Some(handle) = self.registry.update(cx, |r, cx| r.handle(agent, cx)) {
            cancel(&handle).ok();
        }
    }

    /// Joins the open list; the sidebar follows its status and title. While the window is in the
    /// background, the session notifies when it needs the user or finishes a turn.
    fn add_session(
        &mut self,
        session: Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(&session, window, |this, session, event, window, cx| {
            match event {
                // Its pane keeps the notice; the next launch does not try again.
                ChatEvent::RestoreFailed => {
                    this.sessions.retain(|s| s != session);
                    this.sync_agents(cx);
                    this.save_state(cx);
                }
                ChatEvent::OpenFile { path, line } => {
                    return this.open_file(path.clone(), Some(*line), window, cx);
                }
                // The watcher may not see it (a file outside the project): tell the tabs now. A
                // tab the watcher reached first finds its stamp current and does nothing.
                ChatEvent::Reverted(path) => {
                    let changed = [FsChange::Changed(vec![path.clone()])];
                    return this.disk_changed(&changed, window, cx);
                }
                ChatEvent::OpenOutput { tool, text } => {
                    let session = session.read(cx).id.clone();
                    return this.open_output(&session, tool, text.clone(), window, cx);
                }
                _ => {}
            }
            let model = &session.read(cx).model;
            if let Some(kind) = should_notify(event, &model.state, window.is_window_active()) {
                let (title, body) = notify::message(kind, model);
                notify::notify(&title, &body);
            }
            cx.notify();
        })
        .detach();
        self.sessions.push(session);
        self.sync_agents(cx);
    }

    /// The Agents tab follows the open sessions.
    fn sync_agents(&self, cx: &mut Context<Self>) {
        self.agents.update(cx, |a, cx| a.set(&self.sessions, cx));
    }

    /// Focuses pane `ix`: its prompt box, or the workspace when it has no session.
    fn focus_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_pane = ix;
        match &self.panes[ix] {
            Pane::Session(session) => session.update(cx, |s, cx| s.focus(window, cx)),
            Pane::Editor(pane) => pane.update(cx, |p, cx| p.focus(window, cx)),
            _ => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    /// Shows `session` in the focused pane (beside the editor pane, see `session_pane`), or with
    /// `side` in a new pane while there are fewer than `MAX_PANES` (an empty focused pane is used
    /// instead). A session already in a pane is focused there: one session is drawn once.
    pub fn show(
        &mut self,
        session: Entity<ChatSession>,
        side: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = match self
            .panes
            .iter()
            .position(|p| p.session() == Some(&session))
        {
            Some(ix) => ix,
            None => {
                let empty = matches!(self.panes[self.focused_pane], Pane::Empty(_));
                let ix = if side && !empty && self.panes.len() < MAX_PANES {
                    self.panes.push(Pane::Empty(None));
                    self.panes.len() - 1
                } else {
                    self.session_pane()
                };
                self.panes[ix] = Pane::Session(session);
                ix
            }
        };
        self.focus_pane(ix, window, cx);
        self.save_state(cx);
    }

    /// The pane a session takes: the focused one, unless it is the editor pane (its unsaved edits
    /// would go with it); then a new pane to the right, or at `MAX_PANES` the next pane.
    fn session_pane(&mut self) -> usize {
        if !matches!(self.panes[self.focused_pane], Pane::Editor(_)) {
            self.focused_pane
        } else if self.panes.len() < MAX_PANES {
            self.panes.push(Pane::Empty(None));
            self.panes.len() - 1
        } else {
            (self.focused_pane + 1) % self.panes.len()
        }
    }

    /// Opens a history row: the open session with that id, else a new one that loads it.
    pub fn resume(
        &mut self,
        summary: &SessionSummary,
        side: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let agent = self.settings.default_agent.clone();
        if let Some(session) = self.session(&agent, &summary.id, cx) {
            return self.show(session, side, window, cx);
        }
        let (id, cwd) = (summary.id.clone(), summary.cwd.clone());
        let Some(session) = self.loading_session(agent, id, cwd, window, cx) else {
            return;
        };
        session.update(cx, |s, _| s.model.title = summary.title.clone());
        self.show(session.clone(), side, window, cx);
        session.update(cx, |s, cx| s.load(false, window, cx));
    }

    /// Whether `agent` advertised `session/fork` and `session/load` (a fork loads to replay).
    pub(crate) fn can_fork(&self, agent: &str, cx: &App) -> bool {
        matches!(
            self.registry.read(cx).status(agent),
            Some(AgentStatus::Ready(caps)) if caps.fork && caps.load_session
        )
    }

    /// Copies session `id` into a new one that opens in a new pane to the side (the focused
    /// pane at `MAX_PANES`). It is titled like its source plus " (fork)" until the agent sends
    /// one. The fork's history comes from loading it, so it is routed before that load starts.
    pub fn fork(
        &mut self,
        agent: String,
        id: String,
        title: Option<String>,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            return;
        };
        let title = title.map(|t| format!("{t} (fork)"));
        cx.spawn_in(window, async move |this, cx| {
            let forked = handle.fork_session(id, cwd.clone()).await;
            this.update_in(cx, |this, window, cx| match forked {
                Ok(started) => {
                    let Some(session) = this.loading_session(agent, started.id, cwd, window, cx)
                    else {
                        return;
                    };
                    session.update(cx, |s, _| s.model.title = title);
                    this.show(session.clone(), true, window, cx);
                    session.update(cx, |s, cx| s.load(false, window, cx));
                }
                Err(e) => window
                    .push_notification(Notification::error(fork_failure(&e)).autohide(false), cx),
            })
            .ok();
        })
        .detach();
    }

    /// A session about to load, in the open list and in no pane yet. Routed before it loads:
    /// the agent replays the conversation before it answers. `None` when no agent has that name
    /// in the settings.
    fn loading_session(
        &mut self,
        agent: String,
        id: String,
        cwd: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<ChatSession>> {
        let handle = self.registry.update(cx, |r, cx| r.handle(&agent, cx))?;
        let started = SessionStarted {
            id,
            modes: Vec::new(),
            current_mode: None,
        };
        let registry = self.registry.downgrade();
        let session =
            cx.new(|cx| ChatSession::new(agent, started, cwd, handle, registry, window, cx));
        self.add_session(session.clone(), window, cx);
        Some(session)
    }

    /// Starts a session in the focused pane (beside the editor pane, see `session_pane`), which
    /// shows `Preparing <agent>…` until the agent answers. A second ⌘N on that pane waits for
    /// the first.
    pub(crate) fn new_session(
        &mut self,
        _: &NewSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let agent = self.settings.default_agent.clone();
        self.start_session(agent, None, window, cx)
    }

    /// `new_session` for template `ix`: its agent (the default one if it names none), its prompt
    /// pre-filled and not sent. An agent the settings do not define is an error in the pane.
    pub(crate) fn start_template(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(template) = self.templates.get(ix).cloned() else {
            return;
        };
        let agent = template
            .agent
            .clone()
            .unwrap_or_else(|| self.settings.default_agent.clone());
        if !self.settings.agents.contains_key(&agent) {
            let pane = self.session_pane();
            if matches!(self.panes[pane], Pane::Preparing { .. }) {
                return;
            }
            self.panes[pane] = Pane::Empty(Some(format!(
                "Template \"{}\" uses unknown agent \"{agent}\"",
                template.name
            )));
            return cx.notify();
        }
        self.start_session(agent, Some(template.prompt), window, cx)
    }

    fn start_session(
        &mut self,
        agent: String,
        prefill: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self.session_pane();
        if matches!(self.panes[ix], Pane::Preparing { .. }) {
            return;
        }
        if ix != self.focused_pane {
            self.focus_pane(ix, window, cx);
        }
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            self.panes[ix] = Pane::Empty(Some(format!("No agent named {agent} in the settings.")));
            return cx.notify();
        };
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        self.panes[ix] = Pane::Preparing {
            agent: agent.clone(),
            ticket,
        };
        *self.starting.entry(agent.clone()).or_default() += 1;
        cx.notify();
        let cwd = self.project.clone();
        let registry = self.registry.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let started = handle.new_session(cwd.clone()).await;
            this.update_in(cx, |this, window, cx| {
                let early = this.landed(&agent, started.as_ref().ok().map(|s| s.id.as_str()));
                // Replaced meanwhile (a history row shown there), the placeholder is gone: the
                // session still opens. Closed (⌘W), it does not.
                let cancelled = this.cancelled.remove(&ticket);
                let pane = this
                    .panes
                    .iter()
                    .position(|p| matches!(p, Pane::Preparing { ticket: t, .. } if *t == ticket));
                let shown = match started {
                    Ok(_) if cancelled => return,
                    Ok(started) => {
                        // A session that starts proves the agent works (signed in again, say).
                        this.registry.update(cx, |r, cx| r.mark_ready(&agent, cx));
                        let session = cx.new(|cx| {
                            ChatSession::new(agent, started, cwd, handle, registry, window, cx)
                        });
                        if let Some(text) = prefill {
                            session.update(cx, |s, cx| s.prefill(&text, window, cx));
                        }
                        session
                            .update(cx, |s, cx| early.iter().for_each(|e| s.handle_event(e, cx)));
                        this.add_session(session.clone(), window, cx);
                        this.refresh_history(cx);
                        Pane::Session(session)
                    }
                    // The empty pane shows the sign-in message.
                    Err(AgentError::AuthRequired) => {
                        this.registry
                            .update(cx, |r, cx| r.set_status(&agent, AgentStatus::SignedOut, cx));
                        Pane::Empty(None)
                    }
                    Err(e) => Pane::Empty(Some(format!("Could not start a {agent} session: {e}"))),
                };
                if let Some(ix) = pane {
                    this.panes[ix] = shown;
                    // Only the focused pane takes the caret: elsewhere it would pull focus away
                    // from the pane the user is in.
                    if ix == this.focused_pane {
                        this.focus_pane(ix, window, cx);
                    }
                }
                this.save_state(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Shows the terminal, created on the first toggle in the project folder (a shell that exited
    /// starts again), and focuses it. Hiding it hands the focus back to the focused pane.
    fn toggle_terminal(&mut self, _: &ToggleTerminal, window: &mut Window, cx: &mut Context<Self>) {
        self.terminal_visible = !self.terminal_visible;
        self.save_state(cx);
        if !self.terminal_visible {
            return self.focus_pane(self.focused_pane, window, cx);
        }
        let terminal = match &self.terminal {
            Some(terminal) => terminal.clone(),
            None => {
                let (shell, cwd) = (self.settings.terminal.shell.clone(), self.project.clone());
                let terminal = cx.new(|cx| TerminalView::new(shell, cwd, cx));
                self.terminal = Some(terminal.clone());
                terminal
            }
        };
        terminal.update(cx, |t, cx| t.activate(window, cx));
        cx.notify();
    }

    /// Drops the focused pane's session from the panes and the open list (the agent keeps it in
    /// its history). With more than one pane, the pane goes too. A "Preparing…" pane's session
    /// never opens. R20: in an editor pane it closes the active tab instead (the pane goes with
    /// the last one).
    fn close_session(&mut self, _: &CloseSession, window: &mut Window, cx: &mut Context<Self>) {
        let ix = self.focused_pane;
        match &self.panes[ix] {
            Pane::Editor(pane) => return pane.update(cx, |p, cx| p.close(p.active, window, cx)),
            Pane::Session(session) => {
                session.read(cx).stop();
                self.sessions.retain(|s| s != session);
                self.sync_agents(cx);
            }
            Pane::Preparing { ticket, .. } => {
                self.cancelled.insert(*ticket);
            }
            Pane::Empty(_) => {}
        }
        self.drop_pane(ix, window, cx);
    }

    /// A `new_session` of `agent` returned (`id`: the session it started): the events that came
    /// for that session before. None are kept once no session of the agent is starting.
    fn landed(&mut self, agent: &str, id: Option<&str>) -> Vec<SessionEvent> {
        let early = id.and_then(|id| self.early.remove(&(agent.into(), id.into())));
        let starting = self.starting.entry(agent.into()).or_default();
        *starting = starting.saturating_sub(1);
        if *starting == 0 {
            self.early.retain(|(a, _), _| a != agent);
        }
        early.unwrap_or_default()
    }

    /// Removes pane `ix` (empties it when it is the only one) and focuses its neighbour.
    fn drop_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.panes.len() > 1 {
            self.panes.remove(ix);
        } else {
            self.panes[ix] = Pane::Empty(None);
        }
        self.focus_pane(ix.min(self.panes.len() - 1), window, cx);
        self.save_state(cx);
    }

    fn dirty_tabs(&self, cx: &App) -> Vec<Entity<FileEditor>> {
        let tabs = self.panes.iter().flat_map(|pane| match pane {
            Pane::Editor(pane) => pane.read(cx).tabs.clone(),
            _ => Vec::new(),
        });
        tabs.filter(|tab| tab.read(cx).dirty).collect()
    }

    /// Quits, closes the window or opens another project (R28, R29): at once when no tab holds
    /// unsaved edits, else after asking "Save all / Discard / Cancel". Save all stops, and
    /// everything stays, at the first save that fails.
    pub(crate) fn leave(&mut self, then: Leave, window: &mut Window, cx: &mut Context<Self>) {
        // R41: opening the folder already open changes nothing.
        if let Leave::Open(project) = &then
            && canonical(project) == self.project
        {
            return;
        }
        let dirty = self.dirty_tabs(cx);
        if dirty.is_empty() {
            return self.proceed(then, window, cx);
        }
        let title = match dirty.len() {
            1 => format!("Save changes to {}?", dirty[0].read(cx).name()),
            n => format!("Save changes to {n} files?"),
        };
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let answer = |save: bool| {
                let (this, then, dirty) = (this.clone(), then.clone(), dirty.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    let saved =
                        |tab: &Entity<FileEditor>| tab.update(cx, |t, cx| t.save(window, cx));
                    if save && !dirty.iter().all(saved) {
                        return;
                    }
                    this.update(cx, |this, cx| this.proceed(then.clone(), window, cx))
                        .ok();
                }
            };
            dialog
                .title(title.clone())
                .child("Your changes are lost if you don't save them.")
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("discard")
                                .label("Discard")
                                .on_click(answer(false)),
                        )
                        .child(
                            Button::new("save-all")
                                .primary()
                                .label("Save all")
                                .on_click(answer(true)),
                        ),
                )
        });
    }

    fn proceed(&mut self, then: Leave, window: &mut Window, cx: &mut Context<Self>) {
        match then {
            Leave::Quit => cx.quit(),
            Leave::Close => window.remove_window(),
            Leave::Open(project) => self.set_project(project, window, cx),
        }
    }

    /// ⌘S: saves the focused editor pane's active tab.
    fn save(&mut self, _: &Save, window: &mut Window, cx: &mut Context<Self>) {
        if let Pane::Editor(pane) = &self.panes[self.focused_pane] {
            pane.update(cx, |p, cx| p.save(window, cx));
        }
    }

    /// Opens `path` with the caret at `line` (1-based), in the editor pane: its tab when the
    /// file has one, else a new tab. Without an editor pane, the first empty pane becomes one,
    /// else a new pane to the right; at `MAX_PANES` the last pane does. The file is read on the
    /// background executor; one that cannot open says why in a notice.
    pub fn open_file(
        &mut self,
        path: PathBuf,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open(path, line, false, window, cx)
    }

    /// R21: writes a tool call's output to `<config dir>/scratch/<session>-<tool>.txt`, off the
    /// main thread, and opens it read-only. The ids come from the agent: only their letters,
    /// digits, `-` and `_` name the file, so it stays in `scratch/`.
    fn open_output(
        &mut self,
        session: &str,
        tool: &str,
        text: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let safe = |id: &str| -> String {
            let ok = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
            id.chars().map(|c| if ok(c) { c } else { '_' }).collect()
        };
        let dir = self.user_dir.join("scratch");
        let path = dir.join(format!("{}-{}.txt", safe(session), safe(tool)));
        cx.spawn_in(window, async move |this, cx| {
            let target = path.clone();
            let written = cx
                .background_spawn(async move {
                    std::fs::create_dir_all(&dir)?;
                    std::fs::write(&target, text)
                })
                .await;
            this.update_in(cx, |this, window, cx| match written {
                Ok(()) => this.open(path, None, true, window, cx),
                Err(e) => {
                    let notice = format!("Could not write {}: {e}", path.display());
                    window.push_notification(Notification::error(notice), cx)
                }
            })
            .ok();
        })
        .detach();
    }

    /// `open_file`; a `scratch` file (a tool's output) is read-only whatever its size.
    fn open(
        &mut self,
        path: PathBuf,
        line: Option<u32>,
        scratch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // One tab per file, named as the watcher reports it (`/private/var`, not `/var`).
        let path = canonical(&path);
        if self.reveal(&path, line, window, cx) {
            return;
        }
        cx.spawn_in(window, async move |this, cx| {
            let read = path.clone();
            let loaded = cx
                .background_spawn(async move { editor::load(&read) })
                .await;
            this.update_in(cx, |this, window, cx| {
                let loaded = match loaded {
                    Ok(loaded) => loaded,
                    Err(e) => {
                        let name = folder_name(&path);
                        let notice = Notification::error(e.to_string()).title(name);
                        return window.push_notification(notice, cx);
                    }
                };
                // Opened meanwhile (a double click): one tab per file.
                if !this.reveal(&path, line, window, cx) {
                    #[cfg(feature = "demo")]
                    let started = std::time::Instant::now();
                    let tab =
                        cx.new(|cx| FileEditor::new(path.clone(), loaded, scratch, window, cx));
                    let pane = this.editor_pane(window, cx);
                    pane.update(cx, |p, cx| p.add(tab, window, cx));
                    this.reveal(&path, line, window, cx);
                    #[cfg(feature = "demo")]
                    eprintln!(
                        "wai: {} took {:?} on the main thread",
                        path.display(),
                        started.elapsed()
                    );
                }
            })
            .ok();
        })
        .detach();
    }

    /// Focuses `path`'s tab, at `line`, if a pane has one.
    fn reveal(
        &mut self,
        path: &Path,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        let found = self
            .panes
            .iter()
            .enumerate()
            .find_map(|(ix, pane)| match pane {
                Pane::Editor(pane) => Some((ix, pane.clone(), pane.read(cx).find(path, cx)?)),
                _ => None,
            });
        let Some((ix, pane, tab)) = found else {
            return false;
        };
        pane.update(cx, |p, cx| p.activate(tab, window, cx));
        self.focus_pane(ix, window, cx);
        if let Some(line) = line {
            let tab = pane.read(cx).tabs[tab].clone();
            tab.update(cx, |tab, cx| tab.go_to(line, window, cx));
        }
        true
    }

    /// The editor pane, made from the first empty pane, else a new pane to the right, else
    /// (at `MAX_PANES`) the last pane, whose session stays in the open list.
    fn editor_pane(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Entity<EditorPane> {
        let found = self.panes.iter().find_map(|pane| match pane {
            Pane::Editor(pane) => Some(pane.clone()),
            _ => None,
        });
        if let Some(pane) = found {
            return pane;
        }
        let pane = cx.new(|_| EditorPane::new());
        cx.subscribe_in(&pane, window, |this, pane, _: &Emptied, window, cx| {
            let ix = (this.panes.iter()).position(|p| matches!(p, Pane::Editor(e) if e == pane));
            if let Some(ix) = ix {
                this.drop_pane(ix, window, cx);
            }
        })
        .detach();
        let ix = match self.panes.iter().position(|p| matches!(p, Pane::Empty(_))) {
            Some(ix) => ix,
            None if self.panes.len() < MAX_PANES => {
                self.panes.push(Pane::Empty(None));
                self.panes.len() - 1
            }
            None => self.panes.len() - 1,
        };
        self.panes[ix] = Pane::Editor(pane.clone());
        self.save_state(cx);
        pane
    }

    fn focus_next_pane(&mut self, _: &FocusNextPane, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_pane((self.focused_pane + 1) % self.panes.len(), window, cx);
    }

    /// Shows `project`, closing the old project's files: callers ask about unsaved edits first
    /// (`leave`).
    pub fn set_project(&mut self, project: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.panes.iter().position(|p| matches!(p, Pane::Editor(_))) {
            self.drop_pane(ix, window, cx);
        }
        window.set_window_title(&title(&project));
        self.project = canonical(&project);
        // The next shell starts there; a running one stays where it is.
        if let Some(terminal) = &self.terminal {
            terminal.update(cx, |t, _| t.cwd = self.project.clone());
        }
        let root = self.project.clone();
        self.search
            .update(cx, |search, cx| search.set_root(root.clone(), cx));
        self.files.update(cx, |files, cx| files.set_root(root, cx));
        self.watch_project(window, cx);
        let (templates, notices) = load_templates(&self.user_dir, Some(&self.project));
        self.templates = templates;
        for notice in notices {
            window.push_notification(Notification::warning(notice).autohide(false), cx);
        }
        self.suggest_extensions(window, cx);
        let project = self.project.clone();
        self.ext_host
            .update(cx, |host, cx| host.set_project(project, cx));
        self.save_state(cx);
        cx.notify();
    }

    pub(crate) fn extensions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ExtensionsView> {
        if let Some(view) = &self.extensions {
            return view.clone();
        }
        let (dir, host) = (self.user_dir.clone(), self.ext_host.clone());
        let view = cx.new(|cx| ExtensionsView::new(&dir, host, window, cx));
        self.extensions = Some(view.clone());
        view
    }

    /// Starts the enabled extensions' processes: once the window is open, like the agent.
    pub fn start_extensions(&mut self, cx: &mut Context<Self>) {
        let list = match &self.extensions {
            Some(view) => view.read(cx).store().list(),
            None => wai_ext::ExtStore::open(&self.user_dir).list(),
        };
        self.ext_host.update(cx, |host, cx| host.sync(&list, cx));
    }

    /// Shows the Extensions view in the sidebar.
    pub(crate) fn show_extensions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ExtensionsView> {
        self.sidebar_tab = SidebarTab::Extensions;
        cx.notify();
        self.extensions(window, cx)
    }

    /// R15: the project's `.wai/extensions.toml` only suggests. Each extension not installed yet
    /// gets a notice whose Review stages it and asks for consent.
    pub(crate) fn suggest_extensions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (suggestions, notices) = wai_ext::load_suggestions(&self.project);
        for notice in notices {
            window.push_notification(Notification::warning(notice).autohide(false), cx);
        }
        if suggestions.is_empty() {
            return;
        }
        let view = self.extensions(window, cx);
        let installed = view.read(cx).store().list();
        let new = suggestions
            .into_iter()
            .filter(|s| !installed.iter().any(|i| i.manifest.id == s.id));
        for (ix, suggestion) in new.enumerate() {
            let view = view.clone();
            let text = format!("This project suggests extension {}", suggestion.id.as_str());
            let notice = Notification::info(text).action(move |_, _, cx| {
                let (view, suggestion, notice) = (view.clone(), suggestion.clone(), cx.entity());
                Button::new(("suggest-review", ix))
                    .label("Review")
                    .on_click(move |_, window, cx| {
                        notice.update(cx, |n, cx| n.dismiss(window, cx));
                        let (source, id) = (suggestion.source.clone(), suggestion.id.clone());
                        view.update(cx, |v, cx| v.review(source, Some(id), window, cx));
                    })
            });
            window.push_notification(notice, cx);
        }
    }

    fn open_folder(&mut self, _: &OpenFolder, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(project) = paths.pop() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.leave(Leave::Open(project), window, cx)
            })
            .ok();
        })
        .detach();
    }

    /// What an empty pane says is wrong with the default agent, if anything.
    fn agent_problem(&self, cx: &App) -> Option<String> {
        let agent = &self.settings.default_agent;
        match self.registry.read(cx).status(agent)? {
            AgentStatus::SignedOut => Some(signed_out(agent)),
            AgentStatus::Failed(error) => Some(error.clone()),
            _ => None,
        }
    }

    fn empty_pane(
        &self,
        ix: usize,
        error: Option<&String>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let problem = error.cloned().or_else(|| self.agent_problem(cx));
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap_3()
            .child(div().text_color(theme.muted_foreground).child(START_HINT))
            .child(
                Button::new(("new-session", ix))
                    .primary()
                    .label("New session")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.focused_pane = ix;
                        this.new_session(&NewSession, window, cx);
                    })),
            )
            .children(problem.map(|p| div().max_w(px(480.)).text_color(theme.danger).child(p)))
    }
}

pub(crate) fn folder_name(project: &Path) -> String {
    project
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| project.display().to_string())
}

fn title(project: &Path) -> String {
    format!("wai — {}", folder_name(project))
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let sidebar = self.render_sidebar(cx);
        let theme = cx.theme();
        let split = self.panes.len() > 1;
        let panes = self.panes.iter().enumerate().map(|(ix, pane)| {
            let focused = split && ix == self.focused_pane;
            resizable_panel().child(
                v_flex()
                    .debug_selector(move || format!("pane-{ix}"))
                    .size_full()
                    .when(split, |el| {
                        el.border_t_2().border_color(if focused {
                            theme.blue
                        } else {
                            transparent_black()
                        })
                    })
                    // Clicking into a pane focuses it, caret included, whatever the click lands
                    // on; in the focused pane, a click on nothing focusable (the workspace root
                    // took the focus) gives the caret back too. Deferred past the click, which
                    // may also close a pane.
                    .capture_any_mouse_down(cx.listener(move |_, _, window, cx| {
                        cx.defer_in(window, move |this, window, cx| {
                            let away = this.focused_pane != ix || this.focus.is_focused(window);
                            if ix < this.panes.len() && away {
                                this.focus_pane(ix, window, cx)
                            }
                        });
                    }))
                    .child(match pane {
                        Pane::Session(session) => session.clone().into_any_element(),
                        Pane::Editor(pane) => pane.clone().into_any_element(),
                        Pane::Preparing { agent, .. } => v_flex()
                            .size_full()
                            .items_center()
                            .justify_center()
                            .text_color(theme.muted_foreground)
                            .child(format!("Preparing {agent}…"))
                            .into_any_element(),
                        Pane::Empty(error) => {
                            self.empty_pane(ix, error.as_ref(), cx).into_any_element()
                        }
                    }),
            )
        });
        let terminal = div()
            .debug_selector(|| "terminal".into())
            .size_full()
            .children(self.terminal.clone());

        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::new_session))
            .on_action(cx.listener(Self::toggle_terminal))
            .on_action(cx.listener(Self::close_session))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::focus_next_pane))
            .on_action(cx.listener(Self::open_folder))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                h_resizable("workspace")
                    .child(
                        resizable_panel()
                            .size(px(SIDEBAR_WIDTH))
                            .fixed()
                            .child(sidebar),
                    )
                    .child(
                        resizable_panel().child(
                            v_resizable("center")
                                .child(resizable_panel().child(
                                    // Keyed by count: each split starts with equal widths.
                                    h_resizable(("panes", self.panes.len())).children(panes),
                                ))
                                .child(
                                    resizable_panel()
                                        .size(px(TERMINAL_HEIGHT))
                                        .fixed()
                                        .visible(self.terminal_visible)
                                        .child(terminal),
                                ),
                        ),
                    ),
            )
    }
}

fn fork_failure(error: &AgentError) -> String {
    format!("Could not fork session: {error}")
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{AppContext as _, Entity, TestAppContext, VisualTestContext, px, size};
    use wai_agent::{AgentError, PromptBlock, SessionEvent, SessionSummary};
    use wai_settings::{AgentConfig, AppState, OpenSession, Settings};

    use super::{Pane, Workspace};
    use crate::agents::{AgentStatus, RegistryEvent};
    use crate::app;
    use crate::chat::{ChatItem, NoticeKind, TurnState};
    use crate::notify;
    use crate::sidebar::canonical;

    pub(crate) fn open(
        cx: &mut TestAppContext,
        settings: Settings,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        open_at(cx, settings, None)
    }

    /// `open`, keeping the state in `state_path` (never the real `~/.config/wai/state.json`).
    fn open_at(
        cx: &mut TestAppContext,
        mut settings: Settings,
        state_path: Option<PathBuf>,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        // The terminal never loads the developer's shell or its rc files.
        let shell = if cfg!(windows) { "cmd.exe" } else { "/bin/sh" };
        settings.terminal.shell = Some(shell.into());
        cx.update(|cx| app::init(&settings, cx));
        let project = std::env::temp_dir();
        // Under a `Root`, like the app's window: notifications need its state.
        let (_, workspace) = cx
            .update(|cx| {
                gpui_kit::open_window(Default::default(), cx, |window, cx| {
                    // No global git excludes: they come from the developer's ~/.gitconfig.
                    cx.new(|cx| Workspace::new(project, settings, state_path, false, window, cx))
                })
            })
            .expect("open window");
        // set_project re-reads the user's templates: never the developer's.
        workspace.update(cx, |w, _| {
            w.user_dir = std::env::temp_dir().join("wai-no-config")
        });
        let window = cx.update(|cx| cx.windows()[0]);
        let cx = Box::leak(Box::new(VisualTestContext::from_window(window, cx)));
        (workspace, cx)
    }

    /// Settings whose default agent `fake` is `wai-fake-agent <flag>`.
    pub(crate) fn fake_settings(flag: &str) -> Settings {
        // `target/<profile>/wai-fake-agent`, next to the test binary's `deps/`.
        let name = format!("wai-fake-agent{}", std::env::consts::EXE_SUFFIX);
        let fake: PathBuf = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|dir| dir.join(&name))
            .find(|path| path.is_file())
            .expect("run `cargo build -p wai-agent --bin wai-fake-agent` first");
        let command = format!("'{}' {flag}", fake.display());
        Settings {
            default_agent: "fake".into(),
            agents: [("fake".into(), AgentConfig { command })].into(),
            ..Default::default()
        }
    }

    /// Real agent I/O runs outside the test scheduler: sleep, run, check, for up to 10 s.
    pub(crate) fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        mut done: impl FnMut(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(cx) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
            // Test timers run on a fake clock: keep it in step with the real one.
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
        }
    }

    /// The fake agent with `n` history sessions, all in the test project (the temp dir).
    pub(crate) fn history_settings(n: usize) -> Settings {
        let cwd = std::env::temp_dir();
        fake_settings(&format!(
            "--sessions {n} --sessions-cwd '{}'",
            cwd.display()
        ))
    }

    fn history_rows(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
        workspace.read_with(cx, |w, _| w.history_rows().len())
    }

    /// Starts the agent and waits for its `n` history rows; returns them.
    pub(crate) fn load_history(
        workspace: &Entity<Workspace>,
        n: usize,
        cx: &mut VisualTestContext,
    ) -> Vec<SessionSummary> {
        workspace.update(cx, |w, cx| w.start_agent(cx));
        wait_until(cx, "the history", |cx| history_rows(workspace, cx) == n);
        workspace.read_with(cx, |w, _| w.history_rows().into_iter().cloned().collect())
    }

    #[gpui_kit::test]
    fn new_session_appears_in_open_list(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));

        cx.simulate_keystrokes("secondary-n");
        workspace.read_with(cx, |w, _| {
            assert!(
                matches!(&w.panes[0], Pane::Preparing { agent, .. } if agent == "fake"),
                "the pane is reserved while the agent prepares the session"
            )
        });
        // Ignored: the pane is already waiting for a session.
        cx.simulate_keystrokes("secondary-n");

        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.sessions.len(), 1);
            assert_eq!(w.panes[0].session(), w.sessions.first());
        });
    }

    /// An agent may describe a session (title, usage, commands) before `new_session` returns:
    /// those events wait for the session, up to 1,000 of them.
    #[gpui_kit::test]
    fn events_before_the_session_lands_are_replayed(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());

        cx.simulate_keystrokes("secondary-n");
        let preparing = |w: &Workspace, _: &_| matches!(w.panes[0], Pane::Preparing { .. });
        assert!(workspace.read_with(cx, preparing));
        registry.update(cx, |_, cx| {
            for i in 0..=1_000 {
                cx.emit(RegistryEvent::Session {
                    agent: "fake".into(),
                    session: "fake-1".into(),
                    event: SessionEvent::Title(format!("title {i}")),
                });
            }
        });

        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        let title = workspace.read_with(cx, |w, cx| {
            w.panes[0].session().unwrap().read(cx).model.title.clone()
        });
        // The 1,001st event was dropped.
        assert_eq!(title.as_deref(), Some("title 999"));
    }

    /// ⌘W on a "Preparing…" pane cancels the session: it never opens, even once it lands.
    #[gpui_kit::test]
    fn close_while_preparing_cancels_the_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));

        cx.simulate_keystrokes("secondary-n secondary-w");
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[..], [Pane::Empty(None)]))
        });
        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "both sessions to land", |cx| {
            workspace.read_with(cx, |w, _| {
                w.starting.get("fake") == Some(&0) && w.panes[0].session().is_some()
            })
        });

        workspace.read_with(cx, |w, _| {
            assert_eq!(w.sessions.len(), 1, "only the second session opens");
            assert_eq!(w.panes[0].session(), w.sessions.first());
        });
    }

    fn template(name: &str, agent: Option<&str>, prompt: &str) -> wai_settings::Template {
        wai_settings::Template {
            name: name.into(),
            agent: agent.map(Into::into),
            prompt: prompt.into(),
        }
    }

    #[gpui_kit::test]
    fn template_prefills_prompt_without_sending(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        // No templates, no menu.
        assert!(!cx.debug_bounds("sidebar-templates").is_some());
        workspace.update(cx, |w, cx| {
            w.templates = vec![template("review", None, "Review this diff\nline 2")];
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("sidebar-templates").is_some());

        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.start_template(0, window, cx));
        });
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        workspace.read_with(cx, |w, cx| {
            let session = w.panes[0].session().unwrap().read(cx);
            assert_eq!(session.agent, "fake");
            assert_eq!(session.draft(cx), "Review this diff\nline 2");
            assert!(session.model.items.is_empty(), "nothing was sent");
            assert_eq!(session.model.state, TurnState::Idle);
        });
    }

    #[gpui_kit::test]
    fn template_unknown_agent_is_error(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        workspace.update(cx, |w, _| {
            w.templates = vec![template("review", Some("ghost"), "x")];
        });
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.start_template(0, window, cx));
        });
        workspace.read_with(cx, |w, _| {
            assert!(matches!(
                &w.panes[0],
                Pane::Empty(Some(m)) if m == "Template \"review\" uses unknown agent \"ghost\""
            ));
            assert!(w.sessions.is_empty());
        });
    }

    #[gpui_kit::test]
    fn templates_reload_on_project_switch(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        let (a, b, c) = (tmp_project("a"), tmp_project("b"), tmp_project("c"));
        let toml = |p: &PathBuf, body: &str| {
            std::fs::create_dir_all(p.join(".wai")).unwrap();
            std::fs::write(p.join(".wai/templates.toml"), body).unwrap();
        };
        toml(&a, "[[template]]\nname = \"alpha\"\n");
        toml(
            &b,
            "[[template]]\nname = \"beta\"\n[[template]]\nname = \"gamma\"\n",
        );
        toml(&c, "[[template]]\nprompt = 3\n");
        let names = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| {
                w.templates
                    .iter()
                    .map(|t| t.name.clone())
                    .collect::<Vec<_>>()
            })
        };
        let switch = |p: &PathBuf, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                workspace.update(cx, |w, cx| w.set_project(p.clone(), window, cx))
            })
        };
        switch(&a, cx);
        assert_eq!(names(cx), ["alpha"]);
        switch(&b, cx);
        assert_eq!(names(cx), ["beta", "gamma"]);
        switch(&c, cx);
        assert!(names(cx).is_empty(), "the malformed file adds nothing");
        for p in [a, b, c] {
            std::fs::remove_dir_all(p).ok();
        }
    }

    fn tmp_project(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("wai-tpl-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    #[gpui_kit::test]
    fn history_filters_by_project(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(25));

        // The project is the sessions' folder.
        load_history(&workspace, 25, cx);

        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project("/nonexistent".into(), window, cx))
        });
        assert_eq!(history_rows(&workspace, cx), 0);

        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click("all-projects", cx);
        });
        assert_eq!(history_rows(&workspace, cx), 25);
    }

    #[gpui_kit::test]
    fn open_to_side_caps_at_four_panes(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(5));
        let rows = load_history(&workspace, 5, cx);

        let mut panes = Vec::new();
        for row in &rows {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, true, window, cx)));
            panes.push(workspace.read_with(cx, |w, _| w.panes.len()));
        }

        // The empty pane takes the first; at four, the focused pane takes the fifth.
        assert_eq!(panes, [1, 2, 3, 4, 4]);
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), 5);

        let focused = workspace.read_with(cx, |w, _| w.focused_pane);
        cx.simulate_keystrokes("secondary-alt-right");
        assert_eq!(
            workspace.read_with(cx, |w, _| w.focused_pane),
            (focused + 1) % 4
        );
    }

    #[gpui_kit::test]
    fn fork_opens_side_pane_with_history(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(3));
        let rows = load_history(&workspace, 3, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));

        // The sidebar offers Fork on the history rows and on the open session.
        cx.update(|window, cx| window.click(("history-fork", 1usize), cx));
        wait_until(cx, "the fork", |cx| {
            workspace.read_with(cx, |w, _| w.panes.len() == 2)
        });
        let session = workspace
            .read_with(cx, |w, _| w.panes[1].session().cloned())
            .expect("the fork opens to the side");
        wait_until(cx, "the replay", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle && s.model.items.len() == 2
            })
        });
        session.read_with(cx, |s, _| {
            assert!(s.id.starts_with("fork-"), "{}", s.id);
            assert_eq!(s.model.title.as_deref(), Some("Session 1 (fork)"));
            assert_eq!(
                s.model.items[0],
                ChatItem::User {
                    text: "previous prompt".into()
                }
            );
        });
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.sessions.len(), 2);
            assert_eq!(w.focused_pane, 1);
        });

        // An open session forks too, by its row.
        cx.update(|window, cx| window.click(("open-fork", 0usize), cx));
        wait_until(cx, "the second fork", |cx| {
            workspace.read_with(cx, |w, _| w.panes.len() == 3)
        });
        let title = workspace.read_with(cx, |w, cx| {
            w.panes[2].session().unwrap().read(cx).model.title.clone()
        });
        assert_eq!(title.as_deref(), Some("Session 0 (fork)"));
    }

    #[gpui_kit::test]
    fn fork_failure_shows_a_notice_and_adds_no_pane(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let cwd = std::env::temp_dir();
        let flags = format!(
            "--fail-fork --sessions 2 --sessions-cwd '{}'",
            cwd.display()
        );
        let (workspace, cx) = open(cx, fake_settings(&flags));
        load_history(&workspace, 2, cx);

        cx.update(|window, cx| window.click(("history-fork", 0usize), cx));
        wait_until(cx, "the notice", |cx| {
            cx.update(|window, cx| !window.notifications(cx).is_empty())
        });
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 1);
            assert!(w.sessions.is_empty());
        });
        let error = AgentError::Protocol("Internal error".into());
        assert_eq!(
            super::fork_failure(&error),
            "Could not fork session: agent protocol error: Internal error"
        );
    }

    /// Fork loads the copy to replay it, so it needs both capabilities, on the session's agent.
    #[gpui_kit::test]
    fn fork_needs_fork_and_load_on_the_sessions_agent(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let mut settings = fake_settings("");
        let plain = fake_settings("--no-load").agents["fake"].clone();
        settings.agents.insert("plain".into(), plain);
        let (workspace, cx) = open(cx, settings);
        workspace.update(cx, |w, cx| {
            w.registry.update(cx, |r, cx| {
                r.handle("fake", cx);
                r.handle("plain", cx);
            })
        });
        wait_until(cx, "both agents", |cx| {
            workspace.read_with(cx, |w, cx| {
                ["fake", "plain"]
                    .iter()
                    .all(|a| matches!(w.registry.read(cx).status(a), Some(AgentStatus::Ready(_))))
            })
        });
        workspace.read_with(cx, |w, cx| {
            assert!(w.can_fork("fake", cx));
            assert!(!w.can_fork("plain", cx), "fork without load is hidden");
            assert!(!w.can_fork("unknown", cx));
        });
    }

    #[gpui_kit::test]
    fn opens_with_one_empty_pane(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 1);
            assert!(matches!(w.panes[0], Pane::Empty(None)));
        });
    }

    #[gpui_kit::test]
    fn toggle_terminal_action_flips_visibility(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the terminal runs a real shell
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        assert!(workspace.read_with(cx, |w, _| w.terminal_visible));
        cx.simulate_keystrokes("ctrl-`");
        assert!(!workspace.read_with(cx, |w, _| w.terminal_visible));
    }

    #[gpui_kit::test]
    fn sidebar_and_terminal_keep_260_px(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the terminal runs a real shell
        let (_, cx) = open(cx, Settings::default());
        cx.simulate_resize(size(px(1400.), px(900.)));
        cx.simulate_keystrokes("ctrl-`");
        for window in [size(px(1400.), px(900.)), size(px(1000.), px(600.))] {
            cx.simulate_resize(window);
            cx.run_until_parked();
            let sidebar = cx.debug_bounds("sidebar").expect("sidebar drawn");
            let terminal = cx.debug_bounds("terminal").expect("terminal drawn");
            assert_eq!(
                (sidebar.size.width, terminal.size.height),
                (px(260.), px(260.)),
                "window {window:?}"
            );
        }
    }

    #[gpui_kit::test]
    fn registry_reports_auth_required(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--require-auth"));
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());

        cx.simulate_keystrokes("secondary-n");

        wait_until(cx, "SignedOut", |cx| {
            registry.read_with(cx, |r, _| r.status("fake") == Some(&AgentStatus::SignedOut))
        });
    }

    /// claude-agent-acp reports a sign-out when a prompt is sent. After the user signs in, the next
    /// session that starts on the same live process must bring the agent back to `Ready`.
    #[gpui_kit::test]
    fn successful_session_clears_signed_out(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--require-auth-on-prompt"));
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());

        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "a session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        let session = workspace.read_with(cx, |w, cx| {
            w.panes[0].session().unwrap().read(cx).id.clone()
        });
        let handle = registry.update(cx, |r, cx| r.handle("fake", cx)).unwrap();
        handle.prompt(session, vec![PromptBlock::Text("hi".into())]);
        wait_until(cx, "SignedOut", |cx| {
            registry.read_with(cx, |r, _| r.status("fake") == Some(&AgentStatus::SignedOut))
        });
        assert_eq!(
            workspace
                .read_with(cx, |w, cx| w.agent_problem(cx))
                .as_deref(),
            Some(
                "fake is not signed in. Open a terminal and run its login (for Claude: claude /login)."
            )
        );

        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "Ready", |cx| {
            registry.read_with(cx, |r, _| {
                matches!(r.status("fake"), Some(AgentStatus::Ready(_)))
            })
        });
        assert_eq!(workspace.read_with(cx, |w, cx| w.agent_problem(cx)), None);
    }

    /// The process ends ~200 ms after the quit hooks: the agents must be killed by then.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn quit_stops_agents(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("agent.pid");
        let flag = format!("--pid-file '{}'", pid_file.display());
        let (workspace, cx) = open(cx, fake_settings(&flag));
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());
        workspace.update(cx, |w, cx| w.start_agent(cx));
        wait_until(cx, "Ready", |cx| {
            registry.read_with(cx, |r, _| {
                matches!(r.status("fake"), Some(AgentStatus::Ready(_)))
            })
        });
        let pid = std::fs::read_to_string(&pid_file).unwrap();

        // What the platform's quit runs: the quit hooks, then the windows close.
        cx.cx.update(|app| app.shutdown());

        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while alive() {
            assert!(Instant::now() < deadline, "agent {pid} still alive");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// The quit hooks have ~200 ms before the process ends: the agents are signalled first,
    /// so a shell slow to die (this one ignores SIGHUP, which costs portable-pty 200 ms) cannot
    /// hold their kill back.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn quit_signals_agents_before_the_terminal(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent and the shell are real processes
        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("agent.pid");
        let flag = format!("--pid-file '{}'", pid_file.display());
        let (workspace, cx) = open(cx, fake_settings(&flag));
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());
        workspace.update(cx, |w, cx| w.start_agent(cx));
        wait_until(cx, "Ready", |cx| {
            registry.read_with(cx, |r, _| {
                matches!(r.status("fake"), Some(AgentStatus::Ready(_)))
            })
        });
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        // The shell ignores SIGHUP, so the terminal's stop holds it ~200 ms before the kill. It
        // marks the agent's death, which it sees only while it still runs: before that stop.
        let seen = dir.path().join("agent-dead");
        cx.simulate_keystrokes("ctrl-`");
        cx.simulate_input(&format!(
            "trap '' HUP; printf wai-%s watching; while kill -0 {} 2>/dev/null; do :; done; : > '{}'",
            pid.trim(),
            seen.display()
        ));
        cx.simulate_keystrokes("enter");
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().unwrap());
        wait_until(cx, "the watch", |cx| {
            terminal.read_with(cx, |t, _| {
                t.snapshot.rows.iter().any(|row| {
                    let text: String = row.iter().map(|run| run.text.as_str()).collect();
                    text.contains("wai-watching")
                })
            })
        });

        cx.cx.update(|app| app.shutdown());

        assert!(seen.exists(), "the terminal stopped before the agent died");
    }

    /// R41: each agent numbers its requests from 1, so a resolution reaches only the sessions of
    /// the agent it came from. Agents `a` and `b` each ask request #1 (`flag`); `a`'s cancels.
    fn resolution_stays_with_its_agent(flag: &str, cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agents are real processes
        let mut settings = fake_settings(flag);
        let fake = settings.agents.remove("fake").unwrap();
        settings.agents = [("a".into(), fake.clone()), ("b".into(), fake)].into();
        settings.default_agent = "a".into();
        let (workspace, cx) = open(cx, settings);
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());
        let mut sessions = Vec::new();
        for agent in ["a", "b"] {
            cx.update(|window, cx| {
                workspace.update(cx, |w, cx| w.start_session(agent.into(), None, window, cx))
            });
            let find = |cx: &mut VisualTestContext| {
                workspace.read_with(cx, |w, cx| {
                    w.sessions
                        .iter()
                        .find(|s| s.read(cx).agent == agent)
                        .cloned()
                })
            };
            wait_until(cx, "the session", |cx| find(cx).is_some());
            let session = find(cx).unwrap();
            let id = session.read_with(cx, |s, _| s.id.clone());
            let handle = registry.update(cx, |r, cx| r.handle(agent, cx)).unwrap();
            handle.prompt(id, vec![PromptBlock::Text("go".into())]);
            sessions.push(session);
        }
        let request = |ix: usize, cx: &mut VisualTestContext| {
            sessions[ix].read_with(cx, |s, _| {
                s.model.items.iter().find_map(|item| match item {
                    ChatItem::Permission { request, answered } => {
                        Some((request.id, answered.clone()))
                    }
                    ChatItem::Question { request, answered } => {
                        Some((request.id, answered.clone()))
                    }
                    _ => None,
                })
            })
        };
        wait_until(cx, "both requests", |cx| {
            request(0, cx).is_some() && request(1, cx).is_some()
        });
        assert_eq!(request(0, cx), Some((1, None)));
        assert_eq!(request(1, cx), Some((1, None)));

        let (a, id) = sessions[0].read_with(cx, |s, _| (s.agent.clone(), s.id.clone()));
        let handle = registry.update(cx, |r, cx| r.handle(&a, cx)).unwrap();
        handle.cancel(id);
        wait_until(cx, "a's request resolved", |cx| {
            request(0, cx).is_some_and(|(_, answered)| answered.is_some())
        });

        assert_eq!(request(1, cx), Some((1, None)), "b's request still waits");
        let state = sessions[1].read_with(cx, |s, _| s.model.state.clone());
        assert_eq!(state, TurnState::NeedsYou);
    }

    #[gpui_kit::test]
    fn permission_resolution_stays_with_its_agent(cx: &mut TestAppContext) {
        resolution_stays_with_its_agent("--ask-permission", cx);
    }

    #[gpui_kit::test]
    fn question_resolution_stays_with_its_agent(cx: &mut TestAppContext) {
        resolution_stays_with_its_agent("--ask-question", cx);
    }

    /// What `app::run` does once the window is open, but the watcher.
    fn launch(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.start_agent(cx);
                w.restore(window, cx);
            })
        });
    }

    fn saved(agent: &str, id: &str, pane: usize) -> OpenSession {
        OpenSession {
            agent: agent.into(),
            session_id: id.into(),
            cwd: std::env::temp_dir(),
            pane,
        }
    }

    fn pane_session(
        workspace: &Entity<Workspace>,
        ix: usize,
        cx: &mut VisualTestContext,
    ) -> Entity<crate::chat::ChatSession> {
        workspace.read_with(cx, |w, _| {
            w.panes[ix].session().cloned().expect("a session")
        })
    }

    #[gpui_kit::test]
    fn state_saved_on_new_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        // The config dir does not exist yet on a first run.
        let path = dir.path().join("wai").join("state.json");
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));

        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });

        let state = AppState::load(&path);
        assert_eq!(
            state.open_sessions,
            [OpenSession {
                agent: "fake".into(),
                session_id: "fake-1".into(),
                cwd: canonical(&std::env::temp_dir()),
                pane: 0,
            }]
        );
        assert_eq!(state.project, Some(canonical(&std::env::temp_dir())));
    }

    /// Sessions come back into their panes, replayed by `load_session`, without notifying, and
    /// the terminal shows again. A session saved in no pane only joins the open list; an agent the
    /// settings no longer have drops its session.
    #[gpui_kit::test]
    fn restore_reopens_sessions(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let sessions = vec![
            saved("fake", "abc", 0),
            saved("ghost", "gone", 1),
            saved("fake", "def", 1),
            saved("fake", "ghi", super::MAX_PANES),
        ];
        let state = AppState {
            open_sessions: sessions,
            terminal_visible: true,
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));
        cx.deactivate_window();

        launch(&workspace, cx);

        let previous = ChatItem::User {
            text: "previous prompt".into(),
        };
        for ix in 0..2 {
            let session = pane_session(&workspace, ix, cx);
            wait_until(cx, "the replay", |cx| {
                session.read_with(cx, |s, _| {
                    s.model.state == TurnState::Idle && s.model.items.contains(&previous)
                })
            });
        }
        let ids = workspace.read_with(cx, |w, cx| {
            let ids = w
                .panes
                .iter()
                .map(|p| p.session().unwrap().read(cx).id.clone());
            ids.collect::<Vec<_>>()
        });
        assert_eq!(ids, ["abc", "def"]);
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), 3);
        assert!(workspace.read_with(cx, |w, _| w.terminal_visible && w.terminal.is_some()));
        let kept = AppState::load(&path).open_sessions;
        let hidden = saved("fake", "ghi", super::MAX_PANES);
        assert_eq!(
            kept,
            [saved("fake", "abc", 0), saved("fake", "def", 1), hidden]
        );
        assert_eq!(notify::sent(), []);
    }

    /// R14: the state keeps no titles; a restored session takes its title from the history.
    #[gpui_kit::test]
    fn restored_session_takes_history_title(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = AppState {
            open_sessions: vec![saved("fake", "session-1", 0)],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, history_settings(3), Some(path));

        launch(&workspace, cx);

        let session = pane_session(&workspace, 0, cx);
        wait_until(cx, "the history title", |cx| {
            session.read_with(cx, |s, _| s.model.title.as_deref() == Some("Session 1"))
        });
    }

    #[gpui_kit::test]
    fn restore_failure_shows_notice(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = AppState {
            open_sessions: vec![saved("fake", "missing-1", 0)],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));

        launch(&workspace, cx);

        let session = pane_session(&workspace, 0, cx);
        let notice = ChatItem::Notice {
            text: "Could not restore session missing-1".into(),
            kind: NoticeKind::Info,
        };
        wait_until(cx, "the notice", |cx| {
            session.read_with(cx, |s, _| s.model.items.contains(&notice))
        });
        assert!(workspace.read_with(cx, |w, _| w.sessions.is_empty()));
        assert_eq!(AppState::load(&path).open_sessions, []);
    }
}
