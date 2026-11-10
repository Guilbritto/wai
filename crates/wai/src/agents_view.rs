//! The sidebar's Agents tab: one live card per open session. It repaints from the sessions'
//! notifications (a session notifies on each event), never on a timer.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use wai_agent::PlanEntry;

use crate::chat::{ChatSession, TurnState};
use crate::workspace::{Dragged, DraggedSession};

/// A card was clicked: show this session in the focused pane.
pub struct ShowSession(pub Entity<ChatSession>);

pub struct AgentsView {
    sessions: Vec<Entity<ChatSession>>,
    _watching: Vec<Subscription>,
}

impl EventEmitter<ShowSession> for AgentsView {}

/// Plan rows a card shows before folding the rest into `+N more`.
pub const MAX_PLAN_ROWS: usize = 8;

/// The status label of a session. `Done` is `done`: the last turn of this run ended with
/// `EndTurn`; an Idle session without that (resumed, cancelled, never ran) has no label.
pub fn status_label(state: &TurnState, done: bool) -> Option<&'static str> {
    match state {
        TurnState::Working => Some("Working"),
        TurnState::NeedsYou => Some("Needs you"),
        TurnState::Error(_) => Some("Error"),
        TurnState::Idle => done.then_some("Done"),
    }
}

/// What the card says, as the screen-reader name of the card: title, status, plan, tool.
fn summary(title: &str, label: Option<&str>, plan: &[PlanEntry], tool: Option<&str>) -> String {
    let mut text = title.to_string();
    text.extend(label.map(|l| format!(", {l}")));
    for e in plan.iter().take(MAX_PLAN_ROWS) {
        text.push_str(&format!("; {} ({})", e.content, e.status));
    }
    if plan.len() > MAX_PLAN_ROWS {
        text.push_str(&format!("; +{} more", plan.len() - MAX_PLAN_ROWS));
    }
    text.extend(tool.map(|t| format!("; running {t}")));
    text
}

/// `12.3k / 200k (6%)`.
pub fn usage_text(used: u64, size: u64) -> String {
    let k = |n: u64| match n {
        0..1000 => n.to_string(),
        _ if n.is_multiple_of(1000) || n >= 100_000 => format!("{}k", n / 1000),
        _ => format!("{:.1}k", n as f64 / 1000.),
    };
    let pct = (used * 100).checked_div(size).unwrap_or(0);
    format!("{} / {} ({pct}%)", k(used), k(size))
}

impl AgentsView {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            _watching: Vec::new(),
        }
    }

    /// Follows exactly these sessions.
    pub fn set(&mut self, sessions: &[Entity<ChatSession>], cx: &mut Context<Self>) {
        self._watching = sessions
            .iter()
            .map(|s| cx.observe(s, |_, _, cx| cx.notify()))
            .collect();
        self.sessions = sessions.to_vec();
        cx.notify();
    }

    fn card(
        &self,
        ix: usize,
        session: &Entity<ChatSession>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        let theme = cx.theme();
        let s = session.read(cx);
        let m = &s.model;
        let color = crate::sidebar::status_color(&s.model.state, theme);
        let label = status_label(&m.state, m.done);
        let title = m.shown_title().unwrap_or("New session").to_string();
        let open = session.clone();
        let summary = summary(&title, label, &m.plan, m.current_tool());
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::Open(session.clone()),
        };
        // A `Button`, not a styled div: only components are reachable by element id in tests.
        let body = v_flex()
            .w_full()
            .gap_1()
            .text_left()
            .child(
                h_flex()
                    .gap_2()
                    .child(div().flex_none().size(px(8.)).rounded_full().bg(color))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(title),
                    )
                    .children(label.map(|l| div().text_xs().text_color(color).child(l))),
            )
            .children(m.plan.iter().take(MAX_PLAN_ROWS).map(|e| plan_row(e, cx)))
            .children((m.plan.len() > MAX_PLAN_ROWS).then(|| {
                let more = m.plan.len() - MAX_PLAN_ROWS;
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(format!("+{more} more"))
            }))
            .children(m.current_tool().map(|t| {
                div()
                    .truncate()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(t.to_string())
            }))
            .child(
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(div().flex_1().min_w_0().truncate().child(s.agent.clone()))
                    .children(m.usage.as_ref().map(|u| usage_text(u.used, u.size))),
            );
        let button = Button::new(("agent-card", ix))
            .accessibility_label(summary)
            .ghost()
            .w_full()
            .h_auto()
            .p_2()
            .border_1()
            .border_color(theme.border)
            .text_sm()
            .child(body)
            .on_click(cx.listener(move |_, _, _, cx| cx.emit(ShowSession(open.clone()))));
        // The card drags its session onto a pane, as an Open row does (a `Button` cannot drag).
        div()
            .id(("agent-drag", ix))
            .w_full()
            .cursor_grab()
            .child(button)
            .on_drag(dragged, DraggedSession::preview)
    }
}

