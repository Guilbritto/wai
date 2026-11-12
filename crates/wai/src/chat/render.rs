//! How a session draws: the header, each row of the message list, the whole view.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::select::SelectState;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::{PermissionChoice, ToolStatus};
use wai_settings::Processing;

use super::model::{ChatItem, NoticeKind, ToolItem, TurnState, UsageInfo, show_label};
use super::view::{ChatEvent, ChatSession, Mode};
use super::{permission, question};
use crate::aurora::{self, DISPLAY_FONT};
use crate::workspace::{Dragged, DraggedSession};

/// The balloon of a message you sent (`.me`).
fn user_bubble(cx: &App) -> Div {
    let theme = cx.theme();
    let bubble = div()
        .max_w(relative(0.7))
        .px(px(14.))
        .py(px(10.))
        .rounded(px(16.))
        .border_1();
    match theme.is_dark() {
        true => bubble
            .bg(aurora::lilas().alpha(0.22))
            .border_color(aurora::lilas().alpha(0.5))
            .text_color(aurora::tinta()),
        false => bubble.bg(theme.secondary).border_color(theme.border),
    }
}

/// A message waiting in the pinned stack: a dimmed balloon under a muted `caption`, its text
/// clamped to 3 lines, with `remove` to its left.
pub(super) fn pinned_message(
    id: ElementId,
    caption: &'static str,
    text: &str,
    remove: Option<Button>,
    cx: &App,
) -> Stateful<Div> {
    let caption = div()
        .text_xs()
        .text_color(cx.theme().muted_foreground)
        .child(caption);
    h_flex()
        .id(id)
        .w_full()
        .justify_end()
        .items_center()
        .gap_2()
        .children(remove)
        .child(
            user_bubble(cx)
                .opacity(0.75)
                .child(caption)
                .child(div().line_clamp(3).text_ellipsis().child(text.to_string())),
        )
}

/// `43k`, `1M`, `1.5M`.
fn tokens(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{}k", n / 1_000),
        _ if n.is_multiple_of(1_000_000) => format!("{}M", n / 1_000_000),
        _ => format!("{:.1}M", n as f64 / 1e6),
    }
}

/// `43k / 1M`, then the rate-limit status when the agent reports one.
fn usage_label(usage: &UsageInfo) -> String {
    let label = format!("{} / {}", tokens(usage.used), tokens(usage.size));
    match &usage.rate_limit_status {
        Some(status) => format!("{label} · {status}"),
        None => label,
    }
}

/// Where a row sits in its turn's card: which segment edges it draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct CardEdges {
    pub first: bool,
    pub last: bool,
    /// The "✦ agent" header: once per turn, on its first card row that draws anything.
    pub header: bool,
}

/// The card edges of row `ix`, or `None` when it is not drawn inside a card. A run of
/// consecutive `in_card` rows is one card; rows `shown` says draw nothing do not count.
pub(crate) fn card_edges(
    items: &[ChatItem],
    ix: usize,
    shown: impl Fn(usize) -> bool,
) -> Option<CardEdges> {
    if !items.get(ix)?.in_card() || !shown(ix) {
        return None;
    }
    let before = items[..ix].iter().rev().take_while(|i| i.in_card()).count();
    let after = items[ix..].iter().take_while(|i| i.in_card()).count();
    let run = ix - before..ix + after;
    let first = !run.clone().take(ix - run.start).any(&shown);
    let last = !(ix + 1..run.end).any(&shown);
    let turn = (items[..run.start].iter())
        .rposition(|i| matches!(i, ChatItem::User { .. } | ChatItem::Steered { .. }))
        .map_or(0, |u| u + 1);
    let header = first && !(turn..run.start).any(|j| items[j].in_card() && shown(j));
    Some(CardEdges {
        first,
        last,
        header,
    })
}

