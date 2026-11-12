use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;

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

impl Workspace {
    /// The active terminal.
    pub(crate) fn terminal(&self) -> Option<&Entity<TerminalView>> {
        self.terminals.get(self.active_terminal).map(|t| &t.view)
    }

    /// The panel's content: the active terminal.
    pub(crate) fn render_terminal_panel(
        &self,
        _: &mut Window,
        _: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .size_full()
            .children(self.terminal().cloned())
            .into_any_element()
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

    /// Removes tab `ix`, which stops its shell. The last one takes the panel with it.
    pub(crate) fn close_terminal(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if ix >= self.terminals.len() {
            return;
        }
        self.terminals.remove(ix);
        if self.terminals.is_empty() {
            self.terminal_visible = false;
            self.active_terminal = 0;
            self.save_state(cx);
            self.focus_pane(self.focused_pane, window, cx);
        } else {
            self.active_terminal = ix.min(self.terminals.len() - 1);
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
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use super::MAX_TERMINALS;
    use crate::app::NewTerminal;
    use crate::terminal_view::TerminalView;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{open, wait_until};

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
}
