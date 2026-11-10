//! The sidebar: tabs for the open sessions with their status and the agent's history, the
//! project's files, search, the agents, then each running extension's panels.

use std::ops::Range;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::SessionSummary;
use wai_ext::panel::clamp;
pub use wai_settings::canonical;

use crate::agents::AgentStatus;
use crate::aurora;
use crate::chat::{ChatSession, TurnState};
use crate::workspace::{Dragged, DraggedSession, MAX_PANES, Workspace};

pub(crate) const ROW_HEIGHT: f32 = 28.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    Sessions,
    History,
    Files,
    Search,
    Agents,
    /// Opened from the top bar's Extensions button; it has no tab.
    Extensions,
    /// An extension's panel, by its view. When the panel goes, the sessions show again.
    Panel(EntityId),
}

/// A panel's tab title, one line: the manifest's words.
const PANEL_TITLE_CHARS: usize = 24;

/// Each tab with its element id and label.
const TABS: [(SidebarTab, &str, &str); 5] = [
    (SidebarTab::Sessions, "tab-sessions", "Sessions"),
    (SidebarTab::History, "tab-history", "History"),
    (SidebarTab::Files, "tab-files", "Files"),
    (SidebarTab::Search, "tab-search", "Search"),
    (SidebarTab::Agents, "tab-agents", "Agents"),
];

/// The status dot's colour, shared by the open list and the agent cards.
pub(crate) fn status_color(state: &TurnState, theme: &gpui_kit::component::Theme) -> Hsla {
    match state {
        TurnState::Working => theme.cyan,
        TurnState::NeedsYou => theme.warning,
        TurnState::Idle => theme.muted_foreground,
        TurnState::Error(_) => theme.danger,
    }
}

impl Workspace {
    /// The history the sidebar lists: the project's sessions, or all of them.
    pub fn history_rows(&self) -> Vec<&SessionSummary> {
        self.history
            .iter()
            .filter(|(cwd, _)| self.all_projects || cwd.starts_with(&self.project))
            .map(|(_, summary)| summary)
            .collect()
    }

