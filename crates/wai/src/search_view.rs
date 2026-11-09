//! The Search tab: a query with regex and case toggles, results streamed in from a background
//! search and grouped by file.

use std::ops::Range;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::base::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use wai_project::{MAX_MATCHES, Match, SearchHandle, SearchQuery, search_in};

use crate::sidebar;
use crate::workspace::folder_name;

/// Typing waits this long for the next key before it searches.
const DEBOUNCE: Duration = Duration::from_millis(150);

/// A match clicked in the results; the workspace opens the file at the line.
pub struct OpenMatch {
    pub path: PathBuf,
    pub line: u32,
}

enum Row {
    File { path: PathBuf, count: usize },
    Match(Match),
}

pub struct SearchView {
    root: PathBuf,
    global_ignores: bool,
    input: Entity<InputState>,
    regex: bool,
    case_sensitive: bool,
    rows: Vec<Row>,
    total: usize,
    searching: bool,
    /// The query is not a valid regular expression.
    invalid: bool,
    /// The index in `rows` of the file header the last match went under.
    header: usize,
    /// Dropping the handle stops the worker.
    handle: Option<SearchHandle>,
    /// Draining the running search, and waiting out the typing burst: replacing either drops
    /// (cancels) the old one. Nothing runs while the user is not typing.
    drain: Option<Task<()>>,
    pending: Option<Task<()>>,
    _input: Subscription,
}

impl EventEmitter<OpenMatch> for SearchView {}

impl SearchView {
    pub fn new(
        root: PathBuf,
        global_ignores: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder("Search"));
        let subscription = cx.subscribe(&input, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                this.schedule(cx);
            }
        });
        Self {
            root,
            global_ignores,
            input,
            regex: false,
            case_sensitive: false,
            rows: Vec::new(),
            total: 0,
            searching: false,
            invalid: false,
            header: 0,
            handle: None,
            drain: None,
            pending: None,
            _input: subscription,
        }
    }

    pub fn query_focus(&self, cx: &App) -> FocusHandle {
        self.input.focus_handle(cx)
    }

    /// Searches the other project for the same query.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if root != self.root {
            self.root = root;
            self.start(cx);
        }
    }

    /// One search per burst of changes: each one replaces the wait that is still pending.
    fn schedule(&mut self, cx: &mut Context<Self>) {
        self.pending = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(DEBOUNCE).await;
            this.update(cx, |this, cx| this.start(cx)).ok();
        }));
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        self.pending = None;
        self.handle = None;
        self.drain = None;
        self.rows.clear();
        self.total = 0;
        self.header = 0;
        let pattern = self.input.read(cx).value().to_string();
        self.searching = !pattern.is_empty();
        self.invalid = false;
        cx.notify();
        if !self.searching {
            return;
        }
        let query = SearchQuery {
            pattern,
            regex: self.regex,
            case_sensitive: self.case_sensitive,
        };
        let Some((handle, batches)) = search_in(&self.root, query, self.global_ignores) else {
            (self.searching, self.invalid) = (false, true);
            return;
        };
        self.handle = Some(handle);
        self.drain = Some(cx.spawn(async move |this, cx| {
            // Bursts coalesce: one update, one redraw.
            while let Ok(mut batch) = batches.recv().await {
                while let Ok(more) = batches.try_recv() {
                    batch.extend(more);
                }
                if this.update(cx, |this, cx| this.push(batch, cx)).is_err() {
                    return;
                }
            }
            this.update(cx, |this, cx| {
                this.searching = false;
                cx.notify();
            })
            .ok();
        }));
    }

    fn push(&mut self, batch: Vec<Match>, cx: &mut Context<Self>) {
        for m in batch {
            self.total += 1;
            // Matches arrive file by file: a file's rows are one run under its header.
            let same = matches!(self.rows.get(self.header), Some(Row::File { path, .. }) if *path == m.path);
            if !same {
                self.header = self.rows.len();
                self.rows.push(Row::File {
                    path: m.path.clone(),
                    count: 0,
                });
            }
            if let Some(Row::File { count, .. }) = self.rows.get_mut(self.header) {
                *count += 1;
            }
            self.rows.push(Row::Match(m));
        }
        cx.notify();
    }

    fn toggle(&mut self, regex: bool, cx: &mut Context<Self>) {
        if regex {
            self.regex = !self.regex;
        } else {
            self.case_sensitive = !self.case_sensitive;
        }
        self.start(cx);
    }

    fn row(&self, ix: usize, cx: &Context<Self>) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        match &self.rows[ix] {
            Row::File { path, count } => {
                let shown = path.strip_prefix(&self.root).unwrap_or(path);
                h_flex()
                    .w_full()
                    .h(px(sidebar::ROW_HEIGHT))
                    .pl_3()
                    .pr_2()
                    .gap_2()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(div().flex_1().min_w_0().truncate().child(display(shown)))
                    .child(div().text_xs().text_color(muted).child(count.to_string()))
                    .into_any_element()
            }
            Row::Match(m) => {
                let (path, line) = (m.path.clone(), m.line);
                sidebar::row(false, cx)
                    .id(("match", ix))
                    .pl(px(24.))
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(m.line.to_string()),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(m.text.clone()))
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(OpenMatch {
                            path: path.clone(),
                            line,
                        })
                    }))
                    .test_support()
                    .into_any_element()
            }
        }
    }

    fn status(&self, cx: &App) -> Option<String> {
        let files = self
            .rows
            .iter()
            .filter(|r| matches!(r, Row::File { .. }))
            .count();
        if self.total >= MAX_MATCHES {
            Some(format!("Showing first {}", thousands(MAX_MATCHES)))
        } else if self.searching {
            Some("Searching…".into())
        } else if self.input.read(cx).value().is_empty() {
            None
        } else if self.invalid {
            Some("Invalid regular expression".into())
        } else if self.total == 0 {
            Some("No results".into())
        } else {
            Some(summary(self.total, files))
        }
    }
}