fn plan_row(entry: &PlanEntry, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (icon, color) = match entry.status.as_str() {
        "completed" => (IconName::CircleCheck, theme.success),
        "in_progress" => (IconName::Loader, theme.blue),
        _ => (IconName::Minus, theme.muted_foreground),
    };
    h_flex()
        .gap_1()
        .text_xs()
        .text_color(theme.foreground)
        .child(Icon::new(icon).xsmall().text_color(color))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(entry.content.clone()),
        )
}

impl Render for AgentsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.sessions.is_empty() {
            return div()
                .p_3()
                .text_sm()
                .text_color(cx.theme().muted_foreground)
                .child("No open sessions")
                .into_any_element();
        }
        v_flex()
            .id("agents")
            .size_full()
            .pt_2()
            .overflow_y_scroll()
            .children(
                self.sessions
                    .iter()
                    .enumerate()
                    .map(|(ix, s)| div().px_2().pb_2().child(self.card(ix, s, cx))),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};

    use super::{PlanEntry, status_label, summary, usage_text};
    use crate::chat::TurnState;

    use crate::workspace::Workspace;
    use crate::workspace::tests::{fake_settings, open, wait_until};

    #[test]
    fn plan_rows_are_capped() {
        let entry = |i: usize| PlanEntry {
            content: format!("step {i}"),
            status: "pending".into(),
        };
        let plan: Vec<_> = (0..11).map(entry).collect();
        let text = summary("t", None, &plan, None);
        assert!(
            text.contains("step 7") && !text.contains("step 8"),
            "{text}"
        );
        assert!(text.ends_with("; +3 more"), "{text}");
        assert!(!summary("t", None, &plan[..8], None).contains("more"));
    }

    #[test]
    fn labels_and_usage() {
        let err = TurnState::Error("x".into());
        assert_eq!(status_label(&TurnState::Working, true), Some("Working"));
        assert_eq!(status_label(&TurnState::NeedsYou, true), Some("Needs you"));
        assert_eq!(status_label(&err, false), Some("Error"));
        assert_eq!(status_label(&TurnState::Idle, true), Some("Done"));
        assert_eq!(status_label(&TurnState::Idle, false), None);
        assert_eq!(status_label(&err, true), Some("Error"));
        assert_eq!(usage_text(12_300, 200_000), "12.3k / 200k (6%)");
        assert_eq!(usage_text(0, 0), "0 / 0 (0%)");
    }

    fn new_session(workspace: &Entity<Workspace>, n: usize, cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "a session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == n)
        });
    }

    #[gpui_kit::test]
    fn agents_tab_shows_needs_you_and_plan(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-permission --plan"));
        new_session(&workspace, 1, cx);
        let first = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        new_session(&workspace, 2, cx);
        let second = workspace.read_with(cx, |w, _| w.sessions[1].clone());

        // The focused pane holds the second session; the first needs the user, with a plan.
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.show(first.clone(), false, window, cx))
        });
        cx.simulate_input("edit");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "needs you", |cx| {
            first.read_with(cx, |s, _| {
                s.model.state == TurnState::NeedsYou && s.model.plan.len() == 3
            })
        });
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.show(second.clone(), false, window, cx));
            window.render_frame(cx);
            window.click("tab-agents", cx);
        });
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(("agent-card", 1usize));
            let card = window.find(("agent-card", 0usize));
            let label = card.label().unwrap_or_default();
            for text in [
                ", Needs you",
                "; Read the code (completed)",
                "; Write the change (in_progress)",
                "; Run the tests (pending)",
            ] {
                assert!(label.contains(text), "{text:?} not in {label:?}");
            }
            window.click(("agent-card", 0usize), cx);
        });
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes[w.focused_pane].session(), Some(&first));
        });
    }
}
