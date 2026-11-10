//! How a session draws: the header, each row of the message list, the whole view.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
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

/// Whether row `ix` is the first agent message of its turn, which gets the "✦ agent" header.
pub(crate) fn starts_reply(items: &[ChatItem], ix: usize) -> bool {
    matches!(items.get(ix), Some(ChatItem::Agent { .. }))
        && !(items[..ix].iter().rev())
            .take_while(|item| !matches!(item, ChatItem::User { .. }))
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
    let (icon, color) = match status {
        ToolStatus::Pending => (IconName::Ellipsis, theme.muted_foreground),
        ToolStatus::InProgress => (IconName::Loader, theme.foreground),
        ToolStatus::Completed => (IconName::CircleCheck, theme.success),
        ToolStatus::Failed => (IconName::CircleX, theme.danger),
    };
    Icon::new(icon).small().text_color(color)
}

fn chevron(open: bool) -> Icon {
    let icon = match open {
        true => IconName::ChevronDown,
        false => IconName::ChevronRight,
    };
    Icon::new(icon).small()
}

impl ChatSession {
    fn render_header(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let title = self.model.title.clone().unwrap_or("New session".into());
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
    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        if !self.model.items[ix].is_step() || self.processing == Processing::Steps {
            return Some(self.render_row(ix, window, cx));
        }
        if self.processing == Processing::Hidden {
            return None;
        }
        let run = self.model.run(ix, ChatItem::is_step);
        let open = self.open_groups.contains(&run.start);
        let row = open.then(|| self.render_row(ix, window, cx));
        if ix != run.start {
            return row;
        }
        let head = self.render_group(run, open, cx);
        Some(
            v_flex()
                .gap_4()
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
        glass(h_flex(), cx)
            .id(("group", head))
            .gap_2()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .cursor_pointer()
            .child(chevron(open))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
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

    fn render_row(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let (dark, muted) = (theme.is_dark(), theme.muted_foreground);
        match &self.model.items[ix] {
            // `.msg.voce`
            ChatItem::User { text } => {
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
                glass(v_flex(), cx)
                    .gap_1()
                    .p_3()
                    .rounded(px(16.))
                    .rounded_bl(px(6.))
                    .children(header)
                    .children(self.texts.get(&ix).map(|md| TextView::new(&md.state)))
                    .into_any_element()
            }
            ChatItem::Thought { .. } => {
                let open = self.open_thoughts.contains(&ix);
                v_flex()
                    .gap_1()
                    .text_color(muted)
                    .child(
                        h_flex()
                            .id(("thought", ix))
                            .gap_1()
                            .cursor_pointer()
                            .child(chevron(open))
                            .child("Thinking…")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.open_thoughts.remove(&ix) {
                                    this.open_thoughts.insert(ix);
                                }
                                cx.notify();
                            }))
                            // Registers the id so tests can find it.
                            .test_support(),
                    )
                    .when(open, |el| {
                        el.children(self.texts.get(&ix).map(|md| TextView::new(&md.state)))
                    })
                    .into_any_element()
            }
            ChatItem::Tool(tool) => self.render_tool(ix, tool, cx).into_any_element(),
            ChatItem::Permission { request, answered } => {
                let id = request.id;
                let on_choose =
                    Rc::new(cx.listener(move |this, choice: &PermissionChoice, _, cx| {
                        this.answer(id, choice, cx)
                    }));
                let diffs = self.diffs.get(&ix).map_or(&[][..], Vec::as_slice);
                let on_open = Self::on_open(cx);
                permission::card(
                    ix,
                    request,
                    answered.as_deref(),
                    diffs,
                    on_choose,
                    &on_open,
                    cx,
                )
                .into_any_element()
            }
            ChatItem::Question { request, answered } => question::card(
                ix,
                request,
                answered.as_deref(),
                &mut self.questions,
                window,
                cx,
            ),
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
    /// edit that reached the disk reads `✎ Edited <file>`, in every processing mode.
    fn render_tool(
        &self,
        ix: usize,
        tool: &ToolItem,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
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
                ("edited", format!("✎ Edited {}", files.join(", ")))
            }
            false => ("step", tool.title.clone()),
        };
        let (output, hidden) = tool.visible_text();
        let on_open = Self::on_open(cx);
        let line = h_flex()
            .id((id, ix))
            .gap_2()
            .cursor_pointer()
            .child(chevron(open))
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::MEDIUM)
                    .child(title),
            )
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
        let card = glass(v_flex(), cx)
            .id(("tool", ix))
            .gap_2()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .child(line);
        if !open {
            return card;
        }
        card.when(!output.is_empty(), |el| {
            el.child(
                div()
                    .id(("step-output", ix))
                    .font_family(theme.mono_font_family.clone())
                    .text_size(theme.mono_font_size)
                    .text_color(theme.muted_foreground)
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

impl Render for ChatSession {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_mode_select(window, cx);
        self.sync_pickers(window, cx);
        let empty = self.model.items.is_empty() && self.model.state == TurnState::Idle;
        let messages = div().flex_1().min_h_0();
        let messages = match empty {
            true => messages
                .flex()
                .items_center()
                .justify_center()
                .child(aurora::pill(cx).child("No messages yet.")),
            // The Central's bubbles sit 16 px apart, not the scroller's 32: the scroller's gap
            // goes and each row but the last carries its own. The scroller remeasures the row
            // before an append, whose "last" flips.
            false => messages.child(
                MessageScroller::new(
                    "messages",
                    self.scroller.clone(),
                    cx.processor(|this, ix, window, cx| {
                        let last = ix + 1 == this.model.items.len();
                        let row = this.render_item(ix, window, cx);
                        div()
                            .when(!last && row.is_some(), |el| el.pb_4())
                            .children(row)
                    }),
                )
                .with_row_style(StyleRefinement::default().pb_0()),
            ),
        };
        v_flex()
            .key_context("ChatSession")
            .on_action(cx.listener(Self::cancel))
            .on_drop(cx.listener(Self::attach))
            .size_full()
            .child(self.render_header(cx))
            .child(messages)
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
        });
        let items = [user(), agent(), tool, agent()];
        assert!(starts_reply(&items, 1));
        assert!(!starts_reply(&items, 3));
        assert!(starts_reply(&[agent()], 0));
        let items = [user(), ChatItem::Thought { text: text() }, agent()];
        assert!(starts_reply(&items, 2));
        assert!(!starts_reply(&items, 1));
        assert!(starts_reply(&[user(), agent(), user(), agent()], 3));
    }
}