/// A card segment: the side borders always, the top (corners, padding) on the first row and the
/// bottom on the last. Dark: a faint fill; light: a border only.
fn card_segment(edges: CardEdges, cx: &App) -> Div {
    let theme = cx.theme();
    let seg = v_flex()
        .w_full()
        .px(px(14.))
        .border_l_1()
        .border_r_1()
        .when(edges.first, |el| {
            el.border_t_1().rounded_t(px(14.)).pt(px(10.))
        })
        .when(edges.last, |el| {
            el.border_b_1().rounded_b(px(14.)).pb(px(10.))
        });
    match theme.is_dark() {
        true => seg
            .bg(hsla(0., 0., 1., 0.035))
            .border_color(aurora::borda()),
        false => seg.border_color(theme.border),
    }
}

/// `el` as glass (`.msg.claude`): a `vidro` fill and a `borda` border. Light mode: a border.
pub(super) fn glass(el: Div, cx: &App) -> Div {
    let theme = cx.theme();
    let el = el.border_1();
    match theme.is_dark() {
        true => el.bg(aurora::vidro()).border_color(aurora::borda()),
        false => el.border_color(theme.border),
    }
}

/// A step's status mark (`.step svg`): 14 px.
fn status_icon(status: ToolStatus, cx: &App) -> Icon {
    let theme = cx.theme();
    let dark = theme.is_dark();
    let (icon, color) = match (status, dark) {
        (ToolStatus::Pending, true) => (IconName::Ellipsis, aurora::nevoa_2()),
        (ToolStatus::Pending, false) => (IconName::Ellipsis, theme.muted_foreground),
        (ToolStatus::InProgress, true) => (IconName::Loader, aurora::nevoa()),
        (ToolStatus::InProgress, false) => (IconName::Loader, theme.foreground),
        (ToolStatus::Completed, true) => (IconName::Check, aurora::ok()),
        (ToolStatus::Completed, false) => (IconName::Check, theme.success),
        (ToolStatus::Failed, true) => (IconName::CircleX, aurora::erro()),
        (ToolStatus::Failed, false) => (IconName::CircleX, theme.danger),
    };
    Icon::new(icon).size(px(14.)).flex_none().text_color(color)
}

/// `title` with its `` `code` `` spans unquoted: the text, then the byte ranges of the code in it.
/// An unclosed backtick stays text.
pub(crate) fn code_ranges(title: &str) -> (String, Vec<std::ops::Range<usize>>) {
    let (mut text, mut ranges) = (String::new(), Vec::new());
    let mut rest = title;
    while let Some(open) = rest.find('`') {
        let Some(len) = rest[open + 1..].find('`') else {
            break;
        };
        text.push_str(&rest[..open]);
        let start = text.len();
        text.push_str(&rest[open + 1..open + 1 + len]);
        ranges.push(start..text.len());
        rest = &rest[open + len + 2..];
    }
    text.push_str(rest);
    (text, ranges)
}

/// A step title as one line of text, its code spans highlighted (`.step code`).
fn step_title(title: &str, cx: &App) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let (text, ranges) = code_ranges(title);
    let style = match theme.is_dark() {
        true => HighlightStyle {
            color: Some(aurora::tinta_2()),
            background_color: Some(aurora::vidro()),
            ..Default::default()
        },
        false => HighlightStyle {
            color: Some(theme.foreground),
            background_color: Some(theme.muted),
            ..Default::default()
        },
    };
    let mono = theme.mono_font_family.clone();
    let overrides: Vec<_> = ranges.iter().map(|r| (r.clone(), mono.clone())).collect();
    div().flex_1().min_w_0().truncate().child(
        StyledText::new(text)
            .with_highlights(ranges.into_iter().map(|r| (r, style)))
            .with_font_family_overrides(overrides),
    )
}

/// A step's line (`.step`): no box, 13 px, muted and `tinta` on hover.
pub(super) fn step_line(cx: &App) -> Div {
    let theme = cx.theme();
    let line = h_flex()
        .gap(px(8.))
        .py(px(3.))
        .text_size(px(13.))
        .font_weight(FontWeight::NORMAL)
        .cursor_pointer();
    match theme.is_dark() {
        true => line
            .text_color(aurora::nevoa())
            .hover(|el| el.text_color(aurora::tinta())),
        false => line
            .text_color(theme.muted_foreground)
            .hover(|el| el.text_color(theme.foreground)),
    }
}

