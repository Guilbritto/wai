use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::{Path, PathBuf};

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::InputState;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, Theme, h_flex, h_resizable, resizable_panel, v_flex,
    v_resizable,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use serde_json::json;
use unicode_properties::{GeneralCategory, UnicodeGeneralCategory as _};
use wai_agent::{
    AgentError, AgentHandle, BrokerError, QuestionAnswer, SessionEvent, SessionStarted,
    SessionSummary, StopReason,
};
use wai_ext::{CommandSpec, ExtEvent, ExtId, HostEvent, Reply};
use wai_project::{FsChange, ProjectWatcher};
use wai_settings::SessionGroup;
use wai_settings::{
    AgentConfig, AppState, Approval, Change, OpenSession, SeenModel, SeenModes, Settings, Template,
    agent_label, config_dir, is_approved, load_templates, seed_approved, shown, unapproved,
    write_change,
};
use wai_terminal::DEFAULT_BG;

use crate::agents::{AgentRegistry, AgentStatus, RegistryEvent};
use crate::agents_view::{AgentsView, ShowSession};
use crate::app::{
    ClosePane, FocusNextPane, NewSession, OpenFolder, OpenSettings, Quit, Save, ToggleTerminal,
    theme_mode,
};
use crate::aurora;
use crate::chat::{ChatEvent, ChatSession, DraftView, TurnState, signed_out};
use crate::editor::{self, EditorPane, Emptied, FileEditor, Saved, recovery};
use crate::ext_host::{ExtHost, SessionCall};
use crate::extensions_view::{ExtensionsView, InstallPlugin};
use crate::files::{FileTree, OpenFile};
use crate::git;
use crate::layout::{self, Slot, Zone};
use crate::notify::{self, should_notify};
use crate::search_view::{OpenMatch, SearchView};
use crate::settings_view::{
    AgentRow, RowStatus, Section, SettingsData, SettingsEvent, SettingsView, open_modal,
};
use crate::sidebar::{SidebarTab, canonical};
use crate::terminal_view::{self, TerminalView};
use crate::topbar::BRAND;
use crate::update::{self, Outcome, UpdateStatus};

pub(crate) const CHOOSE_AI: &str = "Choose an AI to start a session.";
const SIDEBAR_WIDTH: f32 = 280.;
const TERMINAL_HEIGHT: f32 = 260.;
pub const MAX_PANES: usize = 4;
/// Events kept per session id that is not open yet (see `Workspace::early`).
const EARLY_EVENTS: usize = 1_000;
pub(crate) const START_KEY: &str = if cfg!(target_os = "macos") {
    "⌘N"
} else {
    "Ctrl+N"
};

pub enum Pane {
    /// The start hint, and why the last start failed, if it did.
    Empty(Option<String>),
    /// Reserved while `new_session` runs; `ticket` tells this request's placeholder apart.
    /// `by_extension`: the extension whose `sessions/open` this is; it gives way to the user's
    /// own start.
    Preparing {
        agent: String,
        ticket: u64,
        by_extension: Option<ExtId>,
    },
    Session(Entity<ChatSession>),
    /// Open files in tabs; one pane at most. Not kept between runs.
    Editor(Entity<EditorPane>),
    /// A session that starts when sent; its view is also in `Workspace::drafts`.
    Draft(Entity<DraftView>),
}

impl Pane {
    pub fn session(&self) -> Option<&Entity<ChatSession>> {
        match self {
            Pane::Session(session) => Some(session),
            _ => None,
        }
    }

    pub fn draft(&self) -> Option<&Entity<DraftView>> {
        match self {
            Pane::Draft(draft) => Some(draft),
            _ => None,
        }
    }
}

/// R89: a session dragged from an Open row, a pane's header or a History row onto a pane.
#[derive(Clone)]
pub(crate) struct DraggedSession {
    pub(crate) title: SharedString,
    pub(crate) source: Dragged,
}

#[derive(Clone)]
pub(crate) enum Dragged {
    Open(Entity<ChatSession>),
    /// A draft in the Open list, moved as an open session is.
    Draft(Entity<DraftView>),
    /// Resumed where it drops, as a click on its row resumes it.
    History(SessionSummary),
}

impl DraggedSession {
    /// The `on_drag` preview: a pill with the title.
    pub(crate) fn preview(
        &self,
        offset: Point<Pixels>,
        _: &mut Window,
        cx: &mut App,
    ) -> Entity<Preview> {
        cx.new(|_| Preview(self.title.clone(), offset))
    }
}

/// GPUI draws a drag's view where its source was grabbed: the padding brings the pill under
/// the pointer.
pub(crate) struct Preview(SharedString, Point<Pixels>);

impl Render for Preview {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let pill = aurora::pill(cx).child(self.0.clone());
        div().pl(self.1.x).pt(self.1.y).child(pill)
    }
}

/// What the Open list renames in place.
#[derive(Clone, PartialEq)]
pub(crate) enum Renamed {
    Session(Entity<ChatSession>),
    Group(usize),
}

/// What waits on the question about unsaved edits.
#[derive(Clone)]
pub(crate) enum Leave {
    Quit,
    Close,
    Open(PathBuf),
    /// Quit to install the staged update (asked about like a quit).
    Update,
}

/// The title of the dialog that asks before quitting or closing with `n` busy sessions.
pub(crate) fn still_working(n: usize) -> String {
    match n {
        1 => "1 session is still working".into(),
        n => format!("{n} sessions are still working"),
    }
}

/// Whether the batch names `path`, or lost track of what changed.
/// What the terminal runs to open Claude Code session `id` again; the shell may be anywhere.
pub(crate) fn resume_command(cwd: &Path, id: &str) -> String {
    format!(
        "cd {} && claude --resume {}",
        wai_agent::quote(cwd),
        wai_agent::quote(id)
    )
}

fn touches(batch: &[FsChange], path: &Path) -> bool {
    batch.iter().any(|change| match change {
        FsChange::Rescan => true,
        FsChange::Changed(paths) => paths.iter().any(|p| p == path),
    })
}

/// The window's root view: the top bar over sidebar | chat panes over the terminal panel, each a
/// card on the aurora.
pub struct Workspace {
    /// Canonical, so history cwds compare with it (R3).
    pub project: PathBuf,
    pub settings: Settings,
    /// Why `settings.toml` did not load; Settings shows it and stays read-only.
    pub settings_error: Option<String>,
    /// What the update check found, staged or announced; Settings shows it.
    pub(crate) update: UpdateStatus,
    /// Why this copy cannot install an update, for Settings. Refreshed when Settings opens and on
    /// Check now: finding it writes a probe file, so it is not done per render.
    pub(crate) update_blocker: Option<String>,
    /// The "New from template" menu's entries; empty hides the menu.
    pub templates: Vec<Template>,
    /// The config dir: the user's `templates.toml` (re-read on a project switch) and `scratch/`,
    /// where tool outputs open from (R21; cleared at launch).
    pub user_dir: PathBuf,
    /// Whose `~/.claude` the Claude pages of Settings read.
    pub(crate) claude_home: PathBuf,
    pub(crate) claude_task: Option<Task<()>>,
    /// The `claude` that `mcp list` runs; none means the one on PATH.
    pub(crate) claude_program: Option<PathBuf>,
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
    /// The Agents view is its own panel right of the panes, not a sidebar tab.
    pub(crate) agents_right: bool,
    /// That panel is hidden.
    pub(crate) agents_collapsed: bool,
    /// Created on first use (the sidebar's Extensions button, a project's suggestion): it reads
    /// `user_dir`'s extensions.
    pub(crate) extensions: Option<Entity<ExtensionsView>>,
    /// The enabled extensions' processes.
    pub(crate) ext_host: Entity<ExtHost>,
    /// The project's one watcher and the task that hands its changes out; `None` when the
    /// project cannot be watched.
    watch: Option<(ProjectWatcher, Task<()>)>,
    settings_watch: Option<(ProjectWatcher, Task<()>)>,
    /// The open Settings modal; the dialog owns the view.
    settings_view: Option<WeakEntity<SettingsView>>,
    /// The load notices already shown: a reload pushes only new ones.
    load_notices: Vec<String>,
    /// The last run's sessions `restore` could not reopen (their agent is not in the settings, or
    /// its command waits for an answer): saved again, for a launch that can.
    unrestored: Vec<OpenSession>,
    #[cfg(test)]
    pub(crate) applied: usize,
    next_ticket: u64,
    /// `new_session` calls in flight, per agent.
    pub(crate) starting: HashMap<String, usize>,
    /// Events for session ids not open yet, per agent: an agent may describe a session (title,
    /// usage, commands) before `new_session` returns. Kept, `EARLY_EVENTS` per id, only while a
    /// session of that agent is starting.
    early: HashMap<(String, String), Vec<SessionEvent>>,
    /// The last session each extension opened: its next one takes that one's pane (R57).
    ext_sessions: HashMap<ExtId, EntityId>,
    /// Where the open sessions, the project and the terminal's visibility are kept between runs;
    /// `None` keeps nothing (tests). Unsaved edits are kept beside it, in `recovery/`.
    state_path: Option<PathBuf>,
    /// Agent id -> the models its last Test saw; kept in `state.json`.
    pub models_seen: BTreeMap<String, Vec<SeenModel>>,
    /// Agent id -> the modes its last session reported; kept in `state.json`.
    pub modes_seen: BTreeMap<String, SeenModes>,
    /// Agent id -> the model its last draft sent with; kept in `state.json`.
    pub last_models: BTreeMap<String, String>,
    /// Repository root -> the base its last worktree draft sent from; kept in `state.json`.
    pub last_bases: BTreeMap<PathBuf, String>,
    /// The Open list's drafts, in the order opened; never saved.
    pub drafts: Vec<Entity<DraftView>>,
    /// The Open list's groups, each with its sessions' ids; kept in `state.json`.
    pub groups: Vec<SessionGroup>,
    /// The Open row or group being renamed, with its input.
    pub(crate) renaming: Option<(Renamed, Entity<InputState>, Subscription)>,
    /// The agent commands and shells the user allowed; kept in `state.json`, never in
    /// `settings.toml`.
    pub approved: BTreeSet<String>,
    /// The shell the terminal runs: the settings' once approved, else the last approved one.
    pub shell: Option<String>,
    /// The values waiting for an answer, the first one's dialog open.
    pub(crate) asking: Vec<Approval>,
    /// Those of `asking` the screen edited: Allow writes them to `settings.toml`.
    edits: Vec<Approval>,
    /// The values declined this run: a reload does not ask about them again.
    declined: BTreeSet<String>,
    /// The next recovery file number to try (`recovery::next_file`).
    next_recovery: u64,
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
        let state = state_path.as_deref().map(AppState::read);
        let approved = match &state {
            // Missing or corrupt: breaking the file must not approve what settings.toml says.
            Some(None) => BTreeSet::new(),
            Some(Some(AppState {
                approved: Some(approved),
                ..
            })) => approved.clone(),
            // A state from before approvals, or none kept (tests): today's values are approved.
            _ => seed_approved(&settings),
        };
        let remembered = state.flatten();
        let configs = approved_configs(&Settings::default().agents, &settings, &approved);
        let registry = cx.new(|_| AgentRegistry::new(configs));
        let shell = settings
            .terminal
            .shell
            .clone()
            .filter(|s| approved.contains(s));
        let routing = cx.subscribe(&registry, |this, _, event, cx| this.route(event, cx));
        // The process ends ~200 ms after the quit hooks: nothing it started may outlive it. The
        // agents are signalled and the extensions killed first: stopping a shell that ignores
        // SIGHUP blocks for 200 ms.
        // Hot-exit: every quit runs these, the ones that never ask too (Dock, logout), so the
        // unsaved edits are kept here; their next launch reopens them.
        let quit = cx.on_app_quit(|this, cx| {
            let agents = this.registry.read(cx).shutdown_all();
            this.ext_host.update(cx, |host, _| host.stop_all());
            for tab in this.dirty_tabs(cx) {
                // Logged; no notice can show now.
                tab.read(cx).keep_edits(cx).ok();
            }
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
                let ask = !this.dirty_tabs(cx).is_empty() || this.busy_sessions(cx) > 0;
                if ask {
                    this.leave(Leave::Close, window, cx);
                }
                !ask
            };
            this.update(cx, ask).unwrap_or(true)
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        window.set_window_title(&title(&project));
        let project = canonical(&project);
        let ext_host = cx.new(|_| ExtHost::new(project.clone()));
        // Its panels are sidebar tabs: a shown one that goes leaves the sessions shown. Its agents
        // and MCP servers follow the enabled extensions.
        let panels = cx.observe(&ext_host, |this: &mut Self, host, cx| {
            if let SidebarTab::Panel(shown) = this.sidebar_tab
                && !(host.read(cx).panels().iter()).any(|p| p.view.entity_id() == shown)
            {
                this.sidebar_tab = SidebarTab::Sessions;
            }
            this.sync_contributions(cx);
            cx.notify();
        });
        let calls = cx.subscribe_in(
            &ext_host,
            window,
            |this, _, call: &SessionCall, window, cx| {
                if let Some((ext, call)) = call.take() {
                    this.session_call(ext, call, window, cx);
                }
            },
        );
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
            claude_home: wai_settings::claude::claude_home(),
            claude_task: None,
            claude_program: None,
            registry,
            agents,
            agents_right: false,
            agents_collapsed: false,
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
            settings_watch: None,
            settings_view: None,
            load_notices: Vec::new(),
            unrestored: Vec::new(),
            #[cfg(test)]
            applied: 0,
            next_ticket: 0,
            starting: HashMap::new(),
            early: HashMap::new(),
            ext_sessions: HashMap::new(),
            modes_seen: remembered
                .as_ref()
                .map(|s| s.modes_seen.clone())
                .unwrap_or_default(),
            last_models: remembered
                .as_ref()
                .map(|s| s.last_models.clone())
                .unwrap_or_default(),
            last_bases: remembered
                .as_ref()
                .map(|s| s.last_bases.clone())
                .unwrap_or_default(),
            models_seen: remembered.map(|s| s.models_seen).unwrap_or_default(),
            drafts: Vec::new(),
            groups: Vec::new(),
            renaming: None,
            approved,
            shell,
            asking: Vec::new(),
            edits: Vec::new(),
            declined: BTreeSet::new(),
            state_path,
            next_recovery: 0,
            settings_error: None,
            update: UpdateStatus::Idle,
            update_blocker: None,
            focus,
            _subscriptions: vec![
                routing,
                quit,
                open_file,
                open_match,
                open_session,
                panels,
                calls,
            ],
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

    /// Watches the user's settings folder; `settings.toml` changing (the screen's write or a
    /// hand edit) reloads the settings. `state.json` shares the folder and never reloads.
    pub fn watch_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let _ = std::fs::create_dir_all(&self.user_dir);
        let file = canonical(&self.user_dir).join("settings.toml");
        let (watcher, changes) = match ProjectWatcher::start(&canonical(&self.user_dir)) {
            Ok(started) => started,
            Err(e) => {
                self.settings_watch = None;
                return eprintln!("wai: cannot watch {}: {e}", self.user_dir.display());
            }
        };
        let task = cx.spawn_in(window, async move |this, cx| {
            while let Ok(change) = changes.recv().await {
                let mut batch = vec![change];
                while let Ok(more) = changes.try_recv() {
                    batch.push(more);
                }
                if !touches(&batch, &file) {
                    continue;
                }
                let reloaded = this.update_in(cx, |this, window, cx| {
                    this.reload_settings(window, cx);
                });
                if reloaded.is_err() {
                    break;
                }
            }
        });
        self.settings_watch = Some((watcher, task));
    }

