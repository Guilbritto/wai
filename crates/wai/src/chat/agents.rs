//! The agent list under the prompt box (mockup `.agents`): `main` and the session's subagents,
//! by mouse and keyboard; opening one sets `drawer`.

use gpui_kit::component::input::{Enter, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::SubagentState;

use super::model::{Subagent, elapsed};
use super::render::tokens;
use super::view::ChatSession;
use crate::aurora;

/// A row's height and the gap between rows: the list shows `ROWS` rows and scrolls past them.
const ROW: f32 = 22.;
const GAP: f32 = 1.;
const ROWS: f32 = 6.;

/// `9s`, `done · 32s`, then ` · ↓ 46k tokens` when the agent reports them.
// ponytail: live times repaint on the turn clock's ticks; a background subagent's time holds
// still between turns. Give the session a 1 s tick while one runs if that matters.
fn sub_stat(sub: &Subagent) -> String {
    let time = match sub.ended {
        Some(end) => format!("done · {}", elapsed((end - sub.started).as_secs())),
        None => elapsed(sub.started.elapsed().as_secs()),
    };
    match sub.used {
        Some(n) => format!("{time} · ↓ {} tokens", tokens(n)),
        None => time,
    }
}

/// The non-empty parts, comma-separated: a row's name for assistive tech.
fn join<'a>(parts: impl IntoIterator<Item = &'a str>) -> String {
    let parts: Vec<&str> = parts.into_iter().filter(|p| !p.is_empty()).collect();
    parts.join(", ")
}

