//! The prompt box: a multi-line input whose files (dropped, pasted, picked or mentioned with `@`)
//! sit in its text as tokens, the messages queued during a turn, Send and Stop.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use base64::Engine as _;
use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{
    InlineToken, InlineTokenContext, InlineTokenSpan, InputContent, InputToken, MoveDown, MoveUp,
    Position, Textarea, TextareaState,
};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::select::{Select, SelectDelegate, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex,
    v_flex,
};
use gpui_kit::*;
use wai_agent::{AgentCaps, ConfigCategory, PromptBlock};

use super::model::{ChatItem, NoticeKind, TurnState, elapsed};
use super::view::{ChatEvent, ChatSession};
use super::{composer, mentions};
use crate::agents::AgentStatus;
use crate::app::CancelTurn;
use crate::appearance::fonts;
use crate::workspace::folder_name;
use crate::{aurora, campo};

const IMAGE_MAX_BYTES: u64 = 5 * 1024 * 1024;

/// A message: the prompt box's text with a token per file in it, each token's id the file's path.
pub(super) type Message = InputContent;

/// Where pasted images are written.
fn pasted_dir() -> PathBuf {
    std::env::temp_dir().join("wai-pasted")
}

/// A pasted image, written to a file of its own so an agent without images can read it; its path.
/// What agents take as an image is kept as it is; an SVG stays a file; any other format becomes a
/// PNG.
pub(super) fn save_pasted(image: &gpui_kit::Image) -> anyhow::Result<PathBuf> {
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
            image::load_from_memory(&image.bytes)?.write_to(&mut png, image::ImageFormat::Png)?;
            ("png", png.into_inner())
        }
    };
    // ponytail: never deleted; the system clears its temporary folder.
    let dir = pasted_dir();
    std::fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{:x}.{ext}", image.id));
    std::fs::write(&path, bytes)?;
    Ok(path)
}

/// The mime type of a png/jpeg/gif/webp up to 5 MB: a file that goes to agents as an image.
fn image_mime(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    };
    (std::fs::metadata(path).ok()?.len() <= IMAGE_MAX_BYTES).then_some(mime)
}

/// What the file at `path` sends: an image to an agent that takes them, else a resource link.
fn file_block(path: &Path, images: bool) -> PromptBlock {
    let resource = || PromptBlock::Resource {
        path: path.to_path_buf(),
    };
    let Some(mime) = image_mime(path).filter(|_| images) else {
        return resource();
    };
    // ponytail: read on the UI thread; bounded by the 5 MB cap.
    match std::fs::read(path) {
        Ok(bytes) => PromptBlock::Image {
            mime: mime.into(),
            base64: base64::engine::general_purpose::STANDARD.encode(bytes),
        },
        Err(_) => resource(),
    }
}

/// What `message` sends, in its order: the text between its files, each file where it stands.
/// Blank runs of text are left out.
pub(super) fn prompt_blocks(message: &Message, images: bool) -> Vec<PromptBlock> {
    let text = message.text();
    let mut blocks = Vec::new();
    let push_text = |blocks: &mut Vec<PromptBlock>, run: &str| {
        if !run.trim().is_empty() {
            blocks.push(PromptBlock::Text(run.to_string()));
        }
    };
    let mut at = 0;
    for span in (message.tokens().iter()).filter(|s| !mentions::is_mention_token(s.token())) {
        push_text(&mut blocks, &text[at..span.range().start]);
        blocks.push(file_block(Path::new(span.token().id().as_ref()), images));
        at = span.range().end;
    }
    push_text(&mut blocks, &text[at..]);
    blocks
}

/// `text` with `tokens` laid on it; a token that does not fit is left out.
fn with_tokens(
    text: impl Into<SharedString>,
    tokens: impl IntoIterator<Item = (Range<usize>, InlineToken)>,
) -> Message {
    (tokens.into_iter()).fold(InputContent::new(text), |content, (range, token)| {
        content.clone().with_token(range, token).unwrap_or(content)
    })
}

