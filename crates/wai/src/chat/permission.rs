//! The permission card and the unified diffs it shares with tool rows.

use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use similar::{ChangeTag, TextDiff};
use wai_agent::{ChoiceKind, FileDiff, PermissionChoice, PermissionRequest};

pub const MAX_DIFF_LINES: usize = 200;

/// What a choice button calls.
pub type OnChoose = Rc<dyn Fn(&PermissionChoice, &mut Window, &mut App)>;

/// A file's unified diff, computed once when its row changes.
pub struct DiffView {
    path: SharedString,
    lines: Vec<SharedString>,
}

impl DiffView {
    pub fn new(diff: &FileDiff) -> Self {
        Self {
            path: diff.path.display().to_string().into(),
            lines: diff_lines(diff.old_text.as_deref().unwrap_or(""), &diff.new_text)
                .into_iter()
                .map(SharedString::from)
                .collect(),
        }
    }

    pub fn render(&self, cx: &App) -> impl IntoElement + use<> {
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
        v_flex()
            .gap_1()
            .child(div().text_sm().text_color(muted).child(self.path.clone()))
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
    }
}

/// `@@` hunk headers and ` `/`+`/`-` lines with 3 lines of context, at most `MAX_DIFF_LINES`.
pub fn diff_lines(old: &str, new: &str) -> Vec<String> {
    let diff = TextDiff::from_lines(old, new);
    let mut lines = Vec::new();
    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        if lines.len() >= MAX_DIFF_LINES {
            break;
        }
        lines.push(hunk.header().to_string());
        lines.extend(hunk.iter_changes().map(|change| {
            let sign = match change.tag() {
                ChangeTag::Delete => '-',
                ChangeTag::Insert => '+',
                ChangeTag::Equal => ' ',
            };
            format!("{sign}{}", change.value().trim_end_matches(['\n', '\r']))
        }));
    }
    lines.truncate(MAX_DIFF_LINES);
    lines
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
        .children(diffs.iter().map(|diff| diff.render(cx)))
        .child(footer)
}

#[cfg(test)]
mod tests {
    use super::{MAX_DIFF_LINES, diff_lines};

    #[test]
    fn diff_is_unified_and_capped() {
        assert_eq!(
            diff_lines("a\nb\n", "a\nc\n"),
            ["@@ -1,2 +1,2 @@", " a", "-b", "+c"]
        );
        let new: String = (0..500).map(|i| format!("{i}\n")).collect();
        assert_eq!(diff_lines("", &new).len(), MAX_DIFF_LINES);
    }
}
