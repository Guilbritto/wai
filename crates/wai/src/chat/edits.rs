//! Reviewing a turn's edits: the bounded diffs every view shows, the "Edits (N files)" block that
//! ends a turn, and Revert.
//!
//! Revert rule: each `FileDiff` is one step. A file edited several times in a turn is one row of
//! the block; Revert undoes its latest step not reverted yet, so the steps unwind newest first.
//! A step reverts only while the file on disk equals its `new_text` (Review Focus 2).

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use std::{fmt, fs, io, iter};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use similar::{ChangeTag, TextDiff};
use wai_agent::FileDiff;

use super::model::{ChatItem, EditedFile};
use super::permission::OnOpen;
use super::view::{ChatEvent, ChatSession};
use crate::editor;
use crate::sidebar::canonical;

/// Diffs stop rendering here; "Open file to see all" follows.
pub const MAX_DIFF_LINES: usize = 2000;
/// After this, `similar` gives up on a minimal diff and returns a coarser one.
const DIFF_TIMEOUT: Duration = Duration::from_millis(200);
/// Review Focus 2.
pub const CHANGED: &str = "File changed since this edit — open it to compare";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiffLine {
    /// A hunk header (`@@ -1,2 +1,2 @@`) or a ` `/`+`/`-` line.
    Text(String),
    /// More lines followed `MAX_DIFF_LINES`: the view offers "Open file to see all".
    Truncated,
}

/// The unified diff of `old` and `new` with 3 lines of context, computed within 200 ms and cut at
/// `MAX_DIFF_LINES` lines plus a `Truncated` marker.
pub fn bounded_diff(old: &str, new: &str) -> Vec<DiffLine> {
    let diff = TextDiff::configure()
        .timeout(DIFF_TIMEOUT)
        .diff_lines(old, new);
    let mut lines = Vec::new();
    for hunk in diff.unified_diff().context_radius(3).iter_hunks() {
        let changes = hunk.iter_changes().map(|change| {
            let sign = match change.tag() {
                ChangeTag::Delete => '-',
                ChangeTag::Insert => '+',
                ChangeTag::Equal => ' ',
            };
            format!("{sign}{}", change.value().trim_end_matches(['\n', '\r']))
        });
        for text in iter::once(hunk.header().to_string()).chain(changes) {
            if lines.len() == MAX_DIFF_LINES {
                lines.push(DiffLine::Truncated);
                return lines;
            }
            lines.push(DiffLine::Text(text));
        }
    }
    lines
}

/// The 1-based line where `new` first differs from `old`: where Open puts the caret.
pub fn first_changed_line(old: &str, new: &str) -> u32 {
    let same = old
        .lines()
        .zip(new.lines())
        .take_while(|(a, b)| a == b)
        .count();
    // Lines removed at the end: the caret goes to the last line left.
    (same + 1).min(new.lines().count().max(1)) as u32
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertError {
    /// The file no longer holds what the edit wrote.
    Changed,
    /// The file is gone.
    Missing,
    Io(String),
}

impl fmt::Display for RevertError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RevertError::Changed => f.write_str(CHANGED),
            RevertError::Missing => f.write_str("File deleted since this edit"),
            RevertError::Io(e) => write!(f, "Could not revert: {e}"),
        }
    }
}

/// Undoes one edit: only while the file equals `diff.new_text`, it gets `old_text` back (an atomic
/// write that keeps its permissions and symlinks), or is deleted when the edit created it.
pub fn revert(diff: &FileDiff) -> Result<(), RevertError> {
    let io = |e: io::Error| RevertError::Io(e.to_string());
    match fs::read(&diff.path) {
        Ok(now) if now == diff.new_text.as_bytes() => {}
        Ok(_) => return Err(RevertError::Changed),
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(RevertError::Missing),
        Err(e) => return Err(io(e)),
    }
    // ponytail: a write landing between this check and the write below is lost; a lock or a
    // compare-and-rename would close that gap.
    match &diff.old_text {
        Some(old) => editor::save(&diff.path, old).map(drop).map_err(io),
        None => fs::remove_file(&diff.path).map_err(io),
    }
}