/// A closed card's compact line (`.t`): glass, 13 px regular text, muted.
pub(super) fn boxed_line(cx: &App) -> Div {
    let theme = cx.theme();
    let line = h_flex()
        .gap(px(6.))
        .px(px(9.))
        .py(px(5.))
        .rounded(px(8.))
        .border_1()
        .text_size(px(13.))
        .font_weight(FontWeight::NORMAL)
        .cursor_pointer();
    match theme.is_dark() {
        true => line
            .bg(aurora::vidro())
            .border_color(aurora::borda())
            .text_color(aurora::nevoa()),
        false => line
            .border_color(theme.border)
            .text_color(theme.muted_foreground),
    }
}

/// The box under a step line (`.out`) that holds its output.
fn output_box(cx: &App) -> Div {
    let theme = cx.theme();
    let out = v_flex()
        .px(px(8.))
        .py(px(6.))
        .rounded(px(6.))
        .border_1()
        .font_family(theme.mono_font_family.clone())
        .text_size(px(11.));
    match theme.is_dark() {
        true => out
            .bg(aurora::noite())
            .border_color(aurora::borda())
            .text_color(aurora::nevoa()),
        false => out
            .bg(theme.muted)
            .border_color(theme.border)
            .text_color(theme.muted_foreground),
    }
}