    /// Reads the layers again. A file that does not load keeps the settings in force.
    pub fn reload_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        match Settings::load(&self.user_dir, Some(&self.project)) {
            Ok((new, notices)) => self.apply_settings(new, notices, window, cx),
            Err(e) => {
                let text = e.to_string();
                window.push_notification(Notification::warning(text.clone()), cx);
                self.settings_error = Some(text);
                self.sync_settings_view(cx);
            }
        }
    }

    /// Every reload ends here. Running sessions keep their agent: only the next start differs.
    pub fn apply_settings(
        &mut self,
        new: Settings,
        notices: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        #[cfg(test)]
        {
            self.applied += 1;
        }
        self.settings_error = None;
        if new.theme != self.settings.theme {
            Theme::change(theme_mode(new.theme), Some(window), cx);
            window.refresh();
        }
        let processing = new.conversation.processing;
        for session in &self.sessions {
            session.update(cx, |s, cx| s.set_processing(processing, cx));
        }
        // What runs now: the old values the questions show.
        let mut old = Settings {
            agents: self.registry.read(cx).configs().clone(),
            ..Settings::default()
        };
        old.terminal.shell = self.shell.clone();
        let chosen =
            new.default_agent.is_some() && new.default_agent != self.settings.default_agent;
        self.settings = new;
        self.enforce(cx);
        for notice in &notices {
            if !self.load_notices.contains(notice) {
                let shown = Notification::warning(notice.clone()).autohide(false);
                window.push_notification(shown, cx);
            }
        }
        self.load_notices = notices;
        if chosen {
            // Picked again, its declined command is asked about again.
            let agent = self.default_agent();
            if let Some(config) = agent.and_then(|id| self.settings.agents.get(&id)) {
                self.declined.remove(&config.command);
            }
        }
        let declined = &self.declined;
        let mut pending = unapproved(&old, &self.settings, &self.approved);
        pending.retain(|a| !declined.contains(approval_value(a)));
        self.ask_approval(pending, window, cx);
        // Asked first: a default whose command waits for an answer starts after it.
        if chosen {
            // The old default's history: its rows would resume with the new one.
            self.history.clear();
            self.history_task = None;
            self.start_agent(cx);
            self.refresh_history(cx);
        }
        self.sync_drafts(cx);
        self.sync_settings_view(cx);
        cx.notify();
    }

    /// Shows the launch's load notices (R15) and records them: a reload pushes only new ones.
    pub fn show_load_notices(
        &mut self,
        notices: Vec<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        for notice in &notices {
            let shown = Notification::warning(notice.clone()).autohide(false);
            window.push_notification(shown, cx);
        }
        self.load_notices = notices;
    }

    /// The registry and the terminal get approved values only: an agent or the shell whose
    /// value is not approved keeps the last approved one (an agent never approved, none).
    fn enforce(&mut self, cx: &mut Context<Self>) {
        let (settings, approved) = (&self.settings, &self.approved);
        self.registry.update(cx, |r, _| {
            let configs = approved_configs(r.configs(), settings, approved);
            r.set_configs(configs)
        });
        let shell = &self.settings.terminal.shell;
        if shell.as_ref().is_none_or(|s| self.approved.contains(s)) {
            self.shell = shell.clone();
        }
        if let Some(terminal) = &self.terminal {
            let shell = self.shell.clone();
            terminal.update(cx, |t, _| t.shell = shell);
        }
    }

    /// Asks whether to run each value, one dialog at a time and in order; a value already
    /// waiting is asked once.
    pub fn ask_approval(
        &mut self,
        pending: Vec<Approval>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let idle = self.asking.is_empty();
        for approval in pending {
            if !self.asking.contains(&approval) {
                self.asking.push(approval);
            }
        }
        if idle {
            self.ask_next(window, cx);
        }
    }

    /// Opens the dialog for the first value still waiting. Only its buttons close it.
    fn ask_next(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let approved = &self.approved;
        self.asking
            .retain(|a| !approved.contains(approval_value(a)));
        let Some(approval) = self.asking.first().cloned() else {
            return;
        };
        let text = approval_text(&approval);
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let answer = |allow: bool| {
                let (this, approval) = (this.clone(), approval.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    this.update(cx, |this, cx| this.answer(&approval, allow, window, cx))
                        .ok();
                }
            };
            let title = div()
                .debug_selector(|| "approval-title".into())
                .child("Run a new command?");
            dialog
                .close_button(false)
                .overlay_closable(false)
                .keyboard(false)
                .title(title)
                .child(text.clone())
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("approval-deny")
                                .label("Don't allow")
                                .on_click(answer(false)),
                        )
                        .child(
                            Button::new("approval-allow")
                                .primary()
                                .label("Allow")
                                .on_click(answer(true)),
                        ),
                )
        });
    }

    /// Allow approves the value and puts it in force; Don't allow keeps what runs now, and a
    /// reload does not ask about it again this run. Only Allow of the screen's edit writes
    /// `settings.toml`. A default agent that waited for the answer starts then.
    fn answer(
        &mut self,
        approval: &Approval,
        allow: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let waited = self.default_asked();
        self.asking.retain(|a| a != approval);
        let value = approval_value(approval).clone();
        if allow {
            self.approved.insert(value.clone());
            self.save_state(cx);
            self.enforce(cx);
            // The reload of this write finds the value approved: no second question.
            let (write, keep) = (std::mem::take(&mut self.edits).into_iter())
                .partition::<Vec<_>, _>(|e| *approval_value(e) == value);
            self.edits = keep;
            for edit in write {
                self.write_setting(&edit_change(edit), window, cx);
            }
        } else if let Some(ix) = self.edits.iter().position(|e| e == approval) {
            self.edits.remove(ix);
        } else {
            self.declined.insert(value);
        }
        if waited && !self.default_asked() {
            self.start_agent(cx);
        }
        // A declined edit's field shows the value in force again.
        self.sync_settings_view(cx);
        self.ask_next(window, cx);
    }

    /// Whether a question about the default agent's command from the settings (not the screen,
    /// whose edits apply at the next start) is open.
    fn default_asked(&self) -> bool {
        let Some(agent) = self.default_agent() else {
            return false;
        };
        self.asking.iter().any(|a| match a {
            Approval::Agent { id, new, .. } => {
                *id == agent && !self.approved.contains(new) && !self.edits.contains(a)
            }
            Approval::Shell { .. } => false,
        })
    }

    /// Hands a watcher batch to the open files it touches, and the project's settings file to
    /// `reload_settings`.
    fn disk_changed(&mut self, batch: &[FsChange], window: &mut Window, cx: &mut Context<Self>) {
        if touches(batch, &self.project.join(".wai").join("settings.toml")) {
            self.reload_settings(window, cx);
        }
        let touched = |path: &Path| touches(batch, path);
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

    /// The agent a new session uses; `None` until the user chooses one.
    pub fn default_agent(&self) -> Option<String> {
        self.settings.default_agent.clone()
    }

    /// Starts the default agent, so the history fills in once it is ready. Not while its command
    /// waits for an answer: `answer` starts it then.
    pub fn start_agent(&mut self, cx: &mut Context<Self>) {
        let Some(agent) = self.default_agent() else {
            return;
        };
        if !self.default_asked() {
            self.registry.update(cx, |r, cx| r.handle(&agent, cx));
        }
    }

    /// Reopens the last run's sessions, which `load_session` replays, and its terminal. A session
    /// goes back to its pane: the first one saved for a pane takes it, panes left empty close, and
    /// one saved in no pane only joins the open list. Sessions of agents the settings do not have
    /// (or whose command waits for an answer) stay in the state for a launch that has them.
    pub fn restore(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(path) = &self.state_path else {
            return;
        };
        let state = AppState::load(path);
        self.groups = state.groups;
        let mut panes: [Option<Entity<ChatSession>>; MAX_PANES] = Default::default();
        for saved in state.open_sessions {
            let (agent, id, cwd) = (&saved.agent, &saved.session_id, &saved.cwd);
            let loading = self.loading_session(agent.clone(), id.clone(), cwd.clone(), window, cx);
            let Some(session) = loading else {
                self.unrestored.push(saved);
                continue;
            };
            if let Some(slot @ None) = panes.get_mut(saved.pane) {
                *slot = Some(session.clone());
            }
            session.update(cx, |s, cx| {
                s.model.name = saved.name;
                s.load(true, window, cx)
            });
        }
        let panes: Vec<_> = panes.into_iter().flatten().map(Pane::Session).collect();
        if !panes.is_empty() {
            self.panes = panes;
        }
        if state.terminal_visible {
            self.toggle_terminal(&ToggleTerminal, window, cx);
        }
        self.agents_right = state.agents_right;
        self.agents_collapsed = state.agents_collapsed;
        self.focus_pane(0, window, cx);
        self.save_state(cx);
        self.recover(window, cx);
    }

    /// Where unsaved edits are kept: beside the state file.
    fn recovery_dir(&self) -> Option<PathBuf> {
        Some(self.state_path.as_ref()?.parent()?.join("recovery"))
    }

    /// Hot-exit: reopens the last run's unsaved edits (read on the background executor), each as
    /// an edited tab with the banner.
    fn recover(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(dir) = self.recovery_dir() else {
            return;
        };
        cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move { recovery::load(&dir) })
                .await;
            this.update_in(cx, |this, window, cx| {
                for found in found {
                    // Its name is never handed out again, even once the tab drops it (saved).
                    let after = recovery::number(&found.file).map_or(0, |n| n + 1);
                    this.next_recovery = this.next_recovery.max(after);
                    // ponytail: a second entry for one file (two runs of wai) waits for a launch
                    // without the first.
                    if this.reveal(&found.entry.path, None, window, cx) {
                        continue;
                    }
                    let tab = cx.new(|cx| FileEditor::recovered(found, window, cx));
                    this.add_tab(tab, None, window, cx);
                }
            })
            .ok();
        })
        .detach();
    }

    /// Writes what the next launch restores: on each structural change and at quit, never on a
    /// timer. A session in no pane is saved with pane `MAX_PANES`.
    pub(crate) fn save_state(&self, cx: &App) {
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
                name: s.model.name.clone(),
            }
        });
        let unrestored = self.unrestored.iter().cloned();
        let state = AppState {
            project: Some(self.project.clone()),
            open_sessions: open_sessions.chain(unrestored).collect(),
            terminal_visible: self.terminal_visible,
            // ponytail: the sidebar does not resize yet; save its width once it does.
            sidebar_width: SIDEBAR_WIDTH,
            agents_right: self.agents_right,
            agents_collapsed: self.agents_collapsed,
            approved: Some(self.approved.clone()),
            models_seen: self.models_seen.clone(),
            groups: self.groups.clone(),
            last_models: self.last_models.clone(),
            last_bases: self.last_bases.clone(),
            modes_seen: self.modes_seen.clone(),
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
                self.ext_host.read(cx).broadcast(HostEvent::TurnEnded {
                    agent: agent.clone(),
                    session: session.clone(),
                    reason: stop_reason(reason),
                });
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
                // The AI rows show the status; empty panes, the default agent's problem.
                self.sync_settings_view(cx);
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
        let processing = self.settings.conversation.processing;
        session.update(cx, |s, cx| s.set_processing(processing, cx));
        cx.subscribe_in(&session, window, |this, session, event, window, cx| {
            match event {
                // Its pane keeps the notice; the next launch does not try again.
                ChatEvent::RestoreFailed => {
                    this.sessions.retain(|s| s != session);
                    this.ungroup(&session.read(cx).id);
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
                ChatEvent::Hide => {
                    let pane = this.panes.iter().position(|p| p.session() == Some(session));
                    if let Some(ix) = pane {
                        this.close_pane(ix, window, cx);
                    }
                    return;
                }
                ChatEvent::StateChanged => {
                    let s = session.read(cx);
                    this.ext_host.read(cx).broadcast(HostEvent::SessionState {
                        agent: s.agent.clone(),
                        session: s.id.clone(),
                        state: state_name(s.status()).into(),
                    });
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
        if session.read(cx).head.is_none() {
            let cwd = session.read(cx).cwd.clone();
            let head = cx.background_spawn(async move { git::head(&cwd) });
            let target = session.clone();
            cx.spawn(async move |_, cx| {
                let head = head.await;
                target.update(cx, |s, cx| {
                    s.head = head;
                    cx.notify();
                });
            })
            .detach();
        }
        self.sessions.push(session);
        self.sync_agents(cx);
        self.sync_drafts(cx);
    }

    /// A started session joins the Open list: its early events replay, `model` (else the
    /// settings' model for its agent) applies, and the models its agent reported are kept.
    pub(crate) fn land(
        &mut self,
        session: Entity<ChatSession>,
        early: Vec<SessionEvent>,
        model: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let agent = session.read(cx).agent.clone();
        session.update(cx, |s, cx| early.iter().for_each(|e| s.handle_event(e, cx)));
        let seen = seen_models(&session.read(cx).model.config);
        if !seen.is_empty() {
            self.models_seen.insert(agent.clone(), seen);
            self.sync_settings_view(cx);
        }
        if let Some(seen) = seen_modes(session.read(cx)) {
            self.modes_seen.insert(agent.clone(), seen);
        }
        if let Some(model) = model.or_else(|| self.settings.models.get(&agent).cloned()) {
            session.update(cx, |s, cx| s.apply_default_model(&model, cx));
        }
        self.add_session(session, window, cx);
        self.refresh_history(cx);
    }

    /// The Agents tab follows the open sessions.
    pub(crate) fn sync_agents(&self, cx: &mut Context<Self>) {
        self.agents.update(cx, |a, cx| a.set(&self.sessions, cx));
    }

    /// Focuses pane `ix`: its prompt box, or the workspace when it has no session.
    pub(crate) fn focus_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_pane = ix;
        match &self.panes[ix] {
            Pane::Session(session) => session.update(cx, |s, cx| s.focus(window, cx)),
            Pane::Editor(pane) => pane.update(cx, |p, cx| p.focus(window, cx)),
            Pane::Draft(draft) => draft.update(cx, |d, cx| d.focus(window, cx)),
            _ => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    /// The workspace follows the user to a session in another folder, as Open folder does. Run
    /// where the user moves the focus, not in `focus_pane`: `set_project` focuses too.
    pub(crate) fn follow_session(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(session) = self.panes[self.focused_pane].session() else {
            return;
        };
        let cwd = session.read(cx).cwd.clone();
        if cwd.is_dir() {
            self.leave(Leave::Open(cwd), window, cx);
        }
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
        self.follow_session(window, cx);
        self.save_state(cx);
    }

    /// Shows `draft` as `show` shows a session: a pane that already shows it takes the focus.
    pub(crate) fn show_draft(
        &mut self,
        draft: Entity<DraftView>,
        side: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = match self.panes.iter().position(|p| p.draft() == Some(&draft)) {
            Some(ix) => ix,
            None => {
                let empty = matches!(self.panes[self.focused_pane], Pane::Empty(_));
                let ix = if side && !empty && self.panes.len() < MAX_PANES {
                    self.panes.push(Pane::Empty(None));
                    self.panes.len() - 1
                } else {
                    self.session_pane()
                };
                self.panes[ix] = Pane::Draft(draft);
                ix
            }
        };
        self.focus_pane(ix, window, cx);
        self.follow_session(window, cx);
        self.save_state(cx);
    }

    /// A draft row's ×: discards `draft`, after asking when it holds text or attachments.
    pub(crate) fn close_draft(
        &mut self,
        draft: &Entity<DraftView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let d = draft.read(cx);
        if d.text(cx).is_empty() && d.attachments.is_empty() {
            return self.discard_draft(draft, window, cx);
        }
        let draft = draft.clone();
        self.confirm(
            "Discard this draft?".into(),
            "What you typed is lost.",
            "Discard",
            move |this, window, cx| this.discard_draft(&draft, window, cx),
            window,
            cx,
        );
    }

    pub(crate) fn discard_draft(
        &mut self,
        draft: &Entity<DraftView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.drafts.retain(|d| d != draft);
        // A send in flight ends: its session is closed in the agent when it answers.
        draft.update(cx, |d, _| d.starting = None);
        match self.panes.iter().position(|p| p.draft() == Some(draft)) {
            Some(ix) => self.drop_pane(ix, window, cx),
            None => cx.notify(),
        }
    }

    /// The pane a session takes: the focused one, unless it is the editor pane (its unsaved edits
    /// would go with it); then a new pane to the right, or at `MAX_PANES` the next pane.
    pub(crate) fn session_pane(&mut self) -> usize {
        if !matches!(self.panes[self.focused_pane], Pane::Editor(_)) {
            self.focused_pane
        } else if self.panes.len() < MAX_PANES {
            self.panes.push(Pane::Empty(None));
            self.panes.len() - 1
        } else {
            (self.focused_pane + 1) % self.panes.len()
        }
    }

    /// R57: the pane an extension's session takes: its last session's (or its start's), else an
    /// empty one, else a new one while there are fewer than `MAX_PANES`. `None` when no pane is
    /// free (each shows a user's session, the editor or another start): it opens in the open
    /// list only. R61: never the focused pane, its own session's or an empty one (the user is
    /// in it, maybe reading why their start failed).
    fn ext_pane(&mut self, ext: &ExtId) -> Option<usize> {
        let (last, focused) = (self.ext_sessions.get(ext), self.focused_pane);
        let theirs = self
            .panes
            .iter()
            .enumerate()
            .position(|(ix, pane)| match pane {
                Pane::Session(session) => ix != focused && Some(&session.entity_id()) == last,
                Pane::Preparing { by_extension, .. } => by_extension.as_ref() == Some(ext),
                _ => false,
            });
        let empty = || {
            (self.panes.iter().enumerate())
                .position(|(ix, p)| ix != focused && matches!(p, Pane::Empty(_)))
        };
        if let Some(ix) = theirs.or_else(empty) {
            return Some(ix);
        }
        (self.panes.len() < MAX_PANES).then(|| {
            self.panes.push(Pane::Empty(None));
            self.panes.len() - 1
        })
    }

    /// Opens a history row: the open session with that id, else a new one that loads it.
    pub fn resume(
        &mut self,
        summary: &SessionSummary,
        side: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((session, fresh)) = self.resumed(summary, window, cx) else {
            return;
        };
        self.show(session.clone(), side, window, cx);
        if fresh {
            session.update(cx, |s, cx| s.load(false, window, cx));
        }
    }

    /// A history row's session: the open one with its id, else a new one titled like the row,
    /// which is `fresh`: it loads once its caller has put it in a pane.
    fn resumed(
        &mut self,
        summary: &SessionSummary,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<(Entity<ChatSession>, bool)> {
        let agent = self.default_agent()?;
        if let Some(session) = self.session(&agent, &summary.id, cx) {
            return Some((session, false));
        }
        let (id, cwd) = (summary.id.clone(), summary.cwd.clone());
        let session = self.loading_session(agent, id, cwd, window, cx)?;
        session.update(cx, |s, _| s.model.title = summary.title.clone());
        Some((session, true))
    }

    /// R89–R92: `dragged` lands on pane `ix`'s `zone` (`layout::place`) and its pane takes the
    /// focus. A session it replaces stays open, in no pane; a History row resumes as a click on
    /// it does.
    fn drop_session(
        &mut self,
        dragged: &DraggedSession,
        ix: usize,
        zone: Zone,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // The panes may have changed since the zones drew.
        if ix >= self.panes.len() {
            return;
        }
        let (session, draft, fresh) = match &dragged.source {
            Dragged::Open(open) if self.sessions.contains(open) => {
                (Some(open.clone()), None, false)
            }
            Dragged::Draft(d) if self.drafts.contains(d) => (None, Some(d.clone()), false),
            Dragged::Open(_) | Dragged::Draft(_) => return,
            Dragged::History(row) => match self.resumed(row, window, cx) {
                Some((resumed, fresh)) => (Some(resumed), None, fresh),
                None => return,
            },
        };
        let holds = |p: &Pane| match (&session, &draft) {
            (Some(session), _) => p.session() == Some(session),
            (_, Some(draft)) => p.draft() == Some(draft),
            _ => false,
        };
        let from = self.panes.iter().position(holds);
        let empty = matches!(self.panes[ix], Pane::Empty(_));
        let locked = matches!(self.panes[ix], Pane::Editor(_)) && !self.dirty_tabs(cx).is_empty();
        if let Some(slots) = layout::place(self.panes.len(), from, ix, zone, empty, locked) {
            let mut old: Vec<_> = self.panes.drain(..).map(Some).collect();
            self.panes = (slots.into_iter())
                .map(|slot| match slot {
                    Slot::Old(ix) => old[ix].take().expect("each pane lands once"),
                    Slot::Dropped => match (&session, &draft) {
                        (Some(session), _) => Pane::Session(session.clone()),
                        (_, Some(draft)) => Pane::Draft(draft.clone()),
                        _ => unreachable!("a dropped pane is a session or a draft"),
                    },
                })
                .collect();
            self.save_state(cx);
        }
        let shown = self.panes.iter().position(holds);
        if let Some(ix) = shown {
            self.focus_pane(ix, window, cx);
            self.follow_session(window, cx);
        }
        if let (true, Some(session)) = (fresh, &session) {
            session.update(cx, |s, cx| s.load(false, window, cx));
        }
    }

    /// R92: what a session dragged over pane `ix` lights: the whole pane when each of its zones
    /// acts as the center (`Some(true)`), else the zone under the pointer (`Some(false)`); nothing
    /// over its own pane, where a drop changes nothing.
    fn lit(&self, dragged: &DraggedSession, ix: usize, cx: &App) -> Option<bool> {
        let session = match &dragged.source {
            Dragged::Open(session) => Some(session.clone()),
            Dragged::History(row) => self
                .default_agent()
                .and_then(|agent| self.session(&agent, &row.id, cx)),
            Dragged::Draft(_) => None,
        };
        let from = match &dragged.source {
            Dragged::Draft(draft) => (self.panes.iter()).position(|p| p.draft() == Some(draft)),
            _ => session.and_then(|s| self.panes.iter().position(|p| p.session() == Some(&s))),
        };
        let empty = matches!(self.panes.get(ix)?, Pane::Empty(_));
        (from != Some(ix)).then(|| layout::only_center(self.panes.len(), from, empty))
    }

    /// R89: while something is dragged, pane `ix`'s left and right quarters and its middle, each
    /// lit while a session is over it; or the whole pane, when every zone acts as the center (R92).
    fn drop_zones(ix: usize, cx: &Context<Self>) -> impl IntoElement + use<> {
        let this = cx.weak_entity();
        let light = move |whole: bool| {
            let this = this.clone();
            move |style, dragged: &DraggedSession, _: &mut Window, cx: &mut App| {
                let lit = this.upgrade().and_then(|w| w.read(cx).lit(dragged, ix, cx));
                if lit == Some(whole) {
                    aurora::drop_target(style, cx)
                } else {
                    style
                }
            }
        };
        let zone = |zone: Zone| {
            div().h_full().drag_over(light(false)).on_drop(cx.listener(
                move |this, dragged: &DraggedSession, window, cx| {
                    this.drop_session(dragged, ix, zone, window, cx)
                },
            ))
        };
        h_flex()
            .absolute()
            .inset_0()
            .drag_over(light(true))
            .child(zone(Zone::Left).w(relative(0.25)))
            .child(zone(Zone::Center).flex_1())
            .child(zone(Zone::Right).w(relative(0.25)))
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
        let mcp = self.registry.read(cx).mcp_servers();
        let title = title.map(|t| format!("{t} (fork)"));
        cx.spawn_in(window, async move |this, cx| {
            let forked = handle.fork_session(id, cwd.clone(), mcp).await;
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
            config: Vec::new(),
        };
        let registry = self.registry.downgrade();
        let session =
            cx.new(|cx| ChatSession::new(agent, started, cwd, handle, registry, window, cx));
        self.add_session(session.clone(), window, cx);
        Some(session)
    }

    /// Opens a draft of a session with the default agent in the focused pane (beside the editor
    /// pane, see `session_pane`). Nothing starts until the draft is sent.
    pub(crate) fn new_session(
        &mut self,
        _: &NewSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(agent) = self.default_agent() else {
            return self.open_settings(Section::Ai, Some(CHOOSE_AI), window, cx);
        };
        self.open_draft(agent, None, window, cx)
    }

    /// The user's and the project's templates, then the enabled extensions'.
    pub(crate) fn all_templates(&self, cx: &App) -> Vec<Template> {
        let extensions = self.ext_host.read(cx).templates().cloned();
        self.templates.iter().cloned().chain(extensions).collect()
    }

    /// `new_session` for template `ix`: its agent (the default one if it names none), its prompt
    /// pre-filled and not sent. An agent the settings do not define is an error in the pane.
    pub(crate) fn start_template(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(template) = self.all_templates(cx).get(ix).cloned() else {
            return;
        };
        let Some(agent) = template.agent.clone().or_else(|| self.default_agent()) else {
            return self.open_settings(Section::Ai, Some(CHOOSE_AI), window, cx);
        };
        if !self.registry.read(cx).knows(&agent) {
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
        self.open_draft(agent, Some(template.prompt), window, cx)
    }

    /// A palette command: a prompt-only one starts a session with its prompt pre-filled, the
    /// others go to the extension's process as `command/run`.
    pub(crate) fn run_ext_command(
        &mut self,
        ext: &ExtId,
        command: CommandSpec,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match command.prompt {
            Some(prompt) => {
                let Some(agent) = self.default_agent() else {
                    return self.open_settings(Section::Ai, Some(CHOOSE_AI), window, cx);
                };
                self.open_draft(agent, Some(prompt), window, cx)
            }
            None => self.ext_host.read(cx).run_command(ext, &command.id),
        }
    }

    /// The enabled extensions' agents and MCP servers, to the registry.
    fn sync_contributions(&self, cx: &mut Context<Self>) {
        let host = self.ext_host.read(cx);
        let (agents, mcp) = (host.agents(), host.mcp_servers());
        self.registry
            .update(cx, |r, _| r.set_extensions(agents, mcp));
        self.sync_drafts(cx);
    }

    /// R51: an extension's session request, already checked against its permissions.
    fn session_call(
        &mut self,
        ext: ExtId,
        call: ExtEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match call {
            ExtEvent::ListSessions { reply } => {
                let sessions = self.sessions.iter().map(|session| {
                    let s = session.read(cx);
                    json!({
                        "agent": s.agent,
                        "session": s.id,
                        "title": s.model.shown_title(),
                        "state": state_name(s.status()),
                    })
                });
                reply.send(Ok(sessions.collect()));
            }
            ExtEvent::OpenSession {
                agent,
                prompt,
                send,
                reply,
            } => {
                let Some(agent) = agent.or_else(|| self.default_agent()) else {
                    return reply.send(Err("No default AI is chosen".into()));
                };
                if !self.registry.read(cx).knows(&agent) {
                    return reply.send(Err(format!("No agent named {agent}")));
                }
                self.start_session(agent, prompt, send, Some((ext, Some(reply))), window, cx);
            }
            ExtEvent::Prompt {
                agent,
                session,
                text,
            } => {
                let sent = (self.session(&agent, &session, cx))
                    .is_some_and(|s| s.update(cx, |s, cx| s.send_text(&text, cx)));
                if !sent {
                    eprintln!("wai: an extension's prompt for {agent} {session} was dropped");
                }
            }
            _ => {}
        }
    }

    /// Starts a session of `agent` in the session pane with `prompt` pre-filled, or sent with
    /// `send`. `by`: the extension whose `sessions/open` this is, and its reply, which gets
    /// `{agent, session}` or why not.
    ///
    /// R56: an extension's session never takes the keyboard: the user may be typing elsewhere
    /// (its pane shows it all the same). And it never holds up the user: a start of theirs takes
    /// over the pane where an extension's session prepares, which then opens in no pane. R57:
    /// nor does it take the user's pane (`ext_pane`).
    pub(crate) fn start_session(
        &mut self,
        agent: String,
        prompt: Option<String>,
        send: bool,
        by: Option<(ExtId, Option<Reply>)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (ext, reply) = by.map_or((None, None), |(ext, reply)| (Some(ext), reply));
        let refuse = |reply: Option<Reply>, why: &str| {
            if let Some(reply) = reply {
                reply.send(Err(why.into()));
            }
        };
        let by_extension = ext.is_some();
        let ix = match &ext {
            Some(ext) => self.ext_pane(ext),
            None => Some(self.session_pane()),
        };
        if let Some(ix) = ix
            && let Pane::Preparing {
                by_extension: theirs,
                ..
            } = &self.panes[ix]
            && (by_extension || theirs.is_none())
        {
            return refuse(reply, "A session is already starting there");
        }
        if let Some(ix) = ix
            && ix != self.focused_pane
            && !by_extension
        {
            self.focus_pane(ix, window, cx);
        }
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            let error = format!("No agent named {agent}");
            refuse(reply, &error);
            if let Some(ix) = ix {
                let error = format!("{error} in the settings or extensions.");
                self.panes[ix] = Pane::Empty(Some(error));
            }
            return cx.notify();
        };
        let mcp = self.registry.read(cx).mcp_servers();
        let ticket = self.next_ticket;
        self.next_ticket += 1;
        if let Some(ix) = ix {
            self.panes[ix] = Pane::Preparing {
                agent: agent.clone(),
                ticket,
                by_extension: ext.clone(),
            };
        }
        *self.starting.entry(agent.clone()).or_default() += 1;
        cx.notify();
        let cwd = self.project.clone();
        let registry = self.registry.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let started = handle.new_session(cwd.clone(), mcp).await;
            this.update_in(cx, |this, window, cx| {
                let early = this.landed(&agent, started.as_ref().ok().map(|s| s.id.as_str()));
                // Replaced or closed meanwhile, the placeholder is gone: the session still opens,
                // in the sidebar.
                let pane = this
                    .panes
                    .iter()
                    .position(|p| matches!(p, Pane::Preparing { ticket: t, .. } if *t == ticket));
                let shown = match started {
                    Ok(started) => {
                        // A session that starts proves the agent works (signed in again, say).
                        this.registry.update(cx, |r, cx| r.mark_ready(&agent, cx));
                        let id = started.id.clone();
                        let session = cx.new(|cx| {
                            ChatSession::new(
                                agent.clone(),
                                started,
                                cwd,
                                handle,
                                registry,
                                window,
                                cx,
                            )
                        });
                        // Routed and followed before a sent prompt changes its state.
                        this.land(session.clone(), early, None, window, cx);
                        if let Some(text) = prompt {
                            session.update(cx, |s, cx| {
                                if !(send && s.send_text(&text, cx)) {
                                    s.prefill(&text, window, cx)
                                }
                            });
                        }
                        if let Some(reply) = reply {
                            reply.send(Ok(json!({"agent": agent, "session": id})));
                        }
                        if let Some(ext) = ext {
                            this.ext_sessions.insert(ext, session.entity_id());
                        }
                        Pane::Session(session)
                    }
                    // The empty pane shows the sign-in message.
                    Err(AgentError::AuthRequired) => {
                        refuse(reply, &signed_out(&agent));
                        this.registry
                            .update(cx, |r, cx| r.set_status(&agent, AgentStatus::SignedOut, cx));
                        Pane::Empty(None)
                    }
                    Err(e) => {
                        let error = format!("Could not start a {agent} session: {e}");
                        refuse(reply, &error);
                        Pane::Empty(Some(error))
                    }
                };
                if let Some(ix) = pane {
                    this.panes[ix] = shown;
                    // Only the focused pane takes the caret: elsewhere it would pull focus away
                    // from the pane the user is in.
                    if ix == this.focused_pane && !by_extension {
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
    pub(crate) fn toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.terminal_visible = !self.terminal_visible;
        self.save_state(cx);
        if !self.terminal_visible {
            return self.focus_pane(self.focused_pane, window, cx);
        }
        let terminal = match &self.terminal {
            Some(terminal) => terminal.clone(),
            None => {
                let (shell, cwd) = (self.shell.clone(), self.project.clone());
                let terminal = cx.new(|cx| TerminalView::new(shell, cwd, cx));
                self.terminal = Some(terminal.clone());
                terminal
            }
        };
        terminal.update(cx, |t, cx| t.activate(window, cx));
        cx.notify();
    }

    /// Shows the terminal and types `text` at its prompt without Enter: the user reads it and
    /// presses Enter (an extension's suggested Claude Code plugin).
    pub(crate) fn type_in_terminal(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.terminal_visible {
            self.toggle_terminal(&ToggleTerminal, window, cx);
        }
        if let Some(terminal) = self.terminal.clone() {
            terminal.update(cx, |t, cx| {
                t.activate(window, cx);
                t.type_text(text, cx);
            });
        }
    }

    /// ⌘W: `close_pane` on the focused pane.
    fn close_focused_pane(&mut self, _: &ClosePane, window: &mut Window, cx: &mut Context<Self>) {
        self.close_pane(self.focused_pane, window, cx);
    }

    /// Takes pane `ix` off the screen. A session in it stays open, in the sidebar; a "Preparing…"
    /// one still opens there. R20: in an editor pane it closes the
    /// active tab instead (the pane goes with the last one). R87: a pane's × runs it on its own
    /// pane, focused or not.
    pub(crate) fn close_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if let Pane::Editor(pane) = &self.panes[ix] {
            return pane.update(cx, |p, cx| p.close(p.active, window, cx));
        }
        self.drop_pane(ix, window, cx);
        self.follow_session(window, cx);
    }

    /// An Open row's ×: drops `session` from the open list and from its pane, if one shows it
    /// (the agent keeps it in its history).
    /// Frees session `id` in its agent, when the agent can.
    pub(crate) fn close_in_agent(&mut self, agent: String, id: String, cx: &mut Context<Self>) {
        let closable = matches!(
            self.registry.read(cx).status(&agent),
            Some(AgentStatus::Ready(caps)) if caps.close
        );
        if closable && let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) {
            cx.background_spawn(async move {
                if let Err(e) = handle.close_session(id).await {
                    eprintln!("could not close the {agent} session: {e}");
                }
            })
            .detach();
        }
    }

    pub(crate) fn end_session(
        &mut self,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        session.read(cx).stop();
        let (agent, id) = {
            let s = session.read(cx);
            (s.agent.clone(), s.id.clone())
        };
        self.close_in_agent(agent, id, cx);
        self.sessions.retain(|s| s != session);
        self.ungroup(&session.read(cx).id);
        self.sync_agents(cx);
        match self.panes.iter().position(|p| p.session() == Some(session)) {
            Some(ix) => self.drop_pane(ix, window, cx),
            None => {
                self.save_state(cx);
                cx.notify();
            }
        }
    }

    /// Ends `session` and types the shell command that resumes it in Claude Code at the
    /// terminal's prompt, after asking when its turn is still going.
    pub(crate) fn continue_in_terminal(
        &mut self,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (id, cwd, busy) = {
            let s = session.read(cx);
            (s.id.clone(), s.cwd.clone(), s.busy())
        };
        let session = session.clone();
        let run = move |w: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>| {
            w.end_session(&session, window, cx);
            w.type_in_terminal(&resume_command(&cwd, &id), window, cx)
        };
        if busy {
            self.confirm(
                "This session is still working".into(),
                "Ending it stops its turn. End it anyway?",
                "Continue in terminal",
                run,
                window,
                cx,
            )
        } else {
            run(self, window, cx)
        }
    }

    /// A `new_session` of `agent` returned (`id`: the session it started): the events that came
    /// for that session before. None are kept once no session of the agent is starting.
    pub(crate) fn landed(&mut self, agent: &str, id: Option<&str>) -> Vec<SessionEvent> {
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
                    if !save {
                        dirty
                            .iter()
                            .for_each(|tab| tab.update(cx, |t, _| t.discard()));
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
        let busy = self.busy_sessions(cx);
        match then {
            Leave::Open(project) => self.set_project(project, window, cx),
            Leave::Quit if busy == 0 => cx.quit(),
            Leave::Close if busy == 0 => window.remove_window(),
            Leave::Quit => self.confirm(
                still_working(busy),
                "Quitting ends their turns. End them anyway?",
                "Quit anyway",
                |_, _, cx| cx.quit(),
                window,
                cx,
            ),
            Leave::Close => self.confirm(
                still_working(busy),
                "Closing the window ends their turns. End them anyway?",
                "Close anyway",
                |_, window, _| window.remove_window(),
                window,
                cx,
            ),
            Leave::Update if busy == 0 => self.install_update(window, cx),
            Leave::Update => self.confirm(
                still_working(busy),
                "Restarting ends their turns. End them anyway?",
                "Restart anyway",
                |this, window, cx| this.install_update(window, cx),
                window,
                cx,
            ),
        }
    }

    /// Stores the update status and shows it in an open Settings.
    pub(crate) fn set_update(&mut self, status: UpdateStatus, cx: &mut Context<Self>) {
        self.update = status;
        self.sync_settings_view(cx);
    }

    /// Looks for a newer wai and, when this copy can install it, stages it (see `update::run`),
    /// off the main thread. On launch a result worth acting on is a notice and a failure only a
    /// log line; from Settings everything shows in its status row and nothing pops up.
    pub(crate) fn check_updates(
        &mut self,
        from_launch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.update == UpdateStatus::Checking {
            return;
        }
        // Checking is off: nothing runs, not even the blocker's probe file.
        if from_launch && !self.settings.updates.check {
            return;
        }
        let mut settings = self.settings.updates.clone();
        // ponytail: "Check now" is an explicit ask, so it ignores the automatic-updates switch.
        settings.check |= !from_launch;
        if !from_launch {
            self.update_blocker = update::blocker();
        }
        self.set_update(UpdateStatus::Checking, cx);
        let found = cx.background_spawn(async move { update::check_and_stage(&settings) });
        cx.spawn_in(window, async move |this, cx| {
            let found = found.await;
            this.update_in(cx, |this, window, cx| {
                this.checked(found, from_launch, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn checked(
        &mut self,
        found: anyhow::Result<Outcome>,
        from_launch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let status = match found {
            Ok(Outcome::Off) if from_launch => UpdateStatus::Idle,
            Ok(Outcome::Off) => {
                UpdateStatus::Failed("Set a repository to check for updates.".into())
            }
            Ok(Outcome::UpToDate) => UpdateStatus::UpToDate,
            Ok(Outcome::Available { release, why }) => UpdateStatus::Available { release, why },
            Ok(Outcome::Ready(staged)) => UpdateStatus::Ready(staged),
            Err(e) => {
                if from_launch {
                    eprintln!("wai: update check failed: {}", update::logged(&e));
                }
                UpdateStatus::Failed(update::logged(&e))
            }
        };
        if from_launch {
            self.announce_update(&status, window, cx);
        }
        self.set_update(status, cx);
    }

    /// The launch notice: Restart now for a staged build, Open release page for one this copy
    /// will not install.
    fn announce_update(&self, status: &UpdateStatus, window: &mut Window, cx: &mut Context<Self>) {
        let this = cx.entity().downgrade();
        let notice = match status {
            UpdateStatus::Ready(staged) => Notification::info(staged.text())
                .autohide(false)
                .action(move |_, _, cx| {
                    let (this, notice) = (this.clone(), cx.entity());
                    Button::new("update-restart").label("Restart now").on_click(
                        move |_, window, cx| {
                            notice.update(cx, |n, cx| n.dismiss(window, cx));
                            this.update(cx, |w, cx| w.leave(Leave::Update, window, cx))
                                .ok();
                        },
                    )
                }),
            UpdateStatus::Available { release, .. } => {
                let page = release.page.clone();
                Notification::info(release.text()).action(move |_, _, cx| {
                    let (page, notice) = (page.clone(), cx.entity());
                    Button::new("update-open")
                        .label("Open release page")
                        .on_click(move |_, window, cx| {
                            cx.open_url(&page);
                            notice.update(cx, |n, cx| n.dismiss(window, cx));
                        })
                })
            }
            _ => return,
        };
        window.push_notification(notice, cx);
    }

    /// Installs the staged update and quits so it starts; any failure leaves wai open.
    pub(crate) fn install_update(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let UpdateStatus::Ready(staged) = self.update.clone() else {
            return;
        };
        match update::install(&staged) {
            Ok(()) => cx.quit(),
            Err(e) => {
                let message = format!("{e:#}");
                window.push_notification(Notification::error(message.clone()), cx);
                self.set_update(UpdateStatus::Failed(message), cx);
            }
        }
    }

    /// How many open sessions have a turn running or waiting on the user.
    pub(crate) fn busy_sessions(&self, cx: &App) -> usize {
        let starting = self.drafts.iter().filter(|d| d.read(cx).starting.is_some());
        self.sessions.iter().filter(|s| s.read(cx).busy()).count() + starting.count()
    }

    /// A dialog asking `body`; Confirm closes it, then runs `then`.
    pub(crate) fn confirm(
        &mut self,
        title: String,
        body: &'static str,
        confirm: &'static str,
        then: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let this = cx.entity().downgrade();
        let then = std::rc::Rc::new(then);
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, then) = (this.clone(), then.clone());
            let heading = div()
                .id("confirm-heading")
                .debug_selector(|| "confirm-title".into())
                .aria_label(title.clone())
                .test_support()
                .child(title.clone());
            dialog.title(heading).child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(Button::new("confirm").danger().label(confirm).on_click(
                        move |_, window, cx| {
                            window.close_dialog(cx);
                            this.update(cx, |this, cx| then(this, window, cx)).ok();
                        },
                    )),
            )
        });
    }

    /// An Open row's ×: ends `session`, after asking when its turn is still going.
    pub(crate) fn close_session(
        &mut self,
        session: &Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !session.read(cx).busy() {
            return self.end_session(session, window, cx);
        }
        let session = session.clone();
        self.confirm(
            "This session is still working".into(),
            "Ending it stops its turn. End it anyway?",
            "End session",
            move |this, window, cx| this.end_session(&session, window, cx),
            window,
            cx,
        );
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
                    let notice = format!("Could not write {}: {e}", shown(&path));
                    window.push_notification(Notification::error(notice), cx)
                }
            })
            .ok();
        })
        .detach();
    }

    /// `open_file`; a `scratch` file (a tool's output) is read-only whatever its size.
    pub(crate) fn open(
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
                    let keep = this.recovery_dir().filter(|_| !scratch);
                    let keep = keep.map(|dir| recovery::next_file(&dir, &mut this.next_recovery));
                    let tab = cx
                        .new(|cx| FileEditor::new(path.clone(), loaded, scratch, keep, window, cx));
                    this.add_tab(tab, line, window, cx);
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

    /// Adds `tab` to the editor pane and focuses it, at `line`.
    fn add_tab(
        &mut self,
        tab: Entity<FileEditor>,
        line: Option<u32>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe(&tab, |this, tab, _: &Saved, cx| {
            let path = tab.read(cx).path.clone();
            this.ext_host
                .read(cx)
                .broadcast(HostEvent::FileSaved { path });
        })
        .detach();
        let path = tab.read(cx).path.clone();
        let pane = self.editor_pane(window, cx);
        pane.update(cx, |p, cx| p.add(tab, window, cx));
        self.reveal(&path, line, window, cx);
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

    fn settings_data(&self, notice: Option<SharedString>, cx: &App) -> SettingsData {
        SettingsData {
            settings: self.settings.clone(),
            error: self.settings_error.clone(),
            notice,
            context: self.settings_context(),
            extensions: self.extensions.clone(),
            home: self.claude_home.clone(),
            update: self.update.clone(),
            update_blocker: self.update_blocker.clone(),
            agents: self.agent_rows(cx),
            project_default: wai_settings::project_default_agent(&self.project)
                .filter(|p| Some(p) == self.settings.default_agent.as_ref()),
        }
    }

    /// The Settings title's suffix: the default agent and the project.
    fn settings_context(&self) -> String {
        let project = folder_name(&self.project);
        match self.default_agent() {
            Some(agent) => format!("({} · {project})", wai_settings::agent_label(&agent)),
            None => format!("({project})"),
        }
    }

    pub fn agent_rows(&self, cx: &App) -> Vec<AgentRow> {
        let registry = self.registry.read(cx);
        registry
            .names()
            .into_iter()
            .map(|id| {
                let command = registry.command(&id).unwrap_or_default().to_string();
                let status = if wai_agent::program(&command).is_none() {
                    RowStatus::CliNotFound
                } else {
                    match registry.status(&id) {
                        None => RowStatus::NotChecked,
                        Some(AgentStatus::Starting) => RowStatus::Starting,
                        Some(AgentStatus::Ready(_)) => RowStatus::Ready,
                        Some(AgentStatus::SignedOut) => RowStatus::SignedOut,
                        Some(AgentStatus::Failed(e)) => RowStatus::Failed(e.clone()),
                    }
                };
                let (extension, label) = match id.split_once('/') {
                    Some((ext, name)) => (Some(ext.to_string()), name.to_string()),
                    None => (None, wai_settings::agent_label(&id)),
                };
                AgentRow {
                    label,
                    extension,
                    status,
                    command,
                    models: self.models_seen.get(&id).cloned().unwrap_or_default(),
                    default_model: self.settings.models.get(&id).cloned(),
                    is_default: self.settings.default_agent.as_deref() == Some(&id),
                    id,
                }
            })
            .collect()
    }

    /// Starts the agent and a session in the scratch folder: that tells whether it runs and
    /// whether the user is signed in, and which models it offers.
    // ponytail: the agent stays running after Test. Stop it when idle if the memory matters.
    pub fn test_agent(&mut self, id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(id, cx)) else {
            return;
        };
        self.sync_settings_view(cx);
        let (id, dir) = (id.to_string(), self.user_dir.join("scratch"));
        cx.spawn_in(window, async move |this, cx| {
            let _ = std::fs::create_dir_all(&dir);
            let started = handle.new_session(dir, vec![]).await;
            this.update(cx, |this, cx| {
                match started {
                    Ok(started) => {
                        this.registry.update(cx, |r, cx| r.mark_ready(&id, cx));
                        if let Some(seen) = started_modes(&started) {
                            this.modes_seen.insert(id.clone(), seen);
                        }
                        this.models_seen.insert(id, seen_models(&started.config));
                        this.sync_drafts(cx);
                        this.save_state(cx);
                    }
                    Err(AgentError::AuthRequired) => this
                        .registry
                        .update(cx, |r, cx| r.set_status(&id, AgentStatus::SignedOut, cx)),
                    Err(e) => this.registry.update(cx, |r, cx| {
                        r.set_status(&id, AgentStatus::Failed(e.to_string()), cx)
                    }),
                }
                this.sync_settings_view(cx);
            })
            .ok();
        })
        .detach();
    }

    /// The open Settings modal, if any.
    pub(crate) fn settings_view(&self) -> Option<Entity<SettingsView>> {
        self.settings_view.as_ref().and_then(|v| v.upgrade())
    }

    /// Closes the Settings modal when it is open.
    pub(crate) fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self
            .settings_view
            .take()
            .and_then(|v| v.upgrade())
            .is_some()
        {
            window.close_dialog(cx);
        }
    }

    /// Shows the settings modal at `section`, with `notice` in it; one already open follows.
    pub fn open_settings(
        &mut self,
        section: Section,
        notice: Option<&str>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let notice = notice.map(|n| SharedString::from(n.to_string()));
        self.update_blocker = update::blocker();
        self.extensions(window, cx);
        let data = self.settings_data(notice, cx);
        self.refresh_claude(cx);
        if let Some(view) = self.settings_view() {
            view.update(cx, |v, cx| {
                v.show(section, window, cx);
                v.set(data, cx);
            });
            return;
        }
        let view = cx.new(|cx| SettingsView::new(section, data, window, cx));
        cx.subscribe_in(
            &view,
            window,
            |this, _, event: &SettingsEvent, window, cx| match event {
                SettingsEvent::Change(change) => this.change_setting(change.clone(), window, cx),
                SettingsEvent::OpenFile => {
                    let path = this.user_dir.join("settings.toml");
                    // Created when missing (a first run): the editor opens only files
                    // that exist. If this fails, `open_file` reports why.
                    std::fs::create_dir_all(&this.user_dir).ok();
                    (std::fs::OpenOptions::new().create(true).append(true))
                        .open(&path)
                        .ok();
                    this.close_settings(window, cx);
                    this.open_file(path, None, window, cx)
                }
                SettingsEvent::Test(id) => this.test_agent(id, window, cx),
                SettingsEvent::CheckMcp => this.check_mcp(cx),
                SettingsEvent::CheckUpdates => this.check_updates(false, window, cx),
                SettingsEvent::RestartToUpdate => this.leave(Leave::Update, window, cx),
                SettingsEvent::Open { path, read_only } => {
                    this.close_settings(window, cx);
                    this.open(path.clone(), None, *read_only, window, cx)
                }
                SettingsEvent::TogglePlugin { id, on, seen } => {
                    this.toggle_plugin(id.clone(), *on, *seen, window, cx)
                }
            },
        )
        .detach();
        self.settings_view = Some(view.downgrade());
        open_modal(view, window, cx);
    }

    /// Writes `change` to the user's file; the watcher applies it. A command or shell not
    /// approved yet is asked about first, and written only on Allow.
    pub fn change_setting(&mut self, change: Change, window: &mut Window, cx: &mut Context<Self>) {
        let asked = match &change {
            Change::AgentCommand { agent, command } if !is_approved(command, &self.approved) => {
                let old = self.registry.read(cx).configs().get(agent);
                Some(Approval::Agent {
                    id: agent.clone(),
                    old: old.map(|c| c.command.clone()),
                    new: command.clone(),
                })
            }
            Change::Shell(Some(shell)) if !self.approved.contains(shell) => Some(Approval::Shell {
                old: self.shell.clone(),
                new: shell.clone(),
            }),
            _ => None,
        };
        match asked {
            Some(approval) => {
                self.edits.push(approval.clone());
                self.ask_approval(vec![approval], window, cx);
            }
            None => self.write_setting(&change, window, cx),
        }
    }

    fn write_setting(&self, change: &Change, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = write_change(&self.user_dir, change) {
            let text = format!("Could not save settings: {e}");
            window.push_notification(Notification::warning(text), cx);
        }
    }

    /// The settings screen follows `self.settings`. Its "Choose an AI" notice goes once one is
    /// chosen.
    pub fn sync_settings_view(&self, cx: &mut Context<Self>) {
        if let Some(view) = self.settings_view() {
            let notice = (view.read(cx).notice()).filter(|_| self.default_agent().is_none());
            let data = self.settings_data(notice, cx);
            view.update(cx, |v, cx| v.set(data, cx));
        }
    }

    fn focus_next_pane(&mut self, _: &FocusNextPane, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_pane((self.focused_pane + 1) % self.panes.len(), window, cx);
        self.follow_session(window, cx);
    }

    /// Shows `project`, closing the old project's files: callers ask about unsaved edits first
    /// (`leave`).
    pub fn set_project(&mut self, project: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(ix) = self.panes.iter().position(|p| matches!(p, Pane::Editor(_))) {
            // A focused session (the one `follow_session` came for) keeps the focus.
            let focused = self.panes[self.focused_pane].session().cloned();
            self.drop_pane(ix, window, cx);
            if let Some(ix) =
                focused.and_then(|s| self.panes.iter().position(|p| p.session() == Some(&s)))
            {
                self.focus_pane(ix, window, cx);
            }
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
        self.reload_settings(window, cx);
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
        cx.subscribe_in(
            &view,
            window,
            |this, _, InstallPlugin(command), window, cx| {
                this.close_settings(window, cx);
                this.type_in_terminal(command, window, cx)
            },
        )
        .detach();
        self.extensions = Some(view.clone());
        view
    }

    /// Starts the enabled extensions' processes and takes their agents and MCP servers: once the
    /// window is open, like the agent, and before the last run's sessions load.
    pub fn start_extensions(&mut self, cx: &mut Context<Self>) {
        let list = match &self.extensions {
            Some(view) => view.read(cx).store().list(),
            None => wai_ext::ExtStore::open(&self.user_dir).list(),
        };
        self.ext_host.update(cx, |host, cx| host.sync(&list, cx));
        self.sync_contributions(cx);
    }

    /// Opens the settings modal on Extensions.
    pub(crate) fn show_extensions(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Entity<ExtensionsView> {
        self.open_settings(Section::Extensions, None, window, cx);
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

    pub(crate) fn open_folder(
        &mut self,
        _: &OpenFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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
        let agent = self.default_agent()?;
        match self.registry.read(cx).status(&agent)? {
            AgentStatus::SignedOut => Some(signed_out(&agent)),
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
            .child(img(BRAND.clone()).size(px(48.)).rounded(px(12.)))
            .child(
                div()
                    .font_family(aurora::DISPLAY_FONT)
                    .text_size(px(20.))
                    .font_weight(FontWeight::BOLD)
                    .child("Start a session"),
            )
            .child(aurora::pill(cx).child(START_KEY))
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
            // In a split, its × drops it; a lone empty pane has nothing to close.
            .when(self.panes.len() > 1, |el| {
                let close = Button::new("pane-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Close pane")
                    .accessibility_label("Close pane")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        // The panes may have changed since this frame drew.
                        if matches!(this.panes.get(ix), Some(Pane::Empty(_))) {
                            this.close_pane(ix, window, cx);
                        }
                    }));
                el.relative()
                    .child(div().absolute().top_2().right_2().child(close))
            })
    }
}

pub(crate) fn folder_name(project: &Path) -> String {
    project
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| shown(project))
}

fn title(project: &Path) -> String {
    format!("wai — {}", folder_name(project))
}

/// Each agent's config while its command is approved, else its `last` one while that is, else
/// none.
fn approved_configs(
    last: &BTreeMap<String, AgentConfig>,
    settings: &Settings,
    approved: &BTreeSet<String>,
) -> BTreeMap<String, AgentConfig> {
    let ok = |c: &&AgentConfig| is_approved(&c.command, approved);
    let config = |id: &String, c| Some(c).filter(ok).or_else(|| last.get(id).filter(ok));
    (settings.agents.iter())
        .filter_map(|(id, c)| Some((id.clone(), config(id, c)?.clone())))
        .collect()
}

fn approval_value(approval: &Approval) -> &String {
    let (Approval::Agent { new, .. } | Approval::Shell { new, .. }) = approval;
    new
}

/// The screen's edit that `approval` asks about.
fn edit_change(approval: Approval) -> Change {
    match approval {
        Approval::Agent { id, new, .. } => Change::AgentCommand {
            agent: id,
            command: new,
        },
        Approval::Shell { new, .. } => Change::Shell(Some(new)),
    }
}

/// Control and format characters show escaped: a value cannot hide its end behind line breaks,
/// bidi overrides or zero-width characters.
pub(crate) fn approval_text(approval: &Approval) -> String {
    let text = match approval {
        Approval::Agent {
            id,
            old: Some(old),
            new,
        } => format!("{}: {old} → {new}", agent_label(id)),
        Approval::Agent { id, old: None, new } => format!("{} (new): {new}", agent_label(id)),
        Approval::Shell { old, new } => {
            let old = old.as_deref().unwrap_or("system default");
            format!("Terminal shell: {old} → {new}")
        }
    };
    let hidden = |c: char| c.is_control() || c.general_category() == GeneralCategory::Format;
    let visible = |c: char| match hidden(c) {
        true => c.escape_default().to_string(),
        false => c.into(),
    };
    text.chars().map(visible).collect()
}

/// A resizable panel's content on `card`, inset 4 px: the cards sit 8 px apart.
fn on_card(card: Div, content: impl IntoElement) -> Div {
    div()
        .size_full()
        .p_1()
        .child(card.size_full().child(content))
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let top_bar = self.render_top_bar(window, cx);
        let sidebar = self.render_sidebar(cx);
        let theme = cx.theme();
        let split = self.panes.len() > 1;
        let dragging = cx.has_active_drag();
        let panes = self.panes.iter().enumerate().map(|(ix, pane)| {
            let focused = split && ix == self.focused_pane;
            let content = v_flex()
                .debug_selector(move || format!("pane-{ix}"))
                .size_full()
                // Clicking into a pane focuses it, caret included, whatever the click lands
                // on; in the focused pane, a click on nothing focusable (the workspace root
                // took the focus) gives the caret back too. Deferred past the click, which
                // may also close a pane.
                .capture_any_mouse_down(cx.listener(move |_, _, window, cx| {
                    cx.defer_in(window, move |this, window, cx| {
                        let away = this.focused_pane != ix || this.focus.is_focused(window);
                        if ix < this.panes.len() && away {
                            this.focus_pane(ix, window, cx);
                            this.follow_session(window, cx);
                        }
                    });
                }))
                .child(match pane {
                    Pane::Session(session) => session.clone().into_any_element(),
                    Pane::Editor(pane) => pane.clone().into_any_element(),
                    Pane::Draft(draft) => draft.clone().into_any_element(),
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
                })
                .when(dragging, |el| el.relative().child(Self::drop_zones(ix, cx)));
            resizable_panel().child(on_card(aurora::card(focused, cx), content))
        });
        let sidebar = on_card(aurora::card(false, cx), sidebar).debug_selector(|| "sidebar".into());
        // The terminal paints its own square background: 5 px in, on a card of its colour, it
        // leaves the card's corners round.
        let terminal_card = aurora::card(false, cx)
            .bg(terminal_view::color(DEFAULT_BG))
            .p(px(5.));
        let terminal = on_card(
            terminal_card,
            div().size_full().children(self.terminal.clone()),
        )
        .debug_selector(|| "terminal".into());
        // ponytail: fixed at the sidebar's width, which does not resize either.
        let agents = (self.agents_right && !self.agents_collapsed).then(|| {
            on_card(aurora::card(false, cx), self.render_agents(cx))
                .debug_selector(|| "agents-panel".into())
        });

        div()
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_action(cx.listener(Self::new_session))
            .on_action(cx.listener(Self::toggle_terminal))
            .on_action(cx.listener(Self::toggle_agents))
            .on_action(cx.listener(Self::close_focused_pane))
            .on_action(cx.listener(Self::save))
            .on_action(cx.listener(Self::focus_next_pane))
            .on_action(cx.listener(Self::open_folder))
            .on_action(cx.listener(Self::open_palette))
            .on_action(cx.listener(|this, _: &OpenSettings, window, cx| {
                this.open_settings(Section::Ai, None, window, cx)
            }))
            .relative()
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .children(aurora::backdrop(cx))
            .child(
                v_flex().size_full().child(top_bar).child(
                    div().flex_1().min_h_0().p_1().child(
                        h_resizable("workspace")
                            .with_handle_appearance(aurora::handle())
                            .child(
                                resizable_panel()
                                    .size(px(SIDEBAR_WIDTH))
                                    .fixed()
                                    .child(sidebar),
                            )
                            .child(
                                resizable_panel().child(
                                    v_resizable("center")
                                        .with_handle_appearance(aurora::handle())
                                        .child(
                                            resizable_panel().child(
                                                // Keyed by count: each split starts with equal
                                                // widths.
                                                h_resizable(("panes", self.panes.len()))
                                                    .with_handle_appearance(aurora::handle())
                                                    .children(panes),
                                            ),
                                        )
                                        .child(
                                            resizable_panel()
                                                .size(px(TERMINAL_HEIGHT))
                                                .fixed()
                                                .visible(self.terminal_visible)
                                                .child(terminal),
                                        ),
                                ),
                            )
                            .child(
                                resizable_panel()
                                    .size(px(SIDEBAR_WIDTH))
                                    .fixed()
                                    .visible(agents.is_some())
                                    .child(div().size_full().children(agents)),
                            ),
                    ),
                ),
            )
    }
}

fn seen_models(config: &[wai_agent::ConfigOption]) -> Vec<SeenModel> {
    (config.iter())
        .filter(|o| o.category == wai_agent::ConfigCategory::Model)
        .flat_map(|o| o.values.iter())
        .map(|v| SeenModel {
            id: v.id.clone(),
            name: v.name.clone(),
        })
        .collect()
}

fn mode_values(
    modes: &[wai_agent::ModeInfo],
    config: &[wai_agent::ConfigOption],
    current: Option<String>,
) -> Option<SeenModes> {
    if !modes.is_empty() {
        let modes = modes.iter().map(|m| SeenModel {
            id: m.id.clone(),
            name: m.name.clone(),
        });
        return Some(SeenModes {
            modes: modes.collect(),
            current,
        });
    }
    let option = (config.iter()).find(|o| o.category == wai_agent::ConfigCategory::Mode)?;
    let modes = (option.values.iter())
        .map(|v| SeenModel {
            id: v.id.clone(),
            name: v.name.clone(),
        })
        .collect::<Vec<_>>();
    (!modes.is_empty()).then(|| SeenModes {
        modes,
        current: Some(option.current.clone()),
    })
}

/// The modes a session offers, from `modes` or else the Mode config option.
fn seen_modes(session: &ChatSession) -> Option<SeenModes> {
    let m = &session.model;
    mode_values(&m.modes, &m.config, m.current_mode.clone())
}

pub(crate) fn started_modes(started: &wai_agent::SessionStarted) -> Option<SeenModes> {
    mode_values(
        &started.modes,
        &started.config,
        started.current_mode.clone(),
    )
}

fn fork_failure(error: &AgentError) -> String {
    format!("Could not fork session: {error}")
}

/// A session's state as extensions see it.
fn state_name(state: &TurnState) -> &'static str {
    match state {
        TurnState::Idle => "idle",
        TurnState::Working => "working",
        TurnState::NeedsYou => "needs_you",
        TurnState::Error(_) => "error",
    }
}

/// ACP's names: `end_turn`, `cancelled`, `max_tokens`, `refusal`, or the agent's own.
fn stop_reason(reason: &StopReason) -> String {
    match reason {
        StopReason::EndTurn => "end_turn".into(),
        StopReason::Cancelled => "cancelled".into(),
        StopReason::MaxTokens => "max_tokens".into(),
        StopReason::Refusal => "refusal".into(),
        StopReason::Other(other) => other.clone(),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use gpui_kit::component::{ActiveTheme as _, WindowExt as _};
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    use gpui_kit::{
        AppContext as _, ElementId, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
        VisualTestContext, point, px, size,
    };
    use wai_agent::{AgentError, PromptBlock, SessionEvent, SessionSummary};
    use wai_settings::{
        AgentConfig, AppState, Change, OpenSession, Settings, ThemeChoice, shown, unapproved,
    };

    use super::{Pane, Workspace};
    use crate::agents::{AgentStatus, RegistryEvent};
    use crate::app::{self, ToggleTerminal};
    use crate::chat::{
        Attachment, ChatEvent, ChatItem, ChatSession, NoticeKind, TurnState, signed_out,
    };
    use crate::notify;
    use crate::sidebar::canonical;

    pub(crate) fn open(
        cx: &mut TestAppContext,
        settings: Settings,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        open_at(cx, settings, None)
    }

    /// `open`, keeping the state in `state_path` (never the real `~/.config/wai/state.json`).
    pub(crate) fn open_at(
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
            w.user_dir = std::env::temp_dir().join("wai-no-config");
            w.claude_home = std::env::temp_dir().join("wai-no-claude");
            w.claude_program = Some(std::env::temp_dir().join("wai-no-claude/claude"));
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
        let command = format!("{} {flag}", wai_agent::quote(&fake));
        Settings {
            default_agent: Some("fake".into()),
            agents: [("fake".into(), AgentConfig { command })].into(),
            ..Default::default()
        }
    }

    /// `fake_settings` whose `session/new` waits for the returned gate file to exist.
    pub(crate) fn gated(flags: &str, dir: &Path) -> (Settings, PathBuf) {
        let gate = dir.join("gate");
        let flags = format!("{flags} --new-session-gate {}", wai_agent::quote(&gate));
        (fake_settings(&flags), gate)
    }

    /// Starts a session with the default agent, without the ⌘N entry point or any wait.
    pub(crate) fn start_unwaited(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                let agent = w.default_agent().unwrap();
                w.start_session(agent, None, false, None, window, cx)
            })
        });
    }

    /// Starts a session with the default agent, without the ⌘N entry point, and waits for it.
    pub(crate) fn start_now(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ChatSession> {
        let before = workspace.read_with(cx, |w, _| w.sessions.len());
        start_unwaited(workspace, cx);
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() > before)
        });
        workspace.read_with(cx, |w, _| w.sessions[before].clone())
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
            "--sessions {n} --sessions-cwd {}",
            wai_agent::quote(&cwd)
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
    fn no_default_opens_settings_on_new_session(cx: &mut TestAppContext) {
        let settings = Settings {
            default_agent: None,
            ..fake_settings("")
        };
        let (workspace, cx) = open(cx, settings);
        cx.simulate_keystrokes("secondary-n");
        workspace.read_with(cx, |w, cx| {
            let view = w.settings_view().expect("the settings modal");
            assert_eq!(view.read(cx).section(), crate::settings_view::Section::Ai);
            assert_eq!(view.read(cx).notice().as_deref(), Some(super::CHOOSE_AI));
            assert!(matches!(w.panes.as_slice(), [Pane::Empty(_)]));
            assert!(w.registry.read(cx).status("fake").is_none());
        });
    }

    #[gpui_kit::test]
    fn new_session_opens_a_draft_without_starting_anything(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let pid = dir.path().join("pid");
        let flag = format!("--pid-file {}", wai_agent::quote(&pid));
        let (workspace, cx) = open(cx, fake_settings(&flag));

        cx.simulate_keystrokes("secondary-n");
        cx.run_until_parked();
        workspace.read_with(cx, |w, cx| {
            assert!(matches!(w.panes[0], Pane::Draft(_)));
            assert_eq!(w.drafts.len(), 1);
            assert!(w.sessions.is_empty());
            assert!(w.registry.read(cx).status("fake").is_none());
        });
        assert!(!pid.exists(), "no agent process started");
    }

    #[gpui_kit::test]
    fn every_entry_point_opens_a_draft(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        workspace.update(cx, |w, _| {
            w.templates = vec![template("t", Some("fake"), "Write tests")]
        });
        let drafts = |cx: &mut VisualTestContext| workspace.read_with(cx, |w, _| w.drafts.len());
        let last = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, cx| w.drafts.last().unwrap().read(cx).text(cx))
        };
        let untouched = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, cx| {
                w.sessions.is_empty() && w.registry.read(cx).status("fake").is_none()
            })
        };

        click("sidebar-new", cx);
        assert_eq!(drafts(cx), 1);
        assert!(untouched(cx));

        cx.update(|window, cx| {
            window.render_frame(cx);
            window.double_click("tab-sessions", cx);
        });
        assert_eq!(drafts(cx), 2);
        assert!(untouched(cx));

        cx.update(|window, cx| workspace.update(cx, |w, cx| w.start_template(0, window, cx)));
        assert_eq!(drafts(cx), 3);
        assert_eq!(last(cx), "Write tests");
        assert!(untouched(cx));

        let ext = wai_ext::ExtId::new("opener").unwrap();
        let command = wai_ext::CommandSpec {
            id: "review".into(),
            title: "Review".into(),
            prompt: Some("Review".into()),
        };
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.run_ext_command(&ext, command, window, cx))
        });
        assert_eq!(drafts(cx), 4);
        assert_eq!(last(cx), "Review");
        assert!(untouched(cx));
    }

    #[gpui_kit::test]
    fn new_session_twice_gives_two_drafts(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.simulate_keystrokes("secondary-n secondary-n");
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.drafts.len(), 2);
            assert_eq!(w.panes[0].draft(), Some(&w.drafts[1]));
        });
    }

    #[gpui_kit::test]
    fn sending_a_draft_starts_one_session_in_its_folder(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.simulate_keystrokes("secondary-n");
        cx.simulate_input("hello");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 1)
        });
        let session = workspace.read_with(cx, |w, _| {
            assert!(w.drafts.is_empty());
            assert_eq!(w.panes[0].session(), w.sessions.first());
            w.sessions[0].clone()
        });
        session.read_with(cx, |s, _| {
            assert_eq!(canonical(&s.cwd), canonical(&std::env::temp_dir()));
            assert_eq!(
                s.model.items[0],
                ChatItem::User {
                    text: "hello".into()
                }
            );
        });
        wait_until(cx, "the reply", |cx| {
            session.read_with(cx, |s, _| {
                s.model.items.iter().any(
                    |i| matches!(i, ChatItem::Agent { text } if text.contains("Hello from fake agent")),
                )
            })
        });
    }

    #[gpui_kit::test]
    fn failed_start_keeps_the_draft(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.txt");
        std::fs::write(&file, "a").unwrap();
        let (workspace, cx) = open(cx, fake_settings("--require-auth"));
        cx.simulate_keystrokes("secondary-n");
        let draft = workspace.read_with(cx, |w, _| w.drafts[0].clone());
        draft.update(cx, |d, cx| {
            d.attachments.push(Attachment::new(&file));
            cx.notify();
        });
        cx.simulate_input("hello");
        cx.simulate_keystrokes("enter");

        wait_until(cx, "the notice", |cx| {
            draft.read_with(cx, |d, _| d.notice == Some(signed_out("fake")))
        });
        draft.read_with(cx, |d, cx| {
            assert!(d.starting.is_none());
            assert_eq!(d.text(cx), "hello");
            assert_eq!(d.attachments.len(), 1);
        });
        workspace.read_with(cx, |w, _| {
            assert!(w.sessions.is_empty());
            assert_eq!(w.panes[0].draft(), Some(&draft));
        });
    }

    #[gpui_kit::test]
    fn send_to_a_missing_folder_keeps_the_draft(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("gone");
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.simulate_keystrokes("secondary-n");
        let draft = workspace.read_with(cx, |w, _| w.drafts[0].clone());
        draft.update(cx, |d, _| d.folder = gone.clone());
        cx.simulate_input("hello");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();

        draft.read_with(cx, |d, cx| {
            let want = format!("The folder {} no longer exists.", shown(&gone));
            assert_eq!(d.notice.as_deref(), Some(want.as_str()));
            assert!(d.starting.is_none());
            assert_eq!(d.text(cx), "hello");
        });
        workspace.read_with(cx, |w, cx| {
            assert!(w.registry.read(cx).status("fake").is_none());
        });
    }

    /// A draft is never saved: the state has no session for it and a relaunch has none.
    #[gpui_kit::test]
    fn drafts_are_not_saved(cx: &mut TestAppContext, relaunch: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));
        launch(&workspace, cx);
        while question(&workspace, cx).is_some() {
            click("approval-allow", cx);
        }
        cx.simulate_keystrokes("secondary-n");
        cx.simulate_input("x");
        workspace.read_with(cx, |w, cx| w.save_state(cx));
        assert!(AppState::load(&path).open_sessions.is_empty());

        relaunch.executor().allow_parking();
        let (workspace, cx) = open_at(relaunch, fake_settings(""), Some(path));
        launch(&workspace, cx);
        workspace.read_with(cx, |w, _| {
            assert!(w.drafts.is_empty());
            assert!(matches!(w.panes[..], [Pane::Empty(None)]));
        });
    }

    #[gpui_kit::test]
    fn choosing_a_default_clears_the_notice(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let settings = Settings {
            default_agent: None,
            ..fake_settings("")
        };
        let (workspace, cx) = open(cx, settings);
        cx.simulate_keystrokes("secondary-n");
        let view = workspace.read_with(cx, |w, _| w.settings_view().expect("the settings modal"));
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.apply_settings(fake_settings(""), vec![], window, cx)
            })
        });
        assert_eq!(view.read_with(cx, |v, _| v.notice()), None);
    }

    #[gpui_kit::test]
    fn choosing_a_default_starts_it(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let fake = Settings {
            default_agent: None,
            ..fake_settings("")
        };
        let file = format!(
            "[agents.fake]\ncommand = \'\'\'{}\'\'\'\n",
            fake.agents["fake"].command
        );
        let (workspace, cx, _dir) = live(cx, fake, Some(&file));
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.change_setting(Change::DefaultAgent("fake".into()), window, cx)
            })
        });
        wait_until(cx, "the agent", |cx| {
            workspace.read_with(cx, |w, cx| {
                matches!(
                    w.registry.read(cx).status("fake"),
                    Some(AgentStatus::Ready(_))
                )
            })
        });
        cx.simulate_keystrokes("secondary-n");
        workspace.read_with(cx, |w, cx| {
            let draft = w.panes[0].draft().expect("a draft");
            assert_eq!(draft.read(cx).agent, "fake");
        });
    }

    #[gpui_kit::test]
    fn new_session_appears_in_open_list(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (settings, gate) = gated("", tmp.path());
        let (workspace, cx) = open(cx, settings);
        let start = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                workspace.update(cx, |w, cx| {
                    w.start_session("fake".into(), None, false, None, window, cx)
                })
            })
        };

        start(cx);
        workspace.read_with(cx, |w, _| {
            assert!(
                matches!(&w.panes[0], Pane::Preparing { agent, .. } if agent == "fake"),
                "the pane is reserved while the agent prepares the session"
            )
        });
        // Ignored: the pane is already waiting for a session.
        start(cx);
        workspace.read_with(cx, |w, _| assert_eq!(w.starting.get("fake"), Some(&1)));

        std::fs::write(&gate, "").unwrap();
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
        let tmp = tempfile::tempdir().unwrap();
        let (settings, gate) = gated("", tmp.path());
        let (workspace, cx) = open(cx, settings);
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());

        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.start_session("fake".into(), None, false, None, window, cx)
            })
        });
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

        std::fs::write(&gate, "").unwrap();
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        let title = workspace.read_with(cx, |w, cx| {
            w.panes[0].session().unwrap().read(cx).model.title.clone()
        });
        // The 1,001st event was dropped.
        assert_eq!(title.as_deref(), Some("title 999"));
    }

    /// ⌘W on a "Preparing…" pane only takes it off the screen: the session still opens, in the
    /// sidebar.
    #[gpui_kit::test]
    fn hide_while_preparing_still_opens_the_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));

        start_unwaited(&workspace, cx);
        cx.simulate_keystrokes("secondary-w");
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[..], [Pane::Empty(None)]))
        });
        start_unwaited(&workspace, cx);
        wait_until(cx, "both sessions to land", |cx| {
            workspace.read_with(cx, |w, _| {
                w.starting.get("fake") == Some(&0) && w.panes[0].session().is_some()
            })
        });

        workspace.read_with(cx, |w, _| {
            assert_eq!(w.sessions.len(), 2, "both sessions open");
            assert_eq!(w.panes[0].session(), w.sessions.last());
        });
    }

    /// R87: a session pane's × does what ⌘W does there, focused or not: its pane goes and the
    /// session stays open in the sidebar, running.
    #[gpui_kit::test]
    fn close_button_hides_its_own_pane(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        let rows = load_history(&workspace, 2, cx);
        for (row, side) in rows.iter().zip([false, true]) {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, side, window, cx)));
        }
        let first = pane_session(&workspace, 0, cx);
        let second = pane_session(&workspace, 1, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.focus_pane(0, window, cx)));

        cx.update(|window, cx| {
            window.render_frame(cx);
            let pane = ElementId::View(second.entity_id());
            window.within(pane).click("session-close", cx);
        });

        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 1);
            assert_eq!(w.panes[0].session(), Some(&first));
            assert!(w.sessions.contains(&second), "it stays in the open list");
        });

        // A click's mouse-down already focuses its pane; the × pressed from the keyboard does
        // not, and still hides its own pane.
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.show(second.clone(), true, window, cx);
                w.focus_pane(0, window, cx);
            })
        });
        second.update(cx, |_, cx| cx.emit(ChatEvent::Hide));
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 1);
            assert_eq!(w.panes[0].session(), Some(&first));
        });

        // ⌘W on the last pane leaves it empty; both sessions stay open.
        cx.simulate_keystrokes("secondary-w");
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[..], [Pane::Empty(None)]));
            assert_eq!(w.sessions, [first.clone(), second.clone()]);
        });
    }

    /// An Open row's × ends that session: it leaves the open list and the pane showing it goes.
    #[gpui_kit::test]
    fn open_row_close_ends_the_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(3));
        let rows = load_history(&workspace, 3, cx);
        for (row, side) in rows.iter().zip([false, true, false]) {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, side, window, cx)));
        }
        let close = |session: &Entity<crate::chat::ChatSession>, cx: &mut VisualTestContext| {
            let ix = workspace.read_with(cx, |w, _| w.sessions.iter().position(|s| s == session));
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.click(("open-close", ix.unwrap()), cx);
            });
        };
        let (left, right) = (
            pane_session(&workspace, 0, cx),
            pane_session(&workspace, 1, cx),
        );
        // One row replaced another in its pane: that one is open, in no pane.
        let hidden = workspace.read_with(cx, |w, _| {
            w.sessions
                .iter()
                .find(|s| ![&left, &right].contains(s))
                .cloned()
        });

        close(&hidden.expect("a session in no pane"), cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.sessions, [left.clone(), right.clone()]);
            assert_eq!(w.panes.len(), 2, "no pane showed it");
        });

        close(&right, cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.sessions, std::slice::from_ref(&left));
            assert_eq!(w.panes.len(), 1);
            assert_eq!(w.panes[0].session(), Some(&left));
        });
    }

    /// The Settings modal leaves the panes alone: the open session keeps its pane and stays in the
    /// Open list, and Escape closes the modal.
    #[gpui_kit::test]
    fn settings_modal_leaves_the_panes_alone(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(1));
        let rows = load_history(&workspace, 1, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));
        let session = pane_session(&workspace, 0, cx);

        cx.simulate_keystrokes("secondary-,");
        workspace.read_with(cx, |w, _| {
            assert!(w.settings_view().is_some());
            assert_eq!(w.panes.len(), 1);
            assert_eq!(w.panes[0].session(), Some(&session));
        });
        cx.simulate_keystrokes("escape");
        workspace.read_with(cx, |w, _| {
            assert!(w.settings_view().is_none());
            assert_eq!(w.panes.len(), 1);
            assert_eq!(w.panes[0].session(), Some(&session));
            assert_eq!(w.sessions, std::slice::from_ref(&session));
        });
    }

    /// The top bar's Settings button opens the modal; Escape closes it.
    #[gpui_kit::test]
    fn settings_button_opens_the_modal(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        let shown = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| w.settings_view().is_some())
        };
        click("topbar-settings", cx);
        assert!(shown(cx));
        cx.simulate_keystrokes("escape");
        assert!(!shown(cx));
    }

    /// A lone empty pane has no ×; in a split, an empty pane's × drops that pane.
    #[gpui_kit::test]
    fn lone_empty_pane_has_no_close(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let close = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find("pane-close").is_some()
            })
        };
        assert!(!close(cx));

        start_now(&workspace, cx);
        workspace.update(cx, |w, _| w.panes.push(Pane::Empty(None)));
        assert!(close(cx));
        cx.update(|window, cx| window.click("pane-close", cx));

        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[..], [Pane::Session(_)]));
            assert_eq!(w.sessions.len(), 1, "the session stays open");
        });
    }

    fn center_of(id: impl Into<ElementId>, cx: &mut VisualTestContext) -> Point<Pixels> {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(id).bounds().center()
        })
    }

    /// Drags from `from` to `x` of the way across pane `ix`, at its middle. Each event has its
    /// own update, as on a real window (`window.drag` sends them all in one): a pane's
    /// mouse-down focus, deferred, lands before the drop.
    pub(crate) fn drag_to_pane(from: Point<Pixels>, ix: usize, x: f32, cx: &mut VisualTestContext) {
        let pane = cx
            .debug_bounds(format!("pane-{ix}").leak())
            .expect("pane drawn");
        let to = point(pane.left() + pane.size.width * x, pane.center().y);
        let none = Modifiers::none();
        cx.simulate_mouse_move(from, None, none);
        cx.simulate_mouse_down(from, MouseButton::Left, none);
        for at in [from + point(px(8.), px(0.)), to] {
            cx.simulate_mouse_move(at, MouseButton::Left, none);
            cx.update(|window, cx| window.render_frame(cx));
        }
        cx.simulate_mouse_up(to, MouseButton::Left, none);
    }

    /// R89: an Open row dropped on a pane's right quarter opens in a new pane there, focused.
    #[gpui_kit::test]
    fn drag_open_row_to_right_edge_splits(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        let rows = load_history(&workspace, 2, cx);
        for row in &rows {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, false, window, cx)));
        }
        // The second took the only pane: the first is open in none.
        let (first, second) =
            workspace.read_with(cx, |w, _| (w.sessions[0].clone(), w.sessions[1].clone()));
        assert_eq!(pane_session(&workspace, 0, cx), second);

        let from = center_of(("open", 0usize), cx);
        drag_to_pane(from, 0, 0.9, cx);

        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2);
            assert_eq!(w.panes[0].session(), Some(&second));
            assert_eq!(w.panes[1].session(), Some(&first));
            assert_eq!(w.focused_pane, 1);
        });
    }

    /// An Agents card drags like an Open row: dropped on a pane's right quarter, its session
    /// opens in a new pane there.
    #[gpui_kit::test]
    fn drag_agent_card_to_right_edge_splits(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        let rows = load_history(&workspace, 2, cx);
        for row in &rows {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, false, window, cx)));
        }
        let (first, second) =
            workspace.read_with(cx, |w, _| (w.sessions[0].clone(), w.sessions[1].clone()));
        assert_eq!(pane_session(&workspace, 0, cx), second);

        click("tab-agents", cx);
        let from = center_of(("agent-card", 0usize), cx);
        drag_to_pane(from, 0, 0.9, cx);

        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2);
            assert_eq!(w.panes[0].session(), Some(&second));
            assert_eq!(w.panes[1].session(), Some(&first));
            assert_eq!(w.focused_pane, 1);
        });
    }

    /// A card drags from the Agents panel on the right as it does from the sidebar's tab.
    #[gpui_kit::test]
    fn drag_agent_card_from_right_panel_splits(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        cx.simulate_resize(size(px(1400.), px(900.)));
        let rows = load_history(&workspace, 2, cx);
        for row in &rows {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, false, window, cx)));
        }
        let (first, second) =
            workspace.read_with(cx, |w, _| (w.sessions[0].clone(), w.sessions[1].clone()));
        assert_eq!(pane_session(&workspace, 0, cx), second);

        click("tab-agents", cx);
        click("agents-move", cx);
        let from = center_of(("agent-card", 0usize), cx);
        let panel = cx.debug_bounds("agents-panel").expect("panel drawn");
        assert!(panel.contains(&from), "the card is in the right panel");
        drag_to_pane(from, 0, 0.9, cx);

        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2);
            assert_eq!(w.panes[0].session(), Some(&second));
            assert_eq!(w.panes[1].session(), Some(&first));
            assert_eq!(w.focused_pane, 1);
        });
    }

    /// R89: a History row dropped on a pane's center resumes there; the session it replaced
    /// stays open, in no pane.
    #[gpui_kit::test]
    fn history_drop_on_center_keeps_the_replaced_session_open(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        let rows = load_history(&workspace, 2, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));
        let first = pane_session(&workspace, 0, cx);
        cx.update(|window, cx| window.click("tab-history", cx));

        let from = center_of(("history", 1usize), cx);
        drag_to_pane(from, 0, 0.5, cx);

        let shown = pane_session(&workspace, 0, cx);
        assert_eq!(shown.read_with(cx, |s, _| s.id.clone()), rows[1].id);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 1);
            assert_eq!(w.sessions.len(), 2);
            assert!(
                w.sessions.contains(&first),
                "the replaced session stays open"
            );
        });
    }

    /// R89: a pane's header dropped on another pane's center swaps the two.
    #[gpui_kit::test]
    fn drag_header_onto_pane_swaps(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        let rows = load_history(&workspace, 2, cx);
        for (row, side) in rows.iter().zip([false, true]) {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, side, window, cx)));
        }
        let (first, second) = (
            pane_session(&workspace, 0, cx),
            pane_session(&workspace, 1, cx),
        );

        let from = cx.update(|window, cx| {
            window.render_frame(cx);
            let header = window.within(ElementId::View(second.entity_id()));
            header.find("session-title").bounds().center()
        });
        drag_to_pane(from, 0, 0.5, cx);

        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2);
            assert_eq!(w.panes[0].session(), Some(&second));
            assert_eq!(w.panes[1].session(), Some(&first));
            assert_eq!(w.focused_pane, 0);
        });
    }

    /// R89: a center drop never replaces an editor with unsaved edits: the session opens beside.
    #[gpui_kit::test]
    fn center_drop_keeps_an_unsaved_editor(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "a").unwrap();
        let (workspace, cx) = open(cx, history_settings(1));
        let rows = load_history(&workspace, 1, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));
        let session = pane_session(&workspace, 0, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_file(path, None, window, cx)));
        cx.run_until_parked();
        // The session stays open, in no pane; the editor holds an unsaved tab.
        let editor = workspace.update(cx, |w, _| {
            w.panes.remove(0);
            w.focused_pane = 0;
            match &w.panes[..] {
                [Pane::Editor(editor)] => editor.clone(),
                _ => panic!("the editor pane alone"),
            }
        });
        let tab = editor.read_with(cx, |e, _| e.tabs[0].clone());
        tab.update(cx, |t, _| t.dirty = true);

        let dragged = super::DraggedSession {
            title: "Session".into(),
            source: super::Dragged::Open(session.clone()),
        };
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.drop_session(&dragged, 0, super::Zone::Center, window, cx)
            })
        });

        workspace.read_with(cx, |w, _| {
            assert!(matches!(&w.panes[0], Pane::Editor(e) if *e == editor));
            assert_eq!(w.panes.len(), 2);
            assert_eq!(w.panes[1].session(), Some(&session));
            assert_eq!(w.focused_pane, 1);
        });
        assert!(tab.read_with(cx, |t, _| t.dirty), "the edits stay");
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
        workspace.read_with(cx, |w, cx| {
            let draft = w.panes[0].draft().unwrap().read(cx);
            assert_eq!(draft.agent, "fake");
            assert_eq!(draft.text(cx), "Review this diff\nline 2");
            assert!(w.sessions.is_empty(), "nothing started");
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
        // The test window is never dropped: stop the project's watcher before its folder goes,
        // or its event lands on the test scheduler from the watcher's thread.
        switch(&PathBuf::from("/nonexistent"), cx);
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

        cx.update(|window, cx| window.click("tab-history", cx));
        cx.update(|window, cx| window.click("all-projects", cx));
        assert_eq!(history_rows(&workspace, cx), 25);
    }

    /// The old default's rows would resume with the new agent: they go until it lists its own.
    #[gpui_kit::test]
    fn switching_the_default_clears_the_old_history(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        load_history(&workspace, 2, cx);
        let mut settings = workspace.read_with(cx, |w, _| w.settings.clone());
        let other = settings.agents["fake"].clone();
        settings.agents.insert("other".into(), other);
        settings.default_agent = Some("other".into());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.apply_settings(settings, vec![], window, cx))
        });
        assert_eq!(history_rows(&workspace, cx), 0);
        wait_until(cx, "the new default's history", |cx| {
            history_rows(&workspace, cx) == 2
        });
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

        // A history row's menu offers Fork, and so does an open session's row.
        cx.update(|window, cx| window.click("tab-history", cx));
        menu_item(("history", 1usize), "Fork", cx);
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
        cx.update(|window, cx| window.click("tab-sessions", cx));
        menu_item(("open", 0usize), "Fork", cx);
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
            "--fail-fork --sessions 2 --sessions-cwd {}",
            wai_agent::quote(&cwd)
        );
        let (workspace, cx) = open(cx, fake_settings(&flags));
        load_history(&workspace, 2, cx);

        cx.update(|window, cx| window.click("tab-history", cx));
        menu_item(("history", 0usize), "Fork", cx);
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
                (px(super::SIDEBAR_WIDTH), px(260.)),
                "window {window:?}"
            );
        }
    }

    #[gpui_kit::test]
    fn light_mode_has_no_backdrop(light: &mut TestAppContext, dark: &mut TestAppContext) {
        let settings = Settings {
            theme: ThemeChoice::Light,
            ..Default::default()
        };
        let (_, light) = open(light, settings);
        light.run_until_parked();
        assert!(light.debug_bounds("aurora-backdrop").is_none());

        let (_, dark) = open(dark, Settings::default());
        dark.run_until_parked();
        assert!(dark.debug_bounds("aurora-backdrop").is_some());
    }

    /// At 900 px, with a 60-character project name and 4 panes, the project name gives way and
    /// every action stays inside the window, clear of the status.
    #[gpui_kit::test]
    fn top_bar_fits_narrow_window(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        workspace.update(cx, |w, cx| {
            // Only its name shows: no folder, no watcher.
            w.project = std::env::temp_dir().join("x".repeat(60));
            w.panes = (0..4).map(|_| Pane::Empty(None)).collect();
            cx.notify();
        });
        let chip_width = |window, cx: &mut VisualTestContext| {
            cx.simulate_resize(window);
            cx.run_until_parked();
            let chip = cx.debug_bounds("topbar-project").expect("project drawn");
            chip.size.width
        };
        let wide = chip_width(size(px(1400.), px(900.)), cx);
        let narrow = chip_width(size(px(900.), px(700.)), cx);

        assert!(
            narrow < wide,
            "the name truncates: {narrow:?} at 900 px, {wide:?} at 1400 px"
        );
        let version = cx.debug_bounds("topbar-version").expect("version drawn");
        let project = cx.debug_bounds("topbar-project").expect("project drawn");
        assert!(version.right() < project.left(), "{version:?} {project:?}");
        let new = cx.debug_bounds("sidebar-new").expect("new session drawn");
        assert!(new.right() <= px(900.), "{new:?}");
        let status = cx.debug_bounds("topbar-status").expect("status drawn");
        let actions = cx.debug_bounds("topbar-actions").expect("actions drawn");
        assert!(status.right() < actions.left(), "{status:?} {actions:?}");
    }

    /// Presses at `from` and moves 1 px: on the macOS title bar, a window move. The test
    /// window's `start_window_move` is `unimplemented!`, so a move panics.
    fn press_and_nudge(from: Point<Pixels>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| window.drag(from, from + point(px(1.), px(0.)), cx));
    }

    /// The title bar moves the window on any press inside it; a top-bar action stops that.
    #[gpui_kit::test]
    fn top_bar_actions_do_not_move_the_window(cx: &mut TestAppContext) {
        let (_, cx) = open(cx, Settings::default());
        let palette = cx.update(|window, cx| {
            window.render_frame(cx);
            window.find("topbar-palette").bounds().center()
        });
        press_and_nudge(palette, cx);
    }

    /// The control: the rest of the bar still moves the window.
    #[cfg(target_os = "macos")]
    #[gpui_kit::test]
    #[should_panic(expected = "not implemented")]
    fn top_bar_project_moves_the_window(cx: &mut TestAppContext) {
        let (_, cx) = open(cx, Settings::default());
        cx.update(|window, cx| window.render_frame(cx));
        let project = cx.debug_bounds("topbar-project").expect("project drawn");
        press_and_nudge(project.center(), cx);
    }

    /// In full screen the traffic lights leave, and so does the title bar's 80 pt gap.
    #[cfg(target_os = "macos")]
    #[gpui_kit::test]
    fn top_bar_drops_the_gap_in_full_screen(cx: &mut TestAppContext) {
        let (_, cx) = open(cx, Settings::default());
        let left = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.render_frame(cx));
            cx.debug_bounds("topbar").expect("top bar drawn").left()
        };
        assert_eq!(left(cx), px(80.));
        cx.update(|window, _| window.toggle_fullscreen());
        assert_eq!(left(cx), px(12.));
    }

    #[gpui_kit::test]
    fn registry_reports_auth_required(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--require-auth"));
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());

        start_unwaited(&workspace, cx);

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

        let session = start_now(&workspace, cx).read_with(cx, |s, _| s.id.clone());
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

        start_now(&workspace, cx);
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
        settings.default_agent = Some("a".into());
        let (workspace, cx) = open(cx, settings);
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());
        let mut sessions = Vec::new();
        for agent in ["a", "b"] {
            cx.update(|window, cx| {
                workspace.update(cx, |w, cx| {
                    w.start_session(agent.into(), None, false, None, window, cx)
                })
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
                (s.model.waiting.iter().chain(&s.model.items)).find_map(|item| match item {
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
    pub(crate) fn launch(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                let pending = unapproved(&Settings::default(), &w.settings, &w.approved);
                w.ask_approval(pending, window, cx);
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
            name: None,
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
        // Nothing but the built-ins is approved yet: the agent and the shell are asked for.
        launch(&workspace, cx);
        while question(&workspace, cx).is_some() {
            click("approval-allow", cx);
        }

        start_now(&workspace, cx);

        let state = AppState::load(&path);
        assert_eq!(
            state.open_sessions,
            [OpenSession {
                agent: "fake".into(),
                session_id: "fake-1".into(),
                cwd: canonical(&std::env::temp_dir()),
                pane: 0,
                name: None,
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
        // The ghost's stays for a launch whose settings have its agent.
        let kept = AppState::load(&path).open_sessions;
        let hidden = saved("fake", "ghi", super::MAX_PANES);
        let ghost = saved("ghost", "gone", 1);
        assert_eq!(
            kept,
            [
                saved("fake", "abc", 0),
                saved("fake", "def", 1),
                hidden,
                ghost
            ]
        );
        assert_eq!(notify::sent(), []);
    }

    /// A session of an agent the settings do not have (a broken file at launch, a pending
    /// command) is not lost: the state keeps it for a launch that can restore it.
    #[gpui_kit::test]
    fn unrestored_sessions_stay_in_the_state(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = AppState {
            open_sessions: vec![saved("x", "1", 0)],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, Settings::default(), Some(path.clone()));
        launch(&workspace, cx);
        assert!(workspace.read_with(cx, |w, _| w.sessions.is_empty()));
        assert_eq!(AppState::load(&path).open_sessions, [saved("x", "1", 0)]);
    }

    #[gpui_kit::test]
    fn restored_session_keeps_the_agents_model(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = AppState {
            open_sessions: vec![saved("fake", "abc", 0)],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let mut settings = fake_settings("--models");
        settings.models.insert("fake".into(), "fake-large".into());
        let (workspace, cx) = open_at(cx, settings, Some(path));
        cx.deactivate_window();
        launch(&workspace, cx);
        let session = pane_session(&workspace, 0, cx);
        wait_until(cx, "the replay", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle && !s.model.items.is_empty()
            })
        });
        session.update(cx, |s, cx| s.send_text("which?", cx));
        wait_until(cx, "the reply", |cx| {
            session.read_with(cx, |s, _| {
                s.model.state == TurnState::Idle
                    && s.model.items.iter().any(
                        |i| matches!(i, ChatItem::Agent { text } if text == "model:fake-small"),
                    )
            })
        });
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

    /// R57: an extension's sessions reuse the pane of its last one (R61: unless the user is in
    /// it), never a pane showing the user's session; with no pane free they open in the open
    /// list only. None takes the focus.
    #[gpui_kit::test]
    fn extension_opens_never_replace_user_sessions(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let mine = start_now(&workspace, cx);
        let ext = wai_ext::ExtId::new("opener").unwrap();
        let theirs = |prompt: &str, cx: &mut VisualTestContext| {
            let n = workspace.read_with(cx, |w, _| w.sessions.len());
            let (prompt, by) = (Some(prompt.to_string()), Some((ext.clone(), None)));
            cx.update(|window, cx| {
                workspace.update(cx, |w, cx| {
                    w.start_session("fake".into(), prompt, false, by, window, cx)
                })
            });
            wait_until(cx, "their session", |cx| {
                workspace.read_with(cx, |w, _| w.sessions.len() == n + 1)
            });
            workspace.read_with(cx, |w, _| w.sessions[n].clone())
        };

        let first = theirs("one", cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2, "a pane beside the user's");
            assert_eq!(w.panes[0].session(), Some(&mine));
            assert_eq!(w.panes[1].session(), Some(&first));
            assert_eq!(w.focused_pane, 0);
        });

        let second = theirs("two", cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2, "its last session's pane, no new one");
            assert_eq!(w.panes[0].session(), Some(&mine));
            assert_eq!(w.panes[1].session(), Some(&second));
        });

        // R61: not while the user is in that pane.
        workspace.update(cx, |w, _| w.focused_pane = 1);
        let third = theirs("three", cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 3);
            assert_eq!(w.panes[1].session(), Some(&second));
            assert_eq!(w.panes[2].session(), Some(&third));
            assert_eq!(w.focused_pane, 1);
        });

        // Every pane the user's (a session, two starts) but the focused one, empty and saying
        // why their own start failed: none is free (R61: not even the focused empty pane).
        let users = |ticket| Pane::Preparing {
            agent: "fake".into(),
            ticket,
            by_extension: None,
        };
        workspace.update(cx, |w, _| {
            let failed = Pane::Empty(Some("Could not start".into()));
            w.panes = vec![Pane::Session(mine.clone()), users(90), users(91), failed];
            w.focused_pane = 3;
        });
        let fourth = theirs("four", cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), super::MAX_PANES);
            assert_eq!(w.panes[0].session(), Some(&mine));
            assert!(
                w.panes[1..3]
                    .iter()
                    .all(|p| matches!(p, Pane::Preparing { .. }))
            );
            assert!(matches!(&w.panes[3], Pane::Empty(Some(why)) if why == "Could not start"));
            assert!(w.sessions.contains(&fourth), "in the open list only");
        });
    }

    /// A window whose settings watcher looks at a fresh user dir holding `file`.
    pub(crate) fn live<'a>(
        cx: &'a mut TestAppContext,
        settings: Settings,
        file: Option<&str>,
    ) -> (
        Entity<Workspace>,
        &'a mut VisualTestContext,
        tempfile::TempDir,
    ) {
        cx.executor().allow_parking(); // the watcher thread is real
        let dir = tempfile::tempdir().unwrap();
        if let Some(file) = file {
            std::fs::write(dir.path().join("settings.toml"), file).unwrap();
        }
        let (workspace, cx) = open(cx, settings);
        let user = dir.path().to_path_buf();
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.user_dir = user;
                w.watch_settings(window, cx);
            })
        });
        (workspace, cx, dir)
    }

    /// `live` with the fake agent also in the settings file, so `set_project`'s reload keeps it.
    pub(crate) fn fake_live<'a>(
        cx: &'a mut TestAppContext,
        flag: &str,
    ) -> (
        Entity<Workspace>,
        &'a mut VisualTestContext,
        tempfile::TempDir,
    ) {
        let settings = fake_settings(flag);
        let file = format!(
            "default_agent = \"fake\"\n[agents.fake]\ncommand = \'\'\'{}\'\'\'\n",
            settings.agents["fake"].command
        );
        live(cx, settings, Some(&file))
    }

    #[gpui_kit::test]
    fn editing_the_file_applies_the_theme(cx: &mut TestAppContext) {
        let (_workspace, cx, dir) = live(cx, Settings::default(), None);
        assert!(cx.update(|_, cx| cx.theme().is_dark()));
        std::fs::write(dir.path().join("settings.toml"), "theme = \"light\"\n").unwrap();
        wait_until(cx, "the light theme", |cx| {
            cx.update(|_, cx| !cx.theme().is_dark())
        });
    }

    #[gpui_kit::test]
    fn broken_edit_keeps_the_last_good_settings(cx: &mut TestAppContext) {
        let light = Settings {
            theme: ThemeChoice::Light,
            ..Default::default()
        };
        let (workspace, cx, dir) = live(cx, light, Some("theme = \"light\"\n"));
        std::fs::write(dir.path().join("settings.toml"), "theme = [").unwrap();
        wait_until(cx, "the error", |cx| {
            workspace.read_with(cx, |w, _| w.settings_error.is_some())
        });
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| assert_eq!(w.settings.theme, ThemeChoice::Light));
        assert!(cx.update(|_, cx| !cx.theme().is_dark()));
        assert!(cx.update(|window, cx| !window.notifications(cx).is_empty()));
    }

    #[gpui_kit::test]
    fn own_write_reloads_once_without_a_second_dialog(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = live(cx, Settings::default(), None);
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.change_setting(Change::Theme(ThemeChoice::Light), window, cx)
            })
        });
        wait_until(cx, "the light theme", |cx| {
            cx.update(|_, cx| !cx.theme().is_dark())
        });
        // Past the debounce: a second batch would show up as a second apply.
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(20));
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
        }
        workspace.read_with(cx, |w, _| assert_eq!(w.applied, 1));
        assert!(cx.update(|window, cx| !window.has_active_dialog(cx)));
    }

    #[gpui_kit::test]
    fn deleted_user_file_falls_back_to_first_run(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let settings = history_settings(1);
        let file = format!(
            "default_agent = \"fake\"\n[agents.fake]\ncommand = \'\'\'{}\'\'\'\n",
            settings.agents["fake"].command
        );
        let (workspace, cx, dir) = live(cx, settings, Some(&file));
        let rows = load_history(&workspace, 1, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&rows[0], false, window, cx)));
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        std::fs::remove_file(dir.path().join("settings.toml")).unwrap();
        wait_until(cx, "the first-run settings", |cx| {
            workspace.read_with(cx, |w, _| w.default_agent().is_none())
        });
        workspace.read_with(cx, |w, cx| {
            assert_eq!(w.sessions.len(), 1);
            let status = w.registry.read(cx).status("fake").cloned();
            assert!(matches!(status, Some(AgentStatus::Ready(_))), "{status:?}");
        });
    }

    #[gpui_kit::test]
    fn project_switch_reloads_settings(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = live(cx, Settings::default(), None);
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".wai")).unwrap();
        std::fs::write(
            project.path().join(".wai/settings.toml"),
            "theme = \"light\"\n",
        )
        .unwrap();
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.set_project(project.path().to_path_buf(), window, cx)
            })
        });
        assert!(cx.update(|_, cx| !cx.theme().is_dark()));
    }

    #[gpui_kit::test]
    fn changed_command_applies_to_the_next_start(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let settings = history_settings(1);
        let (workspace, cx, dir) = live(cx, settings.clone(), None);
        load_history(&workspace, 1, cx);
        std::fs::write(
            dir.path().join("settings.toml"),
            "default_agent = \"fake\"\n[agents.fake]\ncommand = \"/bin/true\"\n",
        )
        .unwrap();
        wait_until(cx, "the question", |cx| question(&workspace, cx).is_some());
        click("approval-allow", cx);
        workspace.read_with(cx, |w, cx| {
            let status = w.registry.read(cx).status("fake").cloned();
            assert!(matches!(status, Some(AgentStatus::Ready(_))), "{status:?}");
            assert_eq!(w.registry.read(cx).command("fake"), Some("/bin/true"));
        });
    }

    #[gpui_kit::test]
    fn shell_change_reaches_the_next_spawn_only(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = live(cx, Settings::default(), None);
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.toggle_terminal(&ToggleTerminal, window, cx));
        });
        let shell = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, cx| {
                w.terminal.as_ref().unwrap().read(cx).shell.clone()
            })
        };
        let before = shell(cx);
        std::fs::write(
            dir.path().join("settings.toml"),
            "[terminal]\nshell = \"/bin/other\"\n",
        )
        .unwrap();
        wait_until(cx, "the question", |cx| question(&workspace, cx).is_some());
        click("approval-allow", cx);
        assert_eq!(shell(cx).as_deref(), Some("/bin/other"));
        assert_ne!(before.as_deref(), Some("/bin/other"));
    }

    #[gpui_kit::test]
    fn reloads_push_a_load_notice_once(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = live(cx, Settings::default(), None);
        let project = tempfile::tempdir().unwrap();
        std::fs::create_dir(project.path().join(".wai")).unwrap();
        std::fs::write(
            project.path().join(".wai/settings.toml"),
            "[agents.x]\ncommand = \"x\"\n",
        )
        .unwrap();
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.set_project(project.path().to_path_buf(), window, cx);
                w.reload_settings(window, cx);
            })
        });
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);
    }

    /// The launch's notices count as shown: the first reload does not push them again.
    #[gpui_kit::test]
    fn launch_notices_are_not_pushed_again(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.show_load_notices(vec!["x".into()], window, cx);
                let settings = w.settings.clone();
                w.apply_settings(settings, vec!["x".into()], window, cx);
            })
        });
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);
    }

    /// Right-clicks the element `target`, then clicks the item labelled `item` in its menu.
    pub(crate) fn menu_item(target: impl Into<ElementId>, item: &str, cx: &mut VisualTestContext) {
        use gpui_kit::test::TestQueryExt as _;
        let at = cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(target).bounds().center()
        });
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        cx.run_until_parked();
        let item = cx.update(|window, cx| {
            window.render_frame(cx);
            window.find_by_label(item).bounds().center()
        });
        cx.simulate_mouse_move(item, None, Modifiers::none());
        cx.simulate_click(item, Modifiers::none());
        cx.run_until_parked();
    }

    pub(crate) fn click(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    /// The approval dialog's text while it is open.
    pub(crate) fn question(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Option<String> {
        let open = cx.update(|window, cx| {
            window.render_frame(cx);
            window.has_active_dialog(cx)
        });
        let titled = cx.debug_bounds("approval-title").is_some();
        let text = workspace.read_with(cx, |w, _| w.asking.first().map(super::approval_text));
        text.filter(|_| open && titled)
    }

    /// `fake` run with `flag`, and a user settings.toml choosing it.
    fn fake_file(flag: &str) -> (String, String) {
        let command = fake_settings(flag).agents["fake"].command.clone();
        let file = format!("default_agent = \"fake\"\n[agents.fake]\ncommand = '''{command}'''\n");
        (command, file)
    }

    /// Writes the user's settings.toml in `dir` and reloads, as the watcher does.
    fn edit_user_file(
        workspace: &Entity<Workspace>,
        dir: &Path,
        file: &str,
        cx: &mut VisualTestContext,
    ) {
        std::fs::write(dir.join("settings.toml"), file).unwrap();
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.user_dir = dir.into();
                w.reload_settings(window, cx)
            })
        });
        cx.run_until_parked();
    }

    /// A new `agent` session that sends "hi": the agent's reply.
    fn reply(workspace: &Entity<Workspace>, agent: &str, cx: &mut VisualTestContext) -> String {
        let n = workspace.read_with(cx, |w, _| w.sessions.len());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                let prompt = Some("hi".into());
                w.start_session(agent.into(), prompt, true, None, window, cx)
            })
        });
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() > n)
        });
        let session = workspace.read_with(cx, |w, _| w.sessions[n].clone());
        let text = |cx: &mut VisualTestContext| {
            session.read_with(cx, |s, _| {
                let idle = s.model.state == TurnState::Idle;
                let agent = s.model.items.iter().rev().find_map(|item| match item {
                    ChatItem::Agent { text } => Some(text.clone()),
                    _ => None,
                });
                agent.filter(|_| idle)
            })
        };
        wait_until(cx, "the reply", |cx| text(cx).is_some());
        text(cx).unwrap()
    }

    #[test]
    fn question_shows_line_breaks_escaped() {
        let hidden = wai_settings::Approval::Agent {
            id: "a\nb".into(),
            old: None,
            new: "ok\n\n\nevil".into(),
        };
        assert_eq!(
            super::approval_text(&hidden),
            "a\\nb (new): ok\\n\\n\\nevil"
        );
    }

    #[test]
    fn question_shows_format_characters_escaped() {
        let hidden = wai_settings::Approval::Shell {
            old: None,
            new: "/bin/sh\u{202e}\u{200b}x".into(),
        };
        assert_eq!(
            super::approval_text(&hidden),
            "Terminal shell: system default → /bin/sh\\u{202e}\\u{200b}x"
        );
    }

    fn asked(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.render_frame(cx));
        cx.debug_bounds("confirm-title").is_some()
    }

    fn working(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        open(
            cx,
            fake_settings("--reply 'a b c d e f g h i j' --chunk-delay-ms 1000"),
        )
    }

    fn work(session: &Entity<ChatSession>, cx: &mut VisualTestContext) {
        session.update(cx, |s, cx| s.send_text("go", cx));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn quit_asks_while_a_session_works(cx: &mut TestAppContext) {
        let (workspace, cx) = working(cx);
        let session = start_now(&workspace, cx);

        cx.simulate_keystrokes("secondary-q");
        assert!(!asked(cx), "an idle session does not stop a quit");

        work(&session, cx);
        cx.simulate_keystrokes("secondary-q");
        assert!(asked(cx));
        cx.update(|window, _| window.find_by_label("1 session is still working"));
        click("cancel", cx);
        assert!(!asked(cx));
        assert_eq!(
            session.read_with(cx, |s, _| s.status().clone()),
            TurnState::Working
        );
    }

    fn staged(file: &Path, sha256: &str) -> crate::update::UpdateStatus {
        crate::update::UpdateStatus::Ready(crate::update::Staged {
            tag: "v9.0.0".into(),
            file: file.into(),
            sha256: sha256.into(),
        })
    }

    fn leave_update(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.leave(super::Leave::Update, window, cx));
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn restart_asks_while_a_session_works(cx: &mut TestAppContext) {
        let (workspace, cx) = working(cx);
        let session = start_now(&workspace, cx);
        work(&session, cx);
        let ready = staged(Path::new("/nonexistent/wai.zip"), "00");
        workspace.update(cx, |w, _| w.update = ready.clone());

        leave_update(&workspace, cx);
        assert!(asked(cx));
        cx.update(|window, _| window.find_by_label("1 session is still working"));
        click("cancel", cx);
        assert!(!asked(cx));
        assert_eq!(workspace.read_with(cx, |w, _| w.update.clone()), ready);
        assert!(!cx.cx.update(|app| app.windows().is_empty()));
        assert_eq!(
            session.read_with(cx, |s, _| s.status().clone()),
            TurnState::Working
        );
    }

    #[gpui_kit::test]
    fn restart_asks_about_unsaved_edits(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        std::fs::write(&path, "a").unwrap();
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_file(path, None, window, cx)));
        cx.run_until_parked();
        let tab = workspace.read_with(cx, |w, cx| {
            let Some(Pane::Editor(editor)) = w.panes.iter().find(|p| matches!(p, Pane::Editor(_)))
            else {
                panic!("an editor pane")
            };
            editor.read(cx).tabs[0].clone()
        });
        tab.update(cx, |t, _| t.dirty = true);

        leave_update(&workspace, cx);
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        assert!(
            !asked(cx),
            "the unsaved-edits dialog comes first, not the busy one"
        );
        click("cancel", cx);
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui_kit::test]
    fn failed_install_keeps_wai_open(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("wai.zip");
        std::fs::write(&file, b"abd").unwrap();
        workspace.update(cx, |w, _| w.update = staged(&file, &"0".repeat(64)));

        cx.update(|window, cx| workspace.update(cx, |w, cx| w.install_update(window, cx)));
        cx.run_until_parked();
        let status = workspace.read_with(cx, |w, _| w.update.clone());
        let crate::update::UpdateStatus::Failed(msg) = status else {
            panic!("{status:?}")
        };
        assert!(msg.contains("changed on disk"), "{msg}");
        assert!(!cx.cx.update(|app| app.windows().is_empty()));
    }

    #[gpui_kit::test]
    fn closing_the_window_asks_while_a_session_works(cx: &mut TestAppContext) {
        let (workspace, cx) = working(cx);
        let session = start_now(&workspace, cx);
        work(&session, cx);

        assert!(!cx.simulate_close());
        assert!(asked(cx));
        click("cancel", cx);
        assert!(!cx.cx.update(|app| app.windows().is_empty()));

        assert!(!cx.simulate_close());
        click("confirm", cx);
        assert!(cx.cx.update(|app| app.windows().is_empty()));
    }

    #[gpui_kit::test]
    fn open_row_close_asks_while_its_session_works(cx: &mut TestAppContext) {
        let (workspace, cx) = working(cx);
        let session = start_now(&workspace, cx);
        work(&session, cx);

        click_close(cx);
        assert!(asked(cx));
        cx.update(|window, _| window.find_by_label("This session is still working"));
        click("cancel", cx);
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), 1);

        click_close(cx);
        click("confirm", cx);
        assert!(workspace.read_with(cx, |w, _| w.sessions.is_empty()));
    }

    #[gpui_kit::test]
    fn idle_session_row_close_does_not_ask(cx: &mut TestAppContext) {
        let (workspace, cx) = working(cx);
        start_now(&workspace, cx);

        click_close(cx);
        assert!(!asked(cx));
        assert!(workspace.read_with(cx, |w, _| w.sessions.is_empty()));
    }

    #[test]
    fn resume_command_quotes_the_folder_and_id() {
        assert_eq!(
            super::resume_command(Path::new("/tmp/it's here"), "abc"),
            r"cd '/tmp/it'\''s here' && claude --resume 'abc'"
        );
    }

    fn closes_file(dir: &Path) -> (PathBuf, String) {
        let file = dir.join("closed");
        let flag = format!("--close-file {}", wai_agent::quote(&file));
        (file, flag)
    }

    fn closed(file: &Path) -> bool {
        std::fs::read_to_string(file).is_ok_and(|s| s.contains("fake-1"))
    }

    #[gpui_kit::test]
    fn ending_a_session_closes_it_in_the_agent(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let (file, flag) = closes_file(dir.path());
        let (workspace, cx) = open(cx, fake_settings(&flag));
        start_now(&workspace, cx);

        click_close(cx);
        wait_until(cx, "the close", |_| closed(&file));
    }

    #[gpui_kit::test]
    fn continue_in_terminal_ends_the_session_and_types_the_resume(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent and the shell are real processes
        let dir = tempfile::tempdir().unwrap();
        let (file, flag) = closes_file(dir.path());
        let mut settings = fake_settings(&flag);
        let fake = settings.agents.remove("fake").unwrap();
        settings.agents = [("claude".into(), fake)].into();
        settings.default_agent = Some("claude".into());
        let (workspace, cx) = open(cx, settings);
        start_now(&workspace, cx);

        menu_item(("open", 0usize), "Continue in terminal", cx);

        assert!(workspace.read_with(cx, |w, _| w.sessions.is_empty()));
        wait_until(cx, "the close", |_| closed(&file));
        assert!(workspace.read_with(cx, |w, _| w.terminal_visible));
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().unwrap());
        wait_until(cx, "the typed command", |cx| {
            terminal.read_with(cx, |t, _| {
                t.snapshot.rows.iter().any(|row| {
                    let text: String = row.iter().map(|run| run.text.as_str()).collect();
                    text.contains("claude --resume 'fake-1'")
                })
            })
        });
    }

    #[gpui_kit::test]
    fn continue_in_terminal_is_only_for_claude(cx: &mut TestAppContext) {
        use gpui_kit::test::TestQueryExt as _;
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        start_now(&workspace, cx);

        let at = cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(("open", 0usize)).bounds().center()
        });
        cx.simulate_mouse_down(at, MouseButton::Right, Modifiers::none());
        cx.simulate_mouse_up(at, MouseButton::Right, Modifiers::none());
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert!(window.try_find_by_label("Continue in terminal").is_none());
            assert!(window.try_find_by_label("Rename").is_some());
        });
    }

    fn click_close(cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(("open-close", 0usize), cx);
        });
        cx.run_until_parked();
    }

    fn status(
        workspace: &Entity<Workspace>,
        agent: &str,
        cx: &mut VisualTestContext,
    ) -> Option<AgentStatus> {
        workspace.read_with(cx, |w, cx| w.registry.read(cx).status(agent).cloned())
    }

    /// A launch whose default agent's command waits for an answer runs nothing before it, not
    /// even the last approved command; Allow runs the new one.
    #[gpui_kit::test]
    fn launch_waits_for_the_default_agents_answer(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        let none = AppState {
            approved: Some(Default::default()),
            ..AppState::default()
        };
        none.save(&state).unwrap();
        let command = fake_settings("--reply other").agents["fake"]
            .command
            .clone();
        let mut settings = Settings::default();
        let config = AgentConfig {
            command: command.clone(),
        };
        settings.agents.insert("antigravity".into(), config);
        settings.default_agent = Some("antigravity".into());
        let (workspace, cx) = open_at(cx, settings, Some(state));
        launch(&workspace, cx);
        assert_eq!(
            question(&workspace, cx),
            Some(format!("Antigravity: agy acp → {command}"))
        );
        assert_eq!(status(&workspace, "antigravity", cx), None);
        click("approval-allow", cx);
        assert!(status(&workspace, "antigravity", cx).is_some());
        // The test's shell.
        click("approval-allow", cx);
        assert_eq!(reply(&workspace, "antigravity", cx), "other");
    }

    /// The same on a switch of the default; Don't allow runs the last approved command.
    #[gpui_kit::test]
    fn switch_waits_for_the_default_agents_answer(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let settings = Settings {
            default_agent: None,
            ..fake_settings("")
        };
        let (workspace, cx) = open(cx, settings);
        let dir = tempfile::tempdir().unwrap();
        let (_, file) = fake_file("--reply other");
        edit_user_file(&workspace, dir.path(), &file, cx);
        assert!(question(&workspace, cx).is_some());
        assert_eq!(status(&workspace, "fake", cx), None);
        click("approval-deny", cx);
        assert!(status(&workspace, "fake", cx).is_some());
        assert_eq!(reply(&workspace, "fake", cx), "Hello from fake agent");
    }

    /// A declined value is not asked about again this run, unless its agent becomes the default;
    /// the next launch asks again.
    #[gpui_kit::test]
    fn declined_value_waits_for_the_next_launch(
        cx: &mut TestAppContext,
        relaunch: &mut TestAppContext,
    ) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        // From before approvals: the first launch approves today's command.
        AppState::default().save(&state).unwrap();
        let settings = Settings {
            default_agent: None,
            ..fake_settings("")
        };
        let (workspace, cx) = open_at(cx, settings, Some(state.clone()));
        launch(&workspace, cx);
        let (new, _) = fake_file("--reply other");
        let file = format!("[agents.fake]\ncommand = '''{new}'''\n");
        edit_user_file(&workspace, dir.path(), &file, cx);
        assert!(question(&workspace, cx).is_some());
        click("approval-deny", cx);
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.change_setting(Change::Theme(ThemeChoice::Light), window, cx);
                w.reload_settings(window, cx);
            })
        });
        cx.run_until_parked();
        assert_eq!(question(&workspace, cx), None);
        let chosen = format!("default_agent = \"fake\"\n{file}");
        edit_user_file(&workspace, dir.path(), &chosen, cx);
        assert!(question(&workspace, cx).is_some());
        click("approval-deny", cx);

        relaunch.executor().allow_parking();
        let (settings, _) = Settings::load(dir.path(), None).unwrap();
        let (workspace, cx) = open_at(relaunch, settings, Some(state));
        launch(&workspace, cx);
        assert_eq!(question(&workspace, cx), Some(format!("fake (new): {new}")));
    }

    /// A command or shell edited on the screen is asked about before anything is written: Allow
    /// writes it, once and with no second question; Don't allow writes nothing.
    fn screen_edit_asks_first(change: Change, allow: bool, cx: &mut TestAppContext) {
        let (workspace, cx, dir) = live(cx, fake_settings(""), None);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.change_setting(change, window, cx)));
        assert!(question(&workspace, cx).is_some());
        let file = dir.path().join("settings.toml");
        assert!(!file.exists(), "nothing is written before the answer");
        click(
            if allow {
                "approval-allow"
            } else {
                "approval-deny"
            },
            cx,
        );
        if allow {
            wait_until(cx, "the reload", |cx| {
                workspace.read_with(cx, |w, _| w.applied == 1)
            });
        }
        // Past the debounce: a second batch would show up as a second apply.
        for _ in 0..20 {
            std::thread::sleep(Duration::from_millis(20));
            cx.executor().advance_clock(Duration::from_millis(20));
            cx.run_until_parked();
        }
        assert_eq!(question(&workspace, cx), None);
        assert_eq!(file.exists(), allow);
        workspace.read_with(cx, |w, _| assert_eq!(w.applied, usize::from(allow)));
    }

    #[gpui_kit::test]
    fn allowed_screen_edit_is_written_once(cx: &mut TestAppContext) {
        let (command, _) = fake_file("--reply other");
        let agent = "fake".into();
        screen_edit_asks_first(Change::AgentCommand { agent, command }, true, cx);
    }

    #[gpui_kit::test]
    fn declined_screen_edit_writes_nothing(cx: &mut TestAppContext) {
        screen_edit_asks_first(Change::Shell(Some("/bin/other".into())), false, cx);
    }

    #[gpui_kit::test]
    fn hand_edited_command_asks_before_running(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = live(cx, fake_settings(""), None);
        let old = fake_settings("").agents["fake"].command.clone();
        let (new, file) = fake_file("--reply other");
        std::fs::write(dir.path().join("settings.toml"), file).unwrap();
        wait_until(cx, "the question", |cx| question(&workspace, cx).is_some());
        assert_eq!(
            question(&workspace, cx).unwrap(),
            format!("fake: {old} → {new}")
        );
        assert_eq!(reply(&workspace, "fake", cx), "Hello from fake agent");
    }

    #[gpui_kit::test]
    fn declined_command_never_runs(cx: &mut TestAppContext, relaunch: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        // From before approvals: the first launch approves today's command.
        AppState::default().save(&state).unwrap();
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(state.clone()));
        launch(&workspace, cx);
        let (new, file) = fake_file("--reply other");
        edit_user_file(&workspace, dir.path(), &file, cx);
        assert!(question(&workspace, cx).is_some());
        click("approval-deny", cx);
        assert_eq!(question(&workspace, cx), None);
        let file_text = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert_eq!(file_text, file, "nothing is written");
        let old = fake_settings("").agents["fake"].command.clone();
        workspace.read_with(cx, |w, cx| {
            assert_eq!(w.registry.read(cx).command("fake"), Some(old.as_str()))
        });
        assert!(!AppState::load(&state).approved.unwrap().contains(&new));

        relaunch.executor().allow_parking();
        let (settings, _) = Settings::load(dir.path(), None).unwrap();
        let (workspace, cx) = open_at(relaunch, settings, Some(state.clone()));
        launch(&workspace, cx);
        assert_eq!(question(&workspace, cx), Some(format!("fake (new): {new}")));
        workspace.read_with(cx, |w, cx| {
            assert_eq!(w.registry.read(cx).command("fake"), None);
            assert_eq!(w.registry.read(cx).status("fake"), None, "never started");
        });
        assert!(!AppState::load(&state).approved.unwrap().contains(&new));
    }

    #[gpui_kit::test]
    fn allowed_command_is_remembered(cx: &mut TestAppContext, relaunch: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        // From before approvals: the first launch approves today's command.
        AppState::default().save(&state).unwrap();
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(state.clone()));
        launch(&workspace, cx);
        let (new, file) = fake_file("--reply other");
        edit_user_file(&workspace, dir.path(), &file, cx);
        click("approval-allow", cx);
        assert_eq!(question(&workspace, cx), None);
        workspace.read_with(cx, |w, cx| {
            assert_eq!(w.registry.read(cx).command("fake"), Some(new.as_str()))
        });
        assert!(AppState::load(&state).approved.unwrap().contains(&new));

        relaunch.executor().allow_parking();
        let (settings, _) = Settings::load(dir.path(), None).unwrap();
        let (workspace, cx) = open_at(relaunch, settings, Some(state));
        launch(&workspace, cx);
        assert_eq!(question(&workspace, cx), None);
        assert_eq!(reply(&workspace, "fake", cx), "other");
    }

    /// A launch whose `state.json` holds `text` (`None`: there is none) approves only the
    /// built-ins: it asks for the user's own command.
    fn asks_without_a_state(text: Option<&str>, cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        if let Some(text) = text {
            std::fs::write(&state, text).unwrap();
        }
        let settings = fake_settings("--reply mine");
        let command = settings.agents["fake"].command.clone();
        let (workspace, cx) = open_at(cx, settings, Some(state.clone()));
        launch(&workspace, cx);
        assert_eq!(
            question(&workspace, cx),
            Some(format!("fake (new): {command}"))
        );
        workspace.read_with(cx, |w, cx| {
            assert!(!w.approved.contains(&command));
            assert_eq!(w.registry.read(cx).command("fake"), None);
        });
        assert!(!AppState::load(&state).approved.unwrap().contains(&command));
    }

    #[gpui_kit::test]
    fn missing_state_approves_nothing(cx: &mut TestAppContext) {
        asks_without_a_state(None, cx);
    }

    #[gpui_kit::test]
    fn corrupt_state_approves_nothing(cx: &mut TestAppContext) {
        asks_without_a_state(Some("{"), cx);
    }

    #[gpui_kit::test]
    fn existing_setup_is_seeded(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("state.json");
        AppState::default().save(&state).unwrap();
        let settings = fake_settings("--reply mine");
        let command = settings.agents["fake"].command.clone();
        let (workspace, cx) = open_at(cx, settings, Some(state.clone()));
        launch(&workspace, cx);
        assert_eq!(question(&workspace, cx), None);
        workspace.read_with(cx, |w, _| assert!(w.approved.contains(&command)));
        assert!(AppState::load(&state).approved.unwrap().contains(&command));
    }

    #[gpui_kit::test]
    fn unapproved_shell_is_not_used(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, Settings::default());
        let before = workspace.read_with(cx, |w, _| w.settings.terminal.shell.clone().unwrap());
        let dir = tempfile::tempdir().unwrap();
        let file = "[terminal]\nshell = \"/bin/other\"\n";
        edit_user_file(&workspace, dir.path(), file, cx);
        assert_eq!(
            question(&workspace, cx),
            Some(format!("Terminal shell: {before} → /bin/other"))
        );
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.toggle_terminal(&ToggleTerminal, window, cx));
        });
        let shell = workspace.read_with(cx, |w, cx| {
            w.terminal.as_ref().unwrap().read(cx).shell.clone()
        });
        assert_eq!(shell, Some(before));
    }

    /// The workspace follows the focused session's folder, as Open folder does: moving to a
    /// session elsewhere switches the project and closes the editor pane; the focus stays put.
    #[gpui_kit::test]
    fn workspace_follows_the_focused_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        let rows = load_history(&workspace, 2, cx);
        for (row, side) in rows.iter().zip([false, true]) {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, side, window, cx)));
        }
        let project =
            |cx: &mut VisualTestContext| workspace.read_with(cx, |w, _| w.project.clone());
        let here = project(cx);
        let there = canonical(&tmp_project("follow"));
        let elsewhere = pane_session(&workspace, 0, cx);
        elsewhere.update(cx, |s, _| s.cwd = there.clone());
        let file = there.join("a.txt");
        std::fs::write(&file, "a").unwrap();
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_file(file, None, window, cx)));
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[2], Pane::Editor(_)));
            assert_eq!(w.focused_pane, 2);
        });

        // A session in this project changes nothing: set_project would close the editor pane.
        let pane = cx.debug_bounds("pane-1").expect("pane drawn");
        cx.simulate_click(pane.center(), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(project(cx), here);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.focused_pane, 1);
            assert!(
                matches!(w.panes[2], Pane::Editor(_)),
                "the editor pane stays"
            );
        });

        let pane = cx.debug_bounds("pane-0").expect("pane drawn");
        cx.simulate_click(pane.center(), Modifiers::none());
        cx.run_until_parked();
        assert_eq!(project(cx), there);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2, "the editor pane closed");
            assert_eq!(w.panes[w.focused_pane].session(), Some(&elsewhere));
        });

        cx.simulate_keystrokes("secondary-alt-right");
        assert_eq!(project(cx), here);

        // Stop the project's watcher before its folder goes (see templates_reload_on_project_switch).
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project("/nonexistent".into(), window, cx))
        });
        std::fs::remove_dir_all(there).ok();
    }

    /// Closing a pane follows the session that takes the focus, as the pane-hide branch intends.
    #[gpui_kit::test]
    fn closing_a_pane_follows_its_neighbour(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(2));
        let rows = load_history(&workspace, 2, cx);
        for (row, side) in rows.iter().zip([false, true]) {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(row, side, window, cx)));
        }
        let here = workspace.read_with(cx, |w, _| w.project.clone());
        let there = canonical(&tmp_project("close-follow"));
        let elsewhere = pane_session(&workspace, 0, cx);
        elsewhere.update(cx, |s, _| s.cwd = there.clone());
        workspace.read_with(cx, |w, _| assert_eq!(w.focused_pane, 1));

        cx.simulate_keystrokes("secondary-w");
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 1, "the pane went");
            assert_eq!(w.panes[0].session(), Some(&elsewhere));
            assert_eq!(w.project, there);
        });

        cx.update(|window, cx| workspace.update(cx, |w, cx| w.set_project(here, window, cx)));
        std::fs::remove_dir_all(there).ok();
    }

    /// Restore puts a session back in its pane without moving the workspace to its folder.
    #[gpui_kit::test]
    fn restore_never_follows_a_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let there = tmp_project("restore");
        let mut away = saved("fake", "abc", 0);
        away.cwd = there.clone();
        let state = AppState {
            open_sessions: vec![away],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path));

        launch(&workspace, cx);

        let session = pane_session(&workspace, 0, cx);
        wait_until(cx, "the replay", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::Idle)
        });
        workspace.read_with(cx, |w, cx| {
            assert_eq!(session.read(cx).cwd, there);
            assert_eq!(w.project, canonical(&std::env::temp_dir()));
        });
        std::fs::remove_dir_all(there).ok();
    }
}
