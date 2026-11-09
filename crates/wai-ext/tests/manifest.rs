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

    // wai-remoto: a skeleton; no process, nothing contributed.
    let remoto = read("wai-remoto");
    assert!(remoto.process.is_none());
    assert_eq!(remoto.permissions, [Permission::SessionsControl]);
    assert_eq!(remoto.contributes, Contributes::default());
}
