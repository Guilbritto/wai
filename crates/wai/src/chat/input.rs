//! The prompt box: a multi-line input, chips for the files dropped, pasted or picked, the
//! messages queued during a turn, Send and Stop.

use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use gpui_kit::component::attachment::{Attachment as Chip, AttachmentContent, AttachmentTitle};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{Select, SelectDelegate, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::{AgentCaps, ConfigCategory, PromptBlock};

use super::composer;
use super::model::{ChatItem, NoticeKind, TurnState, elapsed};
use super::view::{ChatEvent, ChatSession};
use crate::agents::AgentStatus;
use crate::app::CancelTurn;
use crate::aurora;
use crate::workspace::folder_name;

const IMAGE_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// A file dropped, pasted or picked, ready to go out with the next prompt.
#[derive(Clone)]
pub struct Attachment {
    pub(crate) name: String,
    path: PathBuf,
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
        let path = path.to_path_buf();
        Self { name, path, block }
    }

    /// A pasted image, written to a file of its own so an agent without images can read it.
    /// What agents take as an image is kept as it is; an SVG stays a file; any other format
    /// becomes a PNG.
    pub fn pasted(image: &gpui_kit::Image) -> anyhow::Result<Self> {
        use gpui_kit::ImageFormat as F;
        let kept = match image.format {
            F::Png => Some("png"),
            F::Jpeg => Some("jpg"),
            F::Gif => Some("gif"),
            F::Webp => Some("webp"),
            F::Svg => Some("svg"),
            F::Bmp | F::Tiff | F::Ico | F::Pnm => None,
        };
        let (ext, bytes) = match kept {
            Some(ext) => (ext, image.bytes.clone()),
            None => {
                let mut png = std::io::Cursor::new(Vec::new());
                image::load_from_memory(&image.bytes)?
                    .write_to(&mut png, image::ImageFormat::Png)?;
                ("png", png.into_inner())
            }
        };
        // ponytail: never deleted; the system clears its temporary folder.
        let dir = std::env::temp_dir().join("wai-pasted");
        std::fs::create_dir_all(&dir)?;
        let path = dir.join(format!("{:x}.{ext}", image.id));
        std::fs::write(&path, bytes)?;
        let attachment = Self::new(&path);
        Ok(Self {
            name: format!("Pasted image.{ext}"),
            ..attachment
        })
    }
}

/// A prompt's text and attachments, before it goes out.
pub(super) struct Message {
    pub(super) text: String,
    pub(super) attachments: Vec<Attachment>,
}

impl Message {
    /// What the transcript shows of it: the text, then `Attached: <names>`.
    pub(super) fn shown(&self) -> String {
        if self.attachments.is_empty() {
            return self.text.clone();
        }
        let names: Vec<_> = self.attachments.iter().map(|a| a.name.as_str()).collect();
        format!("{}\nAttached: {}", self.text, names.join(", "))
            .trim_start()
            .to_string()
    }
}

/// What `attachment` sends: an image goes as its file to an agent that takes no images.
pub(super) fn prompt_block(attachment: &Attachment, images: bool) -> PromptBlock {
    match &attachment.block {
        PromptBlock::Image { .. } if !images => PromptBlock::Resource {
            path: attachment.path.clone(),
        },
        block => block.clone(),
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

/// ⌘V of files copied in Finder or of an image attaches them; `false` lets text paste.
pub(super) fn paste_into(
    attachments: &mut Vec<Attachment>,
    item: &ClipboardItem,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    for entry in item.entries() {
        match entry {
            ClipboardEntry::ExternalPaths(paths) => {
                attachments.extend(paths.paths().iter().map(|path| Attachment::new(path)));
                return true;
            }
            ClipboardEntry::Image(image) => {
                match Attachment::pasted(image) {
                    Ok(attachment) => attachments.push(attachment),
                    Err(e) => window.push_notification(
                        Notification::warning(format!("Could not attach the image: {e}")),
                        cx,
                    ),
                }
                return true;
            }
            ClipboardEntry::String(_) => {}
        }
    }
    false
}

/// The round send button (`.chat-enviar`); the caller sets its click.
pub(super) fn send_button(disabled: bool, cx: &App) -> Button {
    let dark = cx.theme().is_dark();
    let send = Button::new("send")
        .icon(IconName::ArrowUp)
        .tooltip("Send")
        .accessibility_label("Send")
        .disabled(disabled);
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
    .size(px(36.))
    .rounded(px(18.))
}

/// The paperclip; the caller sets its click.
pub(super) fn attach_button() -> Button {
    Button::new("attach")
        .ghost()
        .icon(IconName::Plus)
        .tooltip("Attach files (or paste, or drop them here)")
        .accessibility_label("Attach files")
        .size(px(36.))
        .rounded(px(18.))
}

/// What the paperclip opens: the system's file picker, files and folders, several at once.
pub(super) fn pick_paths(cx: &mut App) -> impl Future<Output = Option<Vec<PathBuf>>> + use<> {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: true,
        directories: true,
        multiple: true,
        prompt: None,
    });
    async move { picked.await.ok()?.ok()? }
}

