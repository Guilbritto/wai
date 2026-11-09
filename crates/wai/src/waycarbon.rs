//! The `extensions/wai-waycarbon` extension (Go), end to end: built, installed from its folder
//! with consent, started with a fake `jira` on its PATH.

use std::os::unix::fs::PermissionsExt as _;
use std::path::Path;
use std::process::Command;

use gpui_kit::TestAppContext;
use wai_ext::panel::Node;
use wai_ext::{ExtStore, MANIFEST_FILE, Source};

use crate::extensions_view::tests::click;
use crate::workspace::tests::{fake_settings, open, wait_until};

const FAKE_JIRA: &str = r#"#!/bin/sh
case "$*" in
  "me") echo "dev@example.com" ;;
  "issue list -a dev@example.com -s~Done --plain --no-headers --columns key,summary,status")
    printf 'EC-101\tFix the login timeout\tIn Progress\n'
    printf 'EC-102\tAdd the export button\tTo Do\n' ;;
  *) echo "unexpected: $*" >&2; exit 2 ;;
esac
"#;

fn executable(path: &Path, text: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A copy of the extension with its Go binary built, or `None` without a `go` toolchain.
fn build(tmp: &Path) -> Option<std::path::PathBuf> {
    let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions/wai-waycarbon");
    let src = tmp.join("src");
    std::fs::create_dir_all(&src).unwrap();
    for file in [MANIFEST_FILE, "go.mod", "main.go", "jira.go"] {
        std::fs::copy(from.join(file), src.join(file)).unwrap();
    }
    let built = Command::new("go")
        .args(["build", "-o", "bin/wai-waycarbon-real", "."])
        .current_dir(&src)
        .status();
    if !built.is_ok_and(|s| s.success()) {
        eprintln!("SKIPPED: `go build` is unavailable, so wai-waycarbon cannot be built");
        return None;
    }
    // The extension is started from wai's environment: a wrapper puts the fake jira first.
    let fake = tmp.join("fake-bin");
    executable(&fake.join("jira"), FAKE_JIRA);
    let wrapper = format!(
        "#!/bin/sh\nPATH='{}':\"$PATH\" exec \"$(dirname \"$0\")/wai-waycarbon-real\"\n",
        fake.display()
    );
    executable(&src.join("bin/wai-waycarbon"), &wrapper);
    Some(src)
}

#[gpui_kit::test]
fn my_cards_panel_renders_and_click_prefills(cx: &mut TestAppContext) {
    let tmp = tempfile::tempdir().unwrap();
    let Some(src) = build(tmp.path()) else { return };
    let config = tmp.path().join("config");
    let mut store = ExtStore::open(&config);
    let staged = store.stage(&Source::Folder(src)).unwrap();
    let granted = staged.manifest().permissions.clone();
    store.commit(staged, granted).unwrap();

    cx.executor().allow_parking(); // the extension and the agent are real processes
    let (workspace, cx) = open(cx, fake_settings(""));
    workspace.update(cx, |w, cx| {
        w.user_dir = config.clone();
        w.start_extensions(cx);
    });
    let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
    let panel = host.read_with(cx, |h, _| h.panels().first().map(|p| p.view.clone()));
    let panel = panel.expect("a tab for the My cards panel");
    let titles = |cx: &mut gpui_kit::VisualTestContext| {
        let tree = panel.read_with(cx, |p, _| p.tree.clone());
        let mut titles = Vec::new();
        if let Some(Ok(tree)) = tree {
            tree.walk(&mut |n| {
                if let Node::List { items, .. } = n {
                    titles.extend(items.iter().flatten().map(|i| i.title.to_string()));
                }
            });
        }
        titles
    };
    wait_until(cx, "the cards", |cx| !titles(cx).is_empty());
    assert_eq!(
        titles(cx),
        ["Fix the login timeout", "Add the export button"]
    );

    click(("tab-panel", 0usize), cx);
    click(("panel-item", 1usize), cx);
    wait_until(cx, "a pre-filled session", |cx| {
        workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
    });
    let session = workspace.read_with(cx, |w, _| w.panes[0].session().cloned().unwrap());
    let draft = session.read_with(cx, |s, cx| s.draft(cx));
    assert_eq!(draft, "Work on EC-102: Add the export button");
    assert!(
        session.read_with(cx, |s, _| s.model.items.is_empty()),
        "not sent"
    );
}
