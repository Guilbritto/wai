//! Reviewing a turn's edits: the bounded diffs every view shows, the "Edits (N files)" block that
//! ends a turn, and Revert.
//!
//! Revert rules:
//! - Each `FileDiff` is one step. A file edited several times in a turn is one row of the block;
//!   Revert undoes its latest step not reverted yet, so the steps unwind newest first.
//! - A step reverts only while the file still shows what the edit wrote (Review Focus 2, R32):
//!   the whole file equals `new_text`, or else `new_text` (a hunk or snippet, as claude-agent-acp
//!   sends them) occurs exactly once, on whole lines, and only that occurrence goes back. A file
//!   with tabs or CRLF is compared as Claude reports it (leading tabs as two spaces, LF line
//!   ends), and the old text goes back in the file's own form.
//! - Deleting a file needs proof that the edit created it (R34): wai saw it absent before the
//!   tool call ran, and the file still equals the non-empty `new_text`. A symlink is never
//!   deleted (R38).
//! - Revert decides what it would do (`plan`) before it asks (R38): a refusal shows at once,
//!   the question names the outcome, and a yes does exactly that or nothing.

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Duration;
use std::{fmt, fs, io, iter};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use similar::{ChangeTag, TextDiff};
use wai_agent::{FileDiff, Location};

use super::model::{ChatItem, EditedFile, ToolItem, TurnState};
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
/// R34.
pub const EXISTED: &str = "Can't revert: the file existed before this edit.";

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

/// Where Open puts the caret for diff `k` of a tool call (R33), 1-based: the hunk's first line
/// from the location paired with the diff (same count, same path) plus where in the hunk the
/// change starts; else the first location of that file that has a line; else the diff's first
/// changed line, as if it were the whole file.
pub fn open_line(locations: &[Location], diffs: &[FileDiff], k: usize) -> u32 {
    let Some(diff) = diffs.get(k) else {
        return 1;
    };
    let changed = first_changed_line(diff.old_text.as_deref().unwrap_or(""), &diff.new_text);
    let paired = (locations.len() == diffs.len()).then(|| &locations[k]);
    if let Some(start) = paired.filter(|l| l.path == diff.path).and_then(|l| l.line) {
        return start.saturating_add(changed - 1).max(1);
    }
    let mut same_file = locations.iter().filter(|l| l.path == diff.path);
    same_file.find_map(|l| l.line).unwrap_or(changed)
}

/// The step Revert undoes next for `file`, among its turn's `turn_steps`: its latest step not
/// reverted yet. A step that no longer names the file (a late tool update shifted the turn's
/// diffs) is refused.
pub fn next_step<'a>(
    steps: &[(&'a ToolItem, usize)],
    file: &EditedFile,
) -> Result<(&'a ToolItem, usize), RevertError> {
    let step = file.steps.iter().rev().nth(file.reverted);
    let found = step.and_then(|&step| steps.get(step)).copied();
    found
        .filter(|(tool, k)| tool.diffs[*k].path == file.path)
        .ok_or(RevertError::Changed)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RevertError {
    /// The file no longer shows what the edit wrote.
    Changed,
    /// Undoing a creation-shaped edit would delete a file wai did not see it create.
    Existed,
    /// The file is gone.
    Missing,
    Io(String),
}

impl fmt::Display for RevertError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            RevertError::Changed => f.write_str(CHANGED),
            RevertError::Existed => f.write_str(EXISTED),
            RevertError::Missing => f.write_str("File deleted since this edit"),
            RevertError::Io(e) => write!(f, "Could not revert: {e}"),
        }
    }
}

/// What undoing an edit does to its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Undo {
    /// The edit created the file: it goes.
    Delete,
    /// The whole file goes back to this.
    Whole(String),
    /// Only the edit's lines go back: the file becomes this.
    Snippet(String),
}

