//! The permission card and the unified diffs it shares with tool rows and the Edits block.

use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::{ChoiceKind, FileDiff, PermissionChoice, PermissionRequest};

use super::edits::{DiffLine, bounded_diff};

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
            label: (diff.path.strip_prefix(cwd).unwrap_or(&diff.path))
                .display()
                .to_string()
                .into(),
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
                    .rounded_md()
                    .bg(theme.muted)
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

/// Title, diff preview and one button per choice; once answered, the answer instead.
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
        None => h_flex()
            .gap_2()
            .flex_wrap()
            .children(request.options.iter().map(|choice| {
                let button = Button::new(button_id(choice.kind))
                    .small()
                    .label(choice.name.clone());
                let button = match choice.kind {
                    ChoiceKind::AllowOnce => button.primary(),
                    ChoiceKind::RejectOnce | ChoiceKind::RejectAlways => button.danger(),
                    ChoiceKind::AllowAlways => button,
                };
                let (on_choose, choice) = (on_choose.clone(), choice.clone());
                button.on_click(move |_, window, cx| on_choose(&choice, window, cx))
            })),
    };
    v_flex()
        .id(("permission", ix))
        .gap_2()
        .p_3()
        .rounded_md()
        .border_1()
        .border_color(theme.warning)
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .child(request.title.clone()),
        )
        .children(
            diffs
                .iter()
                .enumerate()
                .map(|(k, diff)| diff.render(("diff", k).into(), on_open, cx)),
        )
        .child(footer)
}