    /// Lists the default agent's sessions again: on `Ready`, after each turn and each new
    /// session, never on a timer. A newer refresh drops the one in flight.
    pub(crate) fn refresh_history(&mut self, cx: &mut Context<Self>) {
        let Some(agent) = self.default_agent() else {
            return;
        };
        let ready = matches!(
            self.registry.read(cx).status(&agent),
            Some(AgentStatus::Ready(caps)) if caps.list_sessions
        );
        if !ready {
            return;
        }
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            return;
        };
        self.history_task = Some(cx.spawn(async move |this, cx| {
            let sessions = match handle.list_sessions().await {
                Ok(sessions) => sessions,
                Err(e) => return eprintln!("wai: could not list {agent} sessions: {e}"),
            };
            let rows = cx
                .background_spawn(async move {
                    let rows = sessions.into_iter().map(|s| (canonical(&s.cwd), s));
                    rows.collect()
                })
                .await;
            this.update(cx, |this, cx| {
                this.history = rows;
                // R14: the saved state keeps no titles, so an untitled open session takes its
                // history title; a title the agent sends later still replaces it.
                for session in &this.sessions {
                    session.update(cx, |s, cx| {
                        if s.agent != agent || s.model.title.is_some() {
                            return;
                        }
                        let row = this.history.iter().find(|(_, row)| row.id == s.id);
                        if let Some((_, row)) = row {
                            s.model.title = row.title.clone();
                            cx.notify();
                        }
                    });
                }
                cx.notify();
            })
            .ok();
        }));
    }

    pub(crate) fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let panels = self.ext_host.read(cx).panels();
        let shown = panels
            .iter()
            .find(|p| self.sidebar_tab == SidebarTab::Panel(p.view.entity_id()));
        let content = match self.sidebar_tab {
            SidebarTab::Panel(_) => div()
                .size_full()
                .children(shown.map(|p| p.view.clone()))
                .into_any_element(),
            SidebarTab::Sessions => self.render_sessions(cx).into_any_element(),
            SidebarTab::History => self.render_history(cx).into_any_element(),
            SidebarTab::Files => self.files.clone().into_any_element(),
            SidebarTab::Search => self.search.clone().into_any_element(),
            SidebarTab::Agents => div()
                .size_full()
                .child(self.agents.clone())
                .into_any_element(),
            SidebarTab::Extensions => div()
                .size_full()
                .children(self.extensions.clone())
                .into_any_element(),
        };
        let built_in = TABS.map(|(tab, id, label)| (tab, ElementId::from(id), label.into()));
        let extension = panels.iter().enumerate().map(|(ix, p)| {
            let title = clamp(&p.title, PANEL_TITLE_CHARS);
            let tab = SidebarTab::Panel(p.view.entity_id());
            (
                tab,
                ElementId::from(("tab-panel", ix)),
                SharedString::from(title),
            )
        });
        let ring = theme.ring;
        let mut tabs: Vec<_> = built_in
            .into_iter()
            .chain(extension)
            .map(|(tab, id, label)| {
                aurora::tab(self.sidebar_tab == tab, cx)
                    .id(id)
                    .role(Role::Tab)
                    .aria_label(label.clone())
                    .aria_selected(self.sidebar_tab == tab)
                    .focus_visible(move |el| el.border_color(ring))
                    // Focusable, and Enter or Space clicks it.
                    .tab_index(0)
                    .h(px(26.))
                    .px(px(3.))
                    .rounded(px(9.))
                    .text_size(px(12.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(label)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.sidebar_tab = tab;
                        if tab == SidebarTab::Search {
                            let focus = this.search.read(cx).query_focus(cx);
                            window.focus(&focus, cx);
                        }
                        cx.notify();
                    }))
                    // Registers the id so tests can find and click it (a Button does it itself).
                    .test_support()
            })
            .collect();
        // The built-in tabs share one line: wrapped one by one, the last breaks off even where
        // it fits. The extensions' tabs wrap after them.
        let panel_tabs = tabs.split_off(TABS.len());
        v_flex()
            .size_full()
            .py_2()
            .text_color(theme.sidebar_foreground)
            .child(
                h_flex()
                    .mx_1()
                    .p(px(2.))
                    .gap(px(2.))
                    .flex_wrap()
                    .rounded(px(12.))
                    .border_1()
                    .map(|el| {
                        if theme.is_dark() {
                            el.bg(aurora::vidro()).border_color(aurora::borda())
                        } else {
                            el.border_color(theme.border)
                        }
                    })
                    .child(h_flex().gap(px(2.)).children(tabs))
                    .children(panel_tabs),
            )
            .child(v_flex().flex_1().min_h_0().child(content))
    }

    fn render_sessions(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        v_flex().size_full().child(heading("Open", cx)).children(
            self.sessions
                .iter()
                .enumerate()
                .map(|(ix, session)| self.open_row(ix, session, cx)),
        )
    }

    fn render_history(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let rows = self.history_rows().len();
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .pr_3()
                    .justify_between()
                    .child(heading(&format!("History · {rows}"), cx))
                    .child(
                        Switch::new("all-projects")
                            .xsmall()
                            .label("All projects")
                            .checked(self.all_projects)
                            .on_click(cx.listener(|this, all: &bool, _, cx| {
                                this.all_projects = *all;
                                cx.notify();
                            })),
                    ),
            )
            // Only the visible rows render: an agent can hold thousands of sessions.
            .when(self.default_agent().is_none(), |el| {
                el.child(
                    div()
                        .id("history-hint")
                        .p_2()
                        .text_sm()
                        .text_color(cx.theme().muted_foreground)
                        .child("Choose an AI in Settings to see its history.")
                        .test_support(),
                )
            })
            .when(self.default_agent().is_some(), |el| {
                el.child(
                    uniform_list(
                        "history",
                        rows,
                        cx.processor(|this, range: Range<usize>, _, cx| {
                            let rows = this.history_rows();
                            range
                                .map(|ix| this.history_row(ix, rows[ix], cx))
                                .collect::<Vec<_>>()
                        }),
                    )
                    .flex_1(),
                )
            })
    }

    fn open_row(
        &self,
        ix: usize,
        session: &Entity<ChatSession>,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let s = session.read(cx);
        let dot = status_color(s.status(), theme);
        let label = matches!(s.status(), TurnState::NeedsYou).then_some("Needs you");
        let title = s.model.title.clone().unwrap_or("New session".into());
        let active = self.panes[self.focused_pane].session() == Some(session);
        let (open, side, end) = (session.clone(), session.clone(), session.clone());
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::Open(session.clone()),
        };
        let (agent, id, cwd) = (s.agent.clone(), s.id.clone(), s.cwd.clone());
        let forkable = self.can_fork(&agent, cx);
        let fork_title = s.model.title.clone();
        row(active, cx)
            .child(
                h_flex()
                    .id(("open", ix))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_2()
                    .child(
                        div()
                            .flex_none()
                            .size(px(8.))
                            .rounded_full()
                            .bg(dot)
                            .shadow(vec![
                                BoxShadow::new(px(0.), px(0.), dot.alpha(0.7)).blur_radius(px(4.)),
                            ]),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .children(label.map(|l| div().text_xs().text_color(dot).child(l)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show(open.clone(), false, window, cx)
                    }))
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .when(self.panes.len() < MAX_PANES, |el| {
                el.child(side_button(("open-side", ix)).on_click(cx.listener(
                    move |this, _, window, cx| this.show(side.clone(), true, window, cx),
                )))
            })
            .when(forkable, |el| {
                el.child(fork_button(("open-fork", ix)).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.fork(
                            agent.clone(),
                            id.clone(),
                            fork_title.clone(),
                            cwd.clone(),
                            window,
                            cx,
                        )
                    },
                )))
            })
            .child(
                Button::new(("open-close", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("End session")
                    .accessibility_label("End session")
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.end_session(&end, window, cx)),
                    ),
            )
    }

    fn history_row(
        &self,
        ix: usize,
        summary: &SessionSummary,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let title = summary.title.clone().unwrap_or_else(|| summary.id.clone());
        let (open, side, source) = (summary.clone(), summary.clone(), summary.clone());
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::History(summary.clone()),
        };
        let forkable = self
            .default_agent()
            .is_some_and(|agent| self.can_fork(&agent, cx));
        row(false, cx)
            .child(
                div()
                    .id(("history", ix))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(title)
                    .on_click(
                        cx.listener(move |this, _, window, cx| {
                            this.resume(&open, false, window, cx)
                        }),
                    )
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .when(self.panes.len() < MAX_PANES, |el| {
                el.child(side_button(("history-side", ix)).on_click(
                    cx.listener(move |this, _, window, cx| this.resume(&side, true, window, cx)),
                ))
            })
            .when(forkable, |el| {
                el.child(fork_button(("history-fork", ix)).on_click(cx.listener(
                    move |this, _, window, cx| {
                        let (id, title, cwd) =
                            (source.id.clone(), source.title.clone(), source.cwd.clone());
                        let Some(agent) = this.default_agent() else {
                            return;
                        };
                        this.fork(agent, id, title, cwd, window, cx)
                    },
                )))
            })
    }
}

