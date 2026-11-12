//! The Files tab: the project tree, listed lazily and kept live by the project watcher.

use std::collections::{BTreeSet, HashMap, HashSet};
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use wai_project::{Entry, FsChange, list_dir};

use crate::{aurora, sidebar};

/// A file clicked in the tree; the workspace opens it.
pub struct OpenFile(pub PathBuf);

/// A visible row: an entry of an expanded folder whose ancestors are all expanded.
pub struct Row {
    pub depth: usize,
    pub entry: Entry,
}

pub struct FileTree {
    root: PathBuf,
    /// The expanded folders, `root` included. A collapsed folder's expanded subfolders stay here,
    /// hidden, and show again with it.
    expanded: HashSet<PathBuf>,
    /// The expanded folders of the projects open before, for when they come back.
    other_roots: HashMap<PathBuf, HashSet<PathBuf>>,
    /// Each listed folder's entries, with the request that listed them.
    children: HashMap<PathBuf, (u64, Vec<Entry>)>,
    pub rows: Vec<Row>,
    /// The file open in the editor, drawn active.
    active: Option<PathBuf>,
    requests: u64,
    /// Whether the user's global git excludes apply (`false` in tests: no `~/.gitconfig`).
    global_ignores: bool,
}

impl EventEmitter<OpenFile> for FileTree {}

impl FileTree {
    pub fn new(root: PathBuf, global_ignores: bool, cx: &mut Context<Self>) -> Self {
        let mut this = Self {
            expanded: HashSet::from([root.clone()]),
            root,
            other_roots: HashMap::new(),
            children: HashMap::new(),
            rows: Vec::new(),
            active: None,
            requests: 0,
            global_ignores,
        };
        this.list(vec![this.root.clone()], cx);
        this
    }

    /// Shows another project, with the folders it had expanded when it was last shown.
    pub fn set_root(&mut self, root: PathBuf, cx: &mut Context<Self>) {
        if root == self.root {
            return;
        }
        let expanded =
            (self.other_roots.remove(&root)).unwrap_or_else(|| HashSet::from([root.clone()]));
        let previous = std::mem::replace(&mut self.expanded, expanded);
        self.other_roots
            .insert(std::mem::replace(&mut self.root, root), previous);
        self.children.clear();
        self.rows.clear();
        self.list(self.expanded.iter().cloned().collect(), cx);
        cx.notify();
    }

    /// Re-lists the expanded folders a batch touched: a changed path's folder, or the path itself
    /// when it is an expanded folder. Collapsed and never-opened folders cost nothing.
    pub fn apply(&mut self, changes: Vec<FsChange>, cx: &mut Context<Self>) {
        let mut dirs = BTreeSet::new();
        for change in changes {
            match change {
                FsChange::Rescan => dirs.extend(self.expanded.iter().cloned()),
                FsChange::Changed(paths) => {
                    for path in &paths {
                        let touched = [Some(path.as_path()), path.parent()];
                        let touched = touched.into_iter().flatten();
                        dirs.extend(
                            touched
                                .filter(|dir| self.expanded.contains(*dir))
                                .map(Path::to_path_buf),
                        );
                    }
                }
            }
        }
        if !dirs.is_empty() {
            self.list(dirs.into_iter().collect(), cx);
        }
    }

    pub(crate) fn toggle(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if !self.expanded.remove(&dir) {
            self.expanded.insert(dir.clone());
            self.list(vec![dir], cx);
        }
        self.rebuild();
        cx.notify();
    }

    /// Folds every folder but the root.
    pub(crate) fn collapse_all(&mut self, cx: &mut Context<Self>) {
        let root = self.root.clone();
        self.expanded.retain(|dir| *dir == root);
        self.rebuild();
        cx.notify();
    }

    /// The editor's active file, or none.
    pub(crate) fn set_active(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        if self.active != path {
            self.active = path;
            cx.notify();
        }
    }

    /// Lists `dirs` on the background executor, then shows them.
    fn list(&mut self, dirs: Vec<PathBuf>, cx: &mut Context<Self>) {
        self.requests += 1;
        let (request, root, global) = (self.requests, self.root.clone(), self.global_ignores);
        cx.spawn(async move |this, cx| {
            let listed = cx
                .background_spawn(async move {
                    let listed = dirs.into_iter().map(|dir| {
                        let entries = list_dir(&root, &dir, global);
                        (dir, entries)
                    });
                    listed.collect::<Vec<_>>()
                })
                .await;
            this.update(cx, |this, cx| this.listed(request, listed, cx))
                .ok();
        })
        .detach();
    }

    fn listed(
        &mut self,
        request: u64,
        listed: Vec<(PathBuf, io::Result<Vec<Entry>>)>,
        cx: &mut Context<Self>,
    ) {
        for (dir, entries) in listed {
            // Collapsed meanwhile, or a newer listing already shown.
            let newer = self.children.get(&dir).is_some_and(|(r, _)| *r > request);
            if !self.expanded.contains(&dir) || newer {
                continue;
            }
            match entries {
                Ok(entries) => {
                    self.children.insert(dir, (request, entries));
                }
                // Gone: its parent's listing drops it too.
                Err(_) => {
                    self.children.remove(&dir);
                    if dir != self.root {
                        self.expanded.remove(&dir);
                    }
                }
            }
        }
        self.rebuild();
        cx.notify();
    }

