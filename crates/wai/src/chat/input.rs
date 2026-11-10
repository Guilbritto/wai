//! The prompt box: a multi-line input, dropped-file chips, Send / Stop.

use std::path::Path;

use base64::Engine as _;
use gpui_kit::component::attachment::{Attachment as Chip, AttachmentContent, AttachmentTitle};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, IconName, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::PromptBlock;

use super::model::{TurnState, elapsed};
use super::view::ChatSession;
use crate::app::CancelTurn;
use crate::aurora;

const IMAGE_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// A dropped file, ready to go out with the next prompt.
pub struct Attachment {
    name: String,
    block: PromptBlock,
}

impl Attachment {
    /// png/jpeg/gif/webp up to 5 MB become images; anything else a resource link.
    pub fn new(path: &Path) -> Self {
        let name = path.file_name().map_or_else(
            || wai_settings::shown(path),
            |n| n.to_string_lossy().into_owned(),
        );
        let block = image(path).unwrap_or_else(|| PromptBlock::Resource {
            path: path.to_path_buf(),
        });
        Self { name, block }
    }
}

fn image(path: &Path) -> Option<PromptBlock> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    if std::fs::metadata(path).ok()?.len() > IMAGE_MAX_BYTES {
        return None;
    }
    // ponytail: read on the UI thread; bounded by the 5 MB cap.
    let bytes = std::fs::read(path).ok()?;
    Some(PromptBlock::Image {
        mime: mime.into(),
        base64: base64::engine::general_purpose::STANDARD.encode(bytes),
    })
}

