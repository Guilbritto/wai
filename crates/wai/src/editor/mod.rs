//! The editor: an editor pane of tabs, each an open file.

mod file;
mod pane;
pub mod recovery;

pub use file::{load, save};
pub use pane::{EditorPane, Emptied, FileEditor, Saved};

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ClipboardItem, ElementId, Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use super::file::{OpenError, OpenMode, classify};
    use super::{EditorPane, FileEditor};
    use crate::sidebar::canonical;
    use crate::workspace::tests::{fake_settings, open, open_at, wait_until};
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

    /// Text in another encoding (Latin-1 here) says what it is, not the raw io error.
    #[test]
    fn non_utf8_file_says_so() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("latin1.txt");
        fs::write(&path, b"caf\xe9\n").unwrap();

        let error = super::load(&path).err().expect("refused");

        assert_eq!(error.to_string(), "Not a UTF-8 text file");
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

    /// A CRLF file stays CRLF: the buffer holds `\n` (no stray `\r` to edit around), and Save
    /// writes `\r\n` back, for the lines typed too. An LF file stays LF.
    #[gpui_kit::test]
    fn crlf_roundtrip_preserved(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let [crlf, lf] = ["win.txt", "unix.txt"].map(|name| dir.path().join(name));
        fs::write(&crlf, "one\r\ntwo\r\n").unwrap();
        fs::write(&lf, "one\ntwo\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());

        for (path, ending) in [(&crlf, "\r\n"), (&lf, "\n")] {
            open_file(&workspace, path, None, cx);
            let tab = active_tab(&workspace, cx);
            assert_eq!(text(&tab, cx), "one\ntwo\n");
            cx.simulate_input("zero");
            cx.simulate_keystrokes("enter");
            cx.simulate_keystrokes("secondary-s");
            let saved = ["zero", "one", "two", ""].join(ending);
            assert_eq!(fs::read_to_string(path).unwrap(), saved);
        }
    }

    /// A Windows clipboard holds CRLF text: pasted, it takes the file's line ends, never `\r\r\n`
    /// in a CRLF file nor a stray `\r\n` in an LF one.
    #[gpui_kit::test]
    fn pasted_crlf_takes_the_files_line_ends(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let [crlf, lf] = ["win.txt", "unix.txt"].map(|name| dir.path().join(name));
        fs::write(&crlf, "one\r\n").unwrap();
        fs::write(&lf, "one\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());

        for (path, ending) in [(&crlf, "\r\n"), (&lf, "\n")] {
            open_file(&workspace, path, None, cx);
            let clipboard = ClipboardItem::new_string("a\r\nb\r\n".into());
            cx.update(|_, cx| cx.write_to_clipboard(clipboard));
            cx.simulate_keystrokes("secondary-v");
            cx.simulate_keystrokes("secondary-s");
            let saved = ["a", "b", "one", ""].join(ending);
            assert_eq!(fs::read_to_string(path).unwrap(), saved);
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
            by_extension: None,
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
        cx.executor().allow_parking(); // each project's watcher reports from a real thread
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

        cx.simulate_keystrokes("secondary-s");
        assert_eq!(fs::read(&path).unwrap(), b"\0\x01binary", "Save refused");
    }

    fn notices(cx: &mut VisualTestContext) -> usize {
        cx.update(|window, cx| window.notifications(cx).len())
    }

    /// R39: ⌘S never writes over a change on disk the buffer does not show; the banner asks.
    #[gpui_kit::test]
    fn save_refuses_over_a_change_on_disk(cx: &mut TestAppContext) {
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
        cx.simulate_input("mine ");
        fs::write(&path, "theirs\n").unwrap();
        wait_until(cx, "the banner", |cx| {
            tab.read_with(cx, |t, _| t.disk_changed)
        });

        cx.simulate_keystrokes("secondary-s");

        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
        assert!(tab.read_with(cx, |t, _| t.dirty && t.disk_changed));
        assert_eq!(notices(cx), 1, "a notice says why");
    }

    /// R39: a file no watcher sees (outside the project) still is not overwritten: its stamp on
    /// disk is not the one read. Keep mine takes the disk's stamp, so the next ⌘S writes.
    #[gpui_kit::test]
    fn save_finds_a_change_no_watcher_saw(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("outside.txt");
        fs::write(&path, "one\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);
        cx.simulate_input("mine ");
        fs::write(&path, "theirs\n").unwrap();

        cx.simulate_keystrokes("secondary-s");
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs\n");
        assert!(tab.read_with(cx, |t, _| t.disk_changed), "the banner shows");

        click("keep-mine", cx);
        cx.simulate_keystrokes("secondary-s");
        assert_eq!(fs::read_to_string(&path).unwrap(), "mine one\n");
        assert!(!tab.read_with(cx, |t, _| t.dirty || t.disk_changed));
    }

    /// R42: between a disk event and the reload's text landing, a Save must not pass.
    #[gpui_kit::test]
    fn save_during_reload_window_refuses(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.md");
        fs::write(&path, "one\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);

        fs::write(&path, "theirs, longer\n").unwrap();
        let saved = cx.update(|window, cx| {
            tab.update(cx, |t, cx| {
                t.disk_event(window, cx); // the reload is still reading
                t.dirty = true; // the user types
                t.save(window, cx)
            })
        });

        assert!(!saved);
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs, longer\n");
    }

    /// R42: a clean tab has no edits to keep, so the banner has no Dismiss to adopt the disk.
    #[gpui_kit::test]
    fn clean_tab_banner_has_no_dismiss(cx: &mut TestAppContext) {
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
        cx.update(|window, cx| window.render_frame(cx));
        assert!(cx.debug_bounds("reload").is_some());
        assert!(cx.debug_bounds("keep-mine").is_none());

        // Deleted: Close, which drops the tab.
        let pane = editor_pane(&workspace, cx);
        fs::remove_file(&path).unwrap();
        wait_until(cx, "the deleted banner", |cx| {
            tab.read_with(cx, |t, _| t.disk_changed) && {
                cx.update(|window, cx| window.render_frame(cx));
                cx.debug_bounds("reload").is_none()
            }
        });
        click("close-deleted", cx);
        assert!(paths(&pane, cx).is_empty());
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn save_refuses_when_file_became_a_symlink(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("notes.md");
        let other = dir.path().join("other.md");
        fs::write(&path, "one\n").unwrap();
        fs::write(&other, "secret\n").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);
        cx.simulate_input("mine ");
        fs::write(&path, "theirs\n").unwrap();
        cx.simulate_keystrokes("secondary-s");
        click("keep-mine", cx);

        // A link to a file with the stamp Keep mine took: only the real path tells them apart.
        let modified = fs::metadata(&path).unwrap().modified().unwrap();
        fs::write(&other, "secret\n").unwrap();
        fs::File::options()
            .write(true)
            .open(&other)
            .unwrap()
            .set_modified(modified)
            .unwrap();
        fs::remove_file(&path).unwrap();
        std::os::unix::fs::symlink(&other, &path).unwrap();
        cx.simulate_keystrokes("secondary-s");

        assert_eq!(fs::read_to_string(&other).unwrap(), "secret\n");
        assert!(tab.read_with(cx, |t, _| t.dirty));
    }

    /// R41: ⌘O on the folder already open changes nothing: no question, the files stay.
    #[gpui_kit::test]
    fn opening_the_open_project_changes_nothing(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the project's watcher reports from a real thread
        let dir = tempfile::tempdir().unwrap();
        let root = canonical(dir.path());
        let path = root.join("a.txt");
        fs::write(&path, "a").unwrap();
        let (workspace, cx) = open(cx, Settings::default());
        leave(&workspace, Leave::Open(root.clone()), cx);
        open_file(&workspace, &path, None, cx);
        cx.simulate_input("edit ");

        // Spelled another way (`/var/…` against `/private/var/…`), it still is the same folder.
        leave(&workspace, Leave::Open(dir.path().to_path_buf()), cx);

        assert!(!dialog(cx), "no question");
        assert_eq!(
            text(&active_tab(&workspace, cx), cx),
            "edit a",
            "the tab stays"
        );
    }

    /// A run of wai that keeps its state, and its unsaved edits, beside `state` (a temp dir):
    /// what a launch restores, without the agent.
    fn run_at<'a>(
        state: &Path,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let (workspace, cx) = open_at(cx, Settings::default(), Some(state.to_path_buf()));
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.restore(window, cx)));
        cx.run_until_parked();
        (workspace, cx)
    }

    /// What every quit runs, ⌘Q, Dock and logout alike: the quit hooks.
    fn quit(cx: &mut VisualTestContext) {
        cx.cx.update(|app| app.shutdown());
    }

    fn recovery_files(state: &Path) -> Vec<PathBuf> {
        let dir = fs::read_dir(state.with_file_name("recovery"));
        let files = dir.into_iter().flatten().map(|e| e.unwrap().path());
        files.collect()
    }

    fn recovered_banner(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.render_frame(cx));
        cx.debug_bounds("recovered-banner").is_some()
    }

    fn wait(ms: u64, cx: &mut VisualTestContext) {
        cx.executor().advance_clock(Duration::from_millis(ms));
        cx.run_until_parked();
    }

    /// Hot-exit: a quit that never asks (Dock, logout) keeps the unsaved edits, even before their
    /// snapshot; the next launch reopens them, edited, with the banner. The file is never written
    /// until Save, which keeps its line ends.
    #[gpui_kit::test]
    fn dirty_buffers_are_recovered_after_quit(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        let path = canonical(dir.path()).join("win.txt");
        fs::write(&path, "one\r\n").unwrap();
        let (workspace, cx) = run_at(&state, cx);
        open_file(&workspace, &path, None, cx);
        cx.simulate_input("mine ");

        quit(cx);

        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "one\r\n",
            "never written"
        );
        assert_eq!(recovery_files(&state).len(), 1);
        let (workspace, cx) = run_at(&state, &mut cx.cx);
        let tab = active_tab(&workspace, cx);
        assert_eq!(tab.read_with(cx, |t, _| t.path.clone()), path);
        assert_eq!(text(&tab, cx), "mine one\n");
        assert!(tab.read_with(cx, |t, _| t.dirty && !t.disk_changed));
        assert!(recovered_banner(cx));
        assert_eq!(super::pane::RECOVERED, "Recovered unsaved changes");

        cx.simulate_keystrokes("secondary-s");
        assert_eq!(fs::read_to_string(&path).unwrap(), "mine one\r\n");
        assert!(!recovered_banner(cx));
        assert!(recovery_files(&state).is_empty());
    }

    /// The recovery file goes once the edits are saved or dropped (Don't save, Discard); a quit
    /// after Discard keeps nothing.
    #[gpui_kit::test]
    fn recovery_is_cleared_on_save_and_discard(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        let root = canonical(dir.path());
        let [a, b] = ["a.txt", "b.txt"].map(|name| root.join(name));
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let (workspace, cx) = run_at(&state, cx);

        open_file(&workspace, &a, None, cx);
        cx.simulate_input("saved ");
        wait(1_000, cx);
        assert_eq!(recovery_files(&state).len(), 1);
        cx.simulate_keystrokes("secondary-s");
        assert!(recovery_files(&state).is_empty(), "Save");

        open_file(&workspace, &b, None, cx);
        cx.simulate_input("dropped ");
        wait(1_000, cx);
        assert_eq!(recovery_files(&state).len(), 1);
        cx.simulate_keystrokes("secondary-w");
        click("dont-save", cx);
        assert!(recovery_files(&state).is_empty(), "Don't save");
        assert_eq!(fs::read_to_string(&b).unwrap(), "b");

        cx.simulate_input("discarded ");
        wait(1_000, cx);
        assert_eq!(recovery_files(&state).len(), 1);
        cx.simulate_keystrokes("secondary-q");
        click("discard", cx);
        assert!(recovery_files(&state).is_empty(), "Discard");
        quit(cx);
        assert!(recovery_files(&state).is_empty(), "the quit keeps nothing");
        assert_eq!(fs::read_to_string(&a).unwrap(), "saved a");
    }

    /// One write per burst of typing, 1 s after it pauses; nothing runs while nothing changes.
    #[gpui_kit::test]
    fn recovery_waits_for_typing_to_pause(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        let path = canonical(dir.path()).join("a.txt");
        fs::write(&path, "a").unwrap();
        let (workspace, cx) = run_at(&state, cx);
        open_file(&workspace, &path, None, cx);

        cx.simulate_input("1");
        wait(600, cx);
        cx.simulate_input("2");
        wait(600, cx);
        assert!(recovery_files(&state).is_empty(), "still typing");
        wait(400, cx);
        let files = recovery_files(&state);
        assert_eq!(files.len(), 1);
        assert!(fs::read_to_string(&files[0]).unwrap().contains("\"12a\""));

        fs::remove_file(&files[0]).unwrap();
        wait(5_000, cx);
        assert!(
            recovery_files(&state).is_empty(),
            "no write without an edit"
        );
    }

    /// The file changed after the edits were kept: neither side is dropped. The tab shows the
    /// edits and asks (R39: Save waits); Reload takes the disk's text and the entry goes.
    #[gpui_kit::test]
    fn recovery_keeps_a_change_on_disk(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        let path = canonical(dir.path()).join("notes.md");
        fs::write(&path, "one\n").unwrap();
        let (workspace, cx) = run_at(&state, cx);
        open_file(&workspace, &path, None, cx);
        cx.simulate_input("mine ");
        quit(cx);
        fs::write(&path, "theirs, longer\n").unwrap();

        let (workspace, cx) = run_at(&state, &mut cx.cx);
        let tab = active_tab(&workspace, cx);
        assert_eq!(text(&tab, cx), "mine one\n");
        assert!(tab.read_with(cx, |t, _| t.dirty && t.disk_changed));
        assert!(recovered_banner(cx));
        cx.simulate_keystrokes("secondary-s");
        assert_eq!(fs::read_to_string(&path).unwrap(), "theirs, longer\n");

        click("reload", cx);
        assert_eq!(text(&tab, cx), "theirs, longer\n");
        assert!(recovery_files(&state).is_empty());
    }

    /// The file is gone: its tab comes back, edited, under its path. Save recreates it once Keep
    /// mine answers the Save flow's own question.
    #[gpui_kit::test]
    fn recovery_reopens_a_deleted_file(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        let path = canonical(dir.path()).join("gone.txt");
        fs::write(&path, "one\n").unwrap();
        let (workspace, cx) = run_at(&state, cx);
        open_file(&workspace, &path, None, cx);
        cx.simulate_input("mine ");
        quit(cx);
        fs::remove_file(&path).unwrap();

        let (workspace, cx) = run_at(&state, &mut cx.cx);
        let tab = active_tab(&workspace, cx);
        assert_eq!(tab.read_with(cx, |t, _| t.path.clone()), path);
        assert_eq!(text(&tab, cx), "mine one\n");
        assert!(tab.read_with(cx, |t, _| t.dirty));
        assert!(recovered_banner(cx));
        cx.simulate_keystrokes("secondary-s");
        assert!(!path.exists(), "Save asks first");

        click("keep-mine", cx);
        cx.simulate_keystrokes("secondary-s");
        assert_eq!(fs::read_to_string(&path).unwrap(), "mine one\n");
        assert!(recovery_files(&state).is_empty());
    }

    fn recovered_texts(state: &Path) -> Vec<String> {
        let entry = |file: PathBuf| -> serde_json::Value {
            serde_json::from_str(&fs::read_to_string(file).unwrap()).unwrap()
        };
        let texts = recovery_files(state).into_iter().map(entry);
        let mut texts: Vec<_> = texts.map(|e| e["text"].as_str().unwrap().into()).collect();
        texts.sort();
        texts
    }

    /// A reopened tab keeps its recovery file, and no tab opened later is handed the same name,
    /// not even once the reopened tab's file is gone (saved): each tab's edits stay its own.
    #[gpui_kit::test]
    fn recovered_tabs_keep_their_own_backup(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        let root = canonical(dir.path());
        let [a, b] = ["a.txt", "b.txt"].map(|name| root.join(name));
        fs::write(&a, "a").unwrap();
        fs::write(&b, "b").unwrap();
        let (workspace, cx) = run_at(&state, cx);
        open_file(&workspace, &a, None, cx);
        cx.simulate_input("first ");
        quit(cx);

        let (workspace, cx) = run_at(&state, &mut cx.cx);
        cx.simulate_keystrokes("secondary-s");
        assert!(recovery_files(&state).is_empty());
        open_file(&workspace, &b, None, cx);
        cx.simulate_input("theirs ");
        open_file(&workspace, &a, None, cx);
        cx.simulate_input("again ");
        quit(cx);

        assert_eq!(recovered_texts(&state), ["again first a", "theirs b"]);
    }

    /// R61: an entry over 50 MB is not written, and a notice says so once.
    #[gpui_kit::test]
    fn oversized_backup_is_skipped_with_a_notice(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        let path = canonical(dir.path()).join("big.txt");
        fs::write(&path, "a").unwrap();
        let (workspace, cx) = run_at(&state, cx);
        open_file(&workspace, &path, None, cx);
        let tab = active_tab(&workspace, cx);
        // 51 MB in 1 KB lines, pasted into a small file.
        let big = format!("{}\n", "x".repeat(1023)).repeat(51 * 1024);
        cx.update(|window, cx| {
            let editor = tab.read(cx).editor.clone();
            editor.update(cx, |s, cx| s.set_value(big, window, cx));
        });

        cx.simulate_input("1");
        wait(1_000, cx);
        assert_eq!(notices(cx), 1);
        assert!(recovery_files(&state).is_empty());
        cx.simulate_input("2");
        wait(1_000, cx);
        assert_eq!(notices(cx), 1, "once");
    }

    /// R61: a backup that cannot be written (here `recovery` is a file) says so, once.
    #[gpui_kit::test]
    fn failed_backup_says_so_once(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let state = dir.path().join("config").join("state.json");
        fs::create_dir_all(state.parent().unwrap()).unwrap();
        fs::write(state.with_file_name("recovery"), "").unwrap();
        let path = canonical(dir.path()).join("a.txt");
        fs::write(&path, "a").unwrap();
        let (workspace, cx) = run_at(&state, cx);
        open_file(&workspace, &path, None, cx);

        cx.simulate_input("1");
        wait(1_000, cx);
        assert_eq!(notices(cx), 1);
        cx.simulate_input("2");
        wait(1_000, cx);
        assert_eq!(notices(cx), 1, "once");
        assert_eq!(fs::read_to_string(&path).unwrap(), "a");
    }
}