impl ChatSession {
    /// With the box empty, `1`-`9` answer the top card's n-th choice when it is a
    /// permission; any other key, or a question on top, goes through to the box.
    fn answer_key(&mut self, event: &KeyDownEvent, window: &Window, cx: &mut Context<Self>) {
        let key = &event.keystroke;
        // Only while the caret is in the prompt: a bar picker's keys are its own.
        if !self.input.read(cx).focus_handle(cx).is_focused(window) {
            return;
        }
        let Some(n) = (key.key.parse::<usize>().ok()).filter(|n| (1..=9).contains(n)) else {
            return;
        };
        if key.modifiers.modified()
            || !self.input.read(cx).value().is_empty()
            || !self.attachments.is_empty()
        {
            return;
        }
        let Some(ChatItem::Permission { request, .. }) = self.model.waiting.first() else {
            return;
        };
        let Some(choice) = request.options.get(n - 1).cloned() else {
            return;
        };
        let id = request.id;
        cx.stop_propagation();
        self.answer(id, &choice, cx);
    }

    /// Sends the prompt and the attachments: from an idle session as a prompt; during a turn
    /// into it when the agent steers, else into the queue until it ends.
    pub(super) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.is_starting() || (!self.model.in_turn && self.model.state != TurnState::Idle) {
            return;
        }
        let text = self.input.read(cx).value().trim().to_string();
        if text.is_empty() && self.attachments.is_empty() {
            return;
        }
        let attachments = std::mem::take(&mut self.attachments);
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        let message = Message { text, attachments };
        match self.model.in_turn {
            false => self.send_message(message, cx),
            true if self.caps(cx).is_some_and(|c| c.steering) => self.steer(message, cx),
            true => {
                self.queued.push(message);
                cx.notify();
            }
        }
    }

    fn send_message(&mut self, message: Message, cx: &mut Context<Self>) {
        let blocks = self.blocks(&message, cx);
        self.submit(blocks, &message.shown(), cx);
    }

    /// What `message` sends: its text, then each attachment.
    pub(super) fn blocks(&self, message: &Message, cx: &App) -> Vec<PromptBlock> {
        let images = self.caps(cx).is_some_and(|c| c.images);
        let text = (!message.text.is_empty()).then(|| PromptBlock::Text(message.text.clone()));
        let files = message.attachments.iter().map(|a| prompt_block(a, images));
        text.into_iter().chain(files).collect()
    }

    /// Sends `message` into the running turn; it shows as the user's once the turn took it. With
    /// no turn running any more it goes as a prompt; if the agent fails it, it is queued.
    pub(super) fn steer(&mut self, message: Message, cx: &mut Context<Self>) {
        let blocks = self.blocks(&message, cx);
        let (handle, id) = (self.handle.clone(), self.id.clone());
        let shown = message.shown();
        self.model.steer_sent(&shown);
        cx.notify();
        cx.spawn(async move |this, cx| {
            let steered = handle.steer(id, blocks).await;
            this.update(cx, |this, cx| {
                match steered {
                    Ok(true) => {
                        this.update_model(cx, |m| m.steer_injected(&shown));
                        this.sync_rows(None, cx);
                    }
                    Ok(false) => {
                        this.model.steer_dropped(&shown);
                        this.send_message(message, cx)
                    }
                    Err(e) => {
                        this.model.steer_dropped(&shown);
                        let text = format!(
                            "Could not send the message into the turn ({e}); it is queued."
                        );
                        this.model.notice(text, NoticeKind::Error);
                        this.sync_rows(None, cx);
                        this.queued.push(message);
                        // The turn ended meanwhile: nothing would send the queue.
                        if !this.model.in_turn {
                            this.release_queue(false, cx);
                        }
                    }
                }
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// The queue once a turn ends: after a normal end it goes out as one prompt, its texts a blank
    /// line apart; else its text and attachments go back to the prompt box, unsent.
    pub(super) fn release_queue(&mut self, send: bool, cx: &mut Context<Self>) {
        if self.queued.is_empty() {
            return;
        }
        let queued = std::mem::take(&mut self.queued);
        let texts: Vec<&str> = (queued.iter().map(|m| m.text.as_str()))
            .filter(|t| !t.is_empty())
            .collect();
        let text = texts.join("\n\n");
        let attachments = queued.into_iter().flat_map(|m| m.attachments).collect();
        let message = Message { text, attachments };
        if send {
            return self.send_message(message, cx);
        }
        self.take_back(message);
        cx.notify();
    }

    /// Puts a queued message back in the prompt box: its attachments first among the chips, its
    /// text after any text already taken back, a blank line apart.
    pub(super) fn take_back(&mut self, message: Message) {
        self.attachments.splice(0..0, message.attachments);
        self.unsent = Some(match self.unsent.take() {
            Some(held) if !held.is_empty() && !message.text.is_empty() => {
                format!("{held}\n\n{}", message.text)
            }
            Some(held) if message.text.is_empty() => held,
            _ => message.text,
        });
    }

    /// Puts the text taken back from the queue in the prompt box, before what was typed since.
    pub(super) fn sync_unsent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = self.unsent.take() else {
            return;
        };
        let draft = self.input.read(cx).value().trim().to_string();
        let parts: Vec<String> = [text, draft]
            .into_iter()
            .filter(|t| !t.is_empty())
            .collect();
        self.prefill(&parts.join("\n\n"), window, cx);
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
        if self.is_starting() {
            return cx.emit(ChatEvent::StopStart);
        }
        match self.model.state {
            TurnState::Working | TurnState::NeedsYou => self.handle.cancel(self.id.clone()),
            _ => cx.propagate(),
        }
    }

    pub(super) fn attach(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        self.attach_paths(paths.paths(), cx);
    }

    fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.attachments
            .extend(paths.iter().map(|path| Attachment::new(path)));
        cx.notify();
    }

    fn paste(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let pasted = paste_into(&mut self.attachments, item, window, cx);
        if pasted {
            cx.notify();
        }
        pasted
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

    /// What this session's agent can do, once it is ready.
    fn caps(&self, cx: &App) -> Option<AgentCaps> {
        match self.registry.upgrade()?.read(cx).status(&self.agent) {
            Some(AgentStatus::Ready(caps)) => Some(*caps),
            _ => None,
        }
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
        let (success, warning) = (theme.success, theme.warning);
        // Round 36 px buttons (`.chat-enviar`); light mode keeps the theme's primary and danger.
        let stop = busy.then(|| {
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
            .size(px(36.))
            .rounded(px(18.))
        });
        // During a turn too: the message goes into the turn, or waits in the queue.
        let empty = self.input.read(cx).value().trim().is_empty() && self.attachments.is_empty();
        let disabled = empty
            || self.is_starting()
            || !(self.model.in_turn || self.model.state == TurnState::Idle);
        let send = send_button(disabled, cx)
            .on_click(cx.listener(|this, _, window, cx| this.send(window, cx)));
        // R83: the textarea draws no ring of its own, so the box shows the focus, and a press
        // anywhere on it, its padding included, puts the caret in the textarea.
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let field = v_flex()
            .id("prompt-box")
            .gap_2()
            .p_3()
            .rounded(px(20.))
            .border_1()
            .capture_key_down(
                cx.listener(|this, event, window, cx| this.answer_key(event, window, cx)),
            )
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus(window, cx)),
            );
        let field = match (dark, focused) {
            (true, true) => field
                .bg(aurora::noite_2().alpha(0.78))
                .border_color(aurora::boreal()),
            (true, false) => field
                .bg(aurora::noite_2().alpha(0.78))
                .border_color(aurora::borda_forte()),
            (false, true) => field.bg(theme.background).border_color(theme.ring),
            (false, false) => field.bg(theme.background).border_color(theme.input),
        };
        // What the agent does, for how long, and how to stop it. The ring turns with the turn
        // clock's tick (see `tick_period`): no animation runs of its own.
        let status = self.model.activity().map(|doing| {
            let started = self.turn.as_ref().map(|(started, ..)| started.elapsed());
            let secs = started.map_or(0, |d| d.as_secs());
            let waiting = self.model.state == TurnState::NeedsYou;
            let (ring, text) = match (dark, waiting) {
                (true, true) => (aurora::ambar(), aurora::ambar()),
                (true, false) => (aurora::boreal(), aurora::nevoa()),
                (false, true) => (warning, warning),
                (false, false) => (success, theme.muted_foreground),
            };
            let mark = gpui_kit::component::Icon::new(gpui_kit::assets::IconName::LoaderCircle)
                .size(px(12.))
                .flex_none()
                .text_color(ring);
            let mark = match waiting {
                true => mark,
                false => mark.rotate(spin_angle(started.unwrap_or_default())),
            };
            h_flex()
                .id("turn-status")
                .gap(px(10.))
                .px_3()
                .text_size(px(12.5))
                .text_color(text)
                .child(mark)
                .child(div().min_w_0().truncate().child(doing))
                .child(div().flex_1())
                .child(
                    div()
                        .id("turn-status-time")
                        .flex_none()
                        .text_color(match dark {
                            true => aurora::nevoa(),
                            false => theme.muted_foreground,
                        })
                        .child(format!("{} · Esc to stop", elapsed(secs)))
                        // Registers the id so tests can find it.
                        .test_support(),
                )
                // Registers the id so tests can find it.
                .test_support()
        });
        let attach = attach_button()
            .on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx)));
        let this = cx.entity().downgrade();
        let textarea =
            Textarea::new(&self.input)
                .appearance(false)
                .on_paste(move |item, window, cx| {
                    let paste = |s: &mut Self, cx: &mut Context<Self>| s.paste(item, window, cx);
                    this.update(cx, paste).unwrap_or(false)
                });
        let strip = self.render_strip(cx);
        let bar = self.render_bar(attach, stop, send, window, cx);
        let prompt = v_flex().gap_2().p_3().children(status).child(
            // The menu opens above the strip, not over it.
            v_flex()
                .relative()
                .child(strip)
                .child(
                    field
                        .when(!self.attachments.is_empty(), |el| {
                            el.child(h_flex().gap_2().flex_wrap().children(chips))
                        })
                        .child(textarea)
                        .child(bar)
                        // Registers the id so tests can find it.
                        .test_support(),
                )
                .children(self.render_menu(cx)),
        );
        self.menu_keys(prompt, cx)
    }

    /// The folder, branch and agent, as read-only chips on the box's top edge.
    fn render_strip(&self, cx: &App) -> impl IntoElement + use<> {
        let icon = |name| Some(Icon::new(name).xsmall().into_any_element());
        let (folder, head) = match &self.place {
            Some(p) => (folder_name(&p.folder), Some(p)),
            None => (folder_name(&self.cwd), self.head.as_ref()),
        };
        let branch = head.and_then(|head| {
            let mark = match head.worktree {
                true => icon(gpui_kit::assets::IconName::GitFork),
                false => icon(gpui_kit::assets::IconName::GitBranch),
            };
            head.branch
                .clone()
                .map(|branch| composer::strip_label("pill-branch", mark, branch, cx))
        });
        // Claude's mark, as the mockup's agent chip shows it.
        let star = (self.agent == "claude").then(|| {
            div()
                .text_color(aurora::brasa())
                .child("✳")
                .into_any_element()
        });
        let tab = composer::strip("composer-strip", cx)
            .child(composer::strip_label(
                "pill-folder",
                icon(gpui_kit::assets::IconName::Folder),
                folder,
                cx,
            ))
            .children(branch)
            .child(composer::strip_label(
                "pill-agent",
                star,
                wai_settings::agent_label(&self.agent),
                cx,
            ));
        // Registers the id so tests can find it.
        composer::strip_row(tab.test_support())
    }

    /// `.cbar`: attach, the mode, then the model and effort, Stop and Send.
    fn render_bar(
        &self,
        attach: Button,
        stop: Option<Button>,
        send: Button,
        window: &Window,
        cx: &App,
    ) -> impl IntoElement + use<> {
        let max_h = window.viewport_size().height * 0.5;
        fn select<D: SelectDelegate + 'static>(
            state: &Entity<SelectState<D>>,
            max_h: Pixels,
        ) -> Select<D> {
            Select::new(state)
                .appearance(false)
                .small()
                .menu_width(px(230.))
                .menu_max_h(max_h)
        }
        let is_mode = |id: &str| {
            (self.model.config.iter()).any(|o| o.id == id && o.category == ConfigCategory::Mode)
        };
        // The shield lies over the select's left padding, so a click on it reaches the select.
        let with_shield = |state: AnyElement| {
            div()
                .relative()
                .child(state)
                .child(
                    div()
                        .absolute()
                        .left(px(6.))
                        .top_0()
                        .bottom_0()
                        .flex()
                        .items_center()
                        .child(Icon::new(gpui_kit::assets::IconName::Shield).xsmall()),
                )
                .into_any_element()
        };
        let bar_chip = |id: String, content: AnyElement| {
            composer::bar_chip(SharedString::from(id), content, cx)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .test_support()
        };
        let mode = match self.pickers.iter().find(|(id, _)| is_mode(id)) {
            Some((id, state)) => Some(bar_chip(
                format!("bar-{id}"),
                with_shield(select(state, max_h).pl(px(24.)).into_any_element()),
            )),
            None => self.legacy_mode_picker().map(|state| {
                bar_chip(
                    "bar-mode".into(),
                    with_shield(select(state, max_h).pl(px(24.)).into_any_element()),
                )
            }),
        };
        let rest = (self.pickers.iter())
            .filter(|(id, _)| !is_mode(id))
            .map(|(id, state)| {
                bar_chip(format!("bar-{id}"), select(state, max_h).into_any_element())
            });
        h_flex()
            .gap_2()
            .items_center()
            .child(attach)
            .children(mode)
            .child(div().flex_1())
            .children(rest)
            .children(stop)
            .child(send)
    }
}

