//! The permission card and the unified diffs it shares with tool rows and the Edits block.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::{ChoiceKind, FileDiff, PermissionChoice, PermissionRequest};

use super::edits::{DiffLine, bounded_diff};
use crate::aurora::{self, DISPLAY_FONT};

/// What a choice button calls.
pub type OnChoose = Rc<dyn Fn(&PermissionChoice, &mut Window, &mut App)>;
/// Opens a file (as the agent named it) at a 1-based line.
pub type OnOpen = Rc<dyn Fn(&(PathBuf, u32), &mut Window, &mut App)>;

/// A file's unified diff, computed once when its row changes.
pub struct DiffView {
    /// The file and the line its first change is on.
    at: (PathBuf, u32),
    label: SharedString,
    lines: Vec<SharedString>,
    truncated: bool,
}

impl DiffView {
    /// The path shows relative to `cwd`, the session's folder, when it is inside; `line` is
    /// where Open puts the caret.
    pub fn new(diff: &FileDiff, cwd: &Path, line: u32) -> Self {
        let old = diff.old_text.as_deref().unwrap_or("");
        let mut truncated = false;
        let lines = bounded_diff(old, &diff.new_text)
            .into_iter()
            .filter_map(|line| match line {
                DiffLine::Text(text) => Some(text.into()),
                DiffLine::Truncated => {
                    truncated = true;
                    None
                }
            })
            .collect();
        Self {
            at: (diff.path.clone(), line),
            label: wai_settings::shown(diff.path.strip_prefix(cwd).unwrap_or(&diff.path)).into(),
            lines,
            truncated,
        }
    }

    /// `id` sets this diff apart from its siblings; its path and "Open file to see all" open the
    /// file at the first change.
    pub fn render(&self, id: ElementId, on_open: &OnOpen, cx: &App) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (added, removed, muted) = (theme.success, theme.danger, theme.muted_foreground);
        let lines = self.lines.iter().map(|line| {
            let color = match line.as_bytes().first() {
                Some(b'+') => added,
                Some(b'-') => removed,
                Some(b'@') => muted,
                _ => theme.foreground,
            };
            div()
                .whitespace_nowrap()
                .text_color(color)
                .child(line.clone())
        });
        let link = |id: &'static str| {
            let (on_open, at) = (on_open.clone(), self.at.clone());
            div()
                .id(id)
                .text_sm()
                .cursor_pointer()
                .on_click(move |_, window, cx| on_open(&at, window, cx))
        };
        v_flex()
            .id(id)
            .gap_1()
            .child(
                link("diff-path")
                    .text_color(muted)
                    .child(self.label.clone()),
            )
            .child(
                v_flex()
                    .p_2()
                    .rounded(px(12.))
                    .bg(match theme.is_dark() {
                        true => rgb(0x0a0e24).into(),
                        false => theme.muted,
                    })
                    .overflow_hidden()
                    .font_family(theme.mono_font_family.clone())
                    .text_size(theme.mono_font_size)
                    .children(lines),
            )
            .when(self.truncated, |el| {
                el.child(
                    link("diff-open-all")
                        .text_color(theme.link)
                        .child("Open file to see all"),
                )
            })
    }
}

/// The element id of a choice's button, e.g. `perm-allow_once`.
fn button_id(kind: ChoiceKind) -> &'static str {
    match kind {
        ChoiceKind::AllowOnce => "perm-allow_once",
        ChoiceKind::AllowAlways => "perm-allow_always",
        ChoiceKind::RejectOnce => "perm-reject_once",
        ChoiceKind::RejectAlways => "perm-reject_always",
    }
}

