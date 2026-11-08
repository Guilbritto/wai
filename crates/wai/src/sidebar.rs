//! The sidebar: the project, its open sessions with their status, and the agent's history.

use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::SessionSummary;

use crate::agents::AgentStatus;
use crate::app::NewSession;
use crate::chat::{ChatSession, TurnState};
use crate::workspace::{MAX_PANES, Workspace, folder_name};

const ROW_HEIGHT: f32 = 28.;

/// R3: `/tmp` and `/private/tmp` are one folder; a path that does not exist stays as given.
pub fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
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
        let agent = self.settings.default_agent.clone();
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
        let rows = self.history_rows().len();
        v_flex()
            .debug_selector(|| "sidebar".into())
            .size_full()
            .pb_2()
            .bg(theme.sidebar)
            .text_color(theme.sidebar_foreground)
            .child(
                h_flex()
                    .px_3()
                    .pt_3()
                    .pb_2()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(folder_name(&self.project)),
                    )
                    .child(
                        Button::new("sidebar-new")
                            .ghost()
                            .xsmall()
                            .icon(IconName::Plus)
                            .tooltip("New session")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.new_session(&NewSession, window, cx)
                            })),
                    ),
            )
            .child(heading("Open", cx))
            .children(
                self.sessions
                    .iter()
                    .enumerate()
                    .map(|(ix, session)| self.open_row(ix, session, cx)),
            )
            .child(
                h_flex()
                    .justify_between()
                    .pr_3()
                    .child(heading("History", cx))
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
            .child(
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
    }

    fn open_row(
        &self,
        ix: usize,
        session: &Entity<ChatSession>,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let s = session.read(cx);
        let (dot, label) = match s.status() {
            TurnState::Working => (theme.blue, None),
            TurnState::NeedsYou => (theme.warning, Some("Needs you")),
            TurnState::Idle => (theme.muted_foreground, None),
            TurnState::Error(_) => (theme.danger, None),
        };
        let title = s.model.title.clone().unwrap_or("New session".into());
        let active = self.panes[self.focused_pane].session() == Some(session);
        let (open, side) = (session.clone(), session.clone());
        let (id, cwd, forkable) = (s.id.clone(), s.cwd.clone(), self.can_fork(cx));
        let fork_title = s.model.title.clone();
        row(active, cx)
            .child(
                h_flex()
                    .id(("open", ix))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_2()
                    .child(div().flex_none().size(px(8.)).rounded_full().bg(dot))
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .children(label.map(|l| div().text_xs().text_color(dot).child(l)))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show(open.clone(), false, window, cx)
                    })),
            )
            .when(self.panes.len() < MAX_PANES, |el| {
                el.child(side_button(("open-side", ix)).on_click(cx.listener(
                    move |this, _, window, cx| this.show(side.clone(), true, window, cx),
                )))
            })
            .when(forkable, |el| {
                el.child(fork_button(("open-fork", ix)).on_click(cx.listener(
                    move |this, _, window, cx| {
                        this.fork(id.clone(), fork_title.clone(), cwd.clone(), window, cx)
                    },
                )))
            })
    }

    fn history_row(
        &self,
        ix: usize,
        summary: &SessionSummary,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let title = summary.title.clone().unwrap_or_else(|| summary.id.clone());
        let (open, side, source) = (summary.clone(), summary.clone(), summary.clone());
        let forkable = self.can_fork(cx);
        row(false, cx)
            .child(
                div()
                    .id(("history", ix))
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(title)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.resume(&open, false, window, cx)
                    })),
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
                        this.fork(id, title, cwd, window, cx)
                    },
                )))
            })
    }
}

fn heading(text: &'static str, cx: &App) -> impl IntoElement {
    div()
        .px_3()
        .pt_3()
        .pb_1()
        .text_xs()
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

fn row(active: bool, cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .h(px(ROW_HEIGHT))
        .pl_3()
        .pr_1()
        .gap_1()
        .text_sm()
        .cursor_pointer()
        .hover(|el| el.bg(theme.sidebar_accent))
        .when(active, |el| {
            el.bg(theme.sidebar_accent)
                .text_color(theme.sidebar_accent_foreground)
        })
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