fn heading(text: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let color = if theme.is_dark() {
        aurora::nevoa_2()
    } else {
        theme.muted_foreground
    };
    div()
        .px_3()
        .pt_3()
        .pb_1()
        .text_size(px(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(color)
        .child(text.to_uppercase())
}

pub(crate) fn row(active: bool, cx: &App) -> Div {
    aurora::tab(active, cx)
        .w_full()
        .h(px(ROW_HEIGHT))
        .pl_3()
        .pr_1()
        .gap_1()
        .rounded(px(10.))
        .text_sm()
}

fn side_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(IconName::PanelRightOpen)
        .tooltip("Open to the side")
}

fn fork_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(IconName::Copy)
        .tooltip("Fork")
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use gpui_kit::component::ActiveTheme as _;

    use super::{TurnState, status_color};
    use crate::workspace::tests::{history_settings, load_history, open};
    use crate::{app, aurora};

    #[gpui_kit::test]
    fn working_dot_is_boreal(cx: &mut TestAppContext) {
        cx.update(|cx| {
            app::init(&Settings::default(), cx);
            let color = status_color(&TurnState::Working, cx.theme());
            assert_eq!(aurora::hex(color), aurora::hex(aurora::boreal()));
        });
    }

    /// The History tab holds the history rows and the "All projects" switch; the Sessions tab
    /// lists only the open sessions.
    #[gpui_kit::test]
    fn history_has_its_own_tab(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(3));
        load_history(&workspace, 3, cx);
        let shown = |id: ElementId, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find(id).is_some()
            })
        };
        let row = || ElementId::from(("history-side", 0usize));
        assert!(!shown(row(), cx), "no history rows");
        assert!(!shown("all-projects".into(), cx));
        let tab = cx.update(|window, _| window.find("tab-history"));
        assert_eq!(tab.label(), Some("History"));

        cx.update(|window, cx| window.click("tab-history", cx));
        assert!(shown(row(), cx));
        assert!(shown("all-projects".into(), cx));
    }

    #[gpui_kit::test]
    fn history_hint_without_default(cx: &mut TestAppContext) {
        let settings = Settings {
            default_agent: None,
            ..Default::default()
        };
        let (_workspace, cx) = open(cx, settings);
        cx.update(|window, cx| window.click("tab-history", cx));
        let hint = cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find("history-hint")
        });
        assert!(hint.is_some());
    }
}