/// The frame the permission and question cards share (`.chat-escolha`): `accent` tints its
/// border, fill and glow; then `eyebrow` and `title`. Light mode: a warning border.
pub(super) fn frame(
    accent: Hsla,
    eyebrow: &'static str,
    title: impl Into<SharedString>,
    cx: &App,
) -> Div {
    let theme = cx.theme();
    let frame = v_flex().gap_3().p_4().rounded(px(18.)).border_1();
    let (frame, eyebrow_color) = match theme.is_dark() {
        // An opaque base, as the Central's: the glow would show through a clear fill.
        true => (
            frame
                .border_color(accent.alpha(0.55))
                .bg(linear_gradient(
                    180.,
                    linear_color_stop(aurora::noite_2().blend(accent.alpha(0.08)), 0.),
                    linear_color_stop(aurora::noite_2(), 1.),
                ))
                // `0 0 40 -12`: GPUI's blur radius is the Gaussian's sigma, half the CSS one.
                .shadow(vec![
                    BoxShadow::new(px(0.), px(0.), accent.alpha(0.5))
                        .blur_radius(px(20.))
                        .spread_radius(px(-12.)),
                ]),
            aurora::brasa(),
        ),
        false => (frame.border_color(theme.warning), theme.warning),
    };
    let heading = v_flex()
        .gap_1()
        .child(
            div()
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(eyebrow_color)
                .child(eyebrow),
        )
        .child(
            div()
                .font_family(DISPLAY_FONT)
                .text_size(px(16.))
                .font_weight(FontWeight::BOLD)
                .child(title.into()),
        );
    frame.child(heading)
}

/// A choice (`.escolha-opcao`): a full-width row behind its number badge (`.opcao-n`).
fn choice(n: usize, choice: &PermissionChoice, highlighted: bool, cx: &App) -> Button {
    let theme = cx.theme();
    let reject = matches!(
        choice.kind,
        ChoiceKind::RejectOnce | ChoiceKind::RejectAlways
    );
    let dark = theme.is_dark();
    let (fill, hover, border) = match (dark, highlighted) {
        (true, true) => (
            aurora::lilas().alpha(0.16),
            aurora::lilas().alpha(0.24),
            aurora::lilas().alpha(0.45),
        ),
        (true, false) => (aurora::vidro(), aurora::vidro_forte(), aurora::borda()),
        (false, true) => (theme.secondary, theme.secondary_hover, theme.ring),
        (false, false) => (theme.secondary, theme.secondary_hover, theme.border),
    };
    let text = match (dark, reject) {
        (true, true) => aurora::erro(),
        (true, false) => aurora::tinta(),
        (false, true) => theme.danger,
        (false, false) => theme.foreground,
    };
    let (badge, badge_text) = match dark {
        true => (aurora::brasa().alpha(0.18), rgb(0xffb68a).into()),
        false => (theme.background, theme.muted_foreground),
    };
    let number = div()
        .flex_none()
        .size(px(26.))
        .rounded(px(8.))
        .flex()
        .items_center()
        .justify_center()
        .bg(badge)
        .text_color(badge_text)
        .font_weight(FontWeight::BOLD)
        .child((n + 1).to_string());
    let label = div().flex_1().min_w_0().child(choice.name.clone());
    // The name is a child, not the label: it is the accessible name only when given as one.
    let button = Button::new(button_id(choice.kind)).accessibility_label(choice.name.clone());
    aurora::filled(button, fill, hover, text, cx)
        .rounded(px(12.))
        .w_full()
        .h_auto()
        .px_3()
        .py_2()
        .border_1()
        .border_color(border)
        .child(
            h_flex()
                .w_full()
                .gap_3()
                .whitespace_normal()
                .child(number)
                .child(label),
        )
}

/// Title, diff preview and one row per choice; once answered, the answer instead.
pub fn card(
    ix: usize,
    request: &PermissionRequest,
    answered: Option<&str>,
    diffs: &[DiffView],
    on_choose: OnChoose,
    on_open: &OnOpen,
    cx: &App,
) -> impl IntoElement + use<> {
    let theme = cx.theme();
    let footer = match answered {
        Some("Cancelled") => div().text_color(theme.muted_foreground).child("Cancelled"),
        Some(answer) => div()
            .text_color(theme.muted_foreground)
            .child(format!("Answered: {answer}")),
        None => {
            let allow = (request.options.iter())
                .position(|c| matches!(c.kind, ChoiceKind::AllowOnce | ChoiceKind::AllowAlways));
            v_flex()
                .gap_2()
                .children(request.options.iter().enumerate().map(|(n, c)| {
                    let (on_choose, c) = (on_choose.clone(), c.clone());
                    choice(n, &c, allow == Some(n), cx)
                        .on_click(move |_, window, cx| on_choose(&c, window, cx))
                }))
        }
    };
    frame(
        aurora::boreal(),
        "PERMISSION REQUESTED",
        request.title.clone(),
        cx,
    )
    .id(("permission", ix))
    .children(
        diffs
            .iter()
            .enumerate()
            .map(|(k, diff)| diff.render(("diff", k).into(), on_open, cx)),
    )
    .child(footer)
}
