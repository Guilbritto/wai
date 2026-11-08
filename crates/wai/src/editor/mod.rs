//! The editor: an editor pane of tabs, each an open file.

mod file;
mod pane;

pub use file::load;
pub use pane::{EditorPane, Emptied, FileEditor};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use super::file::{OpenError, OpenMode, classify};
    use super::{EditorPane, FileEditor};
    use crate::sidebar::canonical;
    use crate::workspace::tests::{fake_settings, open, wait_until};
    use crate::workspace::{Leave, MAX_PANES, Pane, Workspace};

    const MB: u64 = 1024 * 1024;

    fn open_file(
        workspace: &Entity<Workspace>,
        path: &Path,
        line: Option<u32>,
        cx: &mut VisualTestContext,
    ) {
        let path = path.to_path_buf();
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_file(path, line, window, cx)));
        cx.run_until_parked();
    }

    fn editor_pane(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<EditorPane> {
        let pane = workspace.read_with(cx, |w, _| {
            w.panes.iter().find_map(|pane| match pane {
                Pane::Editor(pane) => Some(pane.clone()),
                _ => None,
            })
        });
        pane.expect("an editor pane")
    }

    fn active_tab(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<FileEditor> {
        let pane = editor_pane(workspace, cx);
        pane.read_with(cx, |p, _| p.tabs[p.active].clone())
    }

    fn paths(pane: &Entity<EditorPane>, cx: &mut VisualTestContext) -> Vec<PathBuf> {
        pane.read_with(cx, |p, cx| {
            p.tabs.iter().map(|t| t.read(cx).path.clone()).collect()
        })
    }

    fn text(tab: &Entity<FileEditor>, cx: &mut VisualTestContext) -> String {
        tab.read_with(cx, |t, cx| t.editor.read(cx).text().to_string())
    }

    fn dialog(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.has_active_dialog(cx))
    }

    fn leave(workspace: &Entity<Workspace>, then: Leave, cx: &mut VisualTestContext) {
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.leave(then, window, cx)));
        cx.run_until_parked();
    }

    fn click(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn binary_file_refused(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("image.png");
        fs::write(&path, b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR").unwrap();
        assert_eq!(classify(&path), Err(OpenError::Binary));
        assert_eq!(OpenError::Binary.to_string(), "Binary file");
        let (workspace, cx) = open(cx, Settings::default());

        open_file(&workspace, &path, None, cx);

        let notices = cx.update(|window, cx| window.notifications(cx).len());
        assert_eq!(notices, 1, "a notice says why");
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[..], [Pane::Empty(None)]), "no pane opens")
        });
    }

    #[gpui_kit::test]
    fn large_file_read_only(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("big.log");
        // 21 MB in 1 KB lines.
        let line = format!("{}\n", "x".repeat(1023));
        fs::write(&path, line.repeat(21 * 1024)).unwrap();
        assert_eq!(classify(&path), Ok(OpenMode::ReadOnlyLarge));
        // Over 200 MB nothing opens (a sparse file: no disk spent).
        let huge = dir.path().join("huge.log");
        let bytes = 201 * MB;
        fs::File::create(&huge).unwrap().set_len(bytes).unwrap();
        assert_eq!(classify(&huge), Err(OpenError::TooLarge { bytes }));
        assert_eq!(
            OpenError::TooLarge { bytes }.to_string(),
            "Too large to open in wai"
        );
        let (workspace, cx) = open(cx, Settings::default());

        open_file(&workspace, &path, None, cx);

        let tab = active_tab(&workspace, cx);
        assert!(tab.read_with(cx, |t, _| t.read_only));
        cx.update(|window, cx| window.render_frame(cx));
        assert!(cx.debug_bounds("read-only-banner").is_some());
        cx.simulate_input("typed");
        cx.simulate_keystrokes("secondary-s");
        assert!(!tab.read_with(cx, |t, _| t.dirty));
        assert_eq!(text(&tab, cx).len() as u64, 21 * MB, "no edit got in");
        assert_eq!(fs::metadata(&path).unwrap().len(), 21 * MB);
    }

    #[gpui_kit::test]
    fn save_writes_atomically_and_clears_dirty(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("main.rs");
        fs::write(&path, "fn main() {}\n").unwrap();
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
            fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).unwrap();
            fs::metadata(&path).unwrap().ino()
        };
        let (workspace, cx) = open(cx, Settings::default());
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);

        cx.simulate_input("// wai ");
        assert!(tab.read_with(cx, |t, _| t.dirty));
        let on_disk = fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk, "fn main() {}\n", "nothing is written before Save");
        cx.simulate_keystrokes("secondary-s");

        assert_eq!(fs::read_to_string(&path).unwrap(), "// wai fn main() {}\n");
        assert!(!tab.read_with(cx, |t, _| t.dirty));
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["main.rs"], "no temp file left behind");
        #[cfg(unix)]
        {
            use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};
            let meta = fs::metadata(&path).unwrap();
            assert_ne!(meta.ino(), inode, "a new file was renamed over the old one");
            assert_eq!(meta.permissions().mode() & 0o777, 0o755, "the mode is kept");
        }
    }

    /// Review Focus 1: a clean buffer follows the disk; an edited one keeps the edits and asks.
    #[gpui_kit::test]
    fn external_change_reloads_clean_buffer_and_prompts_dirty(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the watcher reports from a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let path = root.join("notes.md");
        fs::write(&path, "one\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);
        let flags =
            |cx: &mut VisualTestContext| tab.read_with(cx, |t, _| (t.dirty, t.disk_changed));

        fs::write(&path, "two\n").unwrap();
        wait_until(cx, "the reload", |cx| text(&tab, cx) == "two\n");
        assert_eq!(flags(cx), (false, false));

        cx.simulate_input("mine ");
        fs::write(&path, "three\n").unwrap();
        wait_until(cx, "the banner", |cx| flags(cx).1);
        assert_eq!(text(&tab, cx), "mine two\n", "the edits stay");

        click("keep-mine", cx);
        assert_eq!(flags(cx), (true, false));
        let on_disk = fs::read_to_string(&path).unwrap();
        assert_eq!(on_disk, "three\n", "never written without Save");

        fs::write(&path, "four\n").unwrap();
        wait_until(cx, "the banner again", |cx| flags(cx).1);
        click("reload", cx);
        wait_until(cx, "the forced reload", |cx| text(&tab, cx) == "four\n");
        assert_eq!(flags(cx), (false, false));
    }

    #[gpui_kit::test]
    fn open_file_reuses_editor_pane(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let [a, b, c] = ["a.rs", "b.rs", "c.rs"].map(|name| root.join(name));
        fs::write(&a, "1\n2\n3\n").unwrap();
        fs::write(&b, "").unwrap();
        fs::write(&c, "").unwrap();
        let (workspace, cx) = open(cx, Settings::default());

        // The empty pane becomes the editor pane; the next file opens as a tab in it.
        open_file(&workspace, &a, None, cx);
        open_file(&workspace, &b, None, cx);
        workspace.read_with(cx, |w, _| assert!(matches!(w.panes[..], [Pane::Editor(_)])));
        let pane = editor_pane(&workspace, cx);
        assert_eq!(paths(&pane, cx), [a.clone(), b.clone()]);
        assert_eq!(pane.read_with(cx, |p, _| p.active), 1);

        // An open file gets its tab focused, at the line asked for, and no second tab.
        open_file(&workspace, &a, Some(3), cx);
        assert_eq!(paths(&pane, cx), [a.clone(), b.clone()]);
        assert_eq!(pane.read_with(cx, |p, _| p.active), 0);
        let tab = active_tab(&workspace, cx);
        let line = tab.read_with(cx, |t, cx| t.editor.read(cx).cursor_position().line);
        assert_eq!(line, 2);

        // No editor pane and no empty pane: a new pane opens to the right…
        let busy = || Pane::Preparing {
            agent: "fake".into(),
            ticket: 0,
        };
        workspace.update(cx, |w, _| {
            w.panes = vec![busy()];
            w.focused_pane = 0;
        });
        open_file(&workspace, &c, None, cx);
        workspace.read_with(cx, |w, _| {
            assert!(matches!(
                w.panes[..],
                [Pane::Preparing { .. }, Pane::Editor(_)]
            ));
            assert_eq!(w.focused_pane, 1);
        });

        // …and at four panes the last one becomes the editor pane.
        workspace.update(cx, |w, _| {
            w.panes = (0..MAX_PANES).map(|_| busy()).collect();
            w.focused_pane = 0;
        });
        open_file(&workspace, &c, None, cx);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), MAX_PANES);
            assert!(matches!(w.panes[MAX_PANES - 1], Pane::Editor(_)));
            assert_eq!(w.focused_pane, MAX_PANES - 1);
        });
    }

    /// A session never takes the editor pane's place: its unsaved edits would go with it.
    #[gpui_kit::test]
    fn new_session_keeps_the_editor_pane(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "a").unwrap();
        let (workspace, cx) = open(cx, fake_settings(""));
        open_file(&workspace, &path, None, cx);
        cx.simulate_input("edit ");

        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| {
                w.panes.get(1).is_some_and(|p| p.session().is_some())
            })
        });
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[0], Pane::Editor(_)));
            assert_eq!(w.focused_pane, 1);
        });
        assert_eq!(text(&active_tab(&workspace, cx), cx), "edit a");
    }

    /// R20: ⌘W in an editor pane closes its active tab, asking first when it holds edits; the
    /// last tab takes the pane with it.
    #[gpui_kit::test]
    fn close_tab_asks_before_dropping_edits(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let [a, b] = ["a.txt", "b.txt"].map(|name| root.join(name));
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        open_file(&workspace, &a, None, cx);
        open_file(&workspace, &b, None, cx);
        let pane = editor_pane(&workspace, cx);

        cx.simulate_input("edit ");
        cx.simulate_keystrokes("secondary-w");
        assert!(dialog(cx), "an edited tab asks");
        click("cancel", cx);
        assert!(!dialog(cx));
        assert_eq!(paths(&pane, cx), [a.clone(), b.clone()]);

        cx.simulate_keystrokes("secondary-w");
        click("dont-save", cx);
        assert_eq!(paths(&pane, cx), std::slice::from_ref(&a));
        assert_eq!(fs::read_to_string(&b).unwrap(), "b");

        cx.simulate_input("saved ");
        cx.simulate_keystrokes("secondary-w");
        click("save", cx);
        assert_eq!(fs::read_to_string(&a).unwrap(), "saved a");
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[..], [Pane::Empty(None)]))
        });
    }

    /// R31: agent and search paths (`/var/…` against `/private/var/…`) reach the same tab, the
    /// one the watcher's real paths name.
    #[gpui_kit::test]
    fn open_file_canonicalizes_paths(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("a.rs"), "").unwrap();
        let (workspace, cx) = open(cx, Settings::default());

        let roundabout = dir.path().join("sub").join("..").join("a.rs");
        open_file(&workspace, &roundabout, None, cx);
        open_file(&workspace, &root.join("a.rs"), None, cx);

        let pane = editor_pane(&workspace, cx);
        assert_eq!(paths(&pane, cx), [root.join("a.rs")]);
    }

    /// R28: quitting or closing the window with unsaved edits asks first.
    #[gpui_kit::test]
    fn quit_and_close_ask_about_unsaved_edits(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = canonical(dir.path()).join("a.txt");
        fs::write(&path, "a").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        open_file(&workspace, &path, None, cx);
        cx.simulate_input("edit ");

        cx.simulate_keystrokes("secondary-q");
        assert!(dialog(cx), "quit asks");
        click("cancel", cx);
        assert!(!dialog(cx));
        assert_eq!(text(&active_tab(&workspace, cx), cx), "edit a");

        assert!(!cx.simulate_close(), "the window stays while it asks");
        assert!(dialog(cx), "closing asks");
        click("cancel", cx);
        assert_eq!(text(&active_tab(&workspace, cx), cx), "edit a");

        assert!(!cx.simulate_close());
        click("save-all", cx);
        assert_eq!(fs::read_to_string(&path).unwrap(), "edit a");
    }

    /// R29: switching project asks about unsaved edits, then closes the old project's files.
    #[gpui_kit::test]
    fn switching_project_asks_then_closes_editors(cx: &mut TestAppContext) {
        let dirs = [(); 2].map(|_| tempfile::tempdir().unwrap());
        let [one, two] = [0, 1].map(|ix| canonical(dirs[ix].path()));
        let [a, b] = [one.join("a.txt"), two.join("b.txt")];
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        let project =
            |cx: &mut VisualTestContext| workspace.read_with(cx, |w, _| w.project.clone());
        let editors = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| {
                w.panes.iter().any(|p| matches!(p, Pane::Editor(_)))
            })
        };
        leave(&workspace, Leave::Open(one.clone()), cx);
        open_file(&workspace, &a, None, cx);
        cx.simulate_input("edit ");

        leave(&workspace, Leave::Open(two.clone()), cx);
        assert!(dialog(cx));
        click("cancel", cx);
        assert_eq!(project(cx), one);
        assert_eq!(text(&active_tab(&workspace, cx), cx), "edit a");

        leave(&workspace, Leave::Open(two.clone()), cx);
        click("discard", cx);
        assert_eq!(project(cx), two);
        assert!(!editors(cx), "the old project's files close");
        assert_eq!(fs::read_to_string(&a).unwrap(), "a");

        open_file(&workspace, &b, None, cx);
        cx.simulate_input("edit ");
        leave(&workspace, Leave::Open(one.clone()), cx);
        click("save-all", cx);
        assert_eq!(fs::read_to_string(&b).unwrap(), "edit b");
        assert_eq!(project(cx), one);
        assert!(!editors(cx));

        // Nothing unsaved: no question.
        open_file(&workspace, &a, None, cx);
        leave(&workspace, Leave::Open(two.clone()), cx);
        assert!(!dialog(cx));
        assert_eq!(project(cx), two);
        assert!(!editors(cx));
    }

    /// R30: the size on disk now, not when the file opened, decides: a file that grew past 20 MB
    /// does not reload by itself, and once reloaded it is read-only and plain.
    #[gpui_kit::test]
    fn growing_past_20_mb_asks_and_reloads_read_only(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the watcher reports from a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let path = root.join("app.rs");
        fs::write(&path, "fn main() {}\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);

        let line = format!("// {}\n", "x".repeat(1020));
        fs::write(&path, line.repeat(21 * 1024)).unwrap();
        wait_until(cx, "the banner", |cx| {
            tab.read_with(cx, |t, _| t.disk_changed)
        });
        assert_eq!(text(&tab, cx), "fn main() {}\n", "no silent reload");

        click("reload", cx);
        wait_until(cx, "the reload", |cx| tab.read_with(cx, |t, _| t.read_only));
        assert_eq!(text(&tab, cx).len() as u64, 21 * MB);
        let language = tab.read_with(cx, |t, cx| t.editor.read(cx).language_name());
        assert_eq!(language.as_ref(), "text");
    }

    /// R31: a clean tab whose file can no longer be read shows the banner, so a later Save does
    /// not overwrite a file the user never saw.
    #[gpui_kit::test]
    fn failed_reload_shows_the_banner(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the watcher reports from a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let path = root.join("data.txt");
        fs::write(&path, "text\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);

        fs::write(&path, b"\0\x01binary").unwrap();
        wait_until(cx, "the banner", |cx| {
            tab.read_with(cx, |t, _| t.disk_changed)
        });
        assert_eq!(text(&tab, cx), "text\n");
    }
}