impl ChatSession {
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let title = self
            .model
            .shown_title()
            .unwrap_or("New session")
            .to_string();
        let dragged = DraggedSession {
            title: title.clone().into(),
            source: Dragged::Open(cx.entity()),
        };
        aurora::card_bar(cx)
            .gap_3()
            .px_3()
            .py_2()
            // R89: the title drags the session to another pane.
            .child(
                div()
                    .id("session-title")
                    .flex_shrink(1.)
                    .min_w_0()
                    .truncate()
                    .cursor_grab()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .aria_label(title.clone())
                    .child(title)
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            // The title takes the room; the rest goes to the right.
            .child(div().flex_1())
            .children(
                (self.model.usage.as_ref())
                    .map(|usage| aurora::pill(cx).flex_none().child(usage_label(usage))),
            )
            .child(
                Button::new("session-close")
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Close pane (the session stays open)")
                    .accessibility_label("Close pane")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(ChatEvent::Hide))),
            )
    }

    /// Whether row `ix` draws anything under the processing mode: Step by step, every row;
    /// Hidden, no step; Grouped, a step only when it heads its run or its group is open.
    pub(super) fn shown(&self, ix: usize) -> bool {
        if !self.model.items[ix].is_step() {
            return true;
        }
        match self.processing {
            Processing::Steps => true,
            Processing::Hidden => false,
            Processing::Grouped => {
                let run = self.model.run(ix, ChatItem::is_step);
                ix == run.start || self.open_groups.contains(&run.start)
            }
        }
    }

    /// Row `ix` as the processing mode shows it; none exactly when `!shown(ix)`. Grouped, a run
    /// of steps is headed by its first row.
    fn render_item(&mut self, ix: usize, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.shown(ix) {
            return None;
        }
        if !self.model.items[ix].is_step() || self.processing == Processing::Steps {
            return Some(self.render_row(ix, cx));
        }
        let run = self.model.run(ix, ChatItem::is_step);
        let open = self.open_groups.contains(&run.start);
        let row = open.then(|| self.render_row(ix, cx));
        if ix != run.start {
            return row;
        }
        let head = self.render_group(run, open, cx);
        Some(v_flex().child(head).children(row).into_any_element())
    }

    /// The "✦ agent" header of a turn's card (`.agent-h`).
    fn card_header(&self, ix: usize, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (star, name) = match theme.is_dark() {
            true => (aurora::brasa(), aurora::brasa()),
            false => (theme.warning, theme.muted_foreground),
        };
        h_flex()
            .id(("reply-head", ix))
            .gap_1()
            .mb(px(6.))
            .text_size(px(12.5))
            .font_weight(FontWeight::SEMIBOLD)
            .child(div().text_color(star).child("✦"))
            .child(
                div()
                    .text_color(name)
                    .child(wai_settings::agent_label(&self.agent)),
            )
            // Registers the id so tests can find it.
            .test_support()
    }

    /// The head of a run of steps: `3 steps · <each tool's title once>`, with the run's status. A click
    /// unfolds the run into its step lines.
    fn render_group(
        &self,
        run: std::ops::Range<usize>,
        open: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let head = run.start;
        let has = |status: ToolStatus| {
            let mut tools = self.model.items[run.clone()].iter();
            tools.any(|item| matches!(item, ChatItem::Tool(t) if t.status == status))
        };
        let status = if has(ToolStatus::Failed) {
            ToolStatus::Failed
        } else if has(ToolStatus::Pending) || has(ToolStatus::InProgress) {
            ToolStatus::InProgress
        } else {
            ToolStatus::Completed
        };
        step_line(cx)
            .id(("group", head))
            .child(status_icon(status, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.model.run_label(run)),
            )
            .child(
                Icon::new(match open {
                    true => IconName::ChevronDown,
                    false => IconName::ChevronRight,
                })
                .size(px(12.))
                .flex_none()
                .text_color(match cx.theme().is_dark() {
                    true => aurora::nevoa_2(),
                    false => cx.theme().muted_foreground,
                }),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.open_groups.remove(&head) {
                    this.open_groups.insert(head);
                }
                this.remeasure(this.model.run(head, ChatItem::is_step), cx);
                cx.notify();
            }))
            // Registers the id so tests can find and click it.
            .test_support()
    }

    fn render_row(&mut self, ix: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (dark, muted) = (theme.is_dark(), theme.muted_foreground);
        match &self.model.items[ix] {
            // `.me`
            ChatItem::User { text } | ChatItem::Steered { text } => h_flex()
                .w_full()
                .justify_end()
                .child(user_bubble(cx).child(text.clone()))
                .into_any_element(),
            // `.say`, inside its turn's card.
            ChatItem::Agent { .. } => {
                // Read at click time, so a frame copies no text.
                let session = cx.weak_entity();
                let copy = Clipboard::new(("copy-message", ix))
                    .tooltip("Copy message")
                    .accessibility_label("Copy message")
                    .xsmall()
                    .value_fn(move |_, cx| {
                        let text =
                            session.read_with(cx, |this, _| match this.model.items.get(ix) {
                                Some(ChatItem::Agent { text }) => text.clone(),
                                _ => String::new(),
                            });
                        text.unwrap_or_default().into()
                    });
                let text =
                    self.texts.get(&ix).map(|md| {
                        TextView::new(&md.state).code_block_actions(move |block, _, _| {
                            h_flex()
                                .gap_1()
                                .children(block.lang().map(|lang| {
                                    div().text_xs().text_color(muted).child(lang.clone())
                                }))
                                .child(
                                    Clipboard::new("copy-code")
                                        .value(block.code())
                                        .tooltip("Copy code")
                                        .accessibility_label("Copy code"),
                                )
                        })
                    });
                let say = div().flex_1().min_w_0().py(px(4.)).children(text);
                let say = match dark {
                    true => say.text_color(aurora::tinta()),
                    false => say,
                };
                h_flex()
                    .items_start()
                    .gap_2()
                    .child(say)
                    .child(copy)
                    .into_any_element()
            }
            ChatItem::Thought { .. } => {
                let open = self.open_thoughts.contains(&ix);
                v_flex()
                    .gap_1()
                    .child(
                        step_line(cx)
                            .id(("thought", ix))
                            .child(Icon::new(IconName::Asterisk).size(px(14.)).text_color(
                                match dark {
                                    true => aurora::nevoa_2(),
                                    false => muted,
                                },
                            ))
                            .child("Thought")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.open_thoughts.remove(&ix) {
                                    this.open_thoughts.insert(ix);
                                }
                                this.remeasure(ix..ix + 1, cx);
                                cx.notify();
                            }))
                            // Registers the id so tests can find it.
                            .test_support(),
                    )
                    .when(open, |el| {
                        el.children(
                            self.texts
                                .get(&ix)
                                .map(|md| output_box(cx).child(TextView::new(&md.state))),
                        )
                    })
                    .into_any_element()
            }
            ChatItem::Tool(tool) => self.render_tool(ix, tool, cx).into_any_element(),
            ChatItem::Permission { request, answered } => permission::closed(
                ("permission", ix).into(),
                &request.title,
                answered.as_deref().unwrap_or_default(),
                cx,
            )
            // Registers the id so tests can find it.
            .test_support()
            .into_any_element(),
            ChatItem::Question { request, answered } => permission::closed(
                ("question", ix).into(),
                request.message.lines().next().unwrap_or_default(),
                answered.as_deref().unwrap_or_default(),
                cx,
            )
            // Registers the id so tests can find it.
            .test_support()
            .into_any_element(),
            ChatItem::Notice { text, kind } => {
                let restart = *kind == NoticeKind::Error
                    && ix + 1 == self.model.items.len()
                    && matches!(self.model.state, TurnState::Error(_));
                // A long notice wraps inside its pill.
                let pill = aurora::pill(cx)
                    .h_auto()
                    .min_h(px(26.))
                    .py_1()
                    .max_w_full()
                    .text_size(px(11.5))
                    .child(div().min_w_0().child(text.clone()));
                let pill = match (kind, dark) {
                    (NoticeKind::Info, true) => pill.text_color(aurora::nevoa_2()),
                    (NoticeKind::Error, true) => pill
                        .text_color(aurora::erro())
                        .bg(aurora::erro().alpha(0.12))
                        .border_color(aurora::erro().alpha(0.4)),
                    (NoticeKind::Info, false) => pill.text_color(muted),
                    (NoticeKind::Error, false) => pill.text_color(theme.danger),
                };
                h_flex()
                    .w_full()
                    .justify_center()
                    .gap_2()
                    .child(pill)
                    .when(restart, |el| {
                        el.child(Button::new("restart").small().label("Restart").on_click(
                            cx.listener(|this, _, window, cx| this.load(false, window, cx)),
                        ))
                    })
                    .into_any_element()
            }
            ChatItem::Edits { turn, files } => {
                self.render_edits(ix, *turn, files, cx).into_any_element()
            }
        }
    }

    /// One folded line: the call's title and status; a click unfolds its output and diffs. An
    /// edit that reached the disk reads `Edited <file>`, in every processing mode.
    fn render_tool(
        &self,
        ix: usize,
        tool: &ToolItem,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let open = self.open_steps.contains(&ix);
        let edit = self.model.items[ix].is_edit();
        let (id, title) = match edit {
            true => {
                let mut files: Vec<String> = Vec::new();
                for diff in &tool.diffs {
                    let path = diff.path.strip_prefix(&self.cwd).unwrap_or(&diff.path);
                    let name = wai_settings::shown(path);
                    if !files.contains(&name) {
                        files.push(name);
                    }
                }
                ("edited", format!("Edited {}", files.join(", ")))
            }
            false => ("step", tool.title.clone()),
        };
        let (output, hidden) = tool.visible_text();
        let on_open = Self::on_open(cx);
        let line = step_line(cx)
            .id((id, ix))
            .child(status_icon(tool.status, cx))
            .children(
                (tool.kind == "subagent" || tool.subagent.is_some()).then(|| {
                    Icon::new(IconName::Bot)
                        .size(px(12.))
                        .flex_none()
                        .text_color(match cx.theme().is_dark() {
                            true => aurora::nevoa_2(),
                            false => cx.theme().muted_foreground,
                        })
                }),
            )
            .child(step_title(&title, cx))
            .on_click(cx.listener(move |this, _, _, cx| {
                if !this.open_steps.remove(&ix) {
                    this.open_steps.insert(ix);
                }
                this.remeasure(ix..ix + 1, cx);
                cx.notify();
            }))
            // Registers the id so tests can find and click it.
            .test_support();
        let card = v_flex().id(("tool", ix)).gap_1().child(line).test_support();
        if !open {
            return card;
        }
        card.when(!output.is_empty(), |el| {
            el.child(
                output_box(cx)
                    .id(("step-output", ix))
                    .child(output.to_string())
                    .test_support(),
            )
        })
        .children(hidden.map(|n| {
            match tool.expanded {
                false => Button::new(("show-all", ix))
                    .ghost()
                    .small()
                    .label(show_label(n))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(ChatItem::Tool(tool)) = this.model.items.get_mut(ix) {
                            tool.expanded = true;
                        }
                        this.sync_rows(Some(ix), cx);
                        cx.notify();
                    })),
                // R21: past what "Show all" draws, the whole output opens in the editor.
                true => Button::new(("open-output", ix))
                    .ghost()
                    .small()
                    .label("Open full output in editor")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(ChatItem::Tool(tool)) = this.model.items.get(ix) {
                            let (tool, text) = (tool.id.clone(), tool.text.clone());
                            cx.emit(ChatEvent::OpenOutput { tool, text });
                        }
                    })),
            }
        }))
        .children(
            self.diffs
                .get(&ix)
                .into_iter()
                .flatten()
                .enumerate()
                .map(|(k, d)| d.render(("diff", k).into(), &on_open, cx)),
        )
    }

    /// The old mode picker, unless the agent offers `mode` as a config option.
    pub(super) fn legacy_mode_picker(&self) -> Option<&Entity<SelectState<Vec<Mode>>>> {
        self.modes.as_ref().filter(|_| !self.has_mode_option())
    }

    fn has_mode_option(&self) -> bool {
        (self.model.config.iter()).any(|o| o.category == wai_agent::ConfigCategory::Mode)
    }

    /// Follows a mode the agent switched to on its own.
    fn sync_mode_select(&self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(select), Some(mode)) = (&self.modes, &self.model.current_mode) else {
            return;
        };
        if select.read(cx).selected_value() != Some(mode) {
            select.update(cx, |select, cx| select.set_selected_value(mode, window, cx));
        }
    }
}