/// What undoing one edit would do now, by the rules above, writing nothing. `created`: wai saw
/// the file absent before the edit ran.
pub fn plan(diff: &FileDiff, created: bool) -> Result<Undo, RevertError> {
    let io = |e: io::Error| RevertError::Io(e.to_string());
    let now = match fs::read(&diff.path) {
        Ok(now) => String::from_utf8(now).map_err(|_| RevertError::Changed)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Err(RevertError::Missing),
        Err(e) => return Err(io(e)),
    };
    let undo = undo(&now, diff, created)?;
    // R38: a link where the edit created a file is not that file; its target may be anything.
    let link = || fs::symlink_metadata(&diff.path).map(|m| m.file_type().is_symlink());
    if undo == Undo::Delete && link().map_err(io)? {
        return Err(RevertError::Changed);
    }
    Ok(undo)
}

/// Undoes one edit as Revert said it would (`asked`, a `plan` from when it asked): the new text
/// goes through an atomic write that keeps the file's permissions and symlinks; a deletion
/// removes it. A file changed since, so that the plan differs now, is refused.
pub fn revert(diff: &FileDiff, created: bool, asked: &Undo) -> Result<(), RevertError> {
    let io = |e: io::Error| RevertError::Io(e.to_string());
    // ponytail: a write landing between this read and the write below is lost; a lock or a
    // compare-and-rename would close that gap.
    match plan(diff, created)? {
        undo if undo != *asked => Err(RevertError::Changed),
        Undo::Delete => fs::remove_file(&diff.path).map_err(io),
        Undo::Whole(text) | Undo::Snippet(text) => {
            editor::save(&diff.path, &text).map(drop).map_err(io)
        }
    }
}

/// What a file holding `now` becomes once `diff` is undone.
fn undo(now: &str, diff: &FileDiff, created: bool) -> Result<Undo, RevertError> {
    let new = diff.new_text.as_str();
    let old = match (&diff.old_text, created) {
        (Some(old), _) => old.as_str(),
        // R34: no old text and a file that was there before (a Write over it, say): wai can't
        // tell what it held, so nothing goes back, not even a snippet.
        (None, false) => return Err(RevertError::Existed),
        (None, true) if now == new && new.is_empty() => return Err(RevertError::Existed),
        (None, true) if now == new => return Ok(Undo::Delete),
        (None, true) => "",
    };
    // Tabs or CRLF: `old` is Claude's rendering, so it goes back in the file's own form.
    if let Some(undo) = as_shown(now, old, new) {
        return Ok(undo);
    }
    if now == new {
        return Ok(Undo::Whole(old.to_string()));
    }
    let Some(at) = only_on_lines(now, new) else {
        return as_shown(now, old, new).ok_or(RevertError::Changed);
    };
    Ok(Undo::Snippet(format!(
        "{}{old}{}",
        &now[..at],
        &now[at + new.len()..]
    )))
}

/// `text` as Claude Code shows it: LF line ends, each leading tab as two spaces.
fn as_claude_shows(text: &str) -> String {
    text.replace("\r\n", "\n")
        .split_inclusive('\n')
        .map(|line| {
            let rest = line.trim_start_matches('\t');
            format!("{}{rest}", "  ".repeat(line.len() - rest.len()))
        })
        .collect()
}

/// `text` (as Claude shows it) in the file's own form: with `tabs`, each leading run of `n`
/// spaces is `n / 2` tabs and `n % 2` spaces; then the file's line ends.
fn as_file_writes(text: &str, tabs: bool, crlf: bool) -> String {
    let text: String = if tabs {
        text.split_inclusive('\n')
            .map(|line| {
                let rest = line.trim_start_matches(' ');
                let n = line.len() - rest.len();
                format!("{}{}{rest}", "\t".repeat(n / 2), " ".repeat(n % 2))
            })
            .collect()
    } else {
        text.to_string()
    };
    editor::line_ends(&text, crlf)
}

/// The place in `now` that `at`, an offset of `seen` (`as_claude_shows(now)`) starting or ending
/// a line, stands for. Both texts have the same lines.
fn file_offset(now: &str, seen: &str, at: usize) -> usize {
    let line = seen[..at].matches('\n').count();
    let begin: usize = now.split_inclusive('\n').take(line).map(str::len).sum();
    if at == 0 || seen.as_bytes()[at - 1] == b'\n' {
        return begin;
    }
    let text = now[begin..].split_inclusive('\n').next().unwrap_or("");
    let text = text.strip_suffix('\n').unwrap_or(text);
    begin + text.strip_suffix('\r').unwrap_or(text).len()
}

