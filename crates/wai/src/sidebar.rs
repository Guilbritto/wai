//! The sidebar: the activity bar's panels (the open sessions with their status and the agent's
//! history, the project's files, search, the agents, then each running extension's panels).

use std::ops::Range;
use std::time::Duration;

use chrono::{DateTime, Utc};
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
use wai_settings::SessionGroup;
pub use wai_settings::canonical;

use crate::agents::AgentStatus;
use crate::app::{FilterSessions, NewSession};
use crate::aurora;
use crate::chat::{ChatItem, ChatModel, ChatSession, DraftView, TurnState};
use crate::settings_view::Section;
use crate::workspace::{Dragged, DraggedSession, MAX_PANES, Renamed, Workspace};

pub(crate) const ROW_HEIGHT: f32 = 28.;

/// The filter's shortcut, as its hint shows it.
const FILTER_KEY: &str = if cfg!(target_os = "macos") {
    "⌘K"
} else {
    "Ctrl+K"
};

/// The groups' swatches, in the order new groups take them.
pub(crate) const GROUP_COLORS: [fn() -> Hsla; 6] = [
    aurora::lilas,
    aurora::boreal,
    aurora::ceu,
    aurora::magenta,
    aurora::ambar,
    aurora::ok,
];

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
            _ => Detail::Doing(m.activity().unwrap_or_default()),
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