/// `1 match in 1 file`, `5 matches in 3 files`.
fn summary(matches: usize, files: usize) -> String {
    let plural =
        |n: usize, one: &str, many: &str| format!("{n} {}", if n == 1 { one } else { many });
    format!(
        "{} in {}",
        plural(matches, "match", "matches"),
        plural(files, "file", "files")
    )
}

fn display(path: &Path) -> String {
    let name = folder_name(path);
    match path
        .parent()
        .and_then(|p| p.to_str())
        .filter(|p| !p.is_empty())
    {
        Some(dir) => format!("{name}  {dir}"),
        None => name,
    }
}

fn thousands(n: usize) -> String {
    format!("{},{:03}", n / 1000, n % 1000)
}

impl Render for SearchView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status(cx);
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .px_2()
                    .py_1()
                    .gap_1()
                    .child(Input::new(&self.input).flex_1())
                    .child(
                        Button::new("search-regex")
                            .ghost()
                            .xsmall()
                            .label(".*")
                            .tooltip("Regular expression")
                            .selected(self.regex)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle(true, cx))),
                    )
                    .child(
                        Button::new("search-case")
                            .ghost()
                            .xsmall()
                            .label("Aa")
                            .tooltip("Match case")
                            .selected(self.case_sensitive)
                            .on_click(cx.listener(|this, _, _, cx| this.toggle(false, cx))),
                    ),
            )
            .children(status.map(|text| {
                div()
                    .px_3()
                    .pb_1()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(text)
            }))
            // Only the visible rows render: a search can find thousands of matches.
            .child(
                uniform_list(
                    "search-results",
                    self.rows.len(),
                    cx.processor(|this, range: Range<usize>, _, cx| {
                        range.map(|ix| this.row(ix, cx)).collect::<Vec<_>>()
                    }),
                )
                .flex_1(),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use super::summary;
    use crate::sidebar::canonical;
    use crate::workspace::tests::{open, wait_until};
    use crate::workspace::{Pane, Workspace};

    #[test]
    fn status_counts_in_singular_and_plural() {
        assert_eq!(summary(1, 1), "1 match in 1 file");
        assert_eq!(summary(2, 1), "2 matches in 1 file");
        assert_eq!(summary(5, 3), "5 matches in 3 files");
    }

    fn click(id: impl Into<gpui_kit::ElementId>, cx: &mut VisualTestContext) {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    fn match_count(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
        workspace.read_with(cx, |w, cx| w.search.read(cx).total)
    }

    #[gpui_kit::test]
    fn clicking_match_opens_file_at_line(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the search runs on a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join("a.txt"), "one\ntwo\nneedle here\nfour\n").unwrap();
        fs::write(root.join("b.txt"), "nothing\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        cx.run_until_parked();

        click("tab-search", cx);
        // Typed text is a change event; the search follows after the debounce.
        cx.update(|window, cx| window.input("needle", cx));
        wait_until(cx, "the match", |cx| match_count(&workspace, cx) == 1);

        // Row 0 is the file's header, row 1 the match.
        click(("match", 1usize), cx);
        wait_until(cx, "the file open", |cx| {
            workspace.read_with(cx, |w, _| matches!(&w.panes[..], [Pane::Editor(_)]))
        });
        let (path, line) = workspace.read_with(cx, |w, cx| match &w.panes[..] {
            [Pane::Editor(pane)] => {
                let tab = pane.read(cx).tabs[0].read(cx);
                (tab.path.clone(), tab.editor.read(cx).cursor_position().line)
            }
            _ => unreachable!(),
        });
        assert_eq!(path, root.join("a.txt"));
        assert_eq!(line, 2); // 0-based
    }

    /// A pattern that is not a regular expression says so, not "No results".
    #[gpui_kit::test]
    fn invalid_regex_says_so(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the search runs on a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::write(root.join("a.txt"), "(x\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        cx.run_until_parked();

        click("tab-search", cx);
        click("search-regex", cx);
        cx.update(|window, cx| window.input("(", cx));

        // The typing debounce, then the search.
        wait_until(cx, "the search to end", |cx| {
            let search = workspace.read_with(cx, |w, _| w.search.clone());
            search.read_with(cx, |s, _| s.pending.is_none() && !s.searching)
        });
        let status = workspace.read_with(cx, |w, cx| w.search.read(cx).status(cx));
        assert_eq!(status.as_deref(), Some("Invalid regular expression"));
    }
}