impl ChatSession {
    /// The O1 stack: every waiting card, oldest first, as a glass panel laid over the list's
    /// bottom. `None` when nothing waits.
    fn render_waiting(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if self.model.waiting.is_empty() && self.queued.is_empty() && self.model.steering.is_empty()
        {
            return None;
        }
        let waiting = self.model.waiting.clone();
        let on_open = Self::on_open(cx);
        let cards = waiting.iter().enumerate().map(|(k, card)| match card {
            ChatItem::Permission { request, .. } => {
                let id = request.id;
                let on_choose =
                    Rc::new(cx.listener(move |this, choice: &PermissionChoice, _, cx| {
                        this.answer(id, choice, cx)
                    }));
                let diffs = self.card_diffs.get(&id).map_or(&[][..], Vec::as_slice);
                permission::card(request, diffs, on_choose, &on_open, k == 0, cx).into_any_element()
            }
            ChatItem::Question { request, .. } => {
                question::card(request, &mut self.questions, window, cx)
            }
            _ => div().into_any_element(),
        });
        let cards: Vec<_> = cards.collect();
        let queued: Vec<_> = (self.queued.iter().enumerate())
            .map(|(ix, message)| {
                let remove = Button::new(("queued-remove", ix))
                    .ghost()
                    .xsmall()
                    .icon(IconName::Close)
                    .tooltip("Back to the prompt box")
                    .accessibility_label("Back to the prompt box")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if ix >= this.queued.len() {
                            return;
                        }
                        let message = this.queued.remove(ix);
                        this.take_back(message);
                        cx.notify();
                    }));
                // Registers the id so tests can find it.
                pinned_message(
                    ("queued", ix).into(),
                    "Queued",
                    &message.shown(),
                    Some(remove),
                    cx,
                )
                .test_support()
            })
            .collect();
        let steering: Vec<_> = (self.model.steering.iter().enumerate())
            .map(|(ix, steer)| {
                pinned_message(("steering", ix).into(), "Sent", &steer.text, None, cx)
                    .test_support()
            })
            .collect();
        let view = cx.entity();
        let height = canvas(
            move |bounds, _, cx| {
                view.update(cx, |this, cx| {
                    this.set_waiting_height(bounds.size.height, cx)
                })
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();
        let theme = cx.theme();
        let stack = v_flex()
            .id("waiting")
            .absolute()
            .left_3()
            .right_3()
            .bottom_2()
            .max_h(relative(0.6))
            .overflow_y_scroll()
            .occlude()
            .gap_2()
            .p_2();
        let stack = match theme.is_dark() {
            true => stack
                .rounded(px(16.))
                .border_1()
                .border_color(aurora::borda_forte())
                .bg(aurora::noite_2().alpha(0.72))
                // CSS `0 12px 40px -12px`: GPUI's blur radius is half the CSS one.
                .shadow(vec![
                    BoxShadow::new(px(0.), px(12.), hsla(0., 0., 0., 0.8))
                        .blur_radius(px(20.))
                        .spread_radius(px(-12.)),
                ]),
            false => stack
                .rounded(px(16.))
                .border_1()
                .border_color(theme.border)
                .bg(theme.background.opacity(0.92))
                .shadow(vec![
                    BoxShadow::new(px(0.), px(8.), hsla(0., 0., 0., 0.25))
                        .blur_radius(px(12.))
                        .spread_radius(px(-8.)),
                ]),
        };
        Some(
            stack
                .child(height)
                .children(cards)
                .children(queued)
                .children(steering)
                .test_support()
                .into_any_element(),
        )
    }
}