    fn rebuild(&mut self) {
        let mut rows = Vec::new();
        if self.expanded.contains(&self.root) {
            push_rows(&self.children, &self.expanded, &self.root, 0, &mut rows);
        }
        self.rows = rows;
    }

    fn row(&self, ix: usize, cx: &Context<Self>) -> impl IntoElement + use<> {
        let Row { depth, entry } = &self.rows[ix];
        let theme = cx.theme();
        let (path, is_dir) = (entry.path.clone(), entry.is_dir);
        let open = !is_dir && self.active.as_ref() == Some(&path);
        let (chevron, mark) = if is_dir {
            let chevron = match self.expanded.contains(&path) {
                true => IconName::ChevronDown,
                false => IconName::ChevronRight,
            };
            let chevron = Icon::new(chevron)
                .xsmall()
                .text_color(theme.muted_foreground);
            let folder = Icon::new(Lucide::Folder).xsmall().text_color(theme.blue);
            (chevron.into_any_element(), folder.into_any_element())
        } else {
            let (text, mark) = file_mark(&entry.name);
            let mark = div()
                .w(px(12.))
                .flex_none()
                .text_center()
                .text_size(px(10.))
                .text_color(mark.color(cx))
                .child(text);
            (
                div().w(px(12.)).flex_none().into_any_element(),
                mark.into_any_element(),
            )
        };
        sidebar::row(open, cx)
            .id(("file", ix))
            .aria_selected(open)
            .pl(px(8. + 14. * (*depth + 1) as f32))
            .child(chevron)
            .child(mark)
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .truncate()
                    .child(entry.name.clone()),
            )
            .on_click(cx.listener(move |this, _, _, cx| {
                if is_dir {
                    this.toggle(path.clone(), cx);
                } else {
                    cx.emit(OpenFile(path.clone()));
                }
            }))
            .test_support()
    }
}

/// What colours a file's mark.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Mark {
    Orange,
    Blue,
    Amber,
    Cyan,
    Muted,
}

impl Mark {
    pub(crate) fn color(self, cx: &App) -> Hsla {
        let theme = cx.theme();
        match self {
            // The mockup's `rs` orange has no Aurora token.
            Mark::Orange => rgb(0xff9a5a).into(),
            Mark::Blue => theme.blue,
            Mark::Amber => aurora::ambar(),
            Mark::Cyan => theme.cyan,
            Mark::Muted => theme.muted_foreground,
        }
    }
}

/// The short mark drawn before a file's name, by its extension.
pub(crate) fn file_mark(name: &str) -> (&'static str, Mark) {
    match name.rsplit_once('.').map(|(_, ext)| ext) {
        Some("rs") => ("rs", Mark::Orange),
        Some("md") => ("M↓", Mark::Blue),
        Some("ts" | "tsx") => ("ts", Mark::Blue),
        Some("js") => ("js", Mark::Amber),
        Some("go") => ("go", Mark::Cyan),
        _ => ("≡", Mark::Muted),
    }
}

/// `dir`'s entries, each followed by its own when it is an expanded folder.
fn push_rows(
    children: &HashMap<PathBuf, (u64, Vec<Entry>)>,
    expanded: &HashSet<PathBuf>,
    dir: &Path,
    depth: usize,
    rows: &mut Vec<Row>,
) {
    let Some((_, entries)) = children.get(dir) else {
        return;
    };
    for entry in entries {
        rows.push(Row {
            depth,
            entry: entry.clone(),
        });
        if entry.is_dir && expanded.contains(&entry.path) {
            push_rows(children, expanded, &entry.path, depth + 1, rows);
        }
    }
}

