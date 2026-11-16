//! The `extensions/wai-waycarbon` extension (Go), end to end: built, installed from its folder
//! with consent, started with fake `jira`, `gh` and `claude` on its PATH.

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

/// Answers `plugin list --json` as installed and everything else with success: no test reaches the
/// real `claude`.
const FAKE_CLAUDE: &str = r#"#!/bin/sh
[ "$1 $2" = "plugin list" ] && echo '[{"id":"waycarbon@waycarbon"}]'
exit 0
"#;

/// Answers the GraphQL call with a viewer and no pull requests; `auth status` and the rest succeed.
const FAKE_GH: &str = r#"#!/bin/sh
[ "$1 $2" = "api graphql" ] && echo '{"data":{"viewer":{"login":"dev"}}}'
exit 0
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
    let mut files = vec![
        MANIFEST_FILE.to_string(),
        "go.mod".into(),
        "icon.svg".into(),
    ];
    for entry in std::fs::read_dir(&from).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".go") && !name.ends_with("_test.go") {
            files.push(name);
        }
    }
    for entry in std::fs::read_dir(from.join("helpapi")).unwrap().flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".go") && !name.ends_with("_test.go") {
            files.push(format!("helpapi/{name}"));
        }
    }
    for file in files {
        let to = src.join(&file);
        std::fs::create_dir_all(to.parent().unwrap()).unwrap();
        std::fs::copy(from.join(&file), to).unwrap();
    }
    let built = Command::new("go")
        .args(["build", "-o", "bin/wai-waycarbon-real", "."])
        .current_dir(&src)
        .status();
    if !built.is_ok_and(|s| s.success()) {
        eprintln!("SKIPPED: `go build` is unavailable, so wai-waycarbon cannot be built");
        return None;
    }
    // The extension is started from wai's environment: a wrapper puts the fakes first.
    let fake = tmp.join("fake-bin");
    executable(&fake.join("jira"), FAKE_JIRA);
    executable(&fake.join("gh"), FAKE_GH);
    executable(&fake.join("claude"), FAKE_CLAUDE);
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
    let panel = panel.expect("a tab for the WayCarbon panel");
    // Card titles with their render index: the element id of a card is its pre-order position.
    let cards = |cx: &mut gpui_kit::VisualTestContext| {
        let tree = panel.read_with(cx, |p, _| p.tree.clone());
        let mut cards = Vec::new();
        if let Some(Ok(tree)) = tree {
            let mut n = 0usize;
            tree.walk(&mut |node| {
                n += 1;
                if let Node::Card {
                    title: Some(title), ..
                } = node
                {
                    cards.push((n, title.to_string()));
                }
            });
        }
        cards
    };
    wait_until(cx, "the cards", |cx| !cards(cx).is_empty());
    let titles: Vec<_> = cards(cx).into_iter().map(|(_, t)| t).collect();
    assert_eq!(titles, ["Fix the login timeout", "Add the export button"]);

    click(("tab-panel", 0usize), cx);
    // The cards sit in the Jira tab; Work opens first.
    click("panel-tab:tab:jira", cx);
    let (k, _) = cards(cx).remove(1);
    click(("panel-card", k), cx);
    // Beside the focused (empty) pane: an extension's session never takes it (R61).
    wait_until(cx, "a pre-filled session", |cx| {
        workspace.read_with(cx, |w, _| {
            w.panes.get(1).is_some_and(|p| p.session().is_some())
        })
    });
    let session = workspace.read_with(cx, |w, _| w.panes[1].session().cloned().unwrap());
    let draft = session.read_with(cx, |s, cx| s.draft(cx));
    assert_eq!(draft, "/waycarbon:solve-card EC-102");
    assert!(
        session.read_with(cx, |s, _| s.model.items.is_empty()),
        "not sent"
    );
}