/// The undo for a file with tabs or CRLF, whose text only matches the edit as Claude shows it.
fn as_shown(now: &str, old: &str, new: &str) -> Option<Undo> {
    let crlf = now.contains("\r\n");
    // Review focus 2: mixed line ends are not what Claude's edit reported.
    if crlf && now.replace("\r\n", "").contains('\n') {
        return None;
    }
    let tabs = now.lines().any(|l| l.starts_with('\t'));
    if !crlf && !tabs {
        return None;
    }
    let (seen, new, old) = (
        as_claude_shows(now),
        as_claude_shows(new),
        as_claude_shows(old),
    );
    let back = as_file_writes(&old, tabs, crlf);
    if seen == new {
        return Some(Undo::Whole(back));
    }
    let at = only_on_lines(&seen, &new)?;
    let (a, b) = (
        file_offset(now, &seen, at),
        file_offset(now, &seen, at + new.len()),
    );
    Some(Undo::Snippet(format!("{}{back}{}", &now[..a], &now[b..])))
}

/// Where `needle` sits in `hay`, if it occurs there exactly once (overlapping occurrences count)
/// and that occurrence starts and ends on line boundaries: at the start of `hay` or after a `\n`,
/// at its end or before a `\n`. A needle that itself begins (ends) with `\n` is on a boundary at
/// that side.
fn only_on_lines(hay: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    let mut found = None;
    let mut from = 0;
    while let Some(at) = hay[from..].find(needle).map(|i| from + i) {
        if found.replace(at).is_some() {
            return None;
        }
        from = at + hay[at..].chars().next().map_or(1, char::len_utf8);
    }
    let at = found?;
    let (end, bytes) = (at + needle.len(), hay.as_bytes());
    let starts = at == 0 || bytes[at - 1] == b'\n' || needle.starts_with('\n');
    let ends = end == hay.len() || bytes[end] == b'\n' || needle.ends_with('\n');
    (starts && ends).then_some(at)
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
        let turn_steps = self.model.turn_steps(turn);
        // R35: the agent may still be writing.
        let busy = matches!(self.model.state, TurnState::Working | TurnState::NeedsYou);
        let rows = files.iter().enumerate().map(|(k, file)| {
            let latest = file.steps.last().and_then(|&step| turn_steps.get(step));
            let line = latest.map_or(1, |(tool, k)| open_line(&tool.locations, &tool.diffs, *k));
            let (on_open, at) = (on_open.clone(), (file.path.clone(), line));
            let name = file.path.strip_prefix(&self.cwd).unwrap_or(&file.path);
            let steps = file.steps.len();
            let revert = if file.reverted < steps {
                Button::new(("edit-revert", k))
                    .small()
                    .label("Revert")
                    .disabled(busy)
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
                                .child(wai_settings::shown(name)),
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
        super::render::glass(v_flex(), cx)
            .id(("edits", ix))
            .gap_2()
            .px_3()
            .py_2()
            .rounded(px(12.))
            .child(
                div()
                    .font_weight(FontWeight::MEDIUM)
                    .child(format!("Edits ({count})")),
            )
            .children(rows)
    }

    /// The step Revert undoes next for file `file` of the block at row `ix` (`next_step`), its
    /// path resolved against the session's folder with its folder canonicalized (a symlink
    /// stays one, R38), and whether wai saw the edit create the file.
    fn next_revert(&self, ix: usize, file: usize) -> Result<(FileDiff, bool), RevertError> {
        let Some(ChatItem::Edits { turn, files }) = self.model.items.get(ix) else {
            return Err(RevertError::Changed);
        };
        let file = files.get(file).ok_or(RevertError::Changed)?;
        let (tool, k) = next_step(&self.model.turn_steps(*turn), file)?;
        let diff = &tool.diffs[k];
        let created = tool.first_sight.contains(&(diff.path.clone(), false));
        let path = self.cwd.join(&diff.path);
        let path = match (path.parent(), path.file_name()) {
            (Some(dir), Some(name)) => canonical(dir).join(name),
            _ => path,
        };
        Ok((
            FileDiff {
                path,
                ..diff.clone()
            },
            created,
        ))
    }

    /// Records a Revert's outcome on its row.
    fn reverted(&mut self, ix: usize, file: usize, result: &Result<(), RevertError>) {
        if let Some(ChatItem::Edits { files, .. }) = self.model.items.get_mut(ix) {
            let file = &mut files[file];
            match result {
                Ok(()) => (file.reverted, file.failed) = (file.reverted + 1, None),
                Err(e) => file.failed = Some(e.to_string()),
            }
        }
    }

    /// Revert asks first: no write without a yes. R38: a Revert that would refuse says why at
    /// once instead, and the question says what a yes does.
    fn confirm_revert(
        &mut self,
        ix: usize,
        file: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let planned = self.next_revert(ix, file);
        let planned = planned.and_then(|(diff, created)| Ok((plan(&diff, created)?, diff)));
        let (undo, diff) = match planned {
            Ok(planned) => planned,
            Err(e) => {
                self.reverted(ix, file, &Err(e));
                self.sync_rows(Some(ix), cx);
                return cx.notify();
            }
        };
        // A file outside the session's folder shows in full: no doubt about which one.
        let name = match diff.path.starts_with(canonical(&self.cwd)) {
            true => diff
                .path
                .file_name()
                .unwrap_or_default()
                .display()
                .to_string(),
            false => wai_settings::shown(&diff.path),
        };
        let title = format!("Revert {name}?");
        let (says, body) = match undo {
            Undo::Delete => (
                "revert-delete",
                "This edit created the file: reverting it deletes the file.",
            ),
            Undo::Whole(_) => (
                "revert-whole",
                "The file goes back to how it was before this edit.",
            ),
            Undo::Snippet(_) => (
                "revert-snippet",
                "The lines this edit changed go back to how they were; the rest of the file \
                 stays as it is.",
            ),
        };
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, undo) = (this.clone(), undo.clone());
            let body = div().debug_selector(move || says.into()).child(body);
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
                                this.update(cx, |this, cx| this.revert_file(ix, file, &undo, cx))
                                    .ok();
                            }),
                    ),
            )
        });
    }

    fn revert_file(&mut self, ix: usize, file: usize, asked: &Undo, cx: &mut Context<Self>) {
        // R35: a turn started while the dialog was open.
        if matches!(self.model.state, TurnState::Working | TurnState::NeedsYou) {
            return;
        }
        // ponytail: reads and writes on the main thread, like ⌘S: one file the agent already
        // sent. Move it to the background executor if large files show.
        let result = self.next_revert(ix, file).and_then(|(diff, created)| {
            revert(&diff, created, asked)?;
            Ok(canonical(&diff.path))
        });
        self.reverted(ix, file, &result.as_ref().map(drop).map_err(Clone::clone));
        if let Ok(path) = result {
            cx.emit(ChatEvent::Reverted(path));
        }
        self.sync_rows(Some(ix), cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::time::{Duration, Instant};

    use wai_agent::{FileDiff, Location, ToolStatus};

    use super::{
        CHANGED, DiffLine, EXISTED, MAX_DIFF_LINES, RevertError, Undo, bounded_diff,
        first_changed_line, next_step, open_line, plan, undo,
    };
    use crate::chat::model::{EditedFile, ToolItem};

    /// Revert as confirmed right after it asked.
    fn revert(diff: &FileDiff, created: bool) -> Result<(), RevertError> {
        super::revert(diff, created, &plan(diff, created)?)
    }

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

        revert(&diff(&path, Some("fn old() {}\n"), "fn main() {}\n"), false).unwrap();

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

        revert(&created, true).unwrap();

        assert!(!path.exists());
        assert_eq!(revert(&created, true), Err(RevertError::Missing));
    }

    /// R38: a yes does what the question said, or nothing: here the file changed while the
    /// dialog was open, so that the whole file would go back instead of the edit's lines.
    #[test]
    fn revert_refuses_when_the_plan_changed_since_it_asked() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "a\nnew\nb\n").unwrap();
        let edit = diff(&path, Some("old\n"), "new\n");
        let asked = plan(&edit, false).unwrap();
        assert_eq!(asked, Undo::Snippet("a\nold\nb\n".into()));

        fs::write(&path, "new\n").unwrap();

        assert_eq!(
            super::revert(&edit, false, &asked),
            Err(RevertError::Changed)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "new\n");
    }

    /// R38: a created file replaced by a symlink is never deleted, nor what the link points to.
    #[cfg(unix)]
    #[test]
    fn revert_never_deletes_a_symlink() {
        let dir = tempfile::tempdir().unwrap();
        let (target, link) = (dir.path().join("target.txt"), dir.path().join("new.md"));
        fs::write(&target, "hi\n").unwrap();
        std::os::unix::fs::symlink(&target, &link).unwrap();

        assert_eq!(
            revert(&diff(&link, None, "hi\n"), true),
            Err(RevertError::Changed)
        );
        assert!(fs::symlink_metadata(&link).is_ok(), "the link stays");
        assert_eq!(fs::read_to_string(&target).unwrap(), "hi\n");
    }

    /// R34: a diff without old text over a file wai saw before the edit (a Write over it) never
    /// deletes it, nor empties a part of it.
    #[test]
    fn revert_refuses_to_delete_a_file_that_existed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("keep.txt");
        fs::write(&path, "written\n").unwrap();

        assert_eq!(
            revert(&diff(&path, None, "written\n"), false),
            Err(RevertError::Existed)
        );
        fs::write(&path, "top\nwritten\n").unwrap();
        assert_eq!(
            revert(&diff(&path, None, "written\n"), false),
            Err(RevertError::Existed)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "top\nwritten\n");
        // Even a file wai saw created stays when the edit wrote nothing.
        fs::write(&path, "").unwrap();
        assert_eq!(
            revert(&diff(&path, None, ""), true),
            Err(RevertError::Existed)
        );
        assert!(path.exists());
        assert_eq!(RevertError::Existed.to_string(), EXISTED);
        assert_eq!(EXISTED, "Can't revert: the file existed before this edit.");
    }

    /// Review Focus 2: an edit made after the agent's (by the user, or the agent again) is never
    /// clobbered.
    #[test]
    fn revert_refuses_when_file_changed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() { mine() }\n").unwrap();

        let edited = diff(&path, Some("fn old() {}\n"), "fn main() {}\n");
        assert_eq!(revert(&edited, false), Err(RevertError::Changed));
        let created = diff(&path, None, "fn main() {}\n");
        assert_eq!(revert(&created, true), Err(RevertError::Changed));

        assert_eq!(fs::read_to_string(&path).unwrap(), "fn main() { mine() }\n");
        assert_eq!(RevertError::Changed.to_string(), CHANGED);
        assert_eq!(CHANGED, "File changed since this edit — open it to compare");
    }

    fn undo_snippet(now: &str, old: Option<&str>, new: &str) -> Result<String, RevertError> {
        let diff = diff(std::path::Path::new("/p/a"), old, new);
        undo(now, &diff, true).map(|undo| match undo {
            Undo::Whole(text) | Undo::Snippet(text) => text,
            Undo::Delete => panic!("a rewrite, not a deletion"),
        })
    }

    /// Claude's hunks show each leading tab as two spaces: the hunk still finds its place, and the
    /// old lines go back with tabs.
    #[test]
    fn snippet_revert_reads_tabs_as_claude_shows_them() {
        let now = "fn f() {\n\tif x {\n\t\tb()\n\t}\n}\n";
        assert_eq!(
            undo_snippet(
                now,
                Some("  if x {\n    a()\n  }"),
                "  if x {\n    b()\n  }"
            ),
            Ok("fn f() {\n\tif x {\n\t\ta()\n\t}\n}\n".into())
        );
        // An odd space stays a space.
        assert_eq!(
            undo_snippet("\tb\n", Some("   a"), "  b"),
            Ok("\t a\n".into())
        );
    }

    /// Claude's hunks have LF line ends: a CRLF file matches them and keeps its CRLF.
    #[test]
    fn snippet_revert_keeps_crlf() {
        let hunk = (Some("a\nb\nc"), "a\nb2\nc");
        assert_eq!(
            undo_snippet("top\r\na\r\nb2\r\nc\r\nend\r\n", hunk.0, hunk.1),
            Ok("top\r\na\r\nb\r\nc\r\nend\r\n".into())
        );
        assert_eq!(
            undo_snippet("x\r\n\tnew\r\ny\r\n", Some("x\n  old\ny"), "x\n  new\ny"),
            Ok("x\r\n\told\r\ny\r\n".into()),
            "tabs and CRLF together"
        );
        // A needle that ends with its line break.
        assert_eq!(
            undo_snippet("k\r\nb2\r\n", Some("b\n"), "b2\n"),
            Ok("k\r\nb\r\n".into())
        );
    }

    /// Review focus 2: a file that mixes line ends could not have been written by Claude's edit as
    /// it reported it.
    #[test]
    fn revert_refuses_mixed_line_ends() {
        assert_eq!(
            undo_snippet("a\r\nb2\nc\r\n", Some("a\nb\nc"), "a\nb2\nc"),
            Err(RevertError::Changed)
        );
        // A real change in a CRLF file is still a change.
        assert_eq!(
            undo_snippet("a\r\nb3\r\nc\r\n", Some("a\nb\nc"), "a\nb2\nc"),
            Err(RevertError::Changed)
        );
    }

    /// Column-0 context matches exactly, yet `old` still goes back with the file's tabs / CRLF.
    #[test]
    fn exact_match_still_retabs_and_keeps_crlf() {
        assert_eq!(
            undo_snippet(
                "package a\nfunc f() {\n}\n",
                Some("func f() {\n  x()\n}"),
                "func f() {\n}"
            ),
            Ok("package a\nfunc f() {\n  x()\n}\n".into()),
            "no tabs in the file: written as is"
        );
        assert_eq!(
            undo_snippet(
                "package a\n\tkeep()\nfunc f() {\n}\n",
                Some("func f() {\n  x()\n}"),
                "func f() {\n}"
            ),
            Ok("package a\n\tkeep()\nfunc f() {\n\tx()\n}\n".into())
        );
        // CRLF, no final newline, single-line hunk at the end.
        assert_eq!(
            undo_snippet("a\r\nb2", Some("b\nc"), "b2"),
            Ok("a\r\nb\r\nc".into())
        );
    }

    /// The whole file, through the disk: a CRLF file comes back CRLF.
    #[test]
    fn revert_puts_back_a_crlf_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("win.txt");
        fs::write(&path, "a\r\nb2\r\n").unwrap();
        revert(&diff(&path, Some("a\nb\n"), "a\nb2\n"), false).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "a\r\nb\r\n");
    }

    /// R32: claude-agent-acp reports an Edit as hunks (context and changed lines, no trailing
    /// newline). The one place the hunk sits goes back; edits elsewhere stay.
    #[test]
    fn snippet_revert_replaces_the_only_occurrence() {
        let hunk = (Some("a\nb\nc"), "a\nb2\nc");
        assert_eq!(
            undo_snippet("top mine\na\nb2\nc\nend\n", hunk.0, hunk.1),
            Ok("top mine\na\nb\nc\nend\n".into())
        );
        // No old text: the inserted lines go.
        assert_eq!(
            undo_snippet("x\nadded\ny\n", None, "added\n"),
            Ok("x\ny\n".into())
        );
        // Twice, nowhere, or empty: which place is meant is unknown.
        let changed = Err(RevertError::Changed);
        assert_eq!(undo_snippet("b2\nb2\n", Some("b"), "b2"), changed);
        assert_eq!(undo_snippet("a\nb3\nc\n", hunk.0, hunk.1), changed);
        assert_eq!(undo_snippet("x\n", Some("y"), ""), changed);
    }

    /// `str::matches` would see one `a\na` here; the hunk sits at two overlapping places.
    #[test]
    fn snippet_revert_counts_overlapping_occurrences() {
        let now = "a\na\na\n";
        assert_eq!(now.matches("a\na").count(), 1);
        assert_eq!(
            undo_snippet(now, Some("z"), "a\na"),
            Err(RevertError::Changed)
        );
    }

    /// A snippet goes back only where it covers whole lines.
    #[test]
    fn snippet_revert_sits_on_whole_lines() {
        let changed = Err(RevertError::Changed);
        assert_eq!(undo_snippet("max = 1\n", Some("x = 0"), "x = 1"), changed);
        assert_eq!(
            undo_snippet("let x = 1;\n", Some("x = 0"), "x = 1"),
            changed
        );
        assert_eq!(
            undo_snippet("y = 2\nx = 1\n", Some("x = 0"), "x = 1"),
            Ok("y = 2\nx = 0\n".into())
        );
        assert_eq!(
            undo_snippet("x = 1", Some("x = 0"), "x = 1"),
            Ok("x = 0".into())
        );
    }

    fn tool(diffs: &[&str], locations: Vec<Location>) -> ToolItem {
        ToolItem {
            id: "t".into(),
            title: String::new(),
            kind: "edit".into(),
            status: ToolStatus::Completed,
            text: String::new(),
            diffs: diffs
                .iter()
                .map(|p| diff(std::path::Path::new(p), Some("a"), "b"))
                .collect(),
            locations,
            first_sight: vec![],
            expanded: false,
            subagent: None,
        }
    }

    /// A late tool update can change a finished turn's diffs: the step must still name the row's
    /// file.
    #[test]
    fn next_step_refuses_a_shifted_step() {
        let (first, second) = (tool(&["/p/a"], vec![]), tool(&["/p/b", "/p/a"], vec![]));
        let steps = [(&first, 0), (&second, 0), (&second, 1)];
        let file = |path: &str, steps: Vec<usize>, reverted| EditedFile {
            path: path.into(),
            steps,
            reverted,
            failed: None,
        };

        let a = file("/p/a", vec![0, 2], 0);
        assert!(matches!(next_step(&steps, &a), Ok((t, 1)) if std::ptr::eq(t, &second)));
        let a = file("/p/a", vec![0, 2], 1);
        assert!(matches!(next_step(&steps, &a), Ok((t, 0)) if std::ptr::eq(t, &first)));
        // Step 1 is /p/b's now.
        let shifted = file("/p/a", vec![1], 0);
        assert_eq!(
            next_step(&steps, &shifted).err(),
            Some(RevertError::Changed)
        );
        let gone = file("/p/a", vec![7], 0);
        assert_eq!(next_step(&steps, &gone).err(), Some(RevertError::Changed));
    }

    /// R33: a hunk opens at its location's line plus where the change starts inside it.
    #[test]
    fn open_line_follows_locations() {
        let at = |path: &str, line: Option<u32>| Location {
            path: path.into(),
            line,
        };
        let hunk = |path: &str| FileDiff {
            path: path.into(),
            old_text: Some("c1\nc2\nold\nc3".into()),
            new_text: "c1\nc2\nnew\nc3".into(),
        };
        // Paired by index: line 10 is `c1`, the change is on the hunk's third line.
        let diffs = [hunk("/p/a"), hunk("/p/b")];
        let paired = [at("/p/a", Some(10)), at("/p/b", Some(40))];
        assert_eq!(open_line(&paired, &diffs, 0), 12);
        assert_eq!(open_line(&paired, &diffs, 1), 42);
        // Counts differ: the first location of that file with a line.
        let loose = [at("/p/a", None), at("/p/b", Some(5)), at("/p/a", Some(7))];
        assert_eq!(open_line(&loose, &diffs, 0), 7);
        // A pair with another path, or no line anywhere: the diff's own first change.
        let crossed = [at("/p/b", Some(40)), at("/p/a", None)];
        assert_eq!(open_line(&crossed, &diffs, 0), 3);
        assert_eq!(open_line(&[], &diffs, 1), 3);
        assert_eq!(open_line(&[], &diffs, 2), 1);
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