/// A history row's date, from the agent's RFC 3339 `updated_at`: its age as `short_age` gives
/// it, but a day old reads `yesterday`.
pub(crate) fn relative_date(updated_at: &str, now: DateTime<Utc>) -> Option<String> {
    let at = DateTime::parse_from_rfc3339(updated_at).ok()?;
    let age = (now - at.to_utc()).to_std().unwrap_or_default();
    Some(match age.as_secs() / 86_400 {
        1 => "yesterday".into(),
        _ => short_age(age),
    })
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
        let claude = (agent == "claude").then(|| {
            let open: std::collections::HashSet<String> = (self.sessions.iter())
                .map(|s| s.read(cx))
                .filter(|s| s.agent == "claude")
                .map(|s| s.id.clone())
                .collect();
            (self.claude_home.clone(), self.quiet(), open)
        });
        self.history_task = Some(cx.spawn(async move |this, cx| {
            let sessions = match handle.list_sessions().await {
                Ok(sessions) => sessions,
                Err(e) => return eprintln!("wai: could not list {agent} sessions: {e}"),
            };
            // ponytail: stats ids × project dirs and runs ps once per refresh; read each projects dir once if it shows.
            let (rows, elsewhere) = cx
                .background_spawn(async move {
                    let ids: Vec<String> = match &claude {
                        Some((_, _, open)) => (sessions.iter())
                            .filter(|s| !open.contains(&s.id))
                            .map(|s| s.id.clone())
                            .collect(),
                        None => Vec::new(),
                    };
                    let found = match &claude {
                        Some((home, quiet, _)) => {
                            crate::elsewhere::open_elsewhere(home, &ids, quiet)
                        }
                        None => Default::default(),
                    };
                    let rows = sessions.into_iter().map(|s| (canonical(&s.cwd), s));
                    (rows.collect(), found)
                })
                .await;
            this.update(cx, |this, cx| {
                this.history = rows;
                this.elsewhere = elsewhere;
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
            let title = p.title.clone();
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

    /// The Sessions panel: its title row and filter, then the Open section (drafts and sessions
    /// outside any group, then each group under its header) over History. A row keeps its index
    /// in `sessions`, whichever group draws it. With text in the filter only matching titles show,
    /// and Open, History and a group holding one show expanded without changing their own folded
    /// state.
    fn render_sessions(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let filter = self.filter(cx);
        let filtering = !filter.is_empty();
        let open_folded = self.open_folded && !filtering;
        let history_folded = self.history_folded && !filtering;
        let shown = |session: &Entity<ChatSession>| shows(&filter, session_title(session.read(cx)));
        let members = |group: Option<usize>| {
            (self.sessions.iter().enumerate())
                .filter(move |(_, session)| self.group_of(&session.read(cx).id) == group)
        };
        let drafts: Vec<_> = (self.drafts.iter().enumerate())
            .filter(|(_, draft)| shows(&filter, &draft_title(draft, cx)))
            .map(|(ix, draft)| self.draft_row(ix, draft, cx).into_any_element())
            .collect();
        let ungrouped: Vec<_> = members(None)
            .filter(|(_, session)| shown(session))
            .map(|(ix, session)| self.open_row(ix, session, false, cx).into_any_element())
            .collect();
        let mut matched = !drafts.is_empty() || !ungrouped.is_empty();
        let mut groups = Vec::new();
        for (gx, group) in self.groups.iter().enumerate() {
            let all: Vec<_> = members(Some(gx)).collect();
            let hits: Vec<_> = all.iter().filter(|(_, session)| shown(session)).collect();
            if filtering && hits.is_empty() {
                continue;
            }
            matched = true;
            let needs = (all.iter())
                .filter(|(_, s)| matches!(s.read(cx).status(), TurnState::NeedsYou))
                .count();
            let collapsed = group.collapsed && !filtering;
            let header = self.group_header(gx, group, all.len(), needs, collapsed, cx);
            groups.push(header.into_any_element());
            if !collapsed {
                let rows = hits.iter().map(|(ix, s)| self.open_row(*ix, s, true, cx));
                groups.extend(rows.map(IntoElement::into_any_element));
            }
        }
        let no_match = filtering && !matched && self.listed_history(cx).is_empty();
        let count = self.drafts.len() + self.sessions.len();
        let faint = faint(cx);
        let title = panel_title("Sessions", cx)
            .child(
                title_button("sessions-new", IconName::Plus, "New session").on_click(
                    cx.listener(|this, _, window, cx| this.new_session(&NewSession, window, cx)),
                ),
            )
            .child(
                title_button("new-group", Lucide::FolderPlus, "New group")
                    .on_click(cx.listener(|this, _, window, cx| this.new_group(None, window, cx))),
            )
            .child(
                title_button("groups-collapse", Lucide::ChevronsDownUp, "Collapse all").on_click(
                    cx.listener(|this, _, _, cx| {
                        for group in &mut this.groups {
                            group.collapsed = true;
                        }
                        this.save_state(cx);
                        cx.notify();
                    }),
                ),
            );
        let kbd = div()
            .px(px(4.))
            .rounded(px(5.))
            .border_1()
            .border_color(theme.border)
            .text_size(px(10.))
            .text_color(faint)
            .child(FILTER_KEY);
        let filter_field = div().id("sessions-filter").px(px(2.)).pb(px(4.)).child(
            Input::new(&self.sessions_filter)
                .small()
                .prefix(Icon::new(IconName::Search).xsmall().text_color(faint))
                .suffix(kbd),
        );
        let open_header =
            section_header("open-section", "Open", count.to_string(), open_folded, cx)
                .on_click(cx.listener(|this, _, _, cx| {
                    if this.filtering(cx) {
                        return;
                    }
                    this.open_folded = !this.open_folded;
                    this.save_state(cx);
                    cx.notify();
                }))
                .test_support();
        v_flex()
            .size_full()
            .child(title)
            .child(filter_field.test_support())
            .children(no_match.then(|| {
                div()
                    .id("sessions-no-match")
                    .px(px(8.))
                    .py(px(4.))
                    .text_size(px(11.))
                    .text_color(faint)
                    .aria_label("No sessions match")
                    .child("No sessions match")
                    .test_support()
            }))
            .child(
                // Sized to its rows, up to 60 % of the panel while History is open; then it
                // scrolls, header included.
                v_flex()
                    .id("open-list")
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(!history_folded, |el| el.max_h(relative(0.6)))
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
                            .child(open_header)
                            .when(!open_folded, |el| el.children(drafts).children(ungrouped))
                            .test_support(),
                    )
                    .when(!open_folded, |el| el.children(groups)),
            )
            .child(self.history_section(history_folded, cx))
    }

    /// The Sessions filter's text, lowercased: rows show when their title holds it.
    fn filter(&self, cx: &App) -> String {
        self.sessions_filter.read(cx).value().to_lowercase()
    }

    /// The filter has text: the sections and groups show unfolded, and their toggles wait.
    fn filtering(&self, cx: &App) -> bool {
        !self.sessions_filter.read(cx).value().is_empty()
    }

    /// The History rows the filter leaves.
    fn listed_history(&self, cx: &App) -> Vec<&SessionSummary> {
        let filter = self.filter(cx);
        let mut rows = self.history_rows();
        rows.retain(|row| shows(&filter, history_title(row)));
        rows
    }

    /// ⌘K: shows the Sessions panel with the caret in its filter.
    pub(crate) fn filter_sessions(
        &mut self,
        _: &FilterSessions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.sidebar_tab = SidebarTab::Sessions;
        self.sessions_filter
            .update(cx, |input, cx| input.focus(window, cx));
        cx.notify();
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
            color: (self.groups.len() % GROUP_COLORS.len()) as u8,
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
                let title = session.read(cx).model.named_title();
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

    /// A group's header (the mockup's `.grp`): its chevron, swatch, name and session count, and
    /// while collapsed an amber `● N` for those that need you; its menu renames or deletes it. An
    /// open session dropped on it joins the group.
    fn group_header(
        &self,
        ix: usize,
        group: &SessionGroup,
        count: usize,
        needs: usize,
        collapsed: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let chevron = match collapsed {
            true => IconName::ChevronRight,
            false => IconName::ChevronDown,
        };
        let input = self.renaming(&Renamed::Group(ix));
        let name = group.name.clone();
        let swatch = GROUP_COLORS[group.color as usize % GROUP_COLORS.len()]();
        let (faint, warn) = (faint(cx), theme.warning);
        let workspace = cx.entity().downgrade();
        h_flex()
            .id(("group-row", ix))
            .w_full()
            .h(px(25.))
            .pt(px(4.))
            .pl(px(4.))
            .pr(px(6.))
            .gap(px(2.))
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(fg2(cx))
            .child(
                Button::new(("group-toggle", ix))
                    .ghost()
                    .xsmall()
                    .icon(chevron)
                    .accessibility_label(match collapsed {
                        true => "Expand group",
                        false => "Collapse group",
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.filtering(cx) {
                            return;
                        }
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
                    .px(px(4.))
                    .gap(px(6.))
                    .rounded(px(7.))
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
                    .child(div().flex_none().size(px(8.)).rounded(px(2.)).bg(swatch))
                    .map(|el| match input {
                        Some(input) => el.child(input),
                        None => el
                            .child(div().min_w_0().truncate().child(name))
                            .child(
                                div()
                                    .flex_none()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(faint)
                                    .child(count.to_string()),
                            )
                            .child(div().flex_1())
                            .children((collapsed && needs > 0).then(|| {
                                let text = format!("● {needs}");
                                div()
                                    .id(("group-needs", ix))
                                    .flex_none()
                                    .text_size(px(10.5))
                                    .text_color(warn)
                                    .aria_label(text.clone())
                                    .child(text)
                                    .test_support()
                            }))
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

    /// History under Open: its header with the All switch (today's All projects), then the
    /// agent's sessions the filter leaves, lazily, or the hint to choose an AI. `folded` as drawn.
    fn history_section(&self, folded: bool, cx: &Context<Self>) -> impl IntoElement + use<> {
        let rows = self.listed_history(cx).len();
        let scope = match self.all_projects {
            true => "· all projects",
            false => "· this project",
        };
        let header = section_header("history-section", "History", scope, folded, cx)
            .on_click(cx.listener(|this, _, _, cx| {
                if this.filtering(cx) {
                    return;
                }
                this.history_folded = !this.history_folded;
                this.save_state(cx);
                cx.notify();
            }))
            .test_support();
        v_flex()
            .flex_1()
            .min_h_0()
            .mt(px(4.))
            .child(
                h_flex().pr(px(4.)).child(header).child(
                    Switch::new("all-projects")
                        .xsmall()
                        .label("All")
                        .checked(self.all_projects)
                        .on_click(cx.listener(|this, all: &bool, _, cx| {
                            this.all_projects = *all;
                            cx.notify();
                        })),
                ),
            )
            .child(
                v_flex()
                    .id("sessions-blank")
                    .flex_1()
                    .min_h_0()
                    .on_click(cx.listener(|this, event: &ClickEvent, window, cx| {
                        if event.click_count() == 2 {
                            this.new_session(&NewSession, window, cx);
                        }
                    }))
                    .when(!folded, |el| {
                        el.map(|el| match self.default_agent() {
                            None => el.child(
                                div()
                                    .id("history-hint")
                                    .p_2()
                                    .text_sm()
                                    .text_color(cx.theme().muted_foreground)
                                    .child("Choose an AI in Settings to see its history.")
                                    .test_support(),
                            ),
                            // Only the visible rows render: an agent can hold thousands of sessions.
                            Some(_) => el.child(
                                uniform_list(
                                    "history",
                                    rows,
                                    cx.processor(|this, range: Range<usize>, _, cx| {
                                        let rows = this.listed_history(cx);
                                        range
                                            .map(|ix| this.history_row(ix, rows[ix], cx))
                                            .collect::<Vec<_>>()
                                    }),
                                )
                                .flex_1()
                                .min_h_0()
                                // The rows' 4 px inset (the mockup's `.r1` margin).
                                .pl(px(4.)),
                            ),
                        })
                    })
                    .test_support(),
            )
    }

    /// An open session's row (the mockup's `.r2`): the status dot, its title and how long it has
    /// been in its state (a turn: since it started), then what it does; Open to the side and ×
    /// show on hover. A group's rows sit further in.
    fn open_row(
        &self,
        ix: usize,
        session: &Entity<ChatSession>,
        grouped: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let s = session.read(cx);
        let dot = status_color(s.status(), theme);
        let title = session_title(s).to_string();
        // ponytail: repainted by the sessions' own ticks, so an idle row's time moves only when
        // something else repaints; a 1 s timer while any row shows a time if that matters.
        let age = s.since.map(|since| match s.status() {
            TurnState::Working => short_age(s.turn_started().unwrap_or(since).elapsed()),
            _ => short_age(since.elapsed()),
        });
        let detail = line_two(
            ("open-detail", ix),
            detail(&s.model),
            live_subagents(&s.model),
            cx,
        );
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
        let (fg, faint) = (theme.foreground, faint(cx));
        let renamed = input.is_some();
        let line_one = h_flex().gap(px(6.)).map(|el| match input {
            Some(input) => el.child(input),
            None => el
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(fg)
                        .child(title.clone()),
                )
                .children(age.map(|age| {
                    div()
                        .id(("open-age", ix))
                        .flex_none()
                        .text_size(px(10.5))
                        .text_color(faint)
                        .aria_label(age.clone())
                        .child(age)
                        .test_support()
                })),
        });
        two_line_row(active, grouped, cx)
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
                    .items_start()
                    .gap(px(9.))
                    .child(status_dot(dot).mt(px(5.)))
                    .child(v_flex().flex_1().min_w_0().child(line_one).child(detail))
                    .aria_label(title)
                    .on_action(cx.listener(|this, _: &Escape, window, cx| {
                        this.end_rename(false, window, cx)
                    }))
                    // A second click renames.
                    .when(!renamed, |el| {
                        el.on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                            match event.click_count() {
                                1 => this.show(open.clone(), false, window, cx),
                                _ => this.rename(Renamed::Session(open.clone()), window, cx),
                            }
                        }))
                        .on_drag(dragged, DraggedSession::preview)
                    })
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .child(
                hover_chip(cx)
                    .when(self.panes.len() < MAX_PANES, |el| {
                        el.child(side_button(("open-side", ix)).on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.show(side.clone(), true, window, cx)
                            },
                        )))
                    })
                    .child(
                        chip_button(("open-close", ix), IconName::Close, "End session").on_click(
                            cx.listener(move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.close_session(&end, window, cx)
                            }),
                        ),
                    ),
            )
            .context_menu(move |menu, _, cx| {
                let Some(w) = workspace.upgrade() else {
                    return menu;
                };
                w.read(cx).session_menu(menu, &w, &menu_session, cx)
            })
    }

    /// A draft's row: a hollow dot, "New session" and `Draft` (once it sends, its session's
    /// dot and line); × discards it.
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
        let title = draft_title(draft, cx);
        let dot = starting.map(|s| status_color(s.status(), theme));
        let line = match starting {
            Some(s) => line_two(
                ("draft-detail", ix),
                detail(&s.model),
                live_subagents(&s.model),
                cx,
            ),
            None => line_two(("draft-detail", ix), Detail::Draft, 0, cx),
        };
        let title_color = match starting {
            Some(_) => theme.foreground,
            None => theme.muted_foreground,
        };
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::Draft(draft.clone()),
        };
        two_line_row(active, false, cx)
            .id(("draft-row", ix))
            .child(
                h_flex()
                    .id(("draft", ix))
                    .flex_1()
                    .min_w_0()
                    .items_start()
                    .gap(px(9.))
                    .child(
                        match dot {
                            Some(dot) => status_dot(dot),
                            None => div()
                                .flex_none()
                                .size(px(8.))
                                .rounded_full()
                                .border_1()
                                .border_dashed()
                                .border_color(faint(cx)),
                        }
                        .mt(px(5.)),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .child(
                                div()
                                    .truncate()
                                    .text_color(title_color)
                                    .child(title.clone()),
                            )
                            .child(line),
                    )
                    .aria_label(title)
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.show_draft(open.clone(), false, window, cx)
                    }))
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .child(hover_chip(cx).child(
                chip_button(("draft-close", ix), IconName::Close, "Discard draft").on_click(
                    cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.close_draft(&end, window, cx)
                    }),
                ),
            ))
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

    /// A History row (the mockup's `.r1`): ↺, the title and how long ago it was updated; Open to
    /// the side shows on hover.
    fn history_row(
        &self,
        ix: usize,
        summary: &SessionSummary,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let title = history_title(summary).to_string();
        let date = (summary.updated_at.as_deref()).and_then(|at| relative_date(at, Utc::now()));
        let (open, side, source) = (summary.clone(), summary.clone(), summary.clone());
        let workspace = cx.entity().downgrade();
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::History(summary.clone()),
        };
        let forkable = self
            .default_agent()
            .is_some_and(|agent| self.can_fork(&agent, cx));
        let (muted, faint) = (cx.theme().muted_foreground, faint(cx));
        let elsewhere = self.elsewhere.contains(&summary.id)
            && self.session("claude", &summary.id, cx).is_none();
        let amber = match cx.theme().is_dark() {
            true => aurora::ambar(),
            false => cx.theme().warning,
        };
        aurora::tab(false, cx)
            .id(("history-row", ix))
            .group(ROW_GROUP)
            .relative()
            // A list's row is laid out on its own: without a width it takes its title's.
            .w_full()
            .h(px(23.))
            .px(px(8.))
            .rounded(px(7.))
            .text_size(px(12.))
            .child(
                h_flex()
                    .id(("history", ix))
                    .flex_1()
                    .min_w_0()
                    .gap(px(7.))
                    .aria_label(title.clone())
                    .child(div().flex_none().text_color(faint).child("↺"))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .text_color(muted)
                            .child(title),
                    )
                    .when(elsewhere, |el| {
                        el.child(
                            div()
                                .id(("history-elsewhere", ix))
                                .flex_none()
                                .text_color(amber)
                                .aria_label("Open elsewhere")
                                .tooltip(|window, cx| {
                                    Tooltip::new("Open elsewhere").build(window, cx)
                                })
                                .child(Icon::new(Lucide::Monitor).size(px(11.)))
                                .test_support(),
                        )
                    })
                    .children(date.map(|date| {
                        div()
                            .flex_none()
                            .text_size(px(10.5))
                            .text_color(faint)
                            .child(date)
                    }))
                    .on_click(cx.listener(move |this, _, window, cx| {
                        cx.stop_propagation();
                        this.resume(&open, false, window, cx)
                    }))
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .when(self.panes.len() < MAX_PANES, |el| {
                el.child(
                    hover_chip(cx)
                        .top_0()
                        .child(side_button(("history-side", ix)).on_click(cx.listener(
                            move |this, _, window, cx| {
                                cx.stop_propagation();
                                this.resume(&side, true, window, cx)
                            },
                        ))),
                )
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

/// Faint text: counts, times, the filter's key (the mockup's `--dim`).
fn faint(cx: &App) -> Hsla {
    match cx.theme().is_dark() {
        true => aurora::nevoa_2(),
        false => cx.theme().muted_foreground,
    }
}

/// Secondary text: group names (the mockup's `--fg2`).
fn fg2(cx: &App) -> Hsla {
    match cx.theme().is_dark() {
        true => aurora::tinta_2(),
        false => cx.theme().foreground,
    }
}

/// A title row's icon button.
fn title_button(id: &'static str, icon: impl Into<Icon>, tip: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(icon.into())
        .tooltip(tip)
        .accessibility_label(tip)
}

/// A Sessions section's header (the mockup's `.sec`): chevron, name, then `note` fainter; the
/// caller's click folds it.
fn section_header(
    id: &'static str,
    name: &str,
    note: impl Into<SharedString>,
    folded: bool,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (faint, ring) = (faint(cx), theme.ring);
    let note = note.into();
    let chevron = match folded {
        true => IconName::ChevronRight,
        false => IconName::ChevronDown,
    };
    h_flex()
        .id(id)
        .flex_1()
        .min_w_0()
        .gap(px(5.))
        .px(px(4.))
        .pt(px(6.))
        .pb(px(3.))
        .rounded(px(6.))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .cursor_pointer()
        .text_size(px(10.5))
        .font_weight(FontWeight::BOLD)
        .text_color(theme.muted_foreground)
        .role(Role::Button)
        .aria_label(format!("{name} {note}"))
        .aria_expanded(!folded)
        .focus_visible(move |el| el.border_color(ring))
        // Focusable, and Enter or Space clicks it.
        .tab_index(0)
        .child(Icon::new(chevron).size(px(10.)).text_color(faint))
        .child(name.to_uppercase())
        .child(
            div()
                .min_w_0()
                .truncate()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(faint)
                .child(note.to_uppercase()),
        )
}

/// A session's status dot, glowing in its colour.
fn status_dot(color: Hsla) -> Div {
    div()
        .flex_none()
        .size(px(8.))
        .rounded_full()
        .bg(color)
        .shadow(vec![
            BoxShadow::new(px(0.), px(0.), color.alpha(0.7)).blur_radius(px(4.)),
        ])
}

/// The group the rows' hover chips follow.
const ROW_GROUP: &str = "session-row";

/// A session or draft row (the mockup's `.r2`): two lines, 9 px corners, 5 × 8 px padding; a
/// group's rows sit 10 px further in.
fn two_line_row(active: bool, grouped: bool, cx: &App) -> Div {
    aurora::tab(active, cx)
        .group(ROW_GROUP)
        .relative()
        .items_start()
        .py(px(5.))
        .px(px(8.))
        .ml(px(if grouped { 14. } else { 4. }))
        .rounded(px(9.))
        .text_size(px(13.))
}

/// A row's buttons (`chip_button`) on a small glass chip at its top right, shown while the row
/// is hovered. Transparent otherwise, they stay in place for the screen reader. The chip lies
/// over the row's own click: its buttons stop theirs.
fn hover_chip(cx: &App) -> Div {
    let theme = cx.theme();
    let (fill, border) = match theme.is_dark() {
        true => (aurora::card_fill(), aurora::borda()),
        false => (theme.background, theme.border),
    };
    h_flex()
        .absolute()
        .top(px(2.))
        .right(px(4.))
        .rounded(px(7.))
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .group_hover(ROW_GROUP, |style| style.bg(fill).border_color(border))
}

/// A hover chip's button: invisible until its row is hovered, or the keyboard focuses it.
fn chip_button(id: impl Into<ElementId>, icon: IconName, tip: &'static str) -> Button {
    Button::new(id)
        .ghost()
        .xsmall()
        .icon(icon)
        .tooltip(tip)
        .accessibility_label(tip)
        .opacity(0.)
        .group_hover(ROW_GROUP, |style| style.opacity(1.))
        .focus_visible(|style| style.opacity(1.))
}

/// A row's second line (11 px, muted): what the session does, its lead word coloured by state,
/// then its live subagents. Its accessible name is the whole line.
fn line_two(id: impl Into<ElementId>, detail: Detail, live: usize, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let (lead, rest) = match detail {
        Detail::Doing(text) => (None, text),
        Detail::Permission(title) => (Some(("Permission:", theme.warning)), format!(" {title}")),
        Detail::Question(text) => (Some(("Question:", theme.warning)), format!(" {text}")),
        Detail::Done => (Some(("✓ Done", theme.success)), String::new()),
        Detail::Idle => (None, "Idle".into()),
        Detail::Exited => (Some(("Agent exited", theme.danger)), " · Restart".into()),
        Detail::Draft => (None, "Draft".into()),
    };
    let mut text = format!("{}{}", lead.map_or("", |(l, _)| l), rest.replace('\n', " "));
    match live {
        0 => {}
        1 => text.push_str(" · 1 subagent"),
        n => text.push_str(&format!(" · {n} subagents")),
    }
    let highlight = lead.map(|(l, color)| {
        let style = HighlightStyle {
            color: Some(color),
            ..Default::default()
        };
        (0..l.len(), style)
    });
    div()
        .id(id)
        .truncate()
        .text_size(px(11.))
        .text_color(theme.muted_foreground)
        .aria_label(text.clone())
        .child(StyledText::new(text).with_highlights(highlight))
        .test_support()
}

fn session_title(s: &ChatSession) -> &str {
    s.model.shown_title().unwrap_or("New session")
}

fn draft_title(draft: &Entity<DraftView>, cx: &App) -> String {
    let starting = draft.read(cx).starting.as_ref().map(|chat| chat.read(cx));
    let title = starting.and_then(|s| s.model.shown_title());
    title.unwrap_or("New session").to_string()
}

fn history_title(summary: &SessionSummary) -> &str {
    summary.title.as_deref().unwrap_or(&summary.id)
}

/// Whether a row titled `title` shows for the lowercased `filter`; an empty one shows all.
fn shows(filter: &str, title: &str) -> bool {
    filter.is_empty() || title.to_lowercase().contains(filter)
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
    chip_button(id, IconName::PanelRightOpen, "Open to the side")
}

#[cfg(test)]
pub(crate) mod tests {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    use gpui_kit::{
        ElementId, Entity, Modifiers, MouseButton, Pixels, Point, TestAppContext,
        VisualTestContext, point, px, size,
    };
    use wai_agent::{SessionEvent, SessionSummary};
    use wai_settings::{AppState, SessionGroup, Settings};

    use gpui_kit::component::ActiveTheme as _;

    use std::time::Duration;

    use super::{Detail, TurnState, detail, initials, relative_date, short_age, status_color};
    use crate::agents::RegistryEvent;
    use crate::chat::ChatModel;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{
        click, drag_to_pane, fake_settings, history_settings, launch, load_history, menu_item,
        open, open_at, question, reveal, start_now, wait_until,
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
    fn history_dates_are_relative() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-12T12:00:00Z").unwrap();
        let date = |at: &str| relative_date(at, now.to_utc());
        assert_eq!(date("2026-11-12T13:00:00+02:00").as_deref(), Some("1h"));
        assert_eq!(
            date("2026-11-11T09:30:00.123Z").as_deref(),
            Some("yesterday")
        );
        assert_eq!(date("2026-11-10T12:00:00Z").as_deref(), Some("2d"));
        assert_eq!(date("yesterday"), None);
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
        m.waiting.clear();
        assert_eq!(detail(&m), Detail::Doing("Waiting for you…".into()));
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

    /// A double click under History opens a draft; one on a History row only opens that row.
    #[gpui_kit::test]
    fn double_click_on_the_empty_panel_opens_a_draft(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, history_settings(1));
        load_history(&workspace, 1, cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.double_click("sessions-blank", cx);
        });
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.drafts.len(), 1);
            assert!(w.sessions.is_empty());
        });

        cx.update(|window, cx| window.double_click(("history", 0usize), cx));
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.sessions.len(), 1, "the row resumed");
            assert_eq!(w.drafts.len(), 1, "and opened no draft");
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

        reveal(("draft-close", 0usize), cx);
        press(("draft-close", 0usize), cx);
        workspace.read_with(cx, |w, _| assert!(w.drafts.is_empty()));
        assert!(!dialog(cx));

        cx.simulate_keystrokes("secondary-n");
        cx.simulate_input("x");
        reveal(("draft-close", 0usize), cx);
        press(("draft-close", 0usize), cx);
        cx.update(|window, _| window.find_by_label("Discard this draft?"));
        press("cancel", cx);
        workspace.read_with(cx, |w, _| assert_eq!(w.drafts.len(), 1));

        reveal(("draft-close", 0usize), cx);
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
    pub(crate) fn with_sessions(
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

    /// Each group's name, collapsed state and sessions (its colour left at 0).
    fn groups(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<SessionGroup> {
        let groups = workspace.read_with(cx, |w, _| w.groups.clone());
        groups
            .into_iter()
            .map(|g| SessionGroup { color: 0, ..g })
            .collect()
    }

    fn colors(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<u8> {
        workspace.read_with(cx, |w, _| w.groups.iter().map(|g| g.color).collect())
    }

    fn named(name: &str, sessions: &[&str]) -> SessionGroup {
        SessionGroup {
            name: name.into(),
            collapsed: false,
            sessions: sessions.iter().map(|s| s.to_string()).collect(),
            color: 0,
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

        reveal(("open-close", 1usize), cx);
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

    /// A row's second line follows the running tool, then says Done; its time shows once the
    /// state first changed.
    #[gpui_kit::test]
    fn rows_say_what_sessions_do(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--steps 1 --chunk-delay-ms 300"));
        let session = start_now(&workspace, cx);
        assert_eq!(label(("open-age", 0usize), cx), None);
        assert_eq!(label(("open-detail", 0usize), cx).as_deref(), Some("Idle"));
        cx.simulate_input("go");
        cx.simulate_keystrokes("enter");
        let doing = |cx: &mut VisualTestContext| {
            session.read_with(cx, |s, _| s.model.activity().unwrap_or_default())
        };
        wait_until(cx, "the tool", |cx| doing(cx).starts_with("Read"));
        let (detail, doing) = cx.update(|window, cx| {
            window.render_frame(cx);
            let detail = window
                .find(("open-detail", 0usize))
                .label()
                .map(str::to_string);
            (detail.unwrap(), session.read(cx).model.activity().unwrap())
        });
        assert!(detail.starts_with(&doing), "{detail:?} for {doing:?}");
        assert!(label(("open-age", 0usize), cx).is_some());
        wait_until(cx, "the turn's end", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::Idle && s.model.done)
        });
        assert_eq!(
            label(("open-detail", 0usize), cx).as_deref(),
            Some("✓ Done")
        );
    }

    /// A collapsed group counts its sessions that wait for the user; expanded, its rows say it.
    #[gpui_kit::test]
    fn collapsed_group_says_who_needs_you(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--ask-permission"));
        let session = start_now(&workspace, cx);
        cx.simulate_input("edit");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "needs you", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::NeedsYou)
        });
        let detail = label(("open-detail", 0usize), cx).unwrap();
        assert!(detail.starts_with("Permission: "), "{detail:?}");
        menu_item(open_row(0), "New group with this session", cx);
        type_name("Mine", "enter", cx);
        assert_eq!(label(("group-needs", 0usize), cx), None);
        press(("group-toggle", 0usize), cx);
        assert_eq!(label(("group-needs", 0usize), cx).as_deref(), Some("● 1"));
        press(("group-toggle", 0usize), cx);
        assert_eq!(label(("group-needs", 0usize), cx), None);
    }

    /// The filter narrows Open and History to matching titles and shows matching groups
    /// expanded; Esc clears it and every group is folded as it was.
    #[gpui_kit::test]
    fn filter_narrows_open_and_history(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(3, cx);
        for name in ["A", "B"] {
            press("new-group", cx);
            type_name(name, "enter", cx);
        }
        menu_item(open_row(0), "Move to A", cx);
        menu_item(open_row(2), "Move to B", cx);
        press(("group-toggle", 0usize), cx);
        let before = groups(&workspace, cx);
        let open = |cx: &mut VisualTestContext| -> Vec<String> {
            (0..3).filter_map(|ix| label(open_row(ix), cx)).collect()
        };
        let history = |cx: &mut VisualTestContext| -> Vec<String> {
            (0..3usize)
                .filter_map(|ix| label(("history", ix), cx))
                .collect()
        };
        assert_eq!(open(cx), ["Session 1", "Session 2"]);

        cx.simulate_keystrokes("secondary-k");
        cx.simulate_input("SESSION 0");
        assert_eq!(open(cx), ["Session 0"], "its group shows expanded");
        assert_eq!(history(cx), ["Session 0"]);
        assert_eq!(label(group(1), cx), None, "a group with no match hides");
        assert_eq!(label("sessions-no-match", cx), None);

        cx.simulate_keystrokes("escape");
        assert_eq!(open(cx), ["Session 1", "Session 2"]);
        cx.simulate_input("zzz");
        assert_eq!(open(cx), Vec::<String>::new());
        assert_eq!(history(cx), Vec::<String>::new());
        let none = label("sessions-no-match", cx);
        assert_eq!(none.as_deref(), Some("No sessions match"));

        cx.simulate_keystrokes("escape");
        assert_eq!(groups(&workspace, cx), before);
        assert_eq!(open(cx), ["Session 1", "Session 2"]);
        assert_eq!(history(cx).len(), 3);
        assert_eq!(label("sessions-no-match", cx), None);
    }

    /// While the filter has text, folded Open and History show their matches and a group's
    /// chevron does nothing; cleared, each is as folded as before.
    #[gpui_kit::test]
    fn filter_unfolds_sections_without_changing_them(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(2, cx);
        press("new-group", cx);
        type_name("A", "enter", cx);
        menu_item(open_row(1), "Move to A", cx);
        press("open-section", cx);
        press("history-section", cx);
        let state = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| {
                (w.open_folded, w.history_folded, w.groups[0].collapsed)
            })
        };
        assert_eq!(state(cx), (true, true, false));

        cx.simulate_keystrokes("secondary-k");
        cx.simulate_input("session 1");
        assert_eq!(label(open_row(1), cx).as_deref(), Some("Session 1"));
        assert_eq!(label(("history", 0usize), cx).as_deref(), Some("Session 1"));
        press(("group-toggle", 0usize), cx);
        assert_eq!(label(open_row(1), cx).as_deref(), Some("Session 1"));
        assert_eq!(
            state(cx),
            (true, true, false),
            "nothing written while filtering"
        );

        cx.simulate_keystrokes("escape");
        assert_eq!(label(open_row(1), cx), None);
        assert_eq!(label(("history", 0usize), cx), None);
    }

    /// Open and History fold, and stay folded on a relaunch, as the groups keep their colours.
    #[gpui_kit::test]
    fn sections_fold_and_survive_a_relaunch(
        cx: &mut TestAppContext,
        relaunch: &mut TestAppContext,
    ) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));
        launch(&workspace, cx);
        while question(&workspace, cx).is_some() {
            click("approval-allow", cx);
        }
        start_now(&workspace, cx);
        for name in ["A", "B"] {
            press("new-group", cx);
            type_name(name, "enter", cx);
        }
        let folded = |workspace: &Entity<Workspace>, cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| (w.open_folded, w.history_folded))
        };
        assert_eq!(folded(&workspace, cx), (false, false));
        assert!(label(open_row(0), cx).is_some());
        press("open-section", cx);
        assert!(label(open_row(0), cx).is_none());
        assert!(label(group(0), cx).is_none());
        press("history-section", cx);
        assert_eq!(folded(&workspace, cx), (true, true));
        let state = AppState::load(&path);
        assert!(state.open_folded && state.history_folded);

        relaunch.executor().allow_parking();
        let (workspace, cx) = open_at(relaunch, fake_settings(""), Some(path.clone()));
        launch(&workspace, cx);
        assert_eq!(folded(&workspace, cx), (true, true));
        assert_eq!(colors(&workspace, cx), [0, 1]);
        assert!(label(open_row(0), cx).is_none());
        press("open-section", cx);
        assert!(label(open_row(0), cx).is_some());
        assert_eq!(
            label(("open-age", 0usize), cx),
            None,
            "a restored session has no time"
        );
    }

    /// A History row spans the panel: a short title's row reaches its right edge, and a title
    /// wider than the panel truncates, leaving Open to the side inside it.
    #[gpui_kit::test]
    fn history_rows_span_the_panel(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        workspace.update(cx, |w, cx| {
            let summary = |id: &str, title: String| SessionSummary {
                id: id.into(),
                cwd: w.project.clone(),
                title: Some(title),
                updated_at: Some("2026-11-11T09:30:00Z".into()),
            };
            w.history = vec![
                (
                    w.project.clone(),
                    summary("long", "a very long title ".repeat(20)),
                ),
                (w.project.clone(), summary("short", "Short".into())),
            ];
            cx.notify();
        });
        let sidebar = cx.debug_bounds("sidebar").unwrap();
        reveal(("history-side", 0usize), cx);
        let (side, short) = cx.update(|window, cx| {
            window.render_frame(cx);
            let side = window.find(("history-side", 0usize));
            assert!(side.visible(), "Open to the side is drawn");
            (side.bounds(), window.find(("history", 1usize)).bounds())
        });
        assert!(side.right() <= sidebar.right(), "{side:?} in {sidebar:?}");
        assert!(
            short.right() > sidebar.right() - px(24.),
            "{short:?} in {sidebar:?}"
        );
    }

    /// History takes the room left under Open and scrolls on its own: a long history keeps the
    /// open rows in view and draws only the rows that fit.
    #[gpui_kit::test]
    fn long_history_scrolls_under_open(cx: &mut TestAppContext) {
        let (workspace, cx) = with_sessions(1, cx);
        cx.simulate_resize(size(px(1000.), px(600.)));
        workspace.update(cx, |w, cx| {
            let row = |ix: usize| SessionSummary {
                id: format!("h{ix}"),
                cwd: w.project.clone(),
                title: Some(format!("History {ix}")),
                updated_at: None,
            };
            w.history = (0..80).map(|ix| (w.project.clone(), row(ix))).collect();
            cx.notify();
        });
        cx.update(|window, cx| {
            window.render_frame(cx);
            let open = window.find(open_row(0));
            assert!(open.visible(), "the open row is in view: {open:?}");
            assert!(window.try_find(("history-side", 0usize)).is_some());
            assert!(
                window.try_find(("history-side", 79usize)).is_none(),
                "the last history row is not drawn until scrolled to"
            );
        });
    }

    /// A row's hover buttons stay invisible until the row is hovered, and one reached with Tab
    /// shows: the keyboard never lands on a button it cannot see.
    #[gpui_kit::test]
    fn hover_buttons_show_on_hover_and_keyboard_focus(cx: &mut TestAppContext) {
        let (_workspace, cx) = with_sessions(1, cx);
        let close = ElementId::from(("open-close", 0usize));
        let button = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.find(close.clone())
            })
        };
        assert!(!button(cx).visible(), "hidden until hovered");
        cx.update(|window, cx| window.hover(open_row(0), cx));
        assert!(button(cx).visible(), "shown on hover");
        cx.simulate_mouse_move(point(px(0.), px(0.)), None, Modifiers::none());
        assert!(!button(cx).visible());

        cx.simulate_keystrokes("secondary-k");
        for _ in 0..100 {
            if button(cx).focused() == Some(true) {
                break;
            }
            cx.simulate_keystrokes("tab");
        }
        let focused = button(cx);
        assert_eq!(focused.focused(), Some(true), "Tab reaches End session");
        assert!(focused.visible(), "shown while focused: {focused:?}");
    }

    #[gpui_kit::test]
    fn new_groups_take_the_next_colour(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, Settings::default());
        for name in ["A", "B", "C"] {
            press("new-group", cx);
            type_name(name, "enter", cx);
        }
        assert_eq!(colors(&workspace, cx), [0, 1, 2]);
    }

    /// Groups saved before swatches all load as colour 0: the first launch spreads them over the
    /// cycle.
    #[gpui_kit::test]
    fn groups_saved_without_colours_take_the_cycle(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        let state = AppState {
            groups: vec![named("A", &[]), named("B", &[]), named("C", &[])],
            ..AppState::default()
        };
        state.save(&path).unwrap();
        let (workspace, cx) = open_at(cx, Settings::default(), Some(path));
        launch(&workspace, cx);
        assert_eq!(colors(&workspace, cx), [0, 1, 2]);
    }
}
