//! The Agents view: one live card per open session, in the sidebar's Agents tab or in its own
//! panel right of the panes. It repaints from the sessions' notifications (a session notifies on
//! each event), never on a timer.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use wai_agent::{PlanEntry, SubagentState};

use crate::app::ToggleAgents;
use crate::aurora::{self, DISPLAY_FONT};
use crate::chat::{ChatSession, TurnState};
use crate::sidebar::SidebarTab;
use crate::workspace::{Dragged, DraggedSession, Workspace};

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

/// A live subagent's line: `{title} · Working` or `{title} · Needs you`, then ` · {tool}`; none
/// once it finished (or the agent lost it).
pub fn subagent_line(title: &str, state: SubagentState, tool: Option<&str>) -> Option<String> {
    let status = match state {
        SubagentState::Running => "Working",
        SubagentState::NeedsYou => "Needs you",
        _ => return None,
    };
    let mut line = format!("{title} · {status}");
    line.extend(tool.map(|t| format!(" · {t}")));
    Some(line)
}

/// What the card says, as the screen-reader name of the card: title, status, plan, subagents,
/// tool.
fn summary(
    title: &str,
    label: Option<&str>,
    plan: &[PlanEntry],
    subagents: &[String],
    tool: Option<&str>,
) -> String {
    let mut text = title.to_string();
    text.extend(label.map(|l| format!(", {l}")));
    for e in plan.iter().take(MAX_PLAN_ROWS) {
        text.push_str(&format!("; {} ({})", e.content, e.status));
    }
    if plan.len() > MAX_PLAN_ROWS {
        text.push_str(&format!("; +{} more", plan.len() - MAX_PLAN_ROWS));
    }
    for line in subagents {
        text.push_str(&format!("; {line}"));
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
        // ponytail: no cap; live subagents are as many as the agent runs at once.
        let subagents: Vec<(TurnState, String)> = m
            .subagents
            .iter()
            .filter_map(|s| {
                let line = subagent_line(&s.title, s.state, m.subagent_tool(&s.id))?;
                let state = match s.state {
                    SubagentState::NeedsYou => TurnState::NeedsYou,
                    _ => TurnState::Working,
                };
                Some((state, line))
            })
            .collect();
        let lines: Vec<String> = subagents.iter().map(|(_, l)| l.clone()).collect();
        let summary = summary(&title, label, &m.plan, &lines, m.current_tool());
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
            .children(subagents.iter().map(|(state, line)| {
                h_flex()
                    .gap_2()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child(
                        div()
                            .flex_none()
                            .size(px(6.))
                            .rounded_full()
                            .bg(crate::sidebar::status_color(state, theme)),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(line.clone()))
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

impl Workspace {
    /// Moves the view between the sidebar's tab and its own panel on the right, shown. The
    /// sidebar has no Agents tab while it is on the right.
    pub(crate) fn move_agents(&mut self, cx: &mut Context<Self>) {
        self.agents_right = !self.agents_right;
        self.agents_collapsed = false;
        if !self.agents_right {
            self.sidebar_tab = SidebarTab::Agents;
        } else if self.sidebar_tab == SidebarTab::Agents {
            self.sidebar_tab = SidebarTab::Sessions;
        }
        self.save_state(cx);
        cx.notify();
    }

    /// Hides the panel on the right, or shows it again; in the sidebar there is nothing to hide.
    pub(crate) fn toggle_agents(
        &mut self,
        _: &ToggleAgents,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.agents_right {
            return;
        }
        self.agents_collapsed = !self.agents_collapsed;
        self.save_state(cx);
        cx.notify();
    }

    /// The view under its header: the button that moves it and, on the right, the one that
    /// collapses it.
    pub(crate) fn render_agents(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let (bar, icon, tip) = if self.agents_right {
            let icon = IconName::PanelLeftOpen;
            (aurora::card_bar(cx), icon, "Move to the sidebar")
        } else {
            (h_flex(), IconName::PanelRightOpen, "Move to the right")
        };
        let button = |id: &'static str, icon: IconName, tip: &'static str| {
            Button::new(id)
                .ghost()
                .xsmall()
                .icon(icon)
                .tooltip(tip)
                .accessibility_label(tip)
        };
        let title = div()
            .flex_1()
            .font_family(DISPLAY_FONT)
            .text_size(px(14.))
            .font_weight(FontWeight::BOLD)
            .child("Agents");
        let header = bar
            .px_3()
            .py_1()
            .gap_1()
            .child(title)
            .child(
                button("agents-move", icon, tip)
                    .on_click(cx.listener(|this, _, _, cx| this.move_agents(cx))),
            )
            .children(self.agents_right.then(|| {
                button("agents-collapse", IconName::PanelRightClose, "Collapse").on_click(
                    cx.listener(|this, _, window, cx| {
                        this.toggle_agents(&ToggleAgents, window, cx)
                    }),
                )
            }));
        v_flex()
            .size_full()
            .child(header)
            .child(div().flex_1().min_h_0().child(self.agents.clone()))
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
    use gpui_kit::{Entity, TestAppContext, VisualTestContext, px, size};
    use wai_settings::{AppState, Settings};

    use super::{PlanEntry, status_label, subagent_line, summary, usage_text};
    use crate::chat::TurnState;
    use crate::sidebar::SidebarTab;
    use wai_agent::SubagentState;

    use crate::workspace::Workspace;
    use crate::workspace::tests::{click, fake_settings, open, open_at, start_now, wait_until};

    #[test]
    fn plan_rows_are_capped() {
        let entry = |i: usize| PlanEntry {
            content: format!("step {i}"),
            status: "pending".into(),
        };
        let plan: Vec<_> = (0..11).map(entry).collect();
        let text = summary("t", None, &plan, &[], None);
        assert!(
            text.contains("step 7") && !text.contains("step 8"),
            "{text}"
        );
        assert!(text.ends_with("; +3 more"), "{text}");
        assert!(!summary("t", None, &plan[..8], &[], None).contains("more"));
    }

    /// Only a running subagent, or one that needs the user, has a line.
    #[test]
    fn subagent_lines_show_only_live_ones() {
        use SubagentState as S;
        assert_eq!(
            subagent_line("Explore", S::Running, Some("Read lib.rs")).as_deref(),
            Some("Explore · Working · Read lib.rs")
        );
        assert_eq!(
            subagent_line("Explore", S::NeedsYou, None).as_deref(),
            Some("Explore · Needs you")
        );
        for state in [S::Done, S::Stopped, S::Failed, S::Ended, S::Unknown] {
            assert_eq!(
                subagent_line("Explore", state, Some("Read")),
                None,
                "{state:?}"
            );
        }
        assert_eq!(
            summary("t", None, &[], &["a · Working".into()], None),
            "t; a · Working"
        );
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
        start_now(workspace, cx);
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), n);
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

    /// A background subagent waiting on its card, after the turn, has a line under the session's
    /// card; the line goes once it finishes.
    #[gpui_kit::test]
    fn agents_tab_lists_running_subagents(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(
            cx,
            fake_settings("--subagent --background --ask-permission"),
        );
        new_session(&workspace, 1, cx);
        let session = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        cx.simulate_input("edit");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the subagent to need the user after the turn", |cx| {
            session.read_with(cx, |s, _| {
                s.model.done
                    && s.model
                        .subagents
                        .iter()
                        .any(|a| a.state == SubagentState::NeedsYou)
            })
        });
        let label = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                let card = window.find(("agent-card", 0usize));
                card.label().unwrap_or_default().to_string()
            })
        };
        click("tab-agents", cx);
        let text = label(cx);
        assert!(
            text.contains("; Explore the code · Needs you · Write notes.md"),
            "{text:?}"
        );

        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click("perm-allow_once", cx);
        });
        wait_until(cx, "the subagent to finish", |cx| {
            session.read_with(cx, |s, _| {
                s.model
                    .subagents
                    .iter()
                    .any(|a| a.state == SubagentState::Done)
            })
        });
        let text = label(cx);
        assert!(!text.contains("Explore the code"), "{text:?}");
    }

    fn found(id: &'static str, cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find(id).is_some()
        })
    }

    /// The header's button moves the view to its own panel right of the panes, without the
    /// sidebar's tab, and back.
    #[gpui_kit::test]
    fn agents_move_to_the_right_and_back(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_resize(size(px(1400.), px(900.)));
        assert!(!found("topbar-agents", cx));
        assert!(cx.debug_bounds("agents-panel").is_none());

        click("tab-agents", cx);
        click("agents-move", cx);
        workspace.read_with(cx, |w, _| {
            assert!(w.agents_right && !w.agents_collapsed);
            assert_eq!(w.sidebar_tab, SidebarTab::Sessions);
        });
        assert!(!found("tab-agents", cx));
        assert!(found("tab-search", cx) && found("topbar-agents", cx));
        let panel = cx.debug_bounds("agents-panel").expect("panel drawn");
        let pane = cx.debug_bounds("pane-0").expect("pane drawn");
        let sidebar = cx.debug_bounds("sidebar").expect("sidebar drawn");
        assert!(panel.left() >= pane.right(), "{panel:?} {pane:?}");
        assert_eq!(panel.size.width, sidebar.size.width);

        click("agents-move", cx);
        workspace.read_with(cx, |w, _| {
            assert!(!w.agents_right);
            assert_eq!(w.sidebar_tab, SidebarTab::Agents);
        });
        assert!(found("tab-agents", cx) && found("agents-move", cx));
        assert!(!found("topbar-agents", cx) && !found("agents-collapse", cx));
        assert!(cx.debug_bounds("agents-panel").is_none());
        // In the sidebar there is nothing to collapse.
        cx.simulate_keystrokes("secondary-alt-b");
        assert!(workspace.read_with(cx, |w, _| !w.agents_collapsed));
    }

    /// Collapsing hides the whole panel; the top bar's button and the shortcut toggle it.
    #[gpui_kit::test]
    fn agents_panel_collapses(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_resize(size(px(1400.), px(900.)));
        cx.update(|_, cx| workspace.update(cx, |w, cx| w.move_agents(cx)));
        let shown = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| window.render_frame(cx));
            let drawn = cx.debug_bounds("agents-panel").is_some();
            assert_eq!(drawn, workspace.read_with(cx, |w, _| !w.agents_collapsed));
            drawn
        };
        assert!(shown(cx));
        click("agents-collapse", cx);
        assert!(!shown(cx));
        assert!(
            found("topbar-agents", cx),
            "the button stays to show it again"
        );
        click("topbar-agents", cx);
        assert!(shown(cx));
        click("topbar-agents", cx);
        assert!(!shown(cx));
        cx.simulate_keystrokes("secondary-alt-b");
        assert!(shown(cx));
        cx.simulate_keystrokes("secondary-alt-b");
        assert!(!shown(cx));
    }

    /// The position and the collapsed state are written on each change and come back at launch.
    #[gpui_kit::test]
    fn agents_position_is_saved_and_restored(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = AppState {
            agents_right: true,
            agents_collapsed: true,
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, Settings::default(), Some(path.clone()));
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.restore(window, cx)));
        cx.run_until_parked();
        assert!(workspace.read_with(cx, |w, _| w.agents_right && w.agents_collapsed));
        assert!(cx.debug_bounds("agents-panel").is_none());

        click("topbar-agents", cx);
        let state = AppState::load(&path);
        assert!(state.agents_right && !state.agents_collapsed);
        click("agents-collapse", cx);
        assert!(AppState::load(&path).agents_collapsed);
        click("topbar-agents", cx);
        click("agents-move", cx);
        let state = AppState::load(&path);
        assert!(!state.agents_right && !state.agents_collapsed);
    }

    /// On the right, a card click still shows its session in the focused pane, and the top bar's
    /// button counts the sessions that need the user.
    #[gpui_kit::test]
    fn right_panel_card_shows_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-permission"));
        cx.simulate_resize(size(px(1400.), px(900.)));
        new_session(&workspace, 1, cx);
        let first = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        new_session(&workspace, 2, cx);
        let second = workspace.read_with(cx, |w, _| w.sessions[1].clone());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.move_agents(cx);
                w.show(first.clone(), false, window, cx)
            })
        });
        let label = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                let button = window.find("topbar-agents");
                button.label().unwrap_or_default().to_string()
            })
        };
        assert_eq!(label(cx), "Toggle agents");
        cx.simulate_input("edit");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "needs you", |cx| {
            first.read_with(cx, |s, _| s.model.state == TurnState::NeedsYou)
        });
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.show(second.clone(), false, window, cx))
        });
        cx.run_until_parked();
        assert_eq!(label(cx), "Toggle agents · 1 needs you");

        cx.update(|window, cx| window.click(("agent-card", 0usize), cx));
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes[w.focused_pane].session(), Some(&first));
        });
    }
}
