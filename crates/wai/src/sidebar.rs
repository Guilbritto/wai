//! The sidebar: the activity bar's panels (the open sessions with their status and the agent's
//! history, the project's files, search, the agents, then each running extension's panels).

use std::ops::Range;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Escape, Input, InputEvent, InputState};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::SessionSummary;
use wai_ext::panel::clamp;
use wai_settings::SessionGroup;
pub use wai_settings::canonical;

use crate::agents::AgentStatus;
use crate::app::NewSession;
use crate::aurora;
use crate::chat::{ChatItem, ChatModel, ChatSession, DraftView, TurnState};
use crate::settings_view::Section;
use crate::workspace::{Dragged, DraggedSession, MAX_PANES, Renamed, Workspace};

pub(crate) const ROW_HEIGHT: f32 = 28.;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SidebarTab {
    #[default]
    Sessions,
    Files,
    Search,
    Agents,
    /// An extension's panel, by its view. When the panel goes, the sessions show again.
    Panel(EntityId),
}

/// A panel's tab title, one line: the manifest's words.
const PANEL_TITLE_CHARS: usize = 24;

/// Each tab with its element id and label.
const TABS: [(SidebarTab, &str, &str, Lucide); 4] = [
    (
        SidebarTab::Sessions,
        "tab-sessions",
        "Sessions",
        Lucide::MessageSquare,
    ),
    (SidebarTab::Files, "tab-files", "Files", Lucide::Folder),
    (SidebarTab::Search, "tab-search", "Search", Lucide::Search),
    (SidebarTab::Agents, "tab-agents", "Agents", Lucide::Sparkles),
];

/// An extension panel's mark on the rail: the first letters of its first two words, else of its
/// one word.
pub(crate) fn initials(title: &str) -> String {
    let mut words = title.split_whitespace();
    let letters: String = match (words.next(), words.next()) {
        (Some(a), Some(b)) => a.chars().take(1).chain(b.chars().take(1)).collect(),
        (Some(a), None) => a.chars().take(2).collect(),
        _ => return "?".into(),
    };
    letters.to_uppercase()
}

/// The status dot's colour, shared by the open list and the agent cards.
pub(crate) fn status_color(state: &TurnState, theme: &gpui_kit::component::Theme) -> Hsla {
    match state {
        TurnState::Working => theme.cyan,
        TurnState::NeedsYou => theme.warning,
        TurnState::Idle => theme.muted_foreground,
        TurnState::Error(_) => theme.danger,
    }
}

/// What a session row says on its second line.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Detail {
    Doing(String),
    Permission(String),
    Question(String),
    Done,
    Idle,
    Exited,
    Draft,
}

pub(crate) fn detail(m: &ChatModel) -> Detail {
    match &m.state {
        TurnState::Working => Detail::Doing(m.activity().unwrap_or_else(|| "Thinking…".into())),
        TurnState::NeedsYou => match m.waiting.first() {
            Some(ChatItem::Permission { request, .. }) => Detail::Permission(request.title.clone()),
            Some(ChatItem::Question { request, .. }) => Detail::Question(request.message.clone()),
            _ => Detail::Idle,
        },
        TurnState::Idle if m.done => Detail::Done,
        TurnState::Idle => Detail::Idle,
        TurnState::Error(_) => Detail::Exited,
    }
}

pub(crate) fn live_subagents(m: &ChatModel) -> usize {
    use wai_agent::SubagentState::{NeedsYou, Running};
    m.subagents
        .iter()
        .filter(|a| matches!(a.state, Running | NeedsYou))
        .count()
}