impl Render for FileTree {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let title = sidebar::panel_title("Files", cx).child(
            Button::new("files-collapse")
                .ghost()
                .xsmall()
                .icon(Lucide::ChevronsDownUp)
                .tooltip("Collapse all")
                .accessibility_label("Collapse all")
                .on_click(cx.listener(|this, _, _, cx| this.collapse_all(cx))),
        );
        let root = self.root.clone();
        let chevron = match self.expanded.contains(&root) {
            true => IconName::ChevronDown,
            false => IconName::ChevronRight,
        };
        let name = root.file_name().map_or_else(
            || root.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        let header = h_flex()
            .id("files-root")
            .items_center()
            .gap_1()
            .px_2()
            .pt(px(6.))
            .pb(px(3.))
            .cursor_pointer()
            .text_size(px(10.5))
            .font_weight(FontWeight::BOLD)
            .text_color(theme.muted_foreground)
            .child(Icon::new(chevron).xsmall())
            .child(div().min_w_0().truncate().child(name.to_uppercase()))
            .on_click(cx.listener(move |this, _, _, cx| this.toggle(root.clone(), cx)))
            .test_support();
        // Only the visible rows render: an expanded folder can hold thousands of entries.
        let list = uniform_list(
            "files",
            self.rows.len(),
            cx.processor(|this, range: Range<usize>, _, cx| {
                range.map(|ix| this.row(ix, cx)).collect::<Vec<_>>()
            }),
        )
        .flex_1()
        .min_h_0();
        v_flex().size_full().child(title).child(header).child(list)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, Entity, Role, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use crate::sidebar::canonical;
    use crate::workspace::tests::{open, wait_until};
    use crate::workspace::{Pane, Workspace};

    fn names(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
        workspace.read_with(cx, |w, cx| {
            let rows = &w.files.read(cx).rows;
            rows.iter().map(|row| row.entry.name.clone()).collect()
        })
    }

    fn click(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    fn set_project(
        workspace: &Entity<Workspace>,
        project: std::path::PathBuf,
        cx: &mut VisualTestContext,
    ) {
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.set_project(project, window, cx)));
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn files_tab_expands_and_emits_open(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the watcher reports from a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        // Joined one name at a time: a canonical Windows path (`\\?\`) takes no `/`.
        let src = root.join("src");
        for d in [root.join(".git"), src.clone(), root.join("target")] {
            fs::create_dir_all(d).unwrap();
        }
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        fs::write(src.join("main.rs"), "").unwrap();
        fs::write(root.join("README.md"), "").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        set_project(&workspace, root.clone(), cx);

        click("tab-files", cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            let tab = |id| {
                let tab = window.find(id);
                (tab.role(), tab.label().map(str::to_owned), tab.selected())
            };
            let files = (Some(Role::Tab), Some("Files".into()), Some(true));
            assert_eq!(tab("tab-files"), files);
            assert_eq!(tab("tab-sessions").2, Some(false));
        });
        assert_eq!(names(&workspace, cx), ["src", ".gitignore", "README.md"]);

        // A folder toggles; only then is it listed.
        click(("file", 0usize), cx);
        assert_eq!(
            names(&workspace, cx),
            ["src", "main.rs", ".gitignore", "README.md"]
        );

        // A file opens in the editor.
        click(("file", 1usize), cx);
        let opened = workspace.read_with(cx, |w, cx| match &w.panes[..] {
            [Pane::Editor(pane)] => Some(pane.read(cx).tabs[0].read(cx).path.clone()),
            _ => None,
        });
        assert_eq!(opened, Some(src.join("main.rs")));

        // The watcher re-lists the open folder.
        fs::write(src.join("lib.rs"), "").unwrap();
        wait_until(cx, "lib.rs in the tree", |cx| {
            names(&workspace, cx).contains(&"lib.rs".to_string())
        });

        // Each project keeps its own expanded folders.
        let other = tempfile::tempdir().unwrap();
        set_project(&workspace, canonical(other.path()), cx);
        assert_eq!(names(&workspace, cx), Vec::<String>::new());
        set_project(&workspace, root.clone(), cx);
        assert_eq!(
            names(&workspace, cx),
            ["src", "lib.rs", "main.rs", ".gitignore", "README.md"]
        );

        click(("file", 0usize), cx);
        assert_eq!(names(&workspace, cx), ["src", ".gitignore", "README.md"]);
    }

    #[test]
    fn file_marks_by_extension() {
        let names = ["main.rs", "README.md", "Cargo.toml", "app.tsx", "x"];
        let marks: Vec<_> = names.iter().map(|n| super::file_mark(n).0).collect();
        assert_eq!(marks, ["rs", "M↓", "≡", "ts", "≡"]);
    }

    #[gpui_kit::test]
    fn collapse_all_folds_every_folder(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the watcher reports from a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        for d in ["a", "b"] {
            fs::create_dir_all(root.join(d)).unwrap();
            fs::write(root.join(d).join("f.txt"), "").unwrap();
        }
        let (workspace, cx) = open(cx, Settings::default());
        set_project(&workspace, root, cx);
        click("tab-files", cx);
        click(("file", 1usize), cx);
        click(("file", 0usize), cx);
        assert_eq!(names(&workspace, cx), ["a", "f.txt", "b", "f.txt"]);
        click("files-collapse", cx);
        assert_eq!(names(&workspace, cx), ["a", "b"]);
        click("files-root", cx);
        assert!(names(&workspace, cx).is_empty());
    }

    #[gpui_kit::test]
    fn the_open_file_is_marked(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the watcher reports from a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::write(root.join("limits.rs"), "").unwrap();
        fs::write(root.join("main.rs"), "").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        set_project(&workspace, root.clone(), cx);
        click("tab-files", cx);
        let selected = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                [0usize, 1].map(|ix| window.find(("file", ix)).selected())
            })
        };
        assert_eq!(selected(cx), [Some(false), Some(false)]);
        click(("file", 0usize), cx);
        wait_until(cx, "limits.rs opened", |cx| selected(cx)[0] == Some(true));
        assert_eq!(selected(cx), [Some(true), Some(false)]);
    }
}
