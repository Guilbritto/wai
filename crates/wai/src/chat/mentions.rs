//! The `@` menu: the project's files and folders, listed above the prompt box while the word at
//! the caret starts with `@`. Picking one puts it in the box as a token. A running chat and the
//! new-session draft share it.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::input::{
    Enter, Escape, IndentInline, InlineTokenSpan, MoveDown, MoveUp, TextareaState,
};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_project::Entry;

use super::input::{file_token, slashed};

/// A row's height: the list shows 8 rows and scrolls past them.
const ROW: f32 = 28.;
/// The most rows listed.
const ROWS: usize = 50;
/// The most files and folders read from a project.
const FILES: usize = 20_000;

/// The `@query` the caret ends: an `@` at the start of the text or after a blank, then no blank,
/// clear of any token. Its range, `@` included, and the query.
fn query<'a>(
    text: &'a str,
    caret: usize,
    tokens: &[InlineTokenSpan],
) -> Option<(Range<usize>, &'a str)> {
    let before = text.get(..caret)?;
    let start = before.trim_end_matches(|c: char| !c.is_whitespace()).len();
    let query = before[start..].strip_prefix('@')?;
    let in_token = (tokens.iter()).any(|s| s.range().start < caret && start < s.range().end);
    (!in_token).then_some((start..caret, query))
}

/// The entries for `query`, ignoring case: names that start with it, then names that contain it,
/// then paths from `folder` that contain it; `ROWS` at most.
fn matching(files: &[Entry], folder: &Path, query: &str) -> Vec<Entry> {
    let query = query.to_lowercase();
    let mut tiers: [Vec<&Entry>; 3] = Default::default();
    for entry in files {
        let name = entry.name.to_lowercase();
        let tier = if name.starts_with(&query) {
            0
        } else if name.contains(&query) {
            1
        } else if relative(entry, folder).to_lowercase().contains(&query) {
            2
        } else {
            continue;
        };
        tiers[tier].push(entry);
    }
    tiers.into_iter().flatten().take(ROWS).cloned().collect()
}

/// `entry`'s path from `folder`, `/` between its parts.
fn relative(entry: &Entry, folder: &Path) -> String {
    slashed(entry.path.strip_prefix(folder).unwrap_or(&entry.path))
}

pub(crate) struct Mentions {
    input: Entity<TextareaState>,
    /// The folder the prompt box is about: the session's or the draft's.
    folder: PathBuf,
    /// The last folder listed and what it holds; `None` until a listing ends.
    files: Option<(PathBuf, Arc<Vec<Entry>>)>,
    /// The `@query` being typed: its range in the text; `None`: no menu.
    range: Option<Range<usize>>,
    rows: Vec<Entry>,
    ix: usize,
    /// Esc closed the menu: it opens again when the text changes.
    closed: bool,
    scroll: ScrollHandle,
    _listing: Option<Task<()>>,
}

impl Mentions {
    pub(crate) fn new(input: Entity<TextareaState>) -> Self {
        Self {
            input,
            folder: PathBuf::new(),
            files: None,
            range: None,
            rows: Vec::new(),
            ix: 0,
            closed: false,
            scroll: ScrollHandle::new(),
            _listing: None,
        }
    }

    /// The prompt box's text changed: the menu follows the word at the caret. A new `@`, or
    /// another `folder`, lists the folder again; the last list shows meanwhile.
    pub(crate) fn changed(&mut self, folder: &Path, cx: &mut Context<Self>) {
        (self.ix, self.closed) = (0, false);
        self.folder = folder.to_path_buf();
        let input = self.input.read(cx);
        let text = input.value();
        let found = query(&text, input.cursor(), input.tokens());
        let listed = self.files.as_ref().map(|(folder, _)| folder);
        let stale = listed != Some(&self.folder);
        if found.is_some_and(|(_, q)| q.is_empty() || stale) {
            self.list(cx);
        }
        self.refresh(cx);
    }

    /// The rows for the `@query` at the caret.
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let input = self.input.read(cx);
        let text = input.value();
        let found = query(&text, input.cursor(), input.tokens());
        self.range = found.as_ref().map(|(range, _)| range.clone());
        self.rows = match (found, &self.files) {
            (Some((_, query)), Some((_, files))) => matching(files, &self.folder, query),
            _ => Vec::new(),
        };
        cx.notify();
    }

    /// Lists the folder off the UI thread.
    fn list(&mut self, cx: &mut Context<Self>) {
        let folder = self.folder.clone();
        let read = cx.background_spawn(async move {
            let files = wai_project::list_all(&folder, true, FILES);
            (folder, Arc::new(files))
        });
        self._listing = Some(cx.spawn(async move |this, cx| {
            let files = read.await;
            this.update(cx, |this, cx| {
                this.files = Some(files);
                this.refresh(cx);
            })
            .ok();
        }));
    }

    /// Whether the menu shows: rows match, Esc did not close it, and the caret is still at the
    /// end of the `@query`.
    pub(crate) fn is_open(&self, cx: &App) -> bool {
        let caret = self.input.read(cx).cursor();
        !self.closed && !self.rows.is_empty() && self.range.as_ref().is_some_and(|r| r.end == caret)
    }

    /// Moves the selection, round the ends.
    fn move_selection(&mut self, by: isize, cx: &mut Context<Self>) {
        if !self.is_open(cx) {
            return;
        }
        let n = self.rows.len() as isize;
        self.ix = (self.ix.min(self.rows.len() - 1) as isize + by).rem_euclid(n) as usize;
        self.scroll.scroll_to_item(self.ix);
        cx.stop_propagation();
        cx.notify();
    }

    /// Puts row `ix` (`None`: the selected one) in place of the `@query`, then a space.
    fn complete(&mut self, ix: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        if !self.is_open(cx) {
            return;
        }
        let ix = ix.unwrap_or(self.ix).min(self.rows.len() - 1);
        let (Some(range), Some(entry)) = (self.range.clone(), self.rows.get(ix)) else {
            return;
        };
        let path = entry.path.clone();
        let folder = self.folder.clone();
        self.input.update(cx, |input, cx| {
            let token = file_token(&path, &folder, input.tokens());
            if input
                .replace_range_with_token(range, token, window, cx)
                .is_ok()
            {
                input.insert(" ", window, cx);
            }
            input.focus(window, cx);
        });
        cx.stop_propagation();
        cx.notify();
    }

    /// Closes the menu, leaving the text; typing opens it again.
    fn close(&mut self, cx: &mut Context<Self>) {
        if !self.is_open(cx) {
            return;
        }
        self.closed = true;
        cx.stop_propagation();
        cx.notify();
    }
}