/// The status ring's angle: an eighth of a turn per 125 ms tick, a full turn a second.
fn spin_angle(elapsed: Duration) -> Radians {
    let tick = (elapsed.as_millis() / 125 % 8) as f32;
    radians(tick * std::f32::consts::FRAC_PI_4)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{Attachment, IMAGE_MAX_BYTES, prompt_block, spin_angle};
    use base64::Engine as _;
    use gpui_kit::{Image, ImageFormat};
    use wai_agent::PromptBlock;

    fn decoded(block: &PromptBlock) -> (String, Vec<u8>) {
        let PromptBlock::Image { mime, base64 } = block else {
            panic!("not an image: {block:?}");
        };
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(base64)
            .unwrap();
        (mime.clone(), bytes)
    }

    #[test]
    fn ring_turns_an_eighth_per_tick() {
        let deg = |ms| spin_angle(Duration::from_millis(ms)).0.to_degrees().round();
        assert_eq!([0, 125, 250, 1000, 1125].map(deg), [0., 45., 90., 0., 45.]);
    }

    /// A pasted PNG goes inline as it is, under a name of its own.
    #[test]
    fn pasted_png_attaches_inline() {
        let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
        let a = Attachment::pasted(&Image::from_bytes(ImageFormat::Png, png.clone())).unwrap();
        assert_eq!(a.name, "Pasted image.png");
        assert_eq!(decoded(&a.block), ("image/png".into(), png));
        assert!(a.path.exists(), "kept on disk for agents without images");
    }

    /// A TIFF (what Preview and Safari copy) is not an image agents take: it goes as a PNG.
    #[test]
    fn pasted_tiff_attaches_as_png() {
        let mut tiff = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2, 2)
            .write_to(&mut tiff, image::ImageFormat::Tiff)
            .unwrap();
        let image = Image::from_bytes(ImageFormat::Tiff, tiff.into_inner());
        let a = Attachment::pasted(&image).unwrap();
        assert_eq!(a.name, "Pasted image.png");
        let (mime, bytes) = decoded(&a.block);
        assert_eq!(mime, "image/png");
        assert_eq!(&bytes[..4], b"\x89PNG");
    }

    /// An agent that takes no images gets an image attachment as its file.
    #[test]
    fn without_image_support_images_go_as_files() {
        let a = Attachment::pasted(&Image::from_bytes(ImageFormat::Png, vec![1])).unwrap();
        assert!(matches!(prompt_block(&a, true), PromptBlock::Image { .. }));
        assert_eq!(
            prompt_block(&a, false),
            PromptBlock::Resource {
                path: a.path.clone()
            }
        );
    }

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