impl Render for ChatSession {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_mode_select(window, cx);
        self.sync_pickers(window, cx);
        self.sync_unsent(window, cx);
        let empty = self.model.items.is_empty() && self.model.state == TurnState::Idle;
        let messages = div()
            .id("transcript")
            .relative()
            .flex_1()
            .min_h_0()
            .test_support();
        let waiting = self.render_waiting(window, cx);
        if waiting.is_none() && self.waiting_height != px(0.) {
            self.set_waiting_height(px(0.), cx);
        }
        let inset = self.waiting_height + px(16.);
        let messages = match empty {
            true => messages
                .flex()
                .items_center()
                .justify_center()
                .child(aurora::pill(cx).child("No messages yet.")),
            // Rows sit 12 px apart, not the scroller's 32: the scroller's gap goes and each
            // row but the last carries its own, except rows inside a card (only the card's last
            // does). The scroller remeasures the row before an append, whose "last" flips.
            false => messages.child({
                let scroller = MessageScroller::new(
                    "messages",
                    self.scroller.clone(),
                    cx.processor(|this, ix, _, cx| {
                        let last = ix + 1 == this.model.items.len();
                        let edges = card_edges(&this.model.items, ix, |j| this.shown(j));
                        let row = this.render_item(ix, cx);
                        let row = match (row, edges) {
                            (Some(row), Some(edges)) => Some(
                                card_segment(edges, cx)
                                    .id(("card", ix))
                                    .children(edges.header.then(|| this.card_header(ix, cx)))
                                    .child(row)
                                    .test_support()
                                    .into_any_element(),
                            ),
                            (row, _) => row,
                        };
                        let gap = edges.is_none_or(|e| e.last);
                        div()
                            .when(!last && gap && row.is_some(), |el| el.pb(px(12.)))
                            .children(row)
                    }),
                )
                .with_row_style(StyleRefinement::default().pb_0());
                match self.waiting_height > px(0.) {
                    true => scroller
                        .with_list_style(StyleRefinement::default().pb(inset))
                        .with_jump_button_style(StyleRefinement::default().mb(inset)),
                    false => scroller,
                }
            }),
        };
        v_flex()
            .key_context("ChatSession")
            .on_action(cx.listener(Self::cancel))
            .on_drop(cx.listener(Self::attach))
            .size_full()
            .child(self.render_header(cx))
            .child(messages.children(waiting))
            .child(self.render_prompt(window, cx))
    }
}