impl ChatSession {
    /// The list, while the session has subagents to show; `None` draws nothing.
    pub(super) fn render_agents(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let subs = self.model.shown_subagents();
        if subs.is_empty() {
            // Nothing to navigate: Esc and Enter are the prompt's again.
            self.agent_cursor = None;
            return None;
        }
        let theme = cx.theme();
        let dark = theme.is_dark();
        let pick = |aurora: Hsla, light: Hsla| if dark { aurora } else { light };
        let (fg, fg2, muted, faint) = (
            pick(aurora::tinta(), theme.foreground),
            pick(aurora::tinta_2(), theme.foreground),
            pick(aurora::nevoa(), theme.muted_foreground),
            pick(aurora::nevoa_2(), theme.muted_foreground),
        );
        let (cyan, amber, green, red) = (
            pick(aurora::boreal(), theme.info),
            pick(aurora::ambar(), theme.warning),
            pick(aurora::ok(), theme.success),
            pick(aurora::erro(), theme.danger),
        );
        // The cursor's row while the list has the keys, else the open drawer's, else `main`.
        let cursor = self.agent_cursor.map(|c| c.min(subs.len()));
        let selected = |ix: usize, id: Option<&str>| match cursor {
            Some(c) => c == ix,
            None => self.drawer.as_deref() == id,
        };
        // `.ag`: the mark, the name, what it does, then its time and tokens.
        let row = |id: ElementId,
                   on: bool,
                   (mark, color): (&str, Hsla),
                   name: Div,
                   what: &str,
                   stat: String| {
            aurora::tab(on, cx)
                .id(id)
                .flex_none()
                .h(px(ROW))
                .gap(px(9.))
                .px(px(8.))
                .rounded(px(7.))
                .when(!on, |el| el.text_color(fg2))
                .role(Role::ListItem)
                .aria_selected(on)
                .child(
                    div()
                        .w(px(10.))
                        .flex_none()
                        .text_center()
                        .text_color(color)
                        .child(mark.to_string()),
                )
                .child(name.min_w(px(116.)).flex_none())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(muted)
                        .child(what.to_string()),
                )
                .child(
                    div()
                        .flex_none()
                        .text_size(px(11.5))
                        .text_color(faint)
                        .child(stat),
                )
        };
        let turn = (self.turn_started()).map(|at| elapsed(at.elapsed().as_secs()));
        let used = (self.model.usage.as_ref()).map(|u| format!("↓ {} tokens", tokens(u.used)));
        let stat: Vec<String> = [turn, used].into_iter().flatten().collect();
        let stat = stat.join(" · ");
        let on = selected(0, None);
        let name = div()
            .font_weight(FontWeight::BOLD)
            .text_color(fg)
            .child("main");
        let mark = ("●", if on { fg } else { fg2 });
        let main = row("agent-main".into(), on, mark, name, "", stat.clone())
            .aria_label(join(["main", &stat]))
            .on_click(cx.listener(|this, _, _, cx| {
                this.agent_cursor = None;
                this.open_drawer(None, cx);
            }))
            // Registers the id so tests can find and click it.
            .test_support();
        let rows = subs.iter().enumerate().map(|(ix, sub)| {
            let (mark, color, state) = match sub.state {
                SubagentState::Running => ("○", cyan, "running"),
                SubagentState::NeedsYou => ("○", amber, "needs you"),
                SubagentState::Done | SubagentState::Ended => ("✓", green, "done"),
                SubagentState::Stopped => ("✗", red, "stopped"),
                SubagentState::Failed => ("✗", red, "failed"),
                SubagentState::Unknown => ("○", muted, "starting"),
            };
            // Without a kind the title is the name and there is no task.
            let (name, task) = match &sub.kind {
                Some(kind) => (kind.as_str(), sub.title.as_str()),
                None => (sub.title.as_str(), ""),
            };
            let stat = sub_stat(sub);
            let id = sub.id.clone();
            let on = selected(ix + 1, Some(&sub.id));
            let name_el = div().child(name.to_string());
            row(
                ("agent-row", ix).into(),
                on,
                (mark, color),
                name_el,
                task,
                stat.clone(),
            )
            .aria_label(join([name, task, state, &stat]))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.agent_cursor = None;
                let again = this.drawer.as_deref() == Some(id.as_str());
                this.open_drawer((!again).then(|| id.clone()), cx);
            }))
            // Registers the id so tests can find and click it.
            .test_support()
        });
        let list = v_flex()
            .id("agent-list")
            .mx(px(6.))
            .gap(px(GAP))
            .max_h(px(ROWS * ROW + (ROWS - 1.) * GAP))
            .overflow_y_scroll()
            .track_scroll(&self.agent_scroll)
            .role(Role::List)
            .aria_label("Agents")
            .child(main)
            .children(rows)
            // Registers the id so tests can find it.
            .test_support();
        let hint = self.agent_cursor.is_some().then(|| {
            div()
                .id("agent-hint")
                .px(px(14.))
                .pt(px(2.))
                .font_family(theme.font_family.clone())
                .text_size(px(10.5))
                .text_color(faint)
                .child("↑↓ navigate · Enter open · Esc back to main")
                // Registers the id so tests can find it.
                .test_support()
        });
        // `.agents` sits 10 px under the box: the prompt's 12 px padding, less 4, plus its 2.
        let el = v_flex()
            .px_3()
            .pt(px(2.))
            .pb(px(8.))
            .mt(px(-4.))
            .font_family(theme.mono_font_family.clone())
            .text_size(px(12.))
            .child(list)
            .children(hint);
        Some(el.into_any_element())
    }

    /// Opens the subagent `id`'s drawer; `None` closes it (back to `main`).
    pub(super) fn open_drawer(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        self.drawer = id;
        cx.notify();
    }

    /// With the prompt box empty, ↓ enters the list at `main` and moves down it, ↑ moves up and
    /// from `main` back to the prompt, Enter opens the row (`main` closes the drawer).
    pub(super) fn agent_keys(&self, el: Div, cx: &Context<Self>) -> Div {
        el.capture_action(cx.listener(|this, _: &MoveDown, _, cx| {
            let n = this.model.shown_subagents().len();
            if n == 0 || (this.agent_cursor.is_none() && !this.prompt_empty(cx)) {
                return;
            }
            this.move_agent_cursor(Some(this.agent_cursor.map_or(0, |c| (c + 1).min(n))), cx);
        }))
        .capture_action(cx.listener(|this, _: &MoveUp, _, cx| {
            if let Some(c) = this.agent_cursor {
                this.move_agent_cursor(c.checked_sub(1), cx);
            }
        }))
        .capture_action(cx.listener(|this, enter: &Enter, _, cx| {
            let Some(c) = this
                .agent_cursor
                .filter(|_| !enter.shift && !enter.secondary)
            else {
                return;
            };
            let id = c.checked_sub(1).and_then(|k| {
                let subs = this.model.shown_subagents();
                subs.get(k.min(subs.len().saturating_sub(1)))
                    .map(|s| s.id.clone())
            });
            cx.stop_propagation();
            this.open_drawer(id, cx);
        }))
    }

    fn move_agent_cursor(&mut self, cursor: Option<usize>, cx: &mut Context<Self>) {
        self.agent_cursor = cursor;
        if let Some(c) = cursor {
            self.agent_scroll.scroll_to_item(c);
        }
        cx.stop_propagation();
        cx.notify();
    }

    fn prompt_empty(&self, cx: &App) -> bool {
        self.input.read(cx).value().is_empty() && self.attachments.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use wai_agent::SubagentState;

    use super::sub_stat;
    use crate::chat::model::Subagent;

    #[test]
    fn stat_says_time_then_tokens() {
        let started = Instant::now() - Duration::from_secs(9);
        let mut sub = Subagent {
            id: "a".into(),
            title: "t".into(),
            kind: None,
            state: SubagentState::Running,
            used: Some(46_900),
            started,
            ended: None,
            turn: 0,
        };
        assert_eq!(sub_stat(&sub), "9s · ↓ 46k tokens");
        (sub.ended, sub.used) = (Some(started + Duration::from_secs(32)), None);
        assert_eq!(sub_stat(&sub), "done · 32s");
    }
}
