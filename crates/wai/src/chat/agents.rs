//! The agent list under the prompt box (mockup `.agents`): `main` and the session's subagents,
//! by mouse and keyboard; opening one sets `drawer`.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Enter, MoveDown, MoveUp};
use gpui_kit::component::text::TextView;
use gpui_kit::component::{h_flex, v_flex, ActiveTheme as _, IconName, Sizable as _};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::SubagentState;

use super::model::{elapsed, ChatItem, Said, Subagent};
use super::render::{output_box, tokens};
use super::view::{sync_markdown, ChatSession};
use crate::agents_view::{plan_row, MAX_PLAN_ROWS};
use crate::aurora;

/// A row's height and the gap between rows: the list shows `ROWS` rows and scrolls past them.
const ROW: f32 = 22.;
const GAP: f32 = 1.;
const ROWS: f32 = 6.;

/// `9s`, `done · 32s`, then ` · ↓ 46k tokens` when the agent reports them.
// ponytail: live times repaint on the turn clock's ticks; a background subagent's time holds
// still between turns. Give the session a 1 s tick while one runs if that matters.
pub(super) fn sub_stat(sub: &Subagent) -> String {
    let time = match sub.ended {
        Some(end) => format!("done · {}", elapsed((end - sub.started).as_secs())),
        None => elapsed(sub.started.elapsed().as_secs()),
    };
    match sub.used {
        Some(n) => format!("{time} · ↓ {} tokens", tokens(n)),
        None => time,
    }
}

