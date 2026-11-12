use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{IconName, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;

use crate::aurora;
use crate::app::{CloseTerminal, NewTerminal, NextTerminal, PrevTerminal, ToggleTerminal};
use crate::terminal_view::{TerminalChanged, TerminalView};
use crate::workspace::Workspace;

pub(crate) const MAX_TERMINALS: usize = 8;

/// One shell in the bottom panel; `number` is its label and is never reused.
pub(crate) struct TerminalTab {
    pub(crate) view: Entity<TerminalView>,
    pub(crate) number: u32,
    _changed: Subscription,
}

/// The tab's text: the shell's title without control characters, cut to 24 characters, else
/// "Terminal N".
fn tab_label(number: u32, title: Option<&str>) -> String {
    let title: String = title.unwrap_or_default().chars().filter(|c| !c.is_control()).collect();
    let title = title.trim();
    match title.chars().count() {
        0 => format!("Terminal {number}"),
        1..=24 => title.to_string(),
        _ => title.chars().take(23).chain(['…']).collect(),
    }
}

impl Workspace {
    /// The active terminal.
    pub(crate) fn terminal(&self) -> Option<&Entity<TerminalView>> {
        self.terminals.get(self.active_terminal).map(|t| &t.view)
    }

    /// The panel's content: the tab strip over the active terminal.
    pub(crate) fn render_terminal_panel(
        &self,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let tabs = self.terminals.iter().enumerate().map(|(ix, tab)| {
            let label = tab_label(tab.number, tab.view.read(cx).title());
            aurora::tab(ix == self.active_terminal, cx)
                .id(("terminal-tab", ix))
                .role(Role::Tab)
                .aria_label(label.clone())
                .h(px(24.))
                .pl_2()
                .pr_1()
                .gap_1()
                .rounded(px(8.))
                .text_xs()
                .child(div().whitespace_nowrap().child(label))
                .child(
                    Button::new(("terminal-tab-close", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close_terminal(ix, window, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.select_terminal(ix, window, cx)))
                // Registers the id so tests can find and click it (a Button does it itself).
                .test_support()
        });
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .h(px(28.))
                    .flex_none()
                    .gap_1()
                    .overflow_x_hidden()
                    .children(tabs)
                    .child(
                        Button::new("new-terminal")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.open_terminal(window, cx);
                            })),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .children(self.terminal().cloned()),
            )
    }

    /// Shows the terminal panel, with a first tab created in the project folder on the first
    /// toggle (a shell that exited starts again), and focuses it. Hiding it hands the focus back
    /// to the focused pane.
    pub(crate) fn toggle_terminal(
        &mut self,
        _: &ToggleTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.terminal_visible {
            self.terminal_visible = false;
            self.save_state(cx);
            return self.focus_pane(self.focused_pane, window, cx);
        }
        if self.terminals.is_empty() {
            self.open_terminal(window, cx);
            return;
        }
        self.terminal_visible = true;
        self.save_state(cx);
        if let Some(terminal) = self.terminal() {
            terminal.update(cx, |t, cx| t.activate(window, cx));
        }
        cx.notify();
    }

    pub(crate) fn new_terminal(
        &mut self,
        _: &NewTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.open_terminal(window, cx);
    }

    pub(crate) fn close_terminal_action(
        &mut self,
        _: &CloseTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.close_terminal(self.active_terminal, window, cx);
    }

    pub(crate) fn next_terminal_action(
        &mut self,
        _: &NextTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.terminals.len();
        if len > 0 {
            self.select_terminal((self.active_terminal + 1) % len, window, cx);
        }
    }

    pub(crate) fn prev_terminal_action(
        &mut self,
        _: &PrevTerminal,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let len = self.terminals.len();
        if len > 0 {
            self.select_terminal((self.active_terminal + len - 1) % len, window, cx);
        }
    }

    /// Shows the panel and adds a terminal tab in the project folder, active and focused.
    pub(crate) fn open_terminal(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Entity<TerminalView>> {
        if self.terminals.len() >= MAX_TERMINALS {
            let notice = format!("At most {MAX_TERMINALS} terminals");
            window.push_notification(Notification::warning(notice), cx);
            return None;
        }
        let (shell, cwd) = (self.shell.clone(), self.project.clone());
        let view = cx.new(|cx| TerminalView::new(shell, cwd, cx));
        let changed = cx.subscribe(&view, |_, _, _: &TerminalChanged, cx| cx.notify());
        self.terminals.push(TerminalTab {
            view: view.clone(),
            number: self.next_terminal,
            _changed: changed,
        });
        self.next_terminal += 1;
        self.active_terminal = self.terminals.len() - 1;
        self.terminal_visible = true;
        self.save_state(cx);
        view.update(cx, |t, cx| t.activate(window, cx));
        cx.notify();
        Some(view)
    }

    pub(crate) fn select_terminal(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.terminals.len() {
            return;
        }
        self.active_terminal = ix;
        self.terminals[ix]
            .view
            .update(cx, |t, cx| t.activate(window, cx));
        cx.notify();
    }

    /// Closes tab `ix`, after asking when a command is still running in it.
    pub(crate) fn close_terminal(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.terminals.get(ix) else {
            return;
        };
        if !tab.view.read(cx).busy() {
            return self.remove_terminal(ix, window, cx);
        }
        let number = tab.number;
        self.confirm(
            "A command is still running".into(),
            "Closing this terminal ends it. Close it anyway?",
            "Close terminal",
            move |this, window, cx| {
                // The tabs may have shifted while the dialog was open.
                if let Some(ix) = this.terminals.iter().position(|t| t.number == number) {
                    this.remove_terminal(ix, window, cx);
                }
            },
            window,
            cx,
        );
    }

    /// Removes tab `ix`, which stops its shell. The last one takes the panel with it.
    fn remove_terminal(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.terminals.remove(ix);
        if self.terminals.is_empty() {
            self.terminal_visible = false;
            self.active_terminal = 0;
            self.save_state(cx);
            self.focus_pane(self.focused_pane, window, cx);
        } else {
            if ix < self.active_terminal || self.active_terminal == self.terminals.len() {
                self.active_terminal -= 1;
            }
            self.save_state(cx);
            self.select_terminal(self.active_terminal, window, cx);
        }
        cx.notify();
    }

    /// Shows a fresh terminal and types `text` at its prompt without Enter: the user reads it and
    /// presses Enter (an extension's suggested Claude Code plugin, a session's resume command).
    pub(crate) fn type_in_terminal(
        &mut self,
        text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let Some(terminal) = self.open_terminal(window, cx) {
            terminal.update(cx, |t, cx| t.type_text(text, cx));
        }
    }
}

#[cfg(all(test, unix))]
mod tests {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use super::MAX_TERMINALS;
    use crate::app::NewTerminal;
    use crate::terminal_view::TerminalView;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{click, open, wait_until};

    fn numbers(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<u32> {
        workspace.read_with(cx, |w, _| w.terminals.iter().map(|t| t.number).collect())
    }

    fn shows(terminal: &Entity<TerminalView>, text: &str, cx: &mut VisualTestContext) -> bool {
        terminal.read_with(cx, |t, _| {
            t.snapshot.rows.iter().any(|row| {
                let line: String = row.iter().map(|run| run.text.as_str()).collect();
                line.contains(text)
            })
        })
    }

    #[gpui_kit::test]
    fn new_terminal_adds_a_tab_and_shows_the_panel(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        cx.simulate_keystrokes("secondary-t");
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.terminals.len(), 2);
            assert_eq!(w.active_terminal, 1);
            assert!(w.terminal_visible);
        });
        assert_eq!(numbers(&workspace, cx), [1, 2]);
    }

    #[gpui_kit::test]
    fn type_in_terminal_opens_a_fresh_tab(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.type_in_terminal("printf wai-%s typed", window, cx)
            })
        });
        let (first, second) = workspace.read_with(cx, |w, _| {
            assert_eq!(w.terminals.len(), 2);
            (w.terminals[0].view.clone(), w.terminals[1].view.clone())
        });
        wait_until(cx, "the typed text", |cx| {
            shows(&second, "printf wai-%s typed", cx)
        });
        assert!(!shows(&first, "printf wai-%s typed", cx));
    }

    #[gpui_kit::test]
    fn closing_the_last_terminal_hides_the_panel(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_terminal(0, window, cx)));
        workspace.read_with(cx, |w, _| {
            assert!(w.terminals.is_empty());
            assert!(!w.terminal_visible);
        });
        cx.simulate_keystrokes("ctrl-`");
        assert_eq!(numbers(&workspace, cx), [2]);
    }

    #[gpui_kit::test]
    fn closing_the_active_tab_selects_its_neighbour(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        cx.simulate_keystrokes("secondary-t");
        cx.simulate_keystrokes("secondary-t");
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.select_terminal(1, window, cx)));
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_terminal(1, window, cx)));
        assert_eq!(workspace.read_with(cx, |w, _| w.active_terminal), 1);
        assert_eq!(numbers(&workspace, cx), [1, 3]);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_terminal(1, window, cx)));
        assert_eq!(workspace.read_with(cx, |w, _| w.active_terminal), 0);
    }

    #[gpui_kit::test]
    fn terminal_shortcuts_cycle_and_close(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        cx.dispatch_action(NewTerminal);
        cx.dispatch_action(NewTerminal);
        let active = |cx: &mut VisualTestContext| workspace.read_with(cx, |w, _| w.active_terminal);
        assert_eq!(active(cx), 2);
        cx.simulate_keystrokes("secondary-}");
        assert_eq!(active(cx), 0);
        cx.simulate_keystrokes("secondary-{");
        assert_eq!(active(cx), 2);
        let panes = workspace.read_with(cx, |w, _| w.panes.len());
        cx.simulate_keystrokes("secondary-w");
        assert_eq!(numbers(&workspace, cx), [1, 2]);
        assert_eq!(workspace.read_with(cx, |w, _| w.panes.len()), panes);
    }

    #[gpui_kit::test]
    fn closing_an_inactive_tab_keeps_the_active_one(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        cx.dispatch_action(NewTerminal);
        cx.dispatch_action(NewTerminal);
        let current = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| w.terminals[w.active_terminal].number)
        };
        assert_eq!(current(cx), 3);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_terminal(0, window, cx)));
        assert_eq!(current(cx), 3);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.select_terminal(0, window, cx)));
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_terminal(1, window, cx)));
        assert_eq!(current(cx), 2);
    }

    #[gpui_kit::test]
    fn ninth_terminal_is_refused(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        for _ in 1..MAX_TERMINALS {
            cx.dispatch_action(NewTerminal);
        }
        assert_eq!(workspace.read_with(cx, |w, _| w.terminals.len()), MAX_TERMINALS);
        cx.dispatch_action(NewTerminal);
        assert_eq!(workspace.read_with(cx, |w, _| w.terminals.len()), MAX_TERMINALS);
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);
    }

    #[gpui_kit::test]
    fn terminal_tab_numbers_are_never_reused(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        cx.simulate_keystrokes("secondary-t");
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_terminal(0, window, cx)));
        cx.simulate_keystrokes("secondary-t");
        assert_eq!(numbers(&workspace, cx), [2, 3]);
    }

    #[test]
    fn tab_label_cleans_the_title() {
        assert_eq!(super::tab_label(1, None), "Terminal 1");
        assert_eq!(super::tab_label(2, Some("  \u{7}build ")), "build");
        let long = "x".repeat(40);
        assert_eq!(super::tab_label(1, Some(&long)), format!("{}…", "x".repeat(23)));
        assert_eq!(super::tab_label(3, Some("   ")), "Terminal 3");
    }

    #[gpui_kit::test]
    fn strip_selects_adds_and_closes(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        click("new-terminal", cx);
        cx.update(|window, _| {
            window.find(("terminal-tab", 0usize));
            window.find(("terminal-tab", 1usize));
        });
        assert_eq!(workspace.read_with(cx, |w, _| w.active_terminal), 1);
        click(("terminal-tab", 0usize), cx);
        assert_eq!(workspace.read_with(cx, |w, _| w.active_terminal), 0);
        click(("terminal-tab-close", 0usize), cx);
        assert_eq!(numbers(&workspace, cx), [2]);
    }

    fn asked(cx: &mut VisualTestContext) -> Option<String> {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find("confirm-heading").and_then(|t| t.label().map(str::to_owned))
        })
    }

    #[gpui_kit::test]
    fn closing_a_tab_with_a_running_job_asks(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal().cloned().expect("first toggle"));
        cx.simulate_input("sleep 30");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the job", |cx| terminal.read_with(cx, |t, _| t.busy()));
        click(("terminal-tab-close", 0usize), cx);
        assert_eq!(asked(cx).as_deref(), Some("A command is still running"));
        assert_eq!(numbers(&workspace, cx), [1]);
        click("cancel", cx);
        assert_eq!(numbers(&workspace, cx), [1]);
        click(("terminal-tab-close", 0usize), cx);
        click("confirm", cx);
        assert!(numbers(&workspace, cx).is_empty());
    }

    #[gpui_kit::test]
    fn exited_tab_closes_without_asking(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal().cloned().expect("first toggle"));
        cx.simulate_input("exit");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the exit", |cx| terminal.read_with(cx, |t, _| !t.has_shell()));
        click(("terminal-tab-close", 0usize), cx);
        assert_eq!(asked(cx), None);
        assert!(numbers(&workspace, cx).is_empty());
    }

    #[gpui_kit::test]
    fn tab_label_follows_the_title(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal().cloned().expect("first toggle"));
        cx.simulate_input(r"printf '\033]0;hello\007'");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the title", |cx| {
            terminal.read_with(cx, |t, _| t.title() == Some("hello"))
        });
        let label = cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(("terminal-tab", 0usize)).label().map(str::to_owned)
        });
        assert_eq!(label.as_deref(), Some("hello"));
    }
}