#[cfg(test)]
mod tests {
    use wai_agent::ToolStatus;

    use super::{CardEdges, card_edges, code_ranges, tokens};
    use crate::chat::model::{ChatItem, ToolItem};

    #[test]
    fn token_counts_are_short() {
        assert_eq!(
            [999, 43_000, 1_000_000, 1_500_000].map(tokens),
            ["999", "43k", "1M", "1.5M"]
        );
    }

    fn u() -> ChatItem {
        ChatItem::User {
            text: String::new(),
        }
    }
    fn s() -> ChatItem {
        ChatItem::Steered {
            text: String::new(),
        }
    }
    fn a() -> ChatItem {
        ChatItem::Agent {
            text: String::new(),
        }
    }
    fn n() -> ChatItem {
        ChatItem::Notice {
            text: String::new(),
            kind: crate::chat::model::NoticeKind::Info,
        }
    }
    fn t() -> ChatItem {
        let text = String::new;
        ChatItem::Tool(ToolItem {
            id: text(),
            title: text(),
            kind: text(),
            status: ToolStatus::Completed,
            text: text(),
            diffs: Vec::new(),
            locations: Vec::new(),
            first_sight: Vec::new(),
            expanded: false,
            subagent: None,
        })
    }

    #[test]
    fn card_edges_frame_each_run() {
        let all = |_| true;
        let e = |first, last, header| {
            Some(CardEdges {
                first,
                last,
                header,
            })
        };
        let items = [u(), t(), t(), a()];
        assert_eq!(card_edges(&items, 0, all), None);
        assert_eq!(card_edges(&items, 1, all), e(true, false, true));
        assert_eq!(card_edges(&items, 2, all), e(false, false, false));
        assert_eq!(card_edges(&items, 3, all), e(false, true, false));
        let items = [u(), a(), n(), a()];
        assert_eq!(card_edges(&items, 1, all), e(true, true, true));
        assert_eq!(
            card_edges(&items, 3, all),
            e(true, true, false),
            "one header per turn"
        );
        assert_eq!(card_edges(&[a()], 0, all), e(true, true, true));
        assert_eq!(
            card_edges(&[u(), a(), u(), a()], 3, all),
            e(true, true, true)
        );
        assert_eq!(
            card_edges(&[u(), a(), s(), a()], 3, all),
            e(true, true, true)
        );
    }