impl ChatSession {
    /// Sends the prompt and the attachments. Only an idle session sends.
    pub(super) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.state != TurnState::Idle {
            return;
        }
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        let mut shown = text.clone();
        let mut blocks = Vec::new();
        if !text.is_empty() {
            blocks.push(PromptBlock::Text(text));
        }
        if !self.attachments.is_empty() {
            let names: Vec<_> = self.attachments.iter().map(|a| a.name.as_str()).collect();
            shown = format!("{shown}\nAttached: {}", names.join(", "))
                .trim_start()
                .to_string();
        }
        blocks.extend(self.attachments.drain(..).map(|a| a.block));
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.submit(blocks, &shown, cx);
    }

    /// Sends `text` as a prompt, leaving the prompt box alone (an extension's `sessions/prompt`).
    /// `false` when the session is not idle.
    pub fn send_text(&mut self, text: &str, cx: &mut Context<Self>) -> bool {
        let text = text.trim();
        if self.model.state != TurnState::Idle || text.is_empty() {
            return false;
        }
        self.submit(vec![PromptBlock::Text(text.into())], text, cx);
        true
    }

    fn submit(&mut self, blocks: Vec<PromptBlock>, shown: &str, cx: &mut Context<Self>) {
        self.handle.prompt(self.id.clone(), blocks);
        let ix = self.update_model(cx, |m| m.push_user(shown));
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }

    /// Puts `text` in the prompt box without sending it.
    pub fn prefill(&mut self, text: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| {
            input.set_value(text.to_string(), window, cx)
        });
    }

    /// The text waiting in the prompt box.
    #[cfg(test)]
    pub fn draft(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    pub(super) fn cancel(&mut self, _: &CancelTurn, _: &mut Window, cx: &mut Context<Self>) {
        match self.model.state {
            TurnState::Working | TurnState::NeedsYou => self.handle.cancel(self.id.clone()),
            _ => cx.propagate(),
        }
    }

    pub(super) fn attach(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        self.attachments
            .extend(paths.paths().iter().map(|path| Attachment::new(path)));
        cx.notify();
    }

    pub(super) fn render_prompt(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let busy = matches!(self.model.state, TurnState::Working | TurnState::NeedsYou);
        let chips = self.attachments.iter().enumerate().map(|(ix, attachment)| {
            Chip::new()
                .id(("attachment", ix))
                .content(
                    AttachmentContent::new().title(AttachmentTitle::new(attachment.name.clone())),
                )
                .on_remove(cx.listener(move |this, _, _, cx| {
                    this.attachments.remove(ix);
                    cx.notify();
                }))
        });
        let theme = cx.theme();
        let dark = theme.is_dark();
        // Round 36 px buttons (`.chat-enviar`); light mode keeps the theme's primary and danger.
        let button = if busy {
            let stop = Button::new("stop")
                .icon(IconName::Square)
                .tooltip("Stop")
                .accessibility_label("Stop")
                .on_click(cx.listener(|this, _, window, cx| this.cancel(&CancelTurn, window, cx)));
            match dark {
                true => aurora::filled(
                    stop,
                    aurora::erro().alpha(0.18),
                    aurora::erro().alpha(0.28),
                    aurora::erro(),
                    cx,
                )
                .border_1()
                .border_color(aurora::erro().alpha(0.5)),
                false => stop.danger(),
            }
        } else {
            let disabled = self.model.state != TurnState::Idle;
            let send = Button::new("send")
                .icon(IconName::ArrowUp)
                .tooltip("Send")
                .accessibility_label("Send")
                .disabled(disabled)
                .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)));
            match (dark, disabled) {
                // A disabled button replays these over its own disabled colors.
                (true, true) => send.bg(aurora::vidro()).text_color(aurora::nevoa()),
                (true, false) => aurora::filled(
                    send,
                    aurora::boreal(),
                    aurora::boreal().blend(hsla(0., 0., 1., 0.2)),
                    rgb(0x0b1024).into(),
                    cx,
                ),
                (false, _) => send.primary(),
            }
        }
        .size(px(36.))
        .rounded(px(18.));
        // R83: the textarea draws no ring of its own, so the pill shows the focus, and a press
        // anywhere on it, its padding included, puts the caret in the textarea.
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let field = h_flex()
            .gap_2()
            .items_end()
            .rounded(px(24.))
            .border_1()
            .pt(px(6.))
            .pr(px(6.))
            .pb(px(6.))
            .pl(px(14.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus(window, cx)),
            );
        let field = match (dark, focused) {
            (true, true) => field.bg(aurora::vidro()).border_color(aurora::boreal()),
            (true, false) => field
                .bg(aurora::vidro())
                .border_color(aurora::borda_forte()),
            (false, true) => field.bg(theme.background).border_color(theme.ring),
            (false, false) => field.bg(theme.background).border_color(theme.input),
        };
        // What the agent does, for how long, and how to stop it. Nothing animates: the mark
        // turns with the clock's tick, so a quiet turn costs one frame a second.
        let status = self.model.activity().map(|doing| {
            let started = self.turn.as_ref().map(|(started, _)| started.elapsed());
            let secs = started.map_or(0, |d| d.as_secs());
            let color = match dark {
                true => aurora::nevoa_2(),
                false => theme.muted_foreground,
            };
            h_flex()
                .id("turn-status")
                .gap_2()
                .px_3()
                .text_size(px(12.))
                .text_color(color)
                .child(["◐", "◓", "◑", "◒"][secs as usize % 4])
                .child(div().min_w_0().truncate().child(doing))
                .child(
                    div()
                        .flex_none()
                        .child(format!("{} · Esc to stop", elapsed(secs))),
                )
                // Registers the id so tests can find it.
                .test_support()
        });
        v_flex()
            .gap_2()
            .p_3()
            .children(status)
            .when(!self.attachments.is_empty(), |el| {
                el.child(h_flex().gap_2().flex_wrap().children(chips))
            })
            .child(
                field
                    .child(
                        div()
                            .flex_1()
                            .child(Textarea::new(&self.input).appearance(false)),
                    )
                    .child(button),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{Attachment, IMAGE_MAX_BYTES};
    use wai_agent::PromptBlock;

    #[test]
    fn small_images_inline_and_the_rest_link() {
        let dir = std::env::temp_dir().join(format!("wai-attach-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let small = dir.join("a.PNG");
        let big = dir.join("b.png");
        let text = dir.join("c.md");
        std::fs::write(&small, [1, 2, 3]).unwrap();
        std::fs::write(&big, vec![0; IMAGE_MAX_BYTES as usize + 1]).unwrap();
        std::fs::write(&text, "x").unwrap();

        let blocks: Vec<_> = [&small, &big, &text]
            .map(|p| Attachment::new(p).block)
            .into();
        std::fs::remove_dir_all(&dir).unwrap();

        assert_eq!(
            blocks[0],
            PromptBlock::Image {
                mime: "image/png".into(),
                base64: "AQID".into()
            }
        );
        assert_eq!(blocks[1], PromptBlock::Resource { path: big });
        assert_eq!(blocks[2], PromptBlock::Resource { path: text });
    }
}
