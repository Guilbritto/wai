use std::path::PathBuf;

use gpui_kit::component::attachment::{Attachment as Chip, AttachmentContent, AttachmentTitle};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::input::{Attachment, attach_button, paste_into, pick_paths, send_button};
use crate::aurora::{self, DISPLAY_FONT};
use crate::workspace::{Dragged, DraggedSession, START_KEY};

pub enum DraftEvent {
    Send,
    Changed,
}

/// A session that has not started: where, with whom, and what to ask. Nothing runs until Send.
pub struct DraftView {
    pub agent: String,
    pub folder: PathBuf,
    pub model: Option<String>,
    pub notice: Option<String>,
    /// A send is in flight (Task 8: the pending `ChatSession`).
    pub starting: bool,
    pub(super) input: Entity<TextareaState>,
    pub(crate) attachments: Vec<Attachment>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DraftEvent> for DraftView {}

impl DraftView {
    pub fn new(
        agent: String,
        folder: PathBuf,
        model: Option<String>,
        prompt: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx)
                .auto_grow(3, 12)
                .placeholder("Pitch your idea");
            input.set_submit_on_enter(true, cx);
            if let Some(prompt) = prompt {
                input.set_value(prompt, window, cx);
            }
            input
        });
        let subscriptions =
            vec![
                cx.subscribe_in(&input, window, |_, _, event: &InputEvent, _, cx| {
                    match event {
                        // Shift+Enter inserted a newline already.
                        InputEvent::PressEnter { shift: false, .. } => cx.emit(DraftEvent::Send),
                        InputEvent::Change => cx.emit(DraftEvent::Changed),
                        _ => {}
                    }
                }),
            ];
        Self {
            agent,
            folder,
            model,
            notice: None,
            starting: false,
            input,
            attachments: Vec::new(),
            _subscriptions: subscriptions,
        }
    }

    /// Puts the caret in the prompt box.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_string()
    }

    pub fn can_send(&self, cx: &App) -> bool {
        !self.starting && !self.text(cx).is_empty()
    }

    /// What goes out: the draft keeps both, in case the start fails.
    pub(crate) fn message(&self, cx: &App) -> (String, Vec<Attachment>) {
        (self.text(cx), self.attachments.clone())
    }

    /// Back to editing, with `error` (if any) shown above the prompt.
    pub fn fail(&mut self, error: Option<String>, cx: &mut Context<Self>) {
        self.starting = false;
        self.notice = error;
        cx.emit(DraftEvent::Changed);
        cx.notify();
    }

    fn attach(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        self.attach_paths(paths.paths(), cx);
    }

    fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.attachments
            .extend(paths.iter().map(|path| Attachment::new(path)));
        cx.notify();
    }

    fn pick_files(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_paths(cx);
        cx.spawn(async move |this, cx| {
            if let Some(paths) = picked.await {
                this.update(cx, |this, cx| this.attach_paths(&paths, cx))
                    .ok();
            }
        })
        .detach();
    }
}

impl Render for DraftView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let dark = theme.is_dark();
        let muted = match dark {
            true => aurora::nevoa_2(),
            false => theme.muted_foreground,
        };
        let dragged = DraggedSession {
            title: "New session".into(),
            source: Dragged::Draft(cx.entity()),
        };
        let header = aurora::card_bar(cx)
            .gap_3()
            .px_3()
            .py_2()
            .child(
                div()
                    .id("draft-title")
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .cursor_grab()
                    .child("New session")
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .child(
                aurora::pill(cx)
                    .flex_none()
                    .border_dashed()
                    .child("Draft · starts when you send"),
            );
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
        let notice = self.notice.clone().map(|notice| {
            div()
                .id("draft-notice")
                .text_size(px(13.))
                .text_color(theme.danger)
                .child(notice)
                // Registers the id so tests can find it.
                .test_support()
        });
        let send = send_button(!self.can_send(cx), cx)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(DraftEvent::Send)));
        let attach = attach_button()
            .on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx)));
        let this = cx.entity().downgrade();
        let textarea =
            Textarea::new(&self.input)
                .appearance(false)
                .on_paste(move |item, window, cx| {
                    let paste = |s: &mut Self, cx: &mut Context<Self>| {
                        let pasted = paste_into(&mut s.attachments, item, window, cx);
                        if pasted {
                            cx.notify();
                        }
                        pasted
                    };
                    this.update(cx, paste).unwrap_or(false)
                });
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let prompt = v_flex()
            .gap_2()
            .p_3()
            .rounded(px(16.))
            .border_1()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus(window, cx)),
            );
        let prompt = match (dark, focused) {
            (true, true) => prompt.bg(aurora::vidro()).border_color(aurora::boreal()),
            (true, false) => prompt
                .bg(aurora::vidro())
                .border_color(aurora::borda_forte()),
            (false, true) => prompt.bg(theme.background).border_color(theme.ring),
            (false, false) => prompt.bg(theme.background).border_color(theme.input),
        }
        .children(notice)
        .child(textarea)
        .when(!self.attachments.is_empty(), |el| {
            el.child(h_flex().gap_2().flex_wrap().children(chips))
        })
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(attach)
                .child(div().flex_1())
                .child(send),
        );
        let hints = h_flex()
            .gap_4()
            .justify_center()
            .text_size(px(12.))
            .text_color(muted)
            .child("⏎ start session")
            .child("⇧⏎ new line")
            .child(format!("{START_KEY} another draft"));
        let body = v_flex()
            .w_full()
            .max_w(px(640.))
            .gap_3()
            .child(
                div()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .child("What are we building?"),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(muted)
                    .child("Pick where and with whom. Nothing starts until you send."),
            )
            .child(prompt)
            .child(hints);
        v_flex()
            .key_context("DraftView")
            .on_drop(cx.listener(Self::attach))
            .size_full()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .p_4()
                    .child(body),
            )
    }
}