impl ChatSession {
    /// Opens `path`, which the agent may have named relative to the session's folder.
    pub(super) fn open(&mut self, (path, line): &(PathBuf, u32), cx: &mut Context<Self>) {
        let path = self.cwd.join(path);
        cx.emit(ChatEvent::OpenFile { path, line: *line });
    }

    pub(super) fn on_open(cx: &Context<Self>) -> OnOpen {
        Rc::new(cx.listener(|this, at: &(PathBuf, u32), _, cx| this.open(at, cx)))
    }

    /// "Edits (N files)", then per file: its name, its step count when above one, Open (at the
    /// latest step's first change) and Revert, until every step is reverted.
    pub(super) fn render_edits(
        &self,
        ix: usize,
        turn: usize,
        files: &[EditedFile],
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        let on_open = Self::on_open(cx);
        let diffs = self.model.turn_edits(turn);
        let rows = files.iter().enumerate().map(|(k, file)| {
            let latest = file.steps.last().and_then(|&step| diffs.get(step));
            let line = latest.map_or(1, |d| {
                first_changed_line(d.old_text.as_deref().unwrap_or(""), &d.new_text)
            });
            let (on_open, at) = (on_open.clone(), (file.path.clone(), line));
            let name = file.path.strip_prefix(&self.cwd).unwrap_or(&file.path);
            let steps = file.steps.len();
            let revert = if file.reverted < steps {
                Button::new(("edit-revert", k))
                    .small()
                    .label("Revert")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.confirm_revert(ix, k, window, cx)
                    }))
                    .into_any_element()
            } else {
                div()
                    .text_sm()
                    .text_color(muted)
                    .child("Reverted")
                    .into_any_element()
            };
            v_flex()
                .gap_1()
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .child(name.display().to_string()),
                        )
                        .when(steps > 1, |el| {
                            el.child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(format!("{steps} edits")),
                            )
                        })
                        .child(
                            Button::new(("edit-open", k))
                                .small()
                                .ghost()
                                .label("Open")
                                .on_click(move |_, window, cx| on_open(&at, window, cx)),
                        )
                        .child(revert),
                )
                .children(
                    file.failed
                        .clone()
                        .map(|why| div().text_sm().text_color(theme.danger).child(why)),
                )
        });
        let count = match files.len() {
            1 => "1 file".to_string(),
            n => format!("{n} files"),
        };
        v_flex()
            .id(("edits", ix))
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border)
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("Edits ({count})")),
            )
            .children(rows)
    }

    /// The step Revert undoes next for file `file` of the block at row `ix`: its latest step not
    /// reverted yet, the path resolved against the session's folder and canonicalized.
    fn next_revert(&self, ix: usize, file: usize) -> Option<FileDiff> {
        let Some(ChatItem::Edits { turn, files }) = self.model.items.get(ix) else {
            return None;
        };
        let file = files.get(file)?;
        let step = *file.steps.iter().rev().nth(file.reverted)?;
        let diff = *self.model.turn_edits(*turn).get(step)?;
        let path = canonical(&self.cwd.join(&diff.path));
        Some(FileDiff {
            path,
            ..diff.clone()
        })
    }

    /// Revert asks first: no write without a yes.
    fn confirm_revert(
        &mut self,
        ix: usize,
        file: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(diff) = self.next_revert(ix, file) else {
            return;
        };
        let name = diff.path.file_name().unwrap_or_default().to_string_lossy();
        let title = format!("Revert {name}?");
        let body = match diff.old_text {
            Some(_) => "The file goes back to how it was before this edit.",
            None => "This edit created the file: reverting it deletes the file.",
        };
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let this = this.clone();
            dialog.title(title.clone()).child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("revert-confirm")
                            .danger()
                            .label("Revert")
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                this.update(cx, |this, cx| this.revert_file(ix, file, cx))
                                    .ok();
                            }),
                    ),
            )
        });
    }

    fn revert_file(&mut self, ix: usize, file: usize, cx: &mut Context<Self>) {
        let Some(diff) = self.next_revert(ix, file) else {
            return;
        };
        // ponytail: reads and writes on the main thread, like ⌘S; the agent sent the file whole
        // already. Move it to the background executor if large files show.
        let result = revert(&diff);
        if let Some(ChatItem::Edits { files, .. }) = self.model.items.get_mut(ix) {
            let file = &mut files[file];
            match &result {
                Ok(()) => (file.reverted, file.failed) = (file.reverted + 1, None),
                Err(e) => file.failed = Some(e.to_string()),
            }
        }
        if result.is_ok() {
            cx.emit(ChatEvent::Reverted(diff.path));
        }
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use wai_agent::FileDiff;

    use super::{
        CHANGED, DiffLine, MAX_DIFF_LINES, RevertError, bounded_diff, first_changed_line, revert,
    };

    fn diff(path: &std::path::Path, old: Option<&str>, new: &str) -> FileDiff {
        FileDiff {
            path: path.to_path_buf(),
            old_text: old.map(str::to_string),
            new_text: new.into(),
        }
    }

    #[test]
    fn revert_restores_old_text() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n").unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
        }

        revert(&diff(&path, Some("fn old() {}\n"), "fn main() {}\n")).unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "fn old() {}\n");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o640, "the file keeps its permissions");
        }
    }

    #[test]
    fn revert_deletes_created_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("new.md");
        fs::write(&path, "hi\n").unwrap();
        let created = diff(&path, None, "hi\n");

        revert(&created).unwrap();

        assert!(!path.exists());
        assert_eq!(revert(&created), Err(RevertError::Missing));
    }

    /// Review Focus 2: an edit made after the agent's (by the user, or the agent again) is never
    /// clobbered.
    #[test]
    fn revert_refuses_when_file_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n// mine\n").unwrap();

        let edited = diff(&path, Some("fn old() {}\n"), "fn main() {}\n");
        assert_eq!(revert(&edited), Err(RevertError::Changed));
        let created = diff(&path, None, "fn main() {}\n");
        assert_eq!(revert(&created), Err(RevertError::Changed));

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "fn main() {}\n// mine\n"
        );
        assert_eq!(RevertError::Changed.to_string(), CHANGED);
        assert_eq!(CHANGED, "File changed since this edit — open it to compare");
    }

    #[test]
    fn bounded_diff_truncates_and_times_out() {
        assert_eq!(
            bounded_diff("a\nb\n", "a\nc\n"),
            ["@@ -1,2 +1,2 @@", " a", "-b", "+c"].map(|l| DiffLine::Text(l.into()))
        );
        // No line in common: a minimal diff of these takes far longer than the deadline.
        let old: String = (0..200_000).map(|i| format!("old {i}\n")).collect();
        let new: String = (0..200_000).map(|i| format!("new {i}\n")).collect();

        let started = Instant::now();
        let lines = bounded_diff(&old, &new);
        let took = started.elapsed();

        assert!(took < Duration::from_secs(1), "took {took:?}");
        assert_eq!(lines.len(), MAX_DIFF_LINES + 1);
        assert_eq!(lines.last(), Some(&DiffLine::Truncated));
        assert_eq!(lines[1], DiffLine::Text("-old 0".into()));
    }

    #[test]
    fn first_changed_line_is_where_new_differs() {
        assert_eq!(first_changed_line("a\nb\nc\n", "a\nB\nc\n"), 2);
        assert_eq!(first_changed_line("", "new\n"), 1);
        // Lines removed at the end: the last line there is.
        assert_eq!(first_changed_line("a\nb\nc\n", "a\n"), 1);
    }
}
