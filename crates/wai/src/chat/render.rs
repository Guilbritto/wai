//! How a session draws: the header, each row of the message list, the whole view.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::clipboard::Clipboard;
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::select::{Select, SelectState};
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

/// Whether row `ix` is the first agent message of its turn, or since a message steered into it:
/// it gets the "✦ agent" header.
pub(crate) fn starts_reply(items: &[ChatItem], ix: usize) -> bool {
    matches!(items.get(ix), Some(ChatItem::Agent { .. }))
        && !(items[..ix].iter().rev())
            .take_while(|item| !matches!(item, ChatItem::User { .. } | ChatItem::Steered { .. }))
            .any(|item| matches!(item, ChatItem::Agent { .. }))
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

/// A tool call's status mark and its color.
fn status_icon(status: ToolStatus, cx: &App) -> Icon {
    let theme = cx.theme();
    let done = match theme.is_dark() {
        true => aurora::boreal(),
        false => theme.success,
    };
    let (icon, color) = match status {
        ToolStatus::Pending => (IconName::Ellipsis, theme.muted_foreground),
        ToolStatus::InProgress => (IconName::Loader, theme.foreground),
        ToolStatus::Completed => (IconName::Check, done),
        ToolStatus::Failed => (IconName::CircleX, theme.danger),
    };
    Icon::new(icon).xsmall().text_color(color)
}

/// The muted color of a step line's chevron and kind icon.
fn icon_color(cx: &App) -> Hsla {
    match cx.theme().is_dark() {
        true => aurora::nevoa_2(),
        false => cx.theme().muted_foreground,
    }
}

fn chevron(open: bool, cx: &App) -> Icon {
    let icon = match open {
        true => IconName::ChevronDown,
        false => IconName::ChevronRight,
    };
    Icon::new(icon).xsmall().text_color(icon_color(cx))
}

/// The icon of a tool call's kind.
fn kind_icon(kind: &str) -> IconName {
    match kind {
        "execute" => IconName::SquareTerminal,
        "read" => IconName::FileText,
        "edit" => IconName::Replace,
        "search" => IconName::Search,
        "fetch" => IconName::Globe,
        "delete" => IconName::Delete,
        "move" => IconName::ArrowRight,
        "subagent" => IconName::Bot,
        "think" => IconName::Asterisk,
        _ => IconName::Dash,
    }
}

/// A step's one compact line (`.t`): glass, 13 px regular text, muted.
pub(super) fn step_line(cx: &App) -> Div {
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
                    .flex_1()
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
            .children(
                self.pickers
                    .iter()
                    .map(|(_, select)| div().w(px(140.)).child(Select::new(select).small())),
            )
            .children(
                self.legacy_mode_picker()
                    // The select's own view is `size_full`: the box sets its width.
                    .map(|modes| div().w(px(140.)).child(Select::new(modes).small())),
            )
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

    /// Row `ix` as the processing mode shows it; none for a step that is hidden or sits in a
    /// folded group. Grouped, a run of steps is headed by its first row.
    fn render_item(&mut self, ix: usize, cx: &mut Context<Self>) -> Option<AnyElement> {
        if !self.model.items[ix].is_step() || self.processing == Processing::Steps {
            return Some(self.render_row(ix, cx));
        }
        if self.processing == Processing::Hidden {
            return None;
        }
        let run = self.model.run(ix, ChatItem::is_step);
        let open = self.open_groups.contains(&run.start);
        let row = open.then(|| self.render_row(ix, cx));
        if ix != run.start {
            return row;
        }
        let head = self.render_group(run, open, cx);
        Some(
            v_flex()
                .gap_2()
                .child(head)
                .children(row)
                .into_any_element(),
        )
    }

    /// The head of a run of steps: `3 steps · <the latest>`, with the run's status. A click
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
            .child(chevron(open, cx))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(self.model.run_label(run)),
            )
            .child(status_icon(status, cx))
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
            // `.msg.voce`
            ChatItem::User { text } | ChatItem::Steered { text } => {
                let bubble = div()
                    .max_w(relative(0.88))
                    .px_3()
                    .py(px(9.))
                    .rounded(px(16.))
                    .rounded_br(px(6.))
                    .border_1();
                let bubble = match dark {
                    true => bubble
                        .bg(linear_gradient(
                            160.,
                            linear_color_stop(aurora::lilas().alpha(0.32), 0.),
                            linear_color_stop(aurora::lilas().alpha(0.18), 1.),
                        ))
                        .border_color(aurora::lilas().alpha(0.35))
                        .text_color(aurora::tinta()),
                    false => bubble.bg(theme.secondary).border_color(theme.border),
                };
                h_flex()
                    .w_full()
                    .justify_end()
                    .child(bubble.child(text.clone()))
                    .into_any_element()
            }
            // `.msg.claude`, its turn's first one headed "✦ agent" (`.msg-quem`).
            ChatItem::Agent { .. } => {
                let header = starts_reply(&self.model.items, ix).then(|| {
                    let (star, name) = match dark {
                        true => (aurora::brasa(), rgb(0xffc6a3).into()),
                        false => (theme.warning, muted),
                    };
                    h_flex()
                        .gap_1()
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child(div().text_color(star).child("✦"))
                        .child(div().text_color(name).child(self.agent.clone()))
                });
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
                glass(h_flex(), cx)
                    .items_start()
                    .gap_2()
                    .p_3()
                    .rounded(px(16.))
                    .rounded_bl(px(6.))
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .children(header)
                            .children(text),
                    )
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
                            .child(chevron(open, cx))
                            .child(
                                Icon::new(kind_icon("think"))
                                    .xsmall()
                                    .text_color(icon_color(cx)),
                            )
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
            .into_any_element(),
            ChatItem::Question { request, answered } => permission::closed(
                ("question", ix).into(),
                &request.message,
                answered.as_deref().unwrap_or_default(),
                cx,
            )
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
        let kind = kind_icon(if edit { "edit" } else { &tool.kind });
        let line = step_line(cx)
            .id((id, ix))
            .child(chevron(open, cx))
            .child(Icon::new(kind).xsmall().text_color(icon_color(cx)))
            .child(div().flex_1().min_w_0().truncate().child(title))
            .child(status_icon(tool.status, cx))
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
        if self.model.waiting.is_empty() {
            return None;
        }
        let waiting = self.model.waiting.clone();
        let on_open = Self::on_open(cx);
        let cards = waiting.iter().map(|card| match card {
            ChatItem::Permission { request, .. } => {
                let id = request.id;
                let on_choose =
                    Rc::new(cx.listener(move |this, choice: &PermissionChoice, _, cx| {
                        this.answer(id, choice, cx)
                    }));
                let diffs = self.card_diffs.get(&id).map_or(&[][..], Vec::as_slice);
                permission::card(request, diffs, on_choose, &on_open, cx).into_any_element()
            }
            ChatItem::Question { request, .. } => {
                question::card(request, &mut self.questions, window, cx)
            }
            _ => div().into_any_element(),
        });
        let cards: Vec<_> = cards.collect();
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
            // Rows sit 8 px apart, not the scroller's 32: the scroller's gap
            // goes and each row but the last carries its own. The scroller remeasures the row
            // before an append, whose "last" flips.
            false => messages.child({
                let scroller = MessageScroller::new(
                    "messages",
                    self.scroller.clone(),
                    cx.processor(|this, ix, _, cx| {
                        let last = ix + 1 == this.model.items.len();
                        let row = this.render_item(ix, cx);
                        div()
                            .when(!last && row.is_some(), |el| el.pb_2())
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

    use super::{starts_reply, tokens};
    use crate::chat::model::{ChatItem, ToolItem};

    #[test]
    fn token_counts_are_short() {
        assert_eq!(
            [999, 43_000, 1_000_000, 1_500_000].map(tokens),
            ["999", "43k", "1M", "1.5M"]
        );
    }

    #[test]
    fn reply_header_once_per_turn() {
        let text = String::new;
        let user = || ChatItem::User { text: text() };
        let agent = || ChatItem::Agent { text: text() };
        let tool = ChatItem::Tool(ToolItem {
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
        });
        let items = [user(), agent(), tool, agent()];
        assert!(starts_reply(&items, 1));
        assert!(!starts_reply(&items, 3));
        assert!(starts_reply(&[agent()], 0));
        let items = [user(), ChatItem::Thought { text: text() }, agent()];
        assert!(starts_reply(&items, 2));
        assert!(!starts_reply(&items, 1));
        assert!(starts_reply(&[user(), agent(), user(), agent()], 3));
        let steered = ChatItem::Steered { text: text() };
        assert!(starts_reply(&[user(), agent(), steered, agent()], 3));
    }
}
