//! The `/` menu: the agent's commands (Claude Code's built-ins, custom commands and skills),
//! listed above the prompt box while its text is `/` and the start of a name.

use gpui_kit::component::input::{Enter, Escape, IndentInline, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::CommandInfo;

use super::view::ChatSession;

/// A row's height: the list shows 8 rows and scrolls past them.
const ROW: f32 = 28.;

/// The name being typed: what follows a leading `/`, until a space follows it.
fn query(text: &str) -> Option<&str> {
    let name = text.strip_prefix('/')?;
    (!name.contains(char::is_whitespace)).then_some(name)
}

/// The commands for `query`, case-insensitive: names that start with it first, then names or
/// descriptions that contain it. ponytail: runs each frame the menu is open; fine for the tens
/// of commands an agent sends, cache it per text if that grows to thousands.
fn matching<'a>(commands: &'a [CommandInfo], query: &str) -> Vec<&'a CommandInfo> {
    let query = query.to_lowercase();
    let (mut starts, mut contains) = (Vec::new(), Vec::new());
    for command in commands {
        let name = command.name.to_lowercase();
        if name.starts_with(&query) {
            starts.push(command);
        } else if name.contains(&query) || command.description.to_lowercase().contains(&query) {
            contains.push(command);
        }
    }
    starts.extend(contains);
    starts
}

impl ChatSession {
    /// The `/` menu's rows; `None` while it is closed. Without the agent's commands yet it is
    /// open and empty; with commands and none matching, closed.
    pub(super) fn menu(&self, cx: &App) -> Option<Vec<&CommandInfo>> {
        let text = self.input.read(cx).value();
        let query = query(&text).filter(|_| !self.menu_closed)?;
        let rows = matching(&self.model.commands, query);
        (!rows.is_empty() || self.model.commands.is_empty()).then_some(rows)
    }

    /// While the menu is open, ↑/↓, Enter, Tab and Esc are its own; else the prompt box's.
    pub(super) fn menu_keys(&self, el: Div, cx: &Context<Self>) -> Div {
        el.capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_selection(-1, cx)))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_selection(1, cx)))
            .capture_action(cx.listener(|this, enter: &Enter, window, cx| {
                if !enter.shift && !enter.secondary {
                    this.complete(None, window, cx);
                }
            }))
            .capture_action(
                cx.listener(|this, _: &IndentInline, window, cx| this.complete(None, window, cx)),
            )
            .capture_action(cx.listener(|this, _: &Escape, _, cx| this.close_menu(cx)))
    }

    /// Moves the selection, round the ends.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        let Some(n) = self.menu(cx).map(|rows| rows.len()).filter(|n| *n > 0) else {
            return;
        };
        let ix = self.menu_ix.min(n - 1) as isize + by;
        self.menu_ix = ix.rem_euclid(n as isize) as usize;
        self.menu_scroll.scroll_to_item(self.menu_ix);
        cx.stop_propagation();
        cx.notify();
    }

    /// Puts `/<name> ` of row `ix` (`None`: the selected one) in the box, caret at its end.
    fn complete(&mut self, ix: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let rows = self.menu(cx).unwrap_or_default();
        let ix = ix.unwrap_or(self.menu_ix).min(rows.len().saturating_sub(1));
        let Some(text) = rows.get(ix).map(|c| format!("/{} ", c.name)) else {
            return;
        };
        self.input.update(cx, |input, cx| {
            input.set_value("", window, cx);
            input.insert(text, window, cx);
            input.focus(window, cx);
        });
        cx.stop_propagation();
        cx.notify();
    }

    /// Closes the menu, leaving the text; typing opens it again.
    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.menu(cx).is_none() {
            return;
        }
        self.menu_closed = true;
        cx.stop_propagation();
        cx.notify();
    }

    /// The open menu, laid over the chat just above the prompt box.
    pub(super) fn render_menu(&self, cx: &Context<Self>) -> Option<impl IntoElement + use<>> {
        let rows = self.menu(cx)?;
        let theme = cx.theme();
        let list = v_flex()
            .id("commands")
            .absolute()
            .bottom_full()
            .left_0()
            .right_0()
            .mb_2()
            .py_1()
            .max_h(px(8. * ROW))
            .overflow_y_scroll()
            .track_scroll(&self.menu_scroll)
            .occlude()
            .rounded(px(12.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_size(px(13.))
            .text_color(theme.popover_foreground)
            // Registers the id so tests can find it.
            .test_support();
        if rows.is_empty() {
            let text = format!(
                "No commands from {} yet",
                wai_settings::agent_label(&self.agent)
            );
            let row = h_flex()
                .id("commands-empty")
                .h(px(ROW))
                .px_3()
                .text_color(theme.muted_foreground)
                .aria_label(text.clone())
                .child(text)
                // Registers the id so tests can find it.
                .test_support();
            return Some(list.child(row));
        }
        let selected = self.menu_ix.min(rows.len() - 1);
        let rows = rows.into_iter().enumerate().map(|(ix, command)| {
            let hint = command.hint.clone().map(|hint| {
                div()
                    .id(("command-hint", ix))
                    .flex_none()
                    .text_color(theme.muted_foreground)
                    .child(hint)
                    .test_support()
            });
            h_flex()
                .id(("command", ix))
                .flex_none()
                .h(px(ROW))
                .px_3()
                .gap_2()
                .cursor_pointer()
                .when(ix == selected, |el| {
                    el.bg(theme.accent).text_color(theme.accent_foreground)
                })
                .child(div().flex_none().child(format!("/{}", command.name)))
                .children(hint)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(command.description.clone()),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.complete(Some(ix), window, cx)),
                )
                // Registers the id so tests can find and click it.
                .test_support()
        });
        Some(list.children(rows))
    }
}

#[cfg(test)]
mod tests {
    use wai_agent::CommandInfo;

    use super::{matching, query};

    #[test]
    fn query_is_the_name_being_typed() {
        assert_eq!(query("/"), Some(""));
        assert_eq!(query("/rev"), Some("rev"));
        assert_eq!(query("/review "), None);
        assert_eq!(query("/review\n"), None);
        assert_eq!(query("review"), None);
        assert_eq!(query(" /review"), None);
    }

    #[test]
    fn names_that_start_with_the_query_come_first() {
        let command = |name: &str, description: &str| CommandInfo {
            name: name.into(),
            description: description.into(),
            hint: None,
        };
        let commands = [
            command("pr-review", "Comment on a PR"),
            command("compact", "Keep a summary of the review"),
            command("Review", "Review a change"),
            command("init", "Set up"),
        ];
        let names = |query| -> Vec<&str> {
            let found = matching(&commands, query);
            found.iter().map(|c| c.name.as_str()).collect()
        };
        assert_eq!(names("re"), ["Review", "pr-review", "compact"]);
        assert_eq!(names("RE"), ["Review", "pr-review", "compact"]);
        assert_eq!(names(""), ["pr-review", "compact", "Review", "init"]);
        assert!(names("zzz").is_empty());
    }
}
