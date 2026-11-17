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
  "issue list -a dev@example.com -s~Done --plain --no-headers --columns key,summary,status"|\
  "issue list -a dev@example.com -s~Done -q sprint in openSprints() --plain --no-headers --columns key,summary,status")
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

/// The Jira tab is a board: clicking a card selects it, and its Solve card pre-fills a session,
/// which brings the main area back in place of the wide panel.
#[gpui_kit::test]
fn jira_board_selects_and_solve_prefills(cx: &mut TestAppContext) {
    use crate::sidebar::SidebarTab;

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
    let panel = host.read_with(cx, |h, _| {
        h.panels().first().map(|p| (p.view.clone(), p.wide))
    });
    let (panel, wide) = panel.expect("a tab for the WayCarbon panel");
    assert!(wide, "the WayCarbon panel takes the main area's place");
    // The board's cards: render index (a card's element id is its pre-order position), key, title
    // and whether it is selected; and whether a button is drawn.
    let board = |cx: &mut gpui_kit::VisualTestContext| {
        let tree = panel.read_with(cx, |p, _| p.tree.clone());
        let mut cards = Vec::new();
        if let Some(Ok(tree)) = tree {
            let mut n = 0usize;
            tree.walk(&mut |node| {
                n += 1;
                if let Node::Card {
                    id: Some(id),
                    title: Some(title),
                    selected,
                    ..
                } = node
                    && let Some(key) = id.strip_prefix("sel:jira:card:")
                {
                    cards.push((n, key.to_string(), title.to_string(), *selected));
                }
            });
        }
        cards
    };
    let button = |id: &str, cx: &mut gpui_kit::VisualTestContext| {
        let tree = panel.read_with(cx, |p, _| p.tree.clone());
        let mut found = false;
        if let Some(Ok(tree)) = tree {
            tree.walk(&mut |node| {
                found |= matches!(node, Node::Button { id: b, .. } if b == id);
            });
        }
        found
    };
    wait_until(cx, "the board", |cx| !board(cx).is_empty());
    // To Do comes first; In Progress is not a column of the board, so it goes last. The first is selected.
    let cards: Vec<_> = (board(cx).into_iter())
        .map(|(_, k, t, s)| (k, t, s))
        .collect();
    assert_eq!(
        cards,
        [
            ("EC-102".into(), "Add the export button".into(), true),
            ("EC-101".into(), "Fix the login timeout".into(), false)
        ]
    );

    click(("tab-panel", 0usize), cx);
    assert!(workspace.read_with(cx, |w, cx| w.wide_shown(cx)));
    // Work opens first.
    click("panel-tab:tab:jira", cx);
    let (k, ..) = board(cx).remove(1);
    click(("panel-card", k), cx);
    wait_until(cx, "EC-101 selected", |cx| {
        button("act:jira:solve:EC-101", cx) && board(cx)[1].3
    });
    assert!(
        workspace.read_with(cx, |w, _| w.panes.iter().all(|p| p.session().is_none())),
        "selecting a card opens nothing"
    );
    click("panel-button:act:jira:solve:EC-101", cx);
    // Beside the focused (empty) pane: an extension's session never takes it (R61).
    wait_until(cx, "a pre-filled session", |cx| {
        workspace.read_with(cx, |w, _| {
            w.panes.get(1).is_some_and(|p| p.session().is_some())
        })
    });
    let session = workspace.read_with(cx, |w, _| w.panes[1].session().cloned().unwrap());
    let draft = session.read_with(cx, |s, cx| s.draft(cx));
    assert_eq!(draft, "/waycarbon:solve-card EC-101");
    assert!(
        session.read_with(cx, |s, _| s.model.items.is_empty()),
        "not sent"
    );
    assert_eq!(
        workspace.read_with(cx, |w, _| w.sidebar_tab),
        SidebarTab::Sessions
    );
}