/// While the menu is open, ↑/↓, Enter, Tab and Esc are its own. `el` holds the prompt box.
pub(super) fn keys(el: Div, mentions: &Entity<Mentions>) -> Div {
    fn on<A: Action>(
        mentions: &Entity<Mentions>,
        f: impl Fn(&mut Mentions, &A, &mut Window, &mut Context<Mentions>) + 'static,
    ) -> impl Fn(&A, &mut Window, &mut App) + 'static {
        let mentions = mentions.downgrade();
        move |action, window, cx| {
            mentions.update(cx, |m, cx| f(m, action, window, cx)).ok();
        }
    }
    el.capture_action(on(mentions, |m, _: &MoveUp, _, cx| {
        m.move_selection(-1, cx)
    }))
    .capture_action(on(mentions, |m, _: &MoveDown, _, cx| {
        m.move_selection(1, cx)
    }))
    .capture_action(on(mentions, |m, enter: &Enter, window, cx| {
        if !enter.shift && !enter.secondary {
            m.complete(None, window, cx);
        }
    }))
    .capture_action(on(mentions, |m, _: &IndentInline, window, cx| {
        m.complete(None, window, cx)
    }))
    .capture_action(on(mentions, |m, _: &Escape, _, cx| m.close(cx)))
}

impl Render for Mentions {
    /// The open menu, laid over what is above the prompt box.
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.is_open(cx) {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let selected = self.ix.min(self.rows.len() - 1);
        let rows = self.rows.iter().enumerate().map(|(ix, entry)| {
            let icon = match entry.is_dir {
                true => Lucide::Folder,
                false => Lucide::File,
            };
            let parent = Path::new(&relative(entry, &self.folder))
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            h_flex()
                .id(("mention", ix))
                .flex_none()
                .h(px(ROW))
                .px_3()
                .gap_2()
                .cursor_pointer()
                .when(ix == selected, |el| {
                    el.bg(theme.accent).text_color(theme.accent_foreground)
                })
                .child(Icon::new(icon).xsmall().text_color(theme.muted_foreground))
                .child(div().flex_none().child(entry.name.clone()))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .text_color(theme.muted_foreground)
                        .child(parent),
                )
                .on_click(
                    cx.listener(move |this, _, window, cx| this.complete(Some(ix), window, cx)),
                )
                // Registers the id so tests can find and click it.
                .test_support()
        });
        v_flex()
            .id("mentions")
            .absolute()
            .bottom_full()
            .left_0()
            .right_0()
            .mb_2()
            .py_1()
            .max_h(px(8. * ROW))
            .overflow_y_scroll()
            .track_scroll(&self.scroll)
            .occlude()
            .rounded(px(12.))
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .text_size(px(13.))
            .text_color(theme.popover_foreground)
            .children(rows)
            // Registers the id so tests can find it.
            .test_support()
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use gpui_kit::component::input::{InlineToken, InputContent};
    use wai_project::Entry;

    use super::{matching, query};

    #[test]
    fn query_is_the_at_word_at_the_caret() {
        let q = |text: &str| query(text, text.len(), &[]).map(|(r, q)| (r, q.to_string()));
        assert_eq!(q("@"), Some((0..1, "".into())));
        assert_eq!(q("see @src/ma"), Some((4..11, "src/ma".into())));
        assert_eq!(q("line\n@x"), Some((5..7, "x".into())));
        assert_eq!(q("a@b"), None);
        assert_eq!(q("@src "), None);
        assert_eq!(q("plain"), None);
        // The caret before the end: the word up to it.
        assert_eq!(query("@abc def", 3, &[]).map(|(r, _)| r), Some(0..3));
        // A token's own `@` text opens nothing.
        let content = InputContent::new("@a.rs")
            .with_token(0..5, InlineToken::new("/p/a.rs", "@a.rs"))
            .unwrap();
        assert_eq!(query(content.text(), 5, content.tokens()), None);
    }

    #[test]
    fn names_that_start_with_the_query_come_first() {
        let folder = Path::new("/p");
        let entry = |path: &str| Entry {
            path: PathBuf::from(path),
            name: Path::new(path)
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into(),
            is_dir: false,
        };
        let files = [
            entry("/p/src/main.rs"),
            entry("/p/domain.md"),
            entry("/p/Main.txt"),
            entry("/p/main/x.rs"),
            entry("/p/other.rs"),
        ];
        let names = |query| -> Vec<String> {
            let found = matching(&files, folder, query);
            found.into_iter().map(|e| e.name).collect()
        };
        assert_eq!(names("MAIN"), ["main.rs", "Main.txt", "domain.md", "x.rs"]);
        assert_eq!(names("src/"), ["main.rs"]);
        assert_eq!(names("").len(), 5);
        assert!(names("zzz").is_empty());
    }
}