/// `message` without its leading and trailing blanks.
pub(super) fn trimmed(message: &Message) -> Message {
    let text = message.text();
    let start = text.len() - text.trim_start().len();
    let spans = message.tokens().iter().map(|span| {
        let range = span.range();
        (range.start - start..range.end - start, span.token().clone())
    });
    with_tokens(text.trim().to_string(), spans)
}

/// The `messages` that hold anything, trimmed, a blank line apart.
pub(super) fn joined(messages: impl IntoIterator<Item = Message>) -> Message {
    let (mut text, mut spans) = (String::new(), Vec::new());
    for message in messages.into_iter().map(|m| trimmed(&m)) {
        if message.text().is_empty() {
            continue;
        }
        if !text.is_empty() {
            text.push_str("\n\n");
        }
        let at = text.len();
        text.push_str(message.text());
        spans.extend(message.tokens().iter().map(|span| {
            let range = span.range();
            (range.start + at..range.end + at, span.token().clone())
        }));
    }
    with_tokens(text, spans)
}

/// The messages sent, oldest first, as typed.
fn sent<'a>(items: &'a [ChatItem], mentions: &'a [(usize, String)]) -> Vec<&'a str> {
    let mut mentions = mentions.iter().peekable();
    let mut sent = Vec::new();
    let mut agent = 0;
    let mut flush = |sent: &mut Vec<&'a str>, agent: usize| {
        while let Some((_, text)) = mentions.next_if(|(n, _)| *n <= agent) {
            sent.push(text.as_str());
        }
    };
    for item in items {
        match item {
            ChatItem::User { text } | ChatItem::Steered { text } if !text.is_empty() => {
                agent += 1;
                flush(&mut sent, agent - 1);
                sent.push(text);
            }
            _ => {}
        }
    }
    flush(&mut sent, usize::MAX);
    sent
}

/// The number after the highest `[Image #n]` among `tokens`, from 1.
fn next_image(tokens: &[InlineTokenSpan]) -> usize {
    let number = |span: &InlineTokenSpan| {
        let text = span.token().text();
        let n = text.strip_prefix("[Image #")?.strip_suffix(']')?;
        n.parse::<usize>().ok()
    };
    tokens.iter().filter_map(number).max().unwrap_or(0) + 1
}

/// The token for the file or folder at `path` in a box already holding `tokens`. Its text: an
/// image's `[Image #n]`, else `@` and the path from `folder` (just its name outside it), a folder's
/// with a trailing `/`. Its label: `Image #n` for a pasted image, else the name.
pub(super) fn file_token(path: &Path, folder: &Path, tokens: &[InlineTokenSpan]) -> InlineToken {
    let name = path.file_name().map_or_else(
        || wai_settings::shown(path),
        |n| n.to_string_lossy().into_owned(),
    );
    let id = path.to_string_lossy().into_owned();
    if image_mime(path).is_some() {
        let image = format!("Image #{}", next_image(tokens));
        let label = match path.starts_with(pasted_dir()) {
            true => image.clone(),
            false => name,
        };
        return InlineToken::new(id, format!("[{image}]")).with_label(label);
    }
    let inside = (path.strip_prefix(folder).ok()).filter(|p| !p.as_os_str().is_empty());
    let shown = inside.map_or_else(|| name.clone(), slashed);
    let slash = if path.is_dir() { "/" } else { "" };
    InlineToken::new(id, format!("@{shown}{slash}")).with_label(name)
}

/// `path` with `/` between its parts on every system, as mentions are typed.
pub(super) fn slashed(path: &Path) -> String {
    let parts = path.components().map(|c| c.as_os_str().to_string_lossy());
    parts.collect::<Vec<_>>().join("/")
}