/// Whole units, rounded down: `45s`, `3m`, `2h`, `5d`.
pub(crate) fn short_age(d: Duration) -> String {
    let s = d.as_secs();
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86_400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86_400),
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
                this.sync_drafts(cx);
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

    /// The selected panel; the activity bar (`render_rail`) switches them.
    pub(crate) fn render_sidebar(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
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
            SidebarTab::Files => self.files.clone().into_any_element(),
            SidebarTab::Search => self.search.clone().into_any_element(),
            SidebarTab::Agents => self.render_agents(cx).into_any_element(),
        };
        v_flex()
            .size_full()
            .py_2()
            .text_color(cx.theme().sidebar_foreground)
            .child(v_flex().flex_1().min_h_0().child(content))
    }

    /// The activity bar: one icon per panel, the sessions' badge counting those that need the
    /// user, then Extensions and Settings at the foot.
    pub(crate) fn render_rail(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let panels = self.ext_host.read(cx).panels();
        let needs_you = (self.sessions.iter())
            .filter(|s| matches!(s.read(cx).status(), TurnState::NeedsYou))
            .count();
        let ring = theme.ring;
        let (idle, selected) = (theme.muted_foreground, theme.foreground);
        let (fill, border, hover) = (
            theme.list_active,
            theme.list_active_border,
            theme.list_hover,
        );
        let item = |id: ElementId, label: SharedString, on: bool| {
            div()
                .id(id)
                .relative()
                .flex_none()
                .size(px(32.))
                .rounded(px(9.))
                .flex()
                .items_center()
                .justify_center()
                .cursor_pointer()
                .border_1()
                .border_color(gpui_kit::transparent_black())
                .text_size(px(10.5))
                .font_weight(FontWeight::EXTRA_BOLD)
                .aria_label(label.clone())
                .focus_visible(move |el| el.border_color(ring))
                // Focusable, and Enter or Space clicks it.
                .tab_index(0)
                .tooltip({
                    let label = label.clone();
                    move |window, cx| Tooltip::new(label.clone()).build(window, cx)
                })
                .map(|el| match on {
                    true => el.bg(fill).border_color(border).text_color(selected).child(
                        div()
                            .absolute()
                            .left(px(-7.))
                            .top(px(7.))
                            .bottom(px(7.))
                            .w(px(2.))
                            .rounded(px(2.))
                            .bg(aurora::lilas()),
                    ),
                    false => el.text_color(idle).hover(move |el| el.bg(hover)),
                })
        };
        let icon = |name: Lucide| Icon::new(name).size(px(17.));
        let tab_item = |tab: SidebarTab, id: ElementId, label: SharedString, body: AnyElement| {
            let on = self.sidebar_tab == tab;
            item(id, label, on)
                .role(Role::Tab)
                .aria_selected(on)
                .child(body)
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.sidebar_tab = tab;
                    if tab == SidebarTab::Search {
                        let focus = this.search.read(cx).query_focus(cx);
                        window.focus(&focus, cx);
                    }
                    // A double click on Sessions starts a new session.
                    if tab == SidebarTab::Sessions && event.click_count() > 1 {
                        this.new_session(&NewSession, window, cx);
                    }
                    cx.notify();
                }))
                // Registers the id so tests can find and click it (a Button does it itself).
                .test_support()
        };
        let built_in = TABS
            .into_iter()
            // On the right, Agents has no icon.
            .filter(|(tab, ..)| !(self.agents_right && *tab == SidebarTab::Agents))
            .map(|(tab, id, label, name)| {
                let mut el = tab_item(tab, id.into(), label.into(), icon(name).into_any_element());
                if tab == SidebarTab::Sessions && needs_you > 0 {
                    let count = needs_you.to_string();
                    el = el.child(
                        div()
                            .id("sessions-badge")
                            .aria_label(count.clone())
                            .absolute()
                            .top(px(-3.))
                            .right(px(-4.))
                            .min_w(px(14.))
                            .h(px(14.))
                            .px(px(3.))
                            .rounded(px(7.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .bg(theme.warning)
                            .text_color(aurora::noite())
                            .text_size(px(9.5))
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .child(count)
                            .test_support(),
                    );
                }
                el
            });
        let extension = panels.iter().enumerate().map(|(ix, p)| {
            let title = clamp(&p.title, PANEL_TITLE_CHARS);
            let mark = div()
                .size(px(20.))
                .rounded(px(6.))
                .border(px(1.5))
                .border_color(
                    if self.sidebar_tab == SidebarTab::Panel(p.view.entity_id()) {
                        selected
                    } else {
                        idle
                    },
                )
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(9.))
                .child(initials(&title));
            tab_item(
                SidebarTab::Panel(p.view.entity_id()),
                ("tab-panel", ix).into(),
                title.into(),
                mark.into_any_element(),
            )
        });
        let foot = |id: &'static str, label: &'static str, name: Lucide, open: Option<Section>| {
            item(id.into(), label.into(), false)
                .role(Role::Button)
                .child(icon(name))
                .on_click(
                    cx.listener(move |this, _: &ClickEvent, window, cx| match open {
                        Some(section) => this.open_settings(section, None, window, cx),
                        None => {
                            this.show_extensions(window, cx);
                        }
                    }),
                )
                .test_support()
        };
        v_flex()
            .debug_selector(|| "rail".into())
            .flex_none()
            .w(px(44.))
            .h_full()
            .items_center()
            .gap(px(6.))
            .py(px(8.))
            .border_r_1()
            .border_color(theme.border)
            .children(built_in)
            .children(extension)
            .child(div().flex_1())
            .child(foot(
                "sidebar-extensions",
                "Extensions",
                Lucide::Puzzle,
                None,
            ))
            .child(foot(
                "topbar-settings",
                "Settings",
                Lucide::Settings,
                Some(Section::Ai),
            ))
    }

    /// The open sessions outside any group, then each group under its header. A row keeps its
    /// index in `sessions`, whichever group draws it.
    fn render_sessions(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let rows = |group: Option<usize>| {
            let sessions = self.sessions.iter().enumerate();
            sessions
                .filter(move |(_, session)| self.group_of(&session.read(cx).id) == group)
                .map(|(ix, session)| self.open_row(ix, session, cx).into_any_element())
        };
        let drafts = (self.drafts.iter().enumerate())
            .map(|(ix, draft)| self.draft_row(ix, draft, cx).into_any_element());
        let groups = self.groups.iter().enumerate().flat_map(|(ix, group)| {
            let header = self.group_header(ix, group, rows(Some(ix)).count(), cx);
            let rows = rows(Some(ix)).filter(|_| !group.collapsed);
            std::iter::once(header.into_any_element()).chain(rows)
        });
        v_flex()
            .size_full()
            .child(
                // A row dropped here leaves its group.
                v_flex()
                    .id("open-ungrouped")
                    // The lit border's room, so nothing moves when it lights.
                    .border_1()
                    .border_color(gpui_kit::transparent_black())
                    .drag_over(light)
                    .on_drop(cx.listener(|this, dragged: &DraggedSession, _, cx| {
                        this.group_session(dragged, None, cx)
                    }))
                    .child(
                        h_flex()
                            .pr_1()
                            .justify_between()
                            .child(heading("Open", cx))
                            .child(
                                Button::new("new-group")
                                    .ghost()
                                    .xsmall()
                                    .icon(IconName::Plus)
                                    .tooltip("New group")
                                    .accessibility_label("New group")
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.new_group(None, window, cx)
                                    })),
                            ),
                    )
                    .children(drafts)
                    .children(rows(None))
                    .test_support(),
            )
            .children(groups)
            // Until the History section replaces it, today's list sits under the open ones.
            .child(div().flex_1().min_h_0().child(self.render_history(cx)))
    }

    /// The group holding session `id`.
    fn group_of(&self, id: &str) -> Option<usize> {
        let holds = |group: &SessionGroup| group.sessions.iter().any(|s| s == id);
        self.groups.iter().position(holds)
    }

    /// Takes session `id` out of its group.
    pub(crate) fn ungroup(&mut self, id: &str) {
        for group in &mut self.groups {
            group.sessions.retain(|s| s != id);
        }
    }

    /// An open session dropped on group `to`'s header joins it; dropped on the ungrouped rows
    /// (`None`), it leaves its group.
    fn group_session(
        &mut self,
        dragged: &DraggedSession,
        to: Option<usize>,
        cx: &mut Context<Self>,
    ) {
        if let Dragged::Open(session) = &dragged.source {
            let id = session.read(cx).id.clone();
            self.move_to_group(&id, to, cx);
        }
    }

    /// An open session dropped on `target`'s row takes its place and joins its group: from above
    /// it lands after it, from below before it. The Agents cards and the saved state follow.
    fn reorder(
        &mut self,
        dragged: &DraggedSession,
        target: &Entity<ChatSession>,
        cx: &mut Context<Self>,
    ) {
        let Dragged::Open(session) = &dragged.source else {
            return;
        };
        let at = |s: &Entity<ChatSession>| self.sessions.iter().position(|x| x == s);
        let (Some(from), Some(to)) = (at(session), at(target)) else {
            return;
        };
        if from == to {
            return;
        }
        let moved = self.sessions.remove(from);
        self.sessions.insert(to, moved);
        self.sync_agents(cx);
        let (id, group) = (
            session.read(cx).id.clone(),
            self.group_of(&target.read(cx).id),
        );
        self.move_to_group(&id, group, cx);
    }

    /// Session `id` joins group `to`, leaving the one it was in; `None` only takes it out.
    fn move_to_group(&mut self, id: &str, to: Option<usize>, cx: &mut Context<Self>) {
        self.ungroup(id);
        if let Some(group) = to.and_then(|ix| self.groups.get_mut(ix)) {
            group.sessions.push(id.to_string());
        }
        self.save_state(cx);
        cx.notify();
    }

    /// A group named "New group" at the end of the list, holding session `with` if given, its
    /// name being typed.
    fn new_group(&mut self, with: Option<&str>, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = with {
            self.ungroup(id);
        }
        self.groups.push(SessionGroup {
            name: "New group".into(),
            collapsed: false,
            sessions: with.map(str::to_string).into_iter().collect(),
        });
        self.save_state(cx);
        self.rename(Renamed::Group(self.groups.len() - 1), window, cx);
    }

    /// Deletes group `ix`; its sessions stay open, in no group.
    fn delete_group(&mut self, ix: usize, cx: &mut Context<Self>) {
        self.renaming = None;
        self.groups.remove(ix);
        self.save_state(cx);
        cx.notify();
    }

    /// Turns a session's title or a group's name into an input holding it: Enter, or the focus
    /// leaving, renames; Esc does not.
    fn rename(&mut self, what: Renamed, window: &mut Window, cx: &mut Context<Self>) {
        let value = match &what {
            Renamed::Session(session) => {
                let title = session.read(cx).model.shown_title();
                title.unwrap_or_default().to_string()
            }
            Renamed::Group(ix) => self.groups[*ix].name.clone(),
        };
        let input = cx.new(|cx| InputState::new(window, cx).default_value(value));
        input.update(cx, |input, cx| {
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        let events = cx.subscribe_in(&input, window, |this, _, event, window, cx| match event {
            InputEvent::PressEnter { .. } => this.end_rename(true, window, cx),
            InputEvent::Blur => this.commit_rename(cx),
            _ => {}
        });
        self.renaming = Some((what, input, events));
        cx.notify();
    }

    /// Enter (`commit`) or Esc in the input: the focus goes back to the focused pane.
    fn end_rename(&mut self, commit: bool, window: &mut Window, cx: &mut Context<Self>) {
        match commit {
            true => self.commit_rename(cx),
            false => self.renaming = None,
        }
        self.focus_pane(self.focused_pane, window, cx);
    }

    /// A session takes the typed name; an empty one, or the agent's title unchanged, gives it
    /// the agent's title back. A group keeps its name when the typed one is empty.
    fn commit_rename(&mut self, cx: &mut Context<Self>) {
        let Some((what, input, _)) = self.renaming.take() else {
            return;
        };
        let name = input.read(cx).value().trim().to_string();
        match what {
            Renamed::Session(session) => session.update(cx, |s, cx| {
                let new = !name.is_empty() && s.model.title.as_ref() != Some(&name);
                s.rename(new.then_some(name), cx)
            }),
            Renamed::Group(ix) => {
                if let Some(group) = self.groups.get_mut(ix).filter(|_| !name.is_empty()) {
                    group.name = name;
                }
            }
        }
        self.save_state(cx);
        cx.notify();
    }

    /// The input of `what` while it is renamed.
    fn renaming(&self, what: &Renamed) -> Option<Input> {
        let (renamed, input, _) = self.renaming.as_ref()?;
        (renamed == what).then(|| Input::new(input).xsmall().flex_1())
    }

    /// A group's header: its chevron, name and session count; its menu renames or deletes it.
    /// An open session dropped on it joins the group.
    fn group_header(
        &self,
        ix: usize,
        group: &SessionGroup,
        count: usize,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let chevron = match group.collapsed {
            true => IconName::ChevronRight,
            false => IconName::ChevronDown,
        };
        let input = self.renaming(&Renamed::Group(ix));
        let name = group.name.clone();
        let workspace = cx.entity().downgrade();
        row(false, cx)
            .id(("group-row", ix))
            .pl_1()
            .child(
                Button::new(("group-toggle", ix))
                    .ghost()
                    .xsmall()
                    .icon(chevron)
                    .accessibility_label(match group.collapsed {
                        true => "Expand group",
                        false => "Collapse group",
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.groups[ix].collapsed = !this.groups[ix].collapsed;
                        this.save_state(cx);
                        cx.notify();
                    })),
            )
            .child(
                h_flex()
                    .id(("group", ix))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_2()
                    .rounded(px(10.))
                    .border_1()
                    .border_color(gpui_kit::transparent_black())
                    .aria_label(format!("{name}, {count}"))
                    .drag_over(light)
                    .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                        this.group_session(dragged, Some(ix), cx)
                    }))
                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                        this.end_rename(false, window, cx)
                    }))
                    .map(|el| match input {
                        Some(input) => el.child(input),
                        None => el
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(name),
                            )
                            .child(div().text_xs().child(count.to_string()))
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                if event.click_count() > 1 {
                                    this.rename(Renamed::Group(ix), window, cx)
                                }
                            })),
                    })
                    // Registers the id so tests can find it and drop on it.
                    .test_support(),
            )
            .context_menu(move |menu, _, _| {
                let (rename, delete) = (workspace.clone(), workspace.clone());
                menu.item(
                    PopupMenuItem::new("Rename group").on_click(move |_, window, cx| {
                        rename
                            .update(cx, |w, cx| w.rename(Renamed::Group(ix), window, cx))
                            .ok();
                    }),
                )
                .item(
                    PopupMenuItem::new("Delete group").on_click(move |_, _, cx| {
                        delete.update(cx, |w, cx| w.delete_group(ix, cx)).ok();
                    }),
                )
            })
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
        let title = s.model.shown_title().unwrap_or("New session").to_string();
        let active = self.panes[self.focused_pane].session() == Some(session);
        let (open, side, end) = (session.clone(), session.clone(), session.clone());
        let menu_session = session.clone();
        let workspace = cx.entity().downgrade();
        let input = self.renaming(&Renamed::Session(session.clone()));
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::Open(session.clone()),
        };
        let (over, target) = (session.clone(), session.clone());
        row(active, cx)
            .id(("open-row", ix))
            // Another open row dropped here takes this one's place.
            .drag_over(
                move |style, dragged: &DraggedSession, _, cx| match &dragged.source {
                    Dragged::Open(s) if *s != over => aurora::drop_target(style, cx),
                    _ => style,
                },
            )
            .on_drop(cx.listener(move |this, dragged: &DraggedSession, _, cx| {
                this.reorder(dragged, &target, cx)
            }))
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
                    .aria_label(title.clone())
                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                        this.end_rename(false, window, cx)
                    }))
                    .map(|el| match input {
                        Some(input) => el.child(input),
                        // A second click renames.
                        None => el
                            .child(div().flex_1().min_w_0().truncate().child(title))
                            .children(label.map(|l| div().text_xs().text_color(dot).child(l)))
                            .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                                match event.click_count() {
                                    1 => this.show(open.clone(), false, window, cx),
                                    _ => this.rename(Renamed::Session(open.clone()), window, cx),
                                }
                            }))
                            .on_drag(dragged, DraggedSession::preview),
                    })
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .when(self.panes.len() < MAX_PANES, |el| {
                el.child(side_button(("open-side", ix)).on_click(cx.listener(
                    move |this, _, window, cx| this.show(side.clone(), true, window, cx),
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
                        cx.listener(move |this, _, window, cx| {
                            this.close_session(&end, window, cx)
                        }),
                    ),
            )
            .context_menu(move |menu, _, cx| {
                let Some(w) = workspace.upgrade() else {
                    return menu;
                };
                w.read(cx).session_menu(menu, &w, &menu_session, cx)
            })
    }

    /// A draft's row: a hollow dot, "New session" and the Draft tag; × discards it.
    fn draft_row(
        &self,
        ix: usize,
        draft: &Entity<DraftView>,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let active = self.panes[self.focused_pane].draft() == Some(draft);
        let (open, end) = (draft.clone(), draft.clone());
        let starting = draft.read(cx).starting.as_ref().map(|chat| chat.read(cx));
        let title = starting.and_then(|s| s.model.shown_title());
        let title = title.unwrap_or("New session").to_string();
        let dot = starting.map(|s| status_color(s.status(), theme));
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::Draft(draft.clone()),
        };
        row(active, cx)
            .id(("draft-row", ix))
            .child(
                h_flex()
                    .id(("draft", ix))
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .gap_2()
                    .child(match dot {
                        Some(dot) => div()
                            .flex_none()
                            .size(px(8.))
                            .rounded_full()
                            .bg(dot)
                            .shadow(vec![
                                BoxShadow::new(px(0.), px(0.), dot.alpha(0.7)).blur_radius(px(4.)),
                            ]),
                        None => div()
                            .flex_none()
                            .size(px(8.))
                            .rounded_full()
                            .border_1()
                            .border_dashed()
                            .border_color(aurora::nevoa()),
                    })
                    .aria_label(title.clone())
                    .child(div().flex_1().min_w_0().truncate().child(title))
                    .children(dot.is_none().then(|| {
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child("Draft")
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_draft(open.clone(), false, window, cx)
                    }))
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .child(
                Button::new(("draft-close", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Discard draft")
                    .accessibility_label("Discard draft")
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.close_draft(&end, window, cx)),
                    ),
            )
    }

    /// An open session's menu: rename, fork, and moving it between groups.
    fn session_menu(
        &self,
        menu: PopupMenu,
        workspace: &Entity<Self>,
        session: &Entity<ChatSession>,
        cx: &App,
    ) -> PopupMenu {
        let s = session.read(cx);
        let (agent, id, cwd) = (s.agent.clone(), s.id.clone(), s.cwd.clone());
        let agent_id = agent.clone();
        let title = s.model.shown_title().map(str::to_string);
        let current = self.group_of(&id);
        let renamed = session.clone();
        let menu = menu
            .item(item(workspace, "Rename", move |w, window, cx| {
                w.rename(Renamed::Session(renamed.clone()), window, cx)
            }))
            .when(self.can_fork(&agent, cx), |menu| {
                let (id, cwd) = (id.clone(), cwd.clone());
                menu.item(item(workspace, "Fork", move |w, window, cx| {
                    let (agent, id, title) = (agent.clone(), id.clone(), title.clone());
                    w.fork(agent, id, title, cwd.clone(), window, cx)
                }))
            })
            .when(agent_id == "claude", |menu| {
                let session = session.clone();
                menu.item(item(
                    workspace,
                    "Continue in terminal",
                    move |w, window, cx| w.continue_in_terminal(&session, window, cx),
                ))
            })
            .separator();
        let groups = self.groups.iter().enumerate();
        let menu = groups
            .filter(|(ix, _)| Some(*ix) != current)
            .fold(menu, |menu, (ix, group)| {
                let id = id.clone();
                let label = format!("Move to {}", group.name);
                menu.item(item(workspace, label, move |w, _, cx| {
                    w.move_to_group(&id, Some(ix), cx)
                }))
            });
        let new = id.clone();
        menu.item(item(
            workspace,
            "New group with this session",
            move |w, window, cx| w.new_group(Some(&new), window, cx),
        ))
        .when(current.is_some(), |menu| {
            menu.item(item(workspace, "Remove from group", move |w, _, cx| {
                w.move_to_group(&id, None, cx)
            }))
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
        let workspace = cx.entity().downgrade();
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::History(summary.clone()),
        };
        let forkable = self
            .default_agent()
            .is_some_and(|agent| self.can_fork(&agent, cx));
        row(false, cx)
            .id(("history-row", ix))
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
            .context_menu(move |menu, _, _| {
                let (workspace, source) = (workspace.clone(), source.clone());
                menu.when(forkable, |menu| {
                    menu.item(PopupMenuItem::new("Fork").on_click(move |_, window, cx| {
                        let (id, title, cwd) =
                            (source.id.clone(), source.title.clone(), source.cwd.clone());
                        workspace
                            .update(cx, |w, cx| {
                                let Some(agent) = w.default_agent() else {
                                    return;
                                };
                                w.fork(agent, id, title, cwd, window, cx)
                            })
                            .ok();
                    }))
                })
            })
    }
}

/// A panel's title row (the mockup's `.ptitle`); the caller appends its buttons. GPUI has no
/// letter spacing, so the mockup's 0.07 em tracking is left out.
pub(crate) fn panel_title(label: &str, cx: &App) -> Div {
    h_flex()
        .items_center()
        .gap_1()
        .px(px(6.))
        .pt(px(4.))
        .pb(px(6.))
        .text_size(px(11.))
        .font_weight(FontWeight::BOLD)
        .text_color(cx.theme().muted_foreground)
        .child(div().flex_1().child(label.to_uppercase()))
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

/// A group's header, or the rows outside any group, while an open session is dragged over it:
/// lit as a pane is.
fn light(
    style: StyleRefinement,
    dragged: &DraggedSession,
    _: &mut Window,
    cx: &mut App,
) -> StyleRefinement {
    match dragged.source {
        Dragged::Open(_) => aurora::drop_target(style, cx),
        _ => style,
    }
}

/// A menu item that runs `f` on the workspace.
fn item(
    workspace: &Entity<Workspace>,
    label: impl Into<SharedString>,
    f: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> PopupMenuItem {
    let workspace = workspace.downgrade();
    PopupMenuItem::new(label).on_click(move |_, window, cx| {
        workspace.update(cx, |w, cx| f(w, window, cx)).ok();
    })
}

fn side_button(id: impl Into<ElementId>) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(IconName::PanelRightOpen)
        .tooltip("Open to the side")
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    use gpui_kit::{
        ElementId, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
        VisualTestContext, point, px,
    };
    use wai_agent::SessionEvent;
    use wai_settings::{AppState, SessionGroup, Settings};

    use gpui_kit::component::ActiveTheme as _;

    use std::time::Duration;

    use super::{Detail, TurnState, detail, initials, short_age, status_color};
    use crate::agents::RegistryEvent;
    use crate::chat::ChatModel;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{
        click, drag_to_pane, fake_settings, history_settings, launch, load_history, menu_item,
        open, open_at, question, start_now, wait_until,
    };
    use crate::{app, aurora};
    use wai_agent::{ChoiceKind, PermissionChoice, PermissionRequest};

    #[test]
    fn short_ages_round_down() {
        let s = Duration::from_secs;
        assert_eq!(
            [
                s(0),
                s(59),
                s(60),
                s(3599),
                s(3600),
                s(86_399),
                s(86_400),
                s(200_000)
            ]
            .map(short_age),
            ["0s", "59s", "1m", "59m", "1h", "23h", "1d", "2d"]
        );
    }

    #[test]
    fn detail_says_what_the_session_does() {
        let mut m = ChatModel::default();
        assert_eq!(detail(&m), Detail::Idle);
        m.done = true;
        assert_eq!(detail(&m), Detail::Done);
        m.state = TurnState::Working;
        assert_eq!(detail(&m), Detail::Doing("Thinking…".into()));
        m.on_permission(PermissionRequest {
            id: 1,
            session: "s".into(),
            tool_call_id: "t".into(),
            title: "Write notes.md".into(),
            diff: None,
            options: vec![PermissionChoice {
                id: "y".into(),
                name: "Yes".into(),
                kind: ChoiceKind::AllowOnce,
            }],
            subagent: None,
        });
        assert_eq!(detail(&m), Detail::Permission("Write notes.md".into()));
        m.state = TurnState::Error("exit 1".into());
        assert_eq!(detail(&m), Detail::Exited);
    }

    #[gpui_kit::test]
    fn working_dot_is_boreal(cx: &mut TestAppContext) {
        cx.update(|cx| {
            app::init(&Settings::default(), cx);
            let color = status_color(&TurnState::Working, cx.theme());
            assert_eq!(aurora::hex(color), aurora::hex(aurora::boreal()));
        });
    }

    /// History is no tab: its rows and the "All projects" switch sit under the open list.
    #[gpui_kit::test]
    fn history_is_a_section_of_sessions(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(3));
        load_history(&workspace, 3, cx);
        let shown = |id: ElementId, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find(id).is_some()
            })
        };
        assert!(!shown("tab-history".into(), cx));
        assert!(shown("all-projects".into(), cx));
        assert!(shown(("history-side", 0usize).into(), cx));
        press("tab-files", cx);
        assert!(!shown("all-projects".into(), cx));
    }

    #[gpui_kit::test]
    fn initials_by_characters(_cx: &mut TestAppContext) {
        let words = ["My cards", "Routines", "Rotinas", "日本語", "", "a b c"];
        let got = words.map(initials);
        assert_eq!(got, ["MC", "RO", "RO", "日本", "?", "AB"]);
    }

    /// The Sessions icon's badge counts the sessions that wait for the user.
    #[gpui_kit::test]
    fn rail_badge_counts_needs_you(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-permission"));
        assert_eq!(label("sessions-badge", cx), None);
        start_now(&workspace, cx);
        let session = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        assert_eq!(label("sessions-badge", cx), None);
        cx.simulate_input("edit");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "needs you", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::NeedsYou)
        });
        cx.run_until_parked();
        assert_eq!(label("sessions-badge", cx).as_deref(), Some("1"));
    }

    /// The rail's foot opens Settings, on Extensions for the puzzle.
    #[gpui_kit::test]
    fn rail_foot_opens_settings(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        let shown = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| w.settings_view().is_some())
        };
        for id in ["topbar-settings", "sidebar-extensions"] {
            assert!(!shown(cx));
            press(id, cx);
            assert!(shown(cx), "{id}");
            cx.simulate_keystrokes("escape");
        }
    }

    #[gpui_kit::test]
    fn history_hint_without_default(cx: &mut TestAppContext) {
        let settings = Settings {
            default_agent: None,
            ..Default::default()
        };
        let (_workspace, cx) = open(cx, settings);
        let hint = cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find("history-hint")
        });
        assert!(hint.is_some());
    }

    /// A click on the Sessions tab only shows it; a double click opens a draft.
    #[gpui_kit::test]
    fn double_click_on_sessions_tab_opens_a_draft(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        press("tab-sessions", cx);
        assert_eq!(workspace.read_with(cx, |w, _| w.drafts.len()), 0);

        cx.update(|window, cx| {
            window.render_frame(cx);
            window.double_click("tab-sessions", cx);
        });
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.drafts.len(), 1);
            assert!(w.sessions.is_empty());
        });
    }

    #[gpui_kit::test]
    fn draft_row_shows_and_discards(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let dialog =
            |cx: &mut VisualTestContext| cx.update(|window, cx| window.has_active_dialog(cx));
        cx.simulate_keystrokes("secondary-n");
        assert_eq!(label(("draft", 0usize), cx).as_deref(), Some("New session"));

        start_now(&workspace, cx);
        workspace.read_with(cx, |w, _| assert!(w.panes[0].draft().is_none()));
        press(("draft", 0usize), cx);
        workspace.read_with(cx, |w, _| assert!(w.panes[0].draft().is_some()));

        press(("draft-close", 0usize), cx);
        workspace.read_with(cx, |w, _| assert!(w.drafts.is_empty()));
        assert!(!dialog(cx));

        cx.simulate_keystrokes("secondary-n");
        cx.simulate_input("x");
        press(("draft-close", 0usize), cx);
        cx.update(|window, _| window.find_by_label("Discard this draft?"));
        press("cancel", cx);
        workspace.read_with(cx, |w, _| assert_eq!(w.drafts.len(), 1));

        press(("draft-close", 0usize), cx);
        press("confirm", cx);
        workspace.read_with(cx, |w, _| {
            assert!(w.drafts.is_empty());
            assert!(w.panes[0].draft().is_none());
        });
    }

    #[gpui_kit::test]
    fn draft_drags_to_the_right_edge(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.simulate_keystrokes("secondary-n secondary-n");
        let from = center_of(("draft", 0usize), cx);
        drag_to_pane(from, 0, 0.9, cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2);
            assert_eq!(w.panes[1].draft(), Some(&w.drafts[0]));
            assert_eq!(w.focused_pane, 1);
        });
    }

    #[gpui_kit::test]
    fn session_rows_keep_their_indexes_next_to_drafts(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_now(&workspace, cx);
        cx.simulate_keystrokes("secondary-n");
        let title = session.read_with(cx, |s, _| s.model.shown_title().map(str::to_string));
        assert_eq!(
            label(("open", 0usize), cx),
            Some(title.unwrap_or("New session".into()))
        );
        assert!(label(("draft", 0usize), cx).is_some());
    }

    /// A workspace with `n` sessions open, resumed from the history: "Session 0", "Session 1"…
    fn with_sessions(
        n: usize,
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, &mut VisualTestContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(n));
        for row in load_history(&workspace, n, cx) {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.resume(&row, false, window, cx)));
        }
        cx.run_until_parked();
        (workspace, cx)
    }

    fn press(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    /// Types `text` over what the focused input holds (a rename selects it all), then `key`.
    fn type_name(text: &str, key: &str, cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("backspace");
        cx.simulate_input(text);
        cx.simulate_keystrokes(key);
        cx.run_until_parked();
    }

    fn label(id: impl Into<ElementId>, cx: &mut VisualTestContext) -> Option<String> {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            let found = window.try_find(id)?;
            Some(found.label().unwrap_or_default().to_string())
        })
    }

    fn center_of(id: impl Into<ElementId>, cx: &mut VisualTestContext) -> Point<Pixels> {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(id).bounds().center()
        })
    }

    /// Drags the element `from` onto the element `to`, each event in its own update.
    fn drag(from: impl Into<ElementId>, to: impl Into<ElementId>, cx: &mut VisualTestContext) {
        let (from, to) = (center_of(from, cx), center_of(to, cx));
        let none = Modifiers::none();
        cx.simulate_mouse_move(from, None, none);
        cx.simulate_mouse_down(from, MouseButton::Left, none);
        for at in [from + point(px(8.), px(0.)), to] {
            cx.simulate_mouse_move(at, MouseButton::Left, none);
            cx.update(|window, cx| window.render_frame(cx));
        }
        cx.simulate_mouse_up(to, MouseButton::Left, none);
        cx.run_until_parked();
    }

    fn open_row(ix: usize) -> ElementId {
        ("open", ix).into()
    }

    fn group(ix: usize) -> ElementId {
        ("group", ix).into()
    }

    /// Each group's name, collapsed state and sessions.
    fn groups(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<SessionGroup> {
        workspace.read_with(cx, |w, _| w.groups.clone())
    }

    fn named(name: &str, sessions: &[&str]) -> SessionGroup {
        SessionGroup {
            name: name.into(),
            collapsed: false,
            sessions: sessions.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// The user's name for a session shows on its Open row, its pane's header and its Agents
    /// card; Esc keeps the old one, an empty one gives the agent's title back, and a title the
    /// agent sends does not replace it.
    #[gpui_kit::test]
    fn rename_session(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(1, cx);
        let session = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        let name = |cx: &mut VisualTestContext| session.read_with(cx, |s, _| s.model.name.clone());
        assert_eq!(label(open_row(0), cx).as_deref(), Some("Session 0"));

        menu_item(open_row(0), "Rename", cx);
        type_name("Mine", "enter", cx);
        assert_eq!(name(cx).as_deref(), Some("Mine"));
        assert_eq!(label(open_row(0), cx).as_deref(), Some("Mine"));
        let header = cx.update(|window, cx| {
            window.render_frame(cx);
            let pane = window.within(ElementId::View(session.entity_id()));
            pane.find("session-title").label().map(str::to_string)
        });
        assert_eq!(header.as_deref(), Some("Mine"));
        press("tab-agents", cx);
        let card = label(("agent-card", 0usize), cx).unwrap();
        assert!(card.starts_with("Mine"), "{card:?}");
        press("tab-sessions", cx);

        // The agent's title is kept, under the user's name.
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());
        let (agent, id) = session.read_with(cx, |s, _| (s.agent.clone(), s.id.clone()));
        registry.update(cx, |_, cx| {
            cx.emit(RegistryEvent::Session {
                agent,
                session: id,
                event: SessionEvent::Title("From the agent".into()),
            })
        });
        cx.run_until_parked();
        let title = session.read_with(cx, |s, _| s.model.title.clone());
        assert_eq!(title.as_deref(), Some("From the agent"));
        assert_eq!(label(open_row(0), cx).as_deref(), Some("Mine"));

        // A double click renames too; Esc leaves the name as it was.
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.double_click(open_row(0), cx);
        });
        cx.run_until_parked();
        assert!(workspace.read_with(cx, |w, _| w.renaming.is_some()));
        type_name("Other", "escape", cx);
        assert!(workspace.read_with(cx, |w, _| w.renaming.is_none()));
        assert_eq!(name(cx).as_deref(), Some("Mine"));

        menu_item(open_row(0), "Rename", cx);
        type_name("", "enter", cx);
        assert_eq!(name(cx), None);
        assert_eq!(label(open_row(0), cx).as_deref(), Some("From the agent"));
    }

    /// New group, rename, collapse and delete; deleting a group keeps its sessions open.
    #[gpui_kit::test]
    fn groups_are_created_renamed_collapsed_and_deleted(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(2, cx);
        let ids: Vec<String> = workspace.read_with(cx, |w, cx| {
            w.sessions.iter().map(|s| s.read(cx).id.clone()).collect()
        });

        press("new-group", cx);
        assert_eq!(groups(&workspace, cx), [named("New group", &[])]);
        assert!(workspace.read_with(cx, |w, _| w.renaming.is_some()));
        type_name("Review", "enter", cx);
        assert_eq!(groups(&workspace, cx), [named("Review", &[])]);
        assert_eq!(label(group(0), cx).as_deref(), Some("Review, 0"));

        // Renamed from its menu and by a double click; an empty name keeps the old one.
        menu_item(group(0), "Rename group", cx);
        type_name("Later", "enter", cx);
        assert_eq!(groups(&workspace, cx), [named("Later", &[])]);
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.double_click(group(0), cx);
        });
        cx.run_until_parked();
        type_name("", "enter", cx);
        menu_item(group(0), "Rename group", cx);
        type_name("Gone", "escape", cx);
        assert_eq!(groups(&workspace, cx), [named("Later", &[])]);

        // A row keeps its index in a group; collapsed, the group hides it.
        drag(open_row(1), group(0), cx);
        assert_eq!(groups(&workspace, cx), [named("Later", &[&ids[1]])]);
        assert_eq!(label(group(0), cx).as_deref(), Some("Later, 1"));
        assert!(label(open_row(1), cx).is_some());
        press(("group-toggle", 0usize), cx);
        assert!(groups(&workspace, cx)[0].collapsed);
        assert!(label(open_row(1), cx).is_none());
        assert!(label(open_row(0), cx).is_some(), "ungrouped rows stay");
        press(("group-toggle", 0usize), cx);
        assert!(label(open_row(1), cx).is_some());

        menu_item(group(0), "Delete group", cx);
        assert_eq!(groups(&workspace, cx), []);
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), 2);
        assert!(label(open_row(1), cx).is_some());
    }

    /// A row's menu moves its session into a group, into a new group, and out; rows and group
    /// headers carry no inline rename, fork or delete buttons.
    #[gpui_kit::test]
    fn row_menu_groups_sessions(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(2, cx);
        let ids: Vec<String> = workspace.read_with(cx, |w, cx| {
            w.sessions.iter().map(|s| s.read(cx).id.clone()).collect()
        });
        press("new-group", cx);
        type_name("A", "enter", cx);

        menu_item(open_row(0), "Move to A", cx);
        assert_eq!(groups(&workspace, cx), [named("A", &[&ids[0]])]);
        menu_item(open_row(1), "New group with this session", cx);
        assert!(workspace.read_with(cx, |w, _| w.renaming.is_some()));
        type_name("B", "enter", cx);
        assert_eq!(
            groups(&workspace, cx),
            [named("A", &[&ids[0]]), named("B", &[&ids[1]])]
        );
        menu_item(open_row(0), "Move to B", cx);
        assert_eq!(
            groups(&workspace, cx),
            [named("A", &[]), named("B", &[&ids[1], &ids[0]])]
        );
        menu_item(open_row(0), "Remove from group", cx);
        assert_eq!(
            groups(&workspace, cx),
            [named("A", &[]), named("B", &[&ids[1]])]
        );

        assert!(label(("history", 0usize), cx).is_some());
        assert!(
            label(("history-fork", 0usize), cx).is_none(),
            "Fork is inline"
        );
        press("tab-sessions", cx);
        for id in [
            ElementId::from(("open-rename", 0usize)),
            ("open-fork", 0usize).into(),
            ("group-rename", 0usize).into(),
            ("group-delete", 0usize).into(),
        ] {
            assert!(label(id.clone(), cx).is_none(), "{id:?} is inline");
        }
    }

    /// A row dropped on a group's header joins that group only; dropped on the ungrouped rows,
    /// it leaves. Ending a session takes it out of its group, which stays.
    #[gpui_kit::test]
    fn rows_drag_into_and_out_of_groups(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(2, cx);
        let ids: Vec<String> = workspace.read_with(cx, |w, cx| {
            w.sessions.iter().map(|s| s.read(cx).id.clone()).collect()
        });
        for name in ["A", "B"] {
            press("new-group", cx);
            type_name(name, "enter", cx);
        }

        drag(open_row(0), group(0), cx);
        assert_eq!(
            groups(&workspace, cx),
            [named("A", &[&ids[0]]), named("B", &[])]
        );
        drag(open_row(0), group(1), cx);
        assert_eq!(
            groups(&workspace, cx),
            [named("A", &[]), named("B", &[&ids[0]])]
        );
        drag(open_row(1), group(1), cx);
        drag(open_row(0), "open-ungrouped", cx);
        assert_eq!(
            groups(&workspace, cx),
            [named("A", &[]), named("B", &[&ids[1]])]
        );

        press(("open-close", 1usize), cx);
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), 1);
        assert_eq!(groups(&workspace, cx), [named("A", &[]), named("B", &[])]);
    }

    /// An open row dropped on another takes its place (after it from above, before it from
    /// below) and joins its group; the Agents cards follow the order.
    #[gpui_kit::test]
    fn rows_reorder_by_dragging(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(3, cx);
        let ids: Vec<String> = workspace.read_with(cx, |w, cx| {
            w.sessions.iter().map(|s| s.read(cx).id.clone()).collect()
        });
        let order = |cx: &mut VisualTestContext| -> Vec<String> {
            (0..3).map(|ix| label(open_row(ix), cx).unwrap()).collect()
        };

        drag(open_row(0), open_row(2), cx);
        assert_eq!(order(cx), ["Session 1", "Session 2", "Session 0"]);
        press("tab-agents", cx);
        let card = label(("agent-card", 0usize), cx).unwrap();
        assert!(card.starts_with("Session 1"), "{card:?}");
        press("tab-sessions", cx);

        drag(open_row(2), open_row(0), cx);
        assert_eq!(order(cx), ["Session 0", "Session 1", "Session 2"]);

        press("new-group", cx);
        type_name("A", "enter", cx);
        menu_item(open_row(2), "Move to A", cx);
        drag(open_row(0), open_row(2), cx);
        assert_eq!(order(cx), ["Session 1", "Session 2", "Session 0"]);
        assert_eq!(groups(&workspace, cx), [named("A", &[&ids[2], &ids[0]])]);
    }

    /// A session's name and the groups are kept in `state.json` and come back on a relaunch.
    #[gpui_kit::test]
    fn names_and_groups_survive_a_relaunch(cx: &mut TestAppContext, relaunch: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));
        launch(&workspace, cx);
        while question(&workspace, cx).is_some() {
            click("approval-allow", cx);
        }
        start_now(&workspace, cx);

        menu_item(open_row(0), "Rename", cx);
        type_name("Mine", "enter", cx);
        press("new-group", cx);
        type_name("Review", "enter", cx);
        drag(open_row(0), group(0), cx);
        press(("group-toggle", 0usize), cx);

        let review = SessionGroup {
            collapsed: true,
            ..named("Review", &["fake-1"])
        };
        let state = AppState::load(&path);
        assert_eq!(state.open_sessions[0].name.as_deref(), Some("Mine"));
        assert_eq!(state.groups, std::slice::from_ref(&review));

        relaunch.executor().allow_parking();
        let (workspace, cx) = open_at(relaunch, fake_settings(""), Some(path.clone()));
        launch(&workspace, cx);
        assert_eq!(groups(&workspace, cx), std::slice::from_ref(&review));
        let name = workspace.read_with(cx, |w, cx| w.sessions[0].read(cx).model.name.clone());
        assert_eq!(name.as_deref(), Some("Mine"));
        assert_eq!(label(group(0), cx).as_deref(), Some("Review, 1"));
        let state = AppState::load(&path);
        assert_eq!(state.open_sessions[0].name.as_deref(), Some("Mine"));
        assert_eq!(state.groups, std::slice::from_ref(&review));
    }
}
