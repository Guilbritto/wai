//! How a session draws: the header, each row of the message list, the whole view.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::message_scroller::MessageScroller;
use gpui_kit::component::select::Select;
use gpui_kit::component::text::TextView;
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::{PermissionChoice, ToolStatus};

use super::model::{ChatItem, NoticeKind, ToolItem, TurnState, UsageInfo, show_label};
use super::view::{ChatEvent, ChatSession};
use super::{permission, question};

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

impl ChatSession {
    fn render_header(&self, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let title = self.model.title.clone().unwrap_or("New session".into());
        h_flex()
            .gap_3()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .children(
                self.modes
                    .as_ref()
                    // The select's own view is `size_full`: the box sets its width.
                    .map(|modes| div().w(px(140.)).child(Select::new(modes).small())),
            )
            .children(self.model.usage.as_ref().map(|usage| {
                div()
                    .text_sm()
                    .text_color(theme.muted_foreground)
                    .child(usage_label(usage))
            }))
    }

    fn render_item(
        &mut self,
        ix: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        match &self.model.items[ix] {
            ChatItem::User { text } => div()
                .p_2()
                .rounded_md()
                .bg(theme.secondary)
                .child(text.clone())
                .into_any_element(),
            ChatItem::Agent { .. } => div()
                .children(self.texts.get(&ix).map(|md| TextView::new(&md.state)))
                .into_any_element(),
            ChatItem::Thought { .. } => {
                let open = self.open_thoughts.contains(&ix);
                let chevron = if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                };
                v_flex()
                    .gap_1()
                    .text_color(muted)
                    .child(
                        h_flex()
                            .id(("thought", ix))
                            .gap_1()
                            .cursor_pointer()
                            .child(Icon::new(chevron).small())
                            .child("Thinking…")
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !this.open_thoughts.remove(&ix) {
                                    this.open_thoughts.insert(ix);
                                }
                                cx.notify();
                            })),
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
                h_flex()
                    .gap_3()
                    .text_color(match kind {
                        NoticeKind::Info => muted,
                        NoticeKind::Error => theme.danger,
                    })
                    .child(text.clone())
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

    fn render_tool(
        &self,
        ix: usize,
        tool: &ToolItem,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (icon, color) = match tool.status {
            ToolStatus::Pending => (IconName::Ellipsis, theme.muted_foreground),
            ToolStatus::InProgress => (IconName::Loader, theme.foreground),
            ToolStatus::Completed => (IconName::CircleCheck, theme.success),
            ToolStatus::Failed => (IconName::CircleX, theme.danger),
        };
        let (output, hidden) = tool.visible_text();
        let on_open = Self::on_open(cx);
        v_flex()
            .id(("tool", ix))
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .gap_2()
                    .child(Icon::new(icon).small().text_color(color))
                    .child(
                        div()
                            .font_weight(FontWeight::MEDIUM)
                            .child(tool.title.clone()),
                    ),
            )
            .when(!output.is_empty(), |el| {
                el.child(
                    div()
                        .font_family(theme.mono_font_family.clone())
                        .text_size(theme.mono_font_size)
                        .text_color(theme.muted_foreground)
                        .child(output.to_string()),
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
        v_flex()
            .key_context("ChatSession")
            .on_action(cx.listener(Self::cancel))
            .on_drop(cx.listener(Self::attach))
            .size_full()
            .child(self.render_header(cx))
            .child(div().flex_1().min_h_0().child(MessageScroller::new(
                "messages",
                self.scroller.clone(),
                cx.processor(Self::render_item),
            )))
            .child(self.render_prompt(cx))
    }
}

#[cfg(test)]
mod tests {
    use super::tokens;

    #[test]
    fn token_counts_are_short() {
        assert_eq!(
            [999, 43_000, 1_000_000, 1_500_000].map(tokens),
            ["999", "43k", "1M", "1.5M"]
        );
    }
}
