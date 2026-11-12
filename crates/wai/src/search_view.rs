//! The Search tab: a query with regex and case toggles, results streamed in from a background
//! search and grouped by file.

use std::ops::Range;
use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::base::Selectable as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use wai_project::{MAX_MATCHES, Match, SearchHandle, SearchQuery, search_in};

use crate::files::file_mark;
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
    File {
        path: PathBuf,
        count: usize,
        folded: bool,
    },
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
    /// The match row last clicked; a new search clears it.
    clicked: Option<usize>,
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
            clicked: None,
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
        self.clicked = None;
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
                    folded: false,
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

    fn fold(&mut self, ix: usize, cx: &mut Context<Self>) {
        if let Some(Row::File { folded, .. }) = self.rows.get_mut(ix) {
            *folded = !*folded;
            cx.notify();
        }
    }

    fn collapse_all(&mut self, cx: &mut Context<Self>) {
        for row in &mut self.rows {
            if let Row::File { folded, .. } = row {
                *folded = true;
            }
        }
        cx.notify();
    }

    /// The indexes in `rows` that show: a folded file hides its matches.
    fn visible(&self) -> Vec<usize> {
        let mut folded = false;
        let mut shown = Vec::with_capacity(self.rows.len());
        for (ix, row) in self.rows.iter().enumerate() {
            match row {
                Row::File { folded: f, .. } => {
                    folded = *f;
                    shown.push(ix);
                }
                Row::Match(_) if !folded => shown.push(ix),
                Row::Match(_) => {}
            }
        }
        shown
    }

    fn row(&self, ix: usize, cx: &Context<Self>) -> AnyElement {
        let theme = cx.theme();
        let muted = theme.muted_foreground;
        match &self.rows[ix] {
            Row::File {
                path,
                count,
                folded,
            } => {
                let shown = path.strip_prefix(&self.root).unwrap_or(path);
                let name = folder_name(shown);
                let dir = shown
                    .parent()
                    .and_then(|p| p.to_str())
                    .filter(|p| !p.is_empty());
                let (text, mark) = file_mark(&name);
                let chevron = match folded {
                    true => IconName::ChevronRight,
                    false => IconName::ChevronDown,
                };
                sidebar::row(false, cx)
                    .id(("search-file", ix))
                    .pl(px(8.))
                    .child(Icon::new(chevron).xsmall().text_color(muted))
                    .child(
                        div()
                            .w(px(12.))
                            .flex_none()
                            .text_center()
                            .text_size(px(10.))
                            .text_color(mark.color(cx))
                            .child(text),
                    )
                    .child(
                        div().flex_1().min_w_0().truncate().child(
                            div().font_weight(FontWeight::SEMIBOLD).child(name).child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .children(dir.map(|dir| format!("  {dir}"))),
                            ),
                        ),
                    )
                    .child(
                        div()
                            .flex_none()
                            .px_1p5()
                            .rounded_full()
                            .bg(muted.opacity(0.15))
                            .text_size(px(10.))
                            .text_color(muted)
                            .child(count.to_string()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| this.fold(ix, cx)))
                    .test_support()
                    .into_any_element()
            }
            Row::Match(m) => {
                let (path, line) = (m.path.clone(), m.line);
                let marks = highlights(
                    &m.text,
                    self.input.read(cx).value().as_ref(),
                    self.regex,
                    self.case_sensitive,
                );
                let hl = HighlightStyle {
                    background_color: Some(theme.warning.opacity(0.25)),
                    ..Default::default()
                };
                let text = StyledText::new(m.text.clone())
                    .with_highlights(marks.into_iter().map(|r| (r, hl)));
                let active = self.clicked == Some(ix);
                sidebar::row(active, cx)
                    .id(("match", ix))
                    .aria_selected(active)
                    .pl(px(24.))
                    .font_family(theme.mono_font_family.clone())
                    .text_size(px(12.))
                    .child(
                        div()
                            .flex_none()
                            .text_xs()
                            .text_color(muted)
                            .child(m.line.to_string()),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(text))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.clicked = Some(ix);
                        cx.notify();
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

/// The byte ranges of `query` in `text`, as the search matches it; none for an invalid regex.
fn highlights(text: &str, query: &str, regex: bool, case: bool) -> Vec<Range<usize>> {
    let pattern = if regex {
        query.into()
    } else {
        regex::escape(query)
    };
    match regex::RegexBuilder::new(&pattern)
        .case_insensitive(!case)
        .build()
    {
        Ok(re) if !query.is_empty() => re
            .find_iter(text)
            .filter(|m| !m.is_empty())
            .map(|m| m.range())
            .collect(),
        _ => Vec::new(),
    }
}

fn thousands(n: usize) -> String {
    format!("{},{:03}", n / 1000, n % 1000)
}

impl Render for SearchView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let status = self.status(cx);
        let theme = cx.theme();
        let focused = self.input.focus_handle(cx).contains_focused(window, cx);
        let title = sidebar::panel_title("Search", cx).child(
            Button::new("search-collapse")
                .ghost()
                .xsmall()
                .icon(Lucide::ChevronsDownUp)
                .tooltip("Collapse all")
                .accessibility_label("Collapse all")
                .on_click(cx.listener(|this, _, _, cx| this.collapse_all(cx))),
        );
        let field = h_flex()
            .mx_2()
            .mb_1()
            .pr_1()
            .gap_1()
            .items_center()
            .rounded(px(8.))
            .border_1()
            .border_color(match focused {
                true => theme.primary,
                false => theme.border,
            })
            .child(Input::new(&self.input).appearance(false).flex_1())
            .child(
                Button::new("search-case")
                    .ghost()
                    .xsmall()
                    .label("Aa")
                    .tooltip("Match case")
                    .selected(self.case_sensitive)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(false, cx))),
            )
            .child(
                Button::new("search-regex")
                    .ghost()
                    .xsmall()
                    .label(".*")
                    .tooltip("Regular expression")
                    .selected(self.regex)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle(true, cx))),
            );
        v_flex()
            .size_full()
            .child(title)
            .child(field)
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
                    self.visible().len(),
                    cx.processor(|this, range: Range<usize>, _, cx| {
                        let visible = this.visible();
                        visible[range]
                            .iter()
                            .map(|&ix| this.row(ix, cx))
                            .collect::<Vec<_>>()
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

    use super::{highlights, summary};
    use crate::sidebar::canonical;
    use crate::workspace::tests::{open, wait_until};
    use crate::workspace::{Pane, Workspace};

    #[test]
    fn status_counts_in_singular_and_plural() {
        assert_eq!(summary(1, 1), "1 match in 1 file");
        assert_eq!(summary(2, 1), "2 matches in 1 file");
        assert_eq!(summary(5, 3), "5 matches in 3 files");
    }

    #[test]
    fn highlights_find_every_match() {
        let text = "let per_route = per_route;";
        assert_eq!(highlights(text, "per_route", false, true), [4..13, 16..25]);
        assert_eq!(highlights(text, "PER", false, false), [4..7, 16..19]);
        assert_eq!(highlights(text, r"per_\w+", true, true), [4..13, 16..25]);
        assert_eq!(highlights("ação ação", "ção", false, true), [1..6, 8..13]);
        assert!(highlights(text, "(", true, true).is_empty());
    }

    /// Two files with one match each, searched: rows are file, match, file, match.
    fn two_files(
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, &mut VisualTestContext, tempfile::TempDir) {
        cx.executor().allow_parking(); // the search runs on a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::create_dir(root.join(".git")).unwrap();
        fs::write(root.join("a.txt"), "one\nneedle\n").unwrap();
        fs::write(root.join("b.txt"), "needle\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        cx.run_until_parked();
        click("tab-search", cx);
        cx.update(|window, cx| window.input("needle", cx));
        wait_until(cx, "both matches", |cx| match_count(&workspace, cx) == 2);
        (workspace, cx, dir)
    }

    fn shown(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
        workspace.read_with(cx, |w, cx| w.search.read(cx).visible().len())
    }

    #[gpui_kit::test]
    fn files_fold_in_results(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = two_files(cx);
        assert_eq!(shown(&workspace, cx), 4);
        click(("search-file", 0usize), cx);
        assert_eq!(shown(&workspace, cx), 3);
        click("search-collapse", cx);
        assert_eq!(shown(&workspace, cx), 2);
        // A new search starts unfolded.
        click("search-case", cx);
        wait_until(cx, "both matches again", |cx| {
            match_count(&workspace, cx) == 2
        });
        assert_eq!(shown(&workspace, cx), 4);
    }

    #[gpui_kit::test]
    fn clicked_match_is_marked(cx: &mut TestAppContext) {
        let (_workspace, cx, _dir) = two_files(cx);
        let marked = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                [1usize, 3].map(|ix| window.find(("match", ix)).selected())
            })
        };
        assert_eq!(marked(cx), [Some(false), Some(false)]);
        click(("match", 1usize), cx);
        assert_eq!(marked(cx), [Some(true), Some(false)]);
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