/// A subagent's state mark, its colour and its word, as the list, the drawer and the card show it.
pub(super) fn state_look(sub: &Subagent, cx: &App) -> (&'static str, Hsla, &'static str) {
    let theme = cx.theme();
    let pick = |aurora: Hsla, light: Hsla| if theme.is_dark() { aurora } else { light };
    let cyan = pick(aurora::boreal(), theme.info);
    match sub.state {
        SubagentState::Running => ("○", cyan, "running"),
        SubagentState::NeedsYou => ("○", pick(aurora::ambar(), theme.warning), "needs you"),
        SubagentState::Done | SubagentState::Ended => {
            ("✓", pick(aurora::ok(), theme.success), "done")
        }
        SubagentState::Stopped => ("✗", pick(aurora::erro(), theme.danger), "stopped"),
        SubagentState::Failed => ("✗", pick(aurora::erro(), theme.danger), "failed"),
        // Parked by the agent's exit (`wind_down`).
        SubagentState::Unknown if sub.ended.is_some() => {
            ("✗", pick(aurora::erro(), theme.danger), "stopped")
        }
        SubagentState::Unknown => (
            "○",
            pick(aurora::nevoa(), theme.muted_foreground),
            "starting",
        ),
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
        self.prune_agents();
        let subs = self.model.shown_subagents();
        if subs.is_empty() {
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
            let (mark, color, state) = state_look(sub, cx);
            // Without a kind the title is the name and there is no task.
            let (name, task) = match &sub.kind {
                Some(kind) => (kind.as_str(), sub.title.as_str()),
                None => (sub.title.as_str(), ""),
            };
            let stat = sub_stat(sub);
            let id = sub.id.clone();
            let on = selected(ix + 1, Some(&sub.id));
            let name_el = div().child(name.to_string());
            let stop = self.model.stoppable(&sub.id).then(|| {
                let target = sub.id.clone();
                Button::new(("agent-stop", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Square)
                    .tooltip("Stop subagent")
                    .accessibility_label("Stop subagent")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        cx.stop_propagation();
                        this.stop_subagent(&target);
                    }))
            });
            row(
                ("agent-row", ix).into(),
                on,
                (mark, color),
                name_el,
                task,
                stat.clone(),
            )
            .aria_label(join([name, task, state, &stat]))
            .children(stop)
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

    /// Closes a drawer whose subagent is gone from the model, and leaves the list once it has
    /// no rows, so Esc and Enter are the prompt's again. A subagent of an earlier turn keeps its
    /// drawer: its transcript card opens it.
    pub(super) fn prune_agents(&mut self) {
        if self.model.shown_subagents().is_empty() {
            self.agent_cursor = None;
        }
        let known = (self.model.subagents.iter()).any(|s| self.drawer.as_deref() == Some(&s.id));
        if !known {
            self.drawer = None;
        }
    }

    /// Opens the subagent `id`'s drawer; `None` closes it (back to `main`).
    pub(crate) fn open_drawer(&mut self, id: Option<String>, cx: &mut Context<Self>) {
        if self.drawer == id {
            return;
        }
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

impl ChatSession {
    /// `.drawer`: the open subagent's calls and answer, as a glass panel laid over the
    /// transcript's bottom (like `render_waiting`, which is drawn after it, on top).
    pub(super) fn render_drawer(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let si = self
            .drawer
            .as_deref()
            .and_then(|id| (self.model.subagents.iter()).position(|s| s.id == id))?;
        for (k, said) in self.model.subagents[si].said.iter().enumerate() {
            if let Said::Text(t) | Said::Thought(t) = said {
                let key = (self.model.subagents[si].id.clone(), k);
                sync_markdown(&mut self.sub_texts, key, t, cx);
            }
        }
        let sub = &self.model.subagents[si];
        let theme = cx.theme();
        let dark = theme.is_dark();
        let (fg, fg2, muted, faint) = match dark {
            true => (
                aurora::tinta(),
                aurora::tinta_2(),
                aurora::nevoa(),
                aurora::nevoa_2(),
            ),
            false => (
                theme.foreground,
                theme.foreground,
                theme.muted_foreground,
                theme.muted_foreground,
            ),
        };
        let (mark, color, _) = state_look(sub, cx);
        let (name, task) = match &sub.kind {
            Some(kind) => (kind.as_str(), sub.title.as_str()),
            None => (sub.title.as_str(), ""),
        };
        let stat = sub_stat(sub);
        let mut body_rows: Vec<AnyElement> = Vec::new();
        for (k, said) in sub.said.iter().enumerate() {
            match said {
                Said::Call(ix) => {
                    if let Some(ChatItem::Tool(t)) = self.model.items.get(*ix) {
                        body_rows.push(self.render_tool(*ix, t, cx).into_any_element());
                    }
                }
                Said::Text(_) => {
                    if let Some(md) = self.sub_texts.get(&(sub.id.clone(), k)) {
                        body_rows.push(
                            div()
                                .id(("subagent-text", k))
                                .child(TextView::new(&md.state))
                                // Registers the id so tests can find it.
                                .test_support()
                                .into_any_element(),
                        );
                    }
                }
                Said::Thought(_) => {
                    if let Some(md) = self.sub_texts.get(&(sub.id.clone(), k)) {
                        body_rows.push(
                            output_box(cx)
                                .id(("subagent-thought", k))
                                .text_color(muted)
                                .child(TextView::new(&md.state))
                                .test_support()
                                .into_any_element(),
                        );
                    }
                }
            }
        }
        // The card is the `Tool` whose id is the subagent's; its row owns the answer's markdown.
        // A subagent that spoke shows its text instead: Claude sends the answer as both.
        let spoke = sub.said.iter().any(|s| matches!(s, Said::Text(_)));
        let answer = (!spoke)
            .then(|| {
                (self.model.items.iter().enumerate())
                    .find(|(_, item)| matches!(item, ChatItem::Tool(t) if t.id == sub.id))
                    .and_then(|(ix, _)| self.texts.get(&ix))
            })
            .flatten()
            .map(|md| {
                div()
                    .id("subagent-answer")
                    .child(TextView::new(&md.state))
                    // Registers the id so tests can find it.
                    .test_support()
            });
        let plan = (!sub.plan.is_empty()).then(|| {
            v_flex()
                .id("subagent-plan")
                .flex_none()
                .gap_1()
                .px(px(12.))
                .py(px(6.))
                .border_b_1()
                .border_color(if dark {
                    aurora::borda()
                } else {
                    cx.theme().border
                })
                .children(sub.plan.iter().take(MAX_PLAN_ROWS).map(|e| plan_row(e, cx)))
                .children((sub.plan.len() > MAX_PLAN_ROWS).then(|| {
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("+{} more", sub.plan.len() - MAX_PLAN_ROWS))
                }))
                .test_support()
        });
        let theme = cx.theme();
        let close = Button::new("subagent-drawer-close")
            .ghost()
            .xsmall()
            .icon(IconName::Close)
            .tooltip("Close")
            .accessibility_label("Close subagent")
            .on_click(cx.listener(|this, _, _, cx| this.open_drawer(None, cx)));
        let stop = self.model.stoppable(&sub.id).then(|| {
            let target = sub.id.clone();
            Button::new("subagent-stop")
                .ghost()
                .xsmall()
                .icon(IconName::Square)
                .tooltip("Stop subagent")
                .accessibility_label("Stop subagent")
                .on_click(cx.listener(move |this, _, _, cx| {
                    cx.stop_propagation();
                    this.stop_subagent(&target);
                }))
        });
        let head = h_flex()
            .flex_none()
            .items_center()
            .gap_2()
            .px(px(12.))
            .py(px(7.))
            .border_b_1()
            .border_color(if dark { aurora::borda() } else { theme.border })
            .text_size(px(12.))
            .child(div().text_color(color).child(mark))
            .child(
                div()
                    .font_weight(FontWeight::BOLD)
                    .text_color(fg)
                    .child(name.to_string()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .text_color(muted)
                    .child(task.to_string()),
            )
            .child(div().flex_none().text_color(faint).child(stat))
            .children(stop)
            .child(close);
        // Follow the bottom only while it is where the reader is. The offset is last frame's,
        // so the view trails a growing body by a frame.
        let at_bottom = {
            let (max, y) = (
                self.drawer_scroll.max_offset().y,
                self.drawer_scroll.offset().y,
            );
            max <= px(0.) || -y >= max - px(1.)
        };
        if at_bottom {
            self.drawer_scroll.scroll_to_bottom();
        }
        let body = v_flex()
            .id("subagent-body")
            .flex_1()
            .min_h_0()
            .gap_1()
            .px(px(12.))
            .py(px(8.))
            .overflow_y_scroll()
            .track_scroll(&self.drawer_scroll)
            .font_family(theme.mono_font_family.clone())
            .text_size(px(11.5))
            .text_color(fg2)
            .children(body_rows)
            .children(answer);
        let panel = v_flex()
            .id("subagent-drawer")
            .h(relative(0.46))
            .min_h_0()
            .occlude()
            .rounded(px(14.))
            .border_1()
            .child(head)
            .children(plan)
            .child(body);
        let panel = match dark {
            true => panel
                .border_color(theme.list_active_border)
                .bg(aurora::noite_2().alpha(0.95))
                .shadow(vec![BoxShadow::new(
                    px(0.),
                    px(-10.),
                    hsla(0., 0., 0., 0.5),
                )
                .blur_radius(px(20.))]),
            false => panel
                .border_color(theme.list_active_border)
                .bg(theme.background.opacity(0.95))
                .shadow(vec![
                    BoxShadow::new(px(0.), px(-6.), hsla(0., 0., 0., 0.2)).blur_radius(px(12.))
                ]),
        };
        // 46 % of the transcript above the waiting cards, shrunk to the room left there when a
        // tall card leaves less. The wrapper has no listeners, so clicks pass through it.
        Some(
            v_flex()
                .absolute()
                .top_0()
                .bottom_0()
                .left(px(10.))
                .right(px(10.))
                .justify_end()
                .child(div().flex_none().h(px(8.)))
                // Registers the id so tests can find it.
                .child(panel.test_support())
                .child(div().flex_none().h(self.waiting_height + px(16.)))
                .into_any_element(),
        )
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
            said: Vec::new(),
            plan: Vec::new(),
            can_stop: false,
        };
        assert_eq!(sub_stat(&sub), "9s · ↓ 46k tokens");
        (sub.ended, sub.used) = (Some(started + Duration::from_secs(32)), None);
        assert_eq!(sub_stat(&sub), "done · 32s");
    }
}
