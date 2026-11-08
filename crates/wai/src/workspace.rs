use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, h_resizable, resizable_panel, v_flex, v_resizable};
use gpui_kit::*;
use wai_agent::AgentError;
use wai_settings::Settings;

use crate::agents::{AgentRegistry, AgentStatus, RegistryEvent};
use crate::app::{CloseSession, FocusNextPane, NewSession, OpenFolder, ToggleTerminal};
use crate::chat::ChatSession;

const SIDEBAR_WIDTH: f32 = 260.;
const TERMINAL_HEIGHT: f32 = 260.;
const START_HINT: &str = if cfg!(target_os = "macos") {
    "Start a session (⌘N)"
} else {
    "Start a session (Ctrl+N)"
};

/// The window's root view: sidebar | chat panes over the terminal panel.
pub struct Workspace {
    pub project: PathBuf,
    pub settings: Settings,
    pub registry: Entity<AgentRegistry>,
    pub panes: Vec<Option<Entity<ChatSession>>>,
    pub focused_pane: usize,
    pub terminal_visible: bool,
    focus: FocusHandle,
    _subscriptions: Vec<Subscription>,
}

impl Workspace {
    pub fn new(
        project: PathBuf,
        settings: Settings,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let registry = cx.new(|_| AgentRegistry::new(&settings));
        let routing = cx.subscribe(&registry, |this, _, event, cx| this.route(event, cx));
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        window.set_window_title(&title(&project));
        Self {
            project,
            settings,
            registry,
            panes: vec![None],
            focused_pane: 0,
            terminal_visible: false,
            focus,
            _subscriptions: vec![routing],
        }
    }

    fn session(&self, id: &str, cx: &App) -> Option<Entity<ChatSession>> {
        self.panes
            .iter()
            .flatten()
            .find(|session| session.read(cx).id == id)
            .cloned()
    }

    fn route(&mut self, event: &RegistryEvent, cx: &mut Context<Self>) {
        match event {
            RegistryEvent::Session { session, event, .. } => {
                if let Some(s) = self.session(session, cx) {
                    s.update(cx, |s, cx| s.handle_event(event, cx));
                }
            }
            RegistryEvent::Permission(request) => {
                if let Some(s) = self.session(&request.session, cx) {
                    s.update(cx, |s, cx| s.handle_permission(request.clone(), cx));
                }
            }
            RegistryEvent::TurnEnded {
                session, reason, ..
            } => {
                if let Some(s) = self.session(session, cx) {
                    s.update(cx, |s, cx| s.handle_turn_ended(reason, cx));
                }
            }
            // ponytail: carries no session id; Task 1.6 decides which sessions it reaches.
            RegistryEvent::PermissionResolved(_) => {}
            // Empty panes show the default agent's problem, if any.
            RegistryEvent::StatusChanged(_) => cx.notify(),
        }
    }

    fn new_session(&mut self, _: &NewSession, window: &mut Window, cx: &mut Context<Self>) {
        let agent = self.settings.default_agent.clone();
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            return eprintln!("wai: no agent named {agent} in the settings");
        };
        let (cwd, pane) = (self.project.clone(), self.focused_pane);
        cx.spawn_in(window, async move |this, cx| {
            let started = handle.new_session(cwd.clone()).await;
            this.update(cx, |this, cx| match started {
                Ok(started) => {
                    // A session that starts proves the agent works (signed in again, say).
                    this.registry.update(cx, |r, cx| r.mark_ready(&agent, cx));
                    let session = cx.new(|_| ChatSession {
                        agent,
                        id: started.id,
                        cwd,
                    });
                    let pane = pane.min(this.panes.len() - 1);
                    this.panes[pane] = Some(session);
                    cx.notify();
                }
                Err(AgentError::AuthRequired) => this
                    .registry
                    .update(cx, |r, cx| r.set_status(&agent, AgentStatus::SignedOut, cx)),
                // ponytail: stderr only until Task 1.7's "Preparing…" placeholder can show it.
                Err(e) => eprintln!("wai: could not start a {agent} session: {e}"),
            })
            .ok();
        })
        .detach();
    }

    fn toggle_terminal(&mut self, _: &ToggleTerminal, _: &mut Window, cx: &mut Context<Self>) {
        self.terminal_visible = !self.terminal_visible;
        cx.notify();
    }

    fn close_session(&mut self, _: &CloseSession, _: &mut Window, cx: &mut Context<Self>) {
        self.panes[self.focused_pane] = None;
        cx.notify();
    }

    fn focus_next_pane(&mut self, _: &FocusNextPane, _: &mut Window, cx: &mut Context<Self>) {
        self.focused_pane = (self.focused_pane + 1) % self.panes.len();
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
            this.update_in(cx, |this, window, cx| {
                window.set_window_title(&title(&project));
                this.project = project;
                cx.notify();
            })
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

    fn empty_pane(&self, ix: usize, cx: &Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let problem = self.agent_problem(cx);
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

fn folder_name(project: &Path) -> String {
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
        let theme = cx.theme();
        let panes = self.panes.iter().enumerate().map(|(ix, pane)| {
            resizable_panel().child(match pane {
                Some(session) => session.clone().into_any_element(),
                None => self.empty_pane(ix, cx).into_any_element(),
            })
        });
        // ponytail: sidebar and terminal are placeholders until Tasks 1.7 and 1.8.
        let sidebar = v_flex()
            .debug_selector(|| "sidebar".into())
            .size_full()
            .p_3()
            .gap_2()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(folder_name(&self.project)),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child("Sessions"),
            );
        let terminal = div()
            .debug_selector(|| "terminal".into())
            .size_full()
            .p_3()
            .text_color(theme.muted_foreground)
            .child("Terminal");

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
                                .child(
                                    resizable_panel().child(h_resizable("panes").children(panes)),
                                )
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
mod tests {
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};
    use wai_agent::PromptBlock;
    use wai_settings::{AgentConfig, Settings};

    use super::Workspace;
    use crate::agents::AgentStatus;
    use crate::app;

    fn open(
        cx: &mut TestAppContext,
        settings: Settings,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        cx.update(|cx| app::init(&settings, cx));
        let project = std::env::temp_dir();
        cx.add_window_view(|window, cx| Workspace::new(project, settings, window, cx))
    }

    /// Settings whose default agent `fake` is `wai-fake-agent <flag>`.
    fn fake_settings(flag: &str) -> Settings {
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
    fn wait_until(
        cx: &mut VisualTestContext,
        what: &str,
        mut done: impl FnMut(&mut VisualTestContext) -> bool,
    ) {
        let deadline = Instant::now() + Duration::from_secs(10);
        while !done(cx) {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
            cx.run_until_parked();
        }
    }

    #[gpui_kit::test]
    fn opens_with_one_empty_pane(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 1);
            assert!(w.panes[0].is_none());
        });
    }

    #[gpui_kit::test]
    fn toggle_terminal_action_flips_visibility(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        assert!(workspace.read_with(cx, |w, _| w.terminal_visible));
        cx.simulate_keystrokes("ctrl-`");
        assert!(!workspace.read_with(cx, |w, _| w.terminal_visible));
    }

    #[gpui_kit::test]
    fn sidebar_and_terminal_keep_260_px(cx: &mut TestAppContext) {
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
            workspace.read_with(cx, |w, _| w.panes[0].is_some())
        });
        let session =
            workspace.read_with(cx, |w, cx| w.panes[0].as_ref().unwrap().read(cx).id.clone());
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
}
