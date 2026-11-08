use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, h_resizable, resizable_panel, v_flex, v_resizable};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::{AgentError, SessionStarted, SessionSummary, StopReason};
use wai_settings::{AppState, OpenSession, Settings};

use crate::agents::{AgentRegistry, AgentStatus, RegistryEvent};
use crate::app::{CloseSession, FocusNextPane, NewSession, OpenFolder, ToggleTerminal};
use crate::chat::{ChatEvent, ChatSession};
use crate::notify::{self, should_notify};
use crate::sidebar::canonical;
use crate::terminal_view::TerminalView;

const SIDEBAR_WIDTH: f32 = 260.;
const TERMINAL_HEIGHT: f32 = 260.;
pub const MAX_PANES: usize = 4;
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
}

impl Pane {
    pub fn session(&self) -> Option<&Entity<ChatSession>> {
        match self {
            Pane::Session(session) => Some(session),
            _ => None,
        }
    }
}

/// The window's root view: sidebar | chat panes over the terminal panel.
pub struct Workspace {
    /// Canonical, so history cwds compare with it (R3).
    pub project: PathBuf,
    pub settings: Settings,
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
    next_ticket: u64,
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
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let registry = cx.new(|_| AgentRegistry::new(&settings));
        let routing = cx.subscribe(&registry, |this, _, event, cx| this.route(event, cx));
        let quit = cx.on_app_quit(|this, cx| {
            this.save_state(cx);
            async {}
        });
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        window.set_window_title(&title(&project));
        Self {
            project: canonical(&project),
            settings,
            registry,
            sessions: Vec::new(),
            panes: vec![Pane::Empty(None)],
            focused_pane: 0,
            terminal_visible: false,
            terminal: None,
            history: Vec::new(),
            all_projects: false,
            history_task: None,
            next_ticket: 0,
            state_path,
            focus,
            _subscriptions: vec![routing, quit],
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
            } => {
                if let Some(s) = self.session(agent, session, cx) {
                    s.update(cx, |s, cx| s.handle_event(event, cx));
                }
            }
            RegistryEvent::Permission { agent, request } => {
                match self.session(agent, &request.session, cx) {
                    Some(s) => s.update(cx, |s, cx| s.handle_permission(request.clone(), cx)),
                    // A dead agent's requests died with it (and `handle` would respawn it).
                    None if matches!(
                        self.registry.read(cx).status(agent),
                        Some(AgentStatus::Failed(_))
                    ) => {}
                    // Nobody can answer it: cancel it so the agent's turn goes on.
                    None => {
                        if let Some(handle) = self.registry.update(cx, |r, cx| r.handle(agent, cx))
                        {
                            handle.answer_permission(request.id, None).ok();
                        }
                    }
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
            // R7: carries no session id; sessions ignore ids they do not hold.
            RegistryEvent::PermissionResolved(id) => {
                for s in &self.sessions {
                    s.update(cx, |s, cx| s.handle_permission_resolved(*id, cx));
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

    /// Joins the open list; the sidebar follows its status and title. While the window is in the
    /// background, the session notifies when it needs the user or finishes a turn.
    fn add_session(
        &mut self,
        session: Entity<ChatSession>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.subscribe_in(&session, window, |this, session, event, window, cx| {
            // Its pane keeps the notice; the next launch does not try again.
            if matches!(event, ChatEvent::RestoreFailed) {
                this.sessions.retain(|s| s != session);
                this.save_state(cx);
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
    }

    /// Focuses pane `ix`: its prompt box, or the workspace when it has no session.
    fn focus_pane(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.focused_pane = ix;
        match &self.panes[ix] {
            Pane::Session(session) => session.update(cx, |s, cx| s.focus(window, cx)),
            _ => window.focus(&self.focus, cx),
        }
        cx.notify();
    }

    /// Shows `session` in the focused pane, or with `side` in a new pane while there are fewer
    /// than `MAX_PANES` (an empty focused pane is used instead). A session already in a pane is
    /// focused there: one session is drawn once.
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
                    self.focused_pane
                };
                self.panes[ix] = Pane::Session(session);
                ix
            }
        };
        self.focus_pane(ix, window, cx);
        self.save_state(cx);
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

    /// Starts a session in the focused pane, which shows `Preparing <agent>…` until the agent
    /// answers. A second ⌘N on that pane waits for the first.
    pub(crate) fn new_session(
        &mut self,
        _: &NewSession,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let ix = self.focused_pane;
        if matches!(self.panes[ix], Pane::Preparing { .. }) {
            return;
        }
        let agent = self.settings.default_agent.clone();
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
        cx.notify();
        let cwd = self.project.clone();
        let registry = self.registry.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let started = handle.new_session(cwd.clone()).await;
            this.update_in(cx, |this, window, cx| {
                // Closed or replaced meanwhile, the placeholder is gone: the session still opens.
                let pane = this
                    .panes
                    .iter()
                    .position(|p| matches!(p, Pane::Preparing { ticket: t, .. } if *t == ticket));
                let shown = match started {
                    Ok(started) => {
                        // A session that starts proves the agent works (signed in again, say).
                        this.registry.update(cx, |r, cx| r.mark_ready(&agent, cx));
                        let session = cx.new(|cx| {
                            ChatSession::new(agent, started, cwd, handle, registry, window, cx)
                        });
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
    /// its history). With more than one pane, the pane goes too.
    fn close_session(&mut self, _: &CloseSession, window: &mut Window, cx: &mut Context<Self>) {
        let ix = self.focused_pane;
        if let Pane::Session(session) = &self.panes[ix] {
            session.read(cx).stop();
            self.sessions.retain(|s| s != session);
        }
        if self.panes.len() > 1 {
            self.panes.remove(ix);
        } else {
            self.panes[ix] = Pane::Empty(None);
        }
        self.focus_pane(ix.min(self.panes.len() - 1), window, cx);
        self.save_state(cx);
    }

    fn focus_next_pane(&mut self, _: &FocusNextPane, window: &mut Window, cx: &mut Context<Self>) {
        self.focus_pane((self.focused_pane + 1) % self.panes.len(), window, cx);
    }

    pub fn set_project(&mut self, project: PathBuf, window: &mut Window, cx: &mut Context<Self>) {
        window.set_window_title(&title(&project));
        self.project = canonical(&project);
        // The next shell starts there; a running one stays where it is.
        if let Some(terminal) = &self.terminal {
            terminal.update(cx, |t, _| t.cwd = self.project.clone());
        }
        self.save_state(cx);
        cx.notify();
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
            this.update_in(cx, |this, window, cx| this.set_project(project, window, cx))
                .ok();
        })
        .detach();
    }

    /// What an empty pane says is wrong with the default agent, if anything.
    fn agent_problem(&self, cx: &App) -> Option<String> {
        let agent = &self.settings.default_agent;
        match self.registry.read(cx).status(agent)? {
            AgentStatus::SignedOut => Some(format!(
                "{agent} is not signed in. Open a terminal and run its login (for Claude: claude /login)."
            )),
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
                    .size_full()
                    .when(split, |el| {
                        el.border_t_2().border_color(if focused {
                            theme.blue
                        } else {
                            transparent_black()
                        })
                    })
                    // Clicking into a pane focuses it, whatever the click lands on.
                    .capture_any_mouse_down(cx.listener(move |this, _, _, cx| {
                        if this.focused_pane != ix {
                            this.focused_pane = ix;
                            cx.notify();
                        }
                    }))
                    .child(match pane {
                        Pane::Session(session) => session.clone().into_any_element(),
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

#[cfg(test)]
pub(crate) mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};
    use wai_agent::{PromptBlock, SessionSummary};
    use wai_settings::{AgentConfig, AppState, OpenSession, Settings};

    use super::{Pane, Workspace};
    use crate::agents::AgentStatus;
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
        cx.add_window_view(|window, cx| Workspace::new(project, settings, state_path, window, cx))
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

    /// What `app::run` does once the window is open.
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