    #[test]
    fn card_edges_skip_hidden_rows() {
        let shown = |items: &[ChatItem]| {
            let steps: Vec<bool> = (items.iter())
                .map(|i| !matches!(i, ChatItem::Tool(_)))
                .collect();
            move |j: usize| steps[j]
        };
        let alone = Some(CardEdges {
            first: true,
            last: true,
            header: true,
        });
        let items = [u(), t(), t(), a()];
        assert_eq!(card_edges(&items, 1, shown(&items)), None);
        assert_eq!(card_edges(&items, 3, shown(&items)), alone);
        let items = [u(), a(), t()];
        assert_eq!(card_edges(&items, 1, shown(&items)), alone);
        let items = [u(), t(), n(), a()];
        assert_eq!(
            card_edges(&items, 3, shown(&items)).map(|e| e.header),
            Some(true),
            "an invisible card gives no header"
        );
    }

    #[test]
    #[allow(clippy::single_range_in_vec_init)]
    fn code_ranges_split_titles() {
        assert_eq!(
            code_ranges("Read README.md"),
            ("Read README.md".into(), vec![])
        );
        assert_eq!(
            code_ranges("`git status`"),
            ("git status".into(), vec![0..10])
        );
        assert_eq!(
            code_ranges("Run `cargo test` now"),
            ("Run cargo test now".into(), vec![4..14])
        );
        assert_eq!(
            code_ranges("a `b"),
            ("a `b".into(), vec![]),
            "an unclosed backtick stays text"
        );
        assert_eq!(code_ranges(""), (String::new(), vec![]));
    }
}