/// Puts a token for each of `paths` at the caret of `input`, each followed by a space.
pub(super) fn insert_files(
    input: &Entity<TextareaState>,
    paths: &[PathBuf],
    folder: &Path,
    window: &mut Window,
    cx: &mut App,
) {
    input.update(cx, |input, cx| {
        for path in paths {
            // Off the word before it, as a typed mention would be.
            let before = input.value()[..input.selected_range().start]
                .chars()
                .next_back();
            if before.is_some_and(|c| !c.is_whitespace()) {
                input.insert(" ", window, cx);
            }
            let token = file_token(path, folder, input.tokens());
            if input.replace_with_token(token, window, cx).is_ok() {
                input.insert(" ", window, cx);
            }
        }
    });
}

/// A token as a pill: its kind's icon, then its label.
pub(super) fn token_pill(token: &InlineTokenContext) -> InputToken {
    let text = token.token().text();
    let icon = if text.starts_with("[Image #") {
        Lucide::Image
    } else if text.ends_with('/') {
        Lucide::Folder
    } else {
        Lucide::File
    };
    InputToken::new(token).icon(Icon::new(icon))
}

/// ⌘V of files copied in Finder or of an image puts them in `input` at the caret; `false` lets
/// text paste.
pub(super) fn paste_into(
    input: &Entity<TextareaState>,
    folder: &Path,
    item: &ClipboardItem,
    window: &mut Window,
    cx: &mut App,
) -> bool {
    for entry in item.entries() {
        match entry {
            ClipboardEntry::ExternalPaths(paths) => {
                insert_files(input, paths.paths(), folder, window, cx);
                return true;
            }
            ClipboardEntry::Image(image) => {
                match save_pasted(image) {
                    Ok(path) => insert_files(input, &[path], folder, window, cx),
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
        (true, true) => send.bg(aurora::vidro(cx)).text_color(aurora::nevoa(cx)),
        (true, false) => aurora::filled(
            send,
            aurora::boreal(cx),
            aurora::boreal(cx).blend(hsla(0., 0., 1., 0.2)),
            aurora::on_boreal(cx),
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
        if key.modifiers.modified() || !self.input.read(cx).value().is_empty() {
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

    /// Sends the prompt, its files with it: from an idle session as a prompt; during a turn into
    /// it when the agent steers, else into the queue until it ends.
    pub(super) fn send(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let message = trimmed(&self.input.read(cx).content());
        let live = cx.try_global::<mentions::ExtMentions>();
        let to = live.and_then(|l| {
            let (m, rest) = mentions::addressed(&message, &l.0)?;
            Some((m.ext.clone(), m.id.clone(), rest))
        });
        if let Some((ext, id, text)) = to {
            if text.is_empty() {
                // Enter's own newline landed before this ran: the box keeps just the mention.
                self.input
                    .update(cx, |input, cx| input.set_value(message, window, cx));
                return;
            }
            self.input
                .update(cx, |input, cx| input.set_value("", window, cx));
            super::composer::set_typing(cx.entity_id(), self.prompt_typed(cx), cx);
            let agent = sent(&self.model.items, &[]).len();
            self.sent_mentions.push((agent, message.text().to_string()));
            cx.emit(ChatEvent::Mention { ext, id, text });
            return;
        }
        if self.is_starting() || (!self.model.in_turn && self.model.state != TurnState::Idle) {
            return;
        }
        if message.text().is_empty() {
            return;
        }
        self.input
            .update(cx, |input, cx| input.set_value("", window, cx));
        super::composer::set_typing(cx.entity_id(), self.prompt_typed(cx), cx);
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
        self.submit(blocks, message.text(), cx);
    }

    /// What `message` sends: its text and files, in their order.
    pub(super) fn blocks(&self, message: &Message, cx: &App) -> Vec<PromptBlock> {
        prompt_blocks(message, self.caps(cx).is_some_and(|c| c.images))
    }

    /// Sends `message` into the running turn; it shows as the user's once the turn took it. With
    /// no turn running any more it goes as a prompt; if the agent fails it, it is queued.
    pub(super) fn steer(&mut self, message: Message, cx: &mut Context<Self>) {
        let blocks = self.blocks(&message, cx);
        let (handle, id) = (self.handle.clone(), self.id.clone());
        let shown = message.text().to_string();
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
    /// line apart; else it goes back to the prompt box, unsent.
    pub(super) fn release_queue(&mut self, send: bool, cx: &mut Context<Self>) {
        if self.queued.is_empty() {
            return;
        }
        let message = joined(std::mem::take(&mut self.queued));
        if send {
            return self.send_message(message, cx);
        }
        self.take_back(message);
        cx.notify();
    }

    /// Puts a queued message back in the prompt box, after any already taken back, a blank line
    /// apart.
    pub(super) fn take_back(&mut self, message: Message) {
        self.unsent = Some(joined(self.unsent.take().into_iter().chain([message])));
    }

    /// Puts what was taken back from the queue in the prompt box, before what was typed since.
    pub(super) fn sync_unsent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(held) = self.unsent.take() else {
            return;
        };
        let content = joined([held, self.input.read(cx).content()]);
        self.input
            .update(cx, |input, cx| input.set_value(content, window, cx));
        super::composer::set_typing(cx.entity_id(), self.prompt_typed(cx), cx);
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
        super::composer::set_typing(cx.entity_id(), self.prompt_typed(cx), cx);
    }

    /// The text waiting in the prompt box.
    #[cfg(test)]
    pub fn draft(&self, cx: &App) -> String {
        self.input.read(cx).value().to_string()
    }

    /// Esc: out of the agent list and its drawer first, keeping the turn; else Stop.
    pub(super) fn cancel(&mut self, _: &CancelTurn, window: &mut Window, cx: &mut Context<Self>) {
        self.prune_agents();
        if self.drawer.is_none() && self.agent_cursor.is_none() {
            return self.stop_turn(cx);
        }
        (self.drawer, self.agent_cursor) = (None, None);
        self.focus(window, cx);
        cx.notify();
    }

    fn stop_turn(&mut self, cx: &mut Context<Self>) {
        if self.is_starting() {
            return cx.emit(ChatEvent::StopStart);
        }
        match self.model.state {
            TurnState::Working | TurnState::NeedsYou => self.handle.cancel(self.id.clone()),
            _ => cx.propagate(),
        }
    }

    /// Dropped or picked files go in at the caret.
    pub(crate) fn attach(
        &mut self,
        paths: &[PathBuf],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        insert_files(&self.input, paths, &self.cwd, window, cx);
    }

    fn paste(&mut self, item: &ClipboardItem, window: &mut Window, cx: &mut Context<Self>) -> bool {
        paste_into(&self.input, &self.cwd, item, window, cx)
    }

    fn pick_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_paths(cx);
        cx.spawn_in(window, async move |this, cx| {
            let Some(paths) = picked.await else {
                return;
            };
            this.update_in(cx, |this, window, cx| this.attach(&paths, window, cx))
                .ok();
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
        let theme = cx.theme();
        let dark = theme.is_dark();
        let (success, warning) = (theme.success, theme.warning);
        // Round 36 px buttons (`.chat-enviar`); light mode keeps the theme's primary and danger.
        let stop = busy.then(|| {
            let stop = Button::new("stop")
                .icon(IconName::Square)
                .tooltip("Stop")
                .accessibility_label("Stop")
                .on_click(cx.listener(|this, _, _, cx| this.stop_turn(cx)));
            match dark {
                true => aurora::filled(
                    stop,
                    aurora::erro(cx).alpha(0.18),
                    aurora::erro(cx).alpha(0.28),
                    aurora::erro(cx),
                    cx,
                )
                .border_1()
                .border_color(aurora::erro(cx).alpha(0.5)),
                false => stop.danger(),
            }
            .size(px(36.))
            .rounded(px(18.))
        });
        // During a turn too: the message goes into the turn, or waits in the queue.
        let empty = self.input.read(cx).value().trim().is_empty();
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
            .font_family(fonts::chat_family(cx))
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
                .bg(aurora::noite_2(cx).alpha(0.78))
                .border_color(theme.ring)
                .shadow(campo::halo(cx)),
            (true, false) => field
                .bg(aurora::noite_2(cx).alpha(0.78))
                .border_color(aurora::borda_forte(cx)),
            (false, true) => field
                .bg(theme.background)
                .border_color(theme.ring)
                .shadow(campo::halo(cx)),
            (false, false) => field.bg(theme.background).border_color(theme.input),
        };
        // What the agent does, for how long, and how to stop it. The ring turns with the turn
        // clock's tick (see `tick_period`): no animation runs of its own.
        let status = self.model.activity().map(|doing| {
            let started = self.turn.as_ref().map(|(started, ..)| started.elapsed());
            let secs = started.map_or(0, |d| d.as_secs());
            let waiting = self.model.state == TurnState::NeedsYou;
            let (ring, text) = match (dark, waiting) {
                (true, true) => (aurora::ambar(cx), aurora::ambar(cx)),
                (true, false) => (aurora::boreal(cx), aurora::nevoa(cx)),
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
                .text_size(fonts::chat(cx, 12.5))
                .text_color(text)
                .child(mark)
                .child(div().min_w_0().truncate().child(doing))
                .child(div().flex_1())
                .child(
                    div()
                        .id("turn-status-time")
                        .flex_none()
                        .text_color(match dark {
                            true => aurora::nevoa(cx),
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
        let textarea = Textarea::new(&self.input)
            .appearance(false)
            .token(|token, _, _| token_pill(token))
            .on_paste(move |item, window, cx| {
                let paste = |s: &mut Self, cx: &mut Context<Self>| s.paste(item, window, cx);
                this.update(cx, paste).unwrap_or(false)
            });
        let strip = self.render_strip(cx);
        let bar = self.render_bar(attach, stop, send, window, cx);
        let content = v_flex()
            .gap_2()
            // A taller prompt panel: the text takes the height, the bar stays at the bottom.
            .child(div().flex_grow(1.).child(textarea))
            .child(bar);
        let view = cx.entity().downgrade();
        let measured = move |width, cx: &mut App| {
            let set = |this: &mut Self, cx: &mut Context<Self>| {
                if this.prompt_width != width {
                    this.prompt_width = width;
                    cx.notify();
                }
            };
            view.update(cx, set).ok();
        };
        let row = composer::prompt_row(
            cx.entity_id(),
            strip,
            field,
            content,
            self.prompt_width,
            measured,
            cx,
        );
        let prompt = v_flex().gap_2().p_3().children(status).child(
            // The menu opens above the strip, not over it.
            v_flex()
                .relative()
                .child(row)
                .children(self.render_menu(cx))
                .child(self.mentions.clone()),
        );
        let prompt = prompt
            .capture_action(
                cx.listener(|this, _: &MoveUp, window, cx| this.recall(true, window, cx)),
            )
            .capture_action(
                cx.listener(|this, _: &MoveDown, window, cx| this.recall(false, window, cx)),
            );
        let prompt = mentions::keys(self.menu_keys(prompt, cx), &self.mentions);
        self.agent_keys(prompt, cx)
    }

    /// ↑ in an empty prompt box puts the last message sent back in it, caret at its end, and each
    /// further ↑ the one before; ↓ walks forward again, past the newest to an empty box. Only
    /// while the box holds nothing or the recalled message unedited, the caret on its first (↑)
    /// or last (↓) line, and neither a menu nor the agent list has the keys.
    fn recall(&mut self, back: bool, window: &mut Window, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        if !input.focus_handle(cx).is_focused(window)
            || self.agent_cursor.is_some()
            || self.menu(cx).is_some()
            || self.mentions.read(cx).is_open(cx)
        {
            return;
        }
        let value = input.value();
        let line = input.cursor_position().line as usize;
        let sent = sent(&self.model.items, &self.sent_mentions);
        let at = self
            .recalled
            .filter(|&ix| sent.get(ix) == Some(&value.as_ref()));
        let target = match (back, at) {
            (true, Some(ix)) if line == 0 => ix.checked_sub(1),
            (true, None) if value.is_empty() => sent.len().checked_sub(1),
            (false, Some(ix)) if line + 1 == value.split('\n').count() => Some(ix + 1),
            _ => None,
        };
        let Some(ix) = target else {
            return;
        };
        let text = sent.get(ix).map_or_else(String::new, |t| t.to_string());
        self.recalled = (ix < sent.len()).then_some(ix);
        // A recalled `/command` does not open the menu until the user types.
        self.menu_closed = true;
        let last = text.split('\n').count() as u32 - 1;
        self.input.update(cx, |input, cx| {
            input.set_value(text, window, cx);
            input.set_cursor_position(Position::new(last, u32::MAX), window, cx);
        });
        super::composer::set_typing(cx.entity_id(), self.prompt_typed(cx), cx);
        cx.stop_propagation();
        cx.notify();
    }

    /// The folder, branch and agent, as read-only chips on the box's top edge.
    fn render_strip(&self, cx: &App) -> impl IntoElement + use<> {
        let icon = |name| Some(Icon::new(name).with_size(px(12.)).into_any_element());
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
                .text_color(aurora::brasa(cx))
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
                        .child(Icon::new(gpui_kit::assets::IconName::Shield).with_size(px(12.))),
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
        let context = self.context_picker.as_ref().map(|state| {
            let select = select(state, max_h).disabled(self.context_locked());
            bar_chip("bar-context".into(), select.into_any_element())
        });
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
            .children(context)
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
    use std::path::Path;
    use std::time::Duration;

    use super::{
        IMAGE_MAX_BYTES, Message, file_token, joined, next_image, prompt_blocks, save_pasted,
        spin_angle, trimmed, with_tokens,
    };
    use base64::Engine as _;
    use gpui_kit::component::input::InlineToken;
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

    /// `text` with a token for each `(its text, its id)`, found in order.
    fn message(text: &str, tokens: &[(&str, &str)]) -> Message {
        let mut at = 0;
        let mut spans = Vec::new();
        for &(token, id) in tokens {
            let start = at + text[at..].find(token).unwrap();
            at = start + token.len();
            spans.push((start..at, InlineToken::new(id, token)));
        }
        with_tokens(text.to_string(), spans)
    }

    fn spans(message: &Message) -> Vec<(usize, usize, String)> {
        (message.tokens().iter())
            .map(|s| (s.range().start, s.range().end, s.token().id().to_string()))
            .collect()
    }

    #[test]
    fn prompt_blocks_keep_mention_tokens_as_text() {
        let message = message("@Bolota oi", &[("@Bolota", "wai-mention:wai-bichinho:pet")]);
        let blocks = prompt_blocks(&message, false);
        assert!(matches!(&blocks[..], [PromptBlock::Text(t)] if t == "@Bolota oi"));
    }

    #[test]
    fn ring_turns_an_eighth_per_tick() {
        let deg = |ms| spin_angle(Duration::from_millis(ms)).0.to_degrees().round();
        assert_eq!([0, 125, 250, 1000, 1125].map(deg), [0., 45., 90., 0., 45.]);
    }

    /// A pasted PNG is kept as it is, in a file of its own, and goes as an image.
    #[test]
    fn pasted_png_goes_as_an_image() {
        let png = vec![0x89, b'P', b'N', b'G', 1, 2, 3];
        let path = save_pasted(&Image::from_bytes(ImageFormat::Png, png.clone())).unwrap();
        assert_eq!(path.extension().unwrap(), "png");
        let token = file_token(&path, Path::new("/elsewhere"), &[]);
        assert_eq!(
            (token.text().as_ref(), token.label().as_ref()),
            ("[Image #1]", "Image #1")
        );
        let id = path.to_string_lossy();
        let blocks = prompt_blocks(&message("[Image #1]", &[("[Image #1]", &id)]), true);
        assert_eq!(decoded(&blocks[0]), ("image/png".into(), png));
    }

    /// A TIFF (what Preview and Safari copy) is not an image agents take: it is saved as a PNG.
    #[test]
    fn pasted_tiff_is_saved_as_png() {
        let mut tiff = std::io::Cursor::new(Vec::new());
        image::RgbImage::new(2, 2)
            .write_to(&mut tiff, image::ImageFormat::Tiff)
            .unwrap();
        let path = save_pasted(&Image::from_bytes(ImageFormat::Tiff, tiff.into_inner())).unwrap();
        assert_eq!(path.extension().unwrap(), "png");
        assert_eq!(&std::fs::read(&path).unwrap()[..4], b"\x89PNG");
    }

    /// Text and files go in the order they were typed; an agent that takes no images gets an
    /// image as its file, as does an image past 5 MB.
    #[test]
    fn files_go_where_they_stand() {
        let dir = tempfile::tempdir().unwrap();
        let (small, big, notes) = (
            dir.path().join("a.PNG"),
            dir.path().join("b.png"),
            dir.path().join("c.md"),
        );
        std::fs::write(&small, [1, 2, 3]).unwrap();
        std::fs::write(&big, vec![0; IMAGE_MAX_BYTES as usize + 1]).unwrap();
        std::fs::write(&notes, "x").unwrap();
        let id = |p: &Path| p.to_string_lossy().into_owned();
        let (small_id, big_id, notes_id) = (id(&small), id(&big), id(&notes));
        let text = "compare [Image #1] with [Image #2] \n @c.md and";
        let tokens = [
            ("[Image #1]", &*small_id),
            ("[Image #2]", &big_id),
            ("@c.md", &notes_id),
        ];
        let message = message(text, &tokens);
        assert_eq!(message.tokens().len(), 3);
        let image = PromptBlock::Image {
            mime: "image/png".into(),
            base64: "AQID".into(),
        };
        let resource = |path: &Path| PromptBlock::Resource {
            path: path.to_path_buf(),
        };
        assert_eq!(
            prompt_blocks(&message, true),
            [
                PromptBlock::Text("compare ".into()),
                image,
                PromptBlock::Text(" with ".into()),
                resource(&big),
                resource(&notes),
                PromptBlock::Text(" and".into()),
            ]
        );
        assert_eq!(prompt_blocks(&message, false)[1], resource(&small));
    }

    #[test]
    fn trimming_and_joining_keep_tokens_on_their_text() {
        let a = message("  see @a.rs  ", &[("@a.rs", "/p/a.rs")]);
        let a = trimmed(&a);
        assert_eq!(
            (a.text().as_ref(), spans(&a)),
            ("see @a.rs", vec![(4, 9, "/p/a.rs".into())])
        );
        let b = message("@b.rs", &[("@b.rs", "/p/b.rs")]);
        let both = joined([message(" ", &[]), a, b]);
        assert_eq!(both.text().as_ref(), "see @a.rs\n\n@b.rs");
        assert_eq!(
            spans(&both),
            [(4, 9, "/p/a.rs".into()), (11, 16, "/p/b.rs".into())]
        );
    }

    #[test]
    fn tokens_read_as_images_paths_or_names() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "").unwrap();
        std::fs::write(root.join("shot.png"), [1]).unwrap();
        let text = |path: &Path, tokens: &[_]| file_token(path, root, tokens).text().to_string();
        assert_eq!(text(&root.join("src/main.rs"), &[]), "@src/main.rs");
        assert_eq!(text(&root.join("src"), &[]), "@src/");
        assert_eq!(text(Path::new("/other/notes.md"), &[]), "@notes.md");
        let image = file_token(&root.join("shot.png"), root, &[]);
        assert_eq!(
            (image.text().as_ref(), image.label().as_ref()),
            ("[Image #1]", "shot.png")
        );
        let tokens = [("[Image #4]", "/a.png"), ("[Image #2]", "/b.png")];
        let held = message("[Image #4] [Image #2]", &tokens);
        assert_eq!(next_image(held.tokens()), 5);
        assert_eq!(text(&root.join("shot.png"), held.tokens()), "[Image #5]");
    }
}
