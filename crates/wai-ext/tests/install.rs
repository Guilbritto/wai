use std::path::{Path, PathBuf};

use wai_ext::*;

const PERMS: &str = r#"["process", "sessions:control", "fs:data"]"#;

/// An extension folder `sample` with a manifest, a nested file and an executable.
fn sample(root: &Path) -> PathBuf {
    let src = root.join("src");
    std::fs::create_dir_all(src.join("bin")).unwrap();
    std::fs::write(
        src.join(MANIFEST_FILE),
        format!(
            "id = \"sample\"\nname = \"Sample\"\nversion = \"1.2.0\"\ndescription = \"A sample\"\n\
             permissions = {PERMS}\n[process]\ncommand = \"bin/run\"\n"
        ),
    )
    .unwrap();
    std::fs::write(src.join("bin/run"), "#!/bin/sh\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        std::fs::set_permissions(src.join("bin/run"), std::fs::Permissions::from_mode(0o755))
            .unwrap();
    }
    src
}

fn perms() -> Vec<Permission> {
    vec![
        Permission::Process,
        Permission::SessionsControl,
        Permission::Fs("data".into()),
    ]
}

/// Whatever is left in the staging area.
fn leftovers(config: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(staging_dir(config)) {
        Ok(entries) => entries.map(|e| e.unwrap().path()).collect(),
        Err(_) => Vec::new(),
    }
}

#[test]
fn stage_from_folder_parses_and_enables_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let store = ExtStore::open(&config);
    let staged = store.stage(&Source::Folder(sample(tmp.path()))).unwrap();

    assert_eq!(staged.manifest().id.as_str(), "sample");
    assert_eq!(staged.manifest().version, "1.2.0");
    assert_eq!(staged.manifest().permissions, perms());
    assert!(staged.dir().starts_with(staging_dir(&config)));
    assert!(staged.dir().join("bin/run").is_file());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        let mode = std::fs::metadata(staged.dir().join("bin/run")).unwrap();
        assert_eq!(mode.permissions().mode() & 0o111, 0o111, "stays executable");
    }
    // Nothing installed, recorded or enabled yet.
    assert!(store.list().is_empty());
    assert!(!config.join("extensions/sample").exists());
    assert!(!config.join("extensions.json").exists());
    assert!(ExtStore::open(&config).list().is_empty());

    drop(staged);
    assert!(
        leftovers(&config).is_empty(),
        "a dropped stage leaves nothing"
    );
}

#[test]
fn commit_records_granted_permissions() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let src = sample(tmp.path());
    let mut store = ExtStore::open(&config);

    // v1 consent is all or nothing.
    let staged = store.stage(&Source::Folder(src.clone())).unwrap();
    let partial = store.commit(staged, vec![Permission::Process]);
    assert!(matches!(partial, Err(InstallError::Consent)), "{partial:?}");
    assert!(store.list().is_empty());

    let staged = store.stage(&Source::Folder(src.clone())).unwrap();
    let granted: Vec<_> = perms().into_iter().rev().collect();
    let id = store.commit(staged, granted.clone()).unwrap();
    assert_eq!(id.as_str(), "sample");
    assert!(leftovers(&config).is_empty());

    let dir = config.join("extensions/sample");
    assert!(dir.join(MANIFEST_FILE).is_file());
    for store in [&store, &ExtStore::open(&config)] {
        let list = store.list();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].manifest.id, id);
        assert_eq!(list[0].dir, dir);
        assert!(list[0].enabled);
        assert_eq!(list[0].granted, granted);
    }
    let json = std::fs::read_to_string(config.join("extensions.json")).unwrap();
    for text in [
        "\"sample\"",
        "\"1.2.0\"",
        "\"sessions:control\"",
        "\"fs:data\"",
    ] {
        assert!(json.contains(text), "{text} not in {json}");
    }

    // v1 has no upgrade: uninstall first.
    let again = store.stage(&Source::Folder(src)).unwrap();
    let again = store.commit(again, perms());
    assert!(matches!(again, Err(InstallError::AlreadyInstalled(_))));
    assert!(leftovers(&config).is_empty());
}

#[test]
fn enable_disable_and_uninstall_persist() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let mut store = ExtStore::open(&config);
    let staged = store.stage(&Source::Folder(sample(tmp.path()))).unwrap();
    let id = store.commit(staged, perms()).unwrap();

    store.set_enabled(&id, false).unwrap();
    assert!(!ExtStore::open(&config).list()[0].enabled);
    store.set_enabled(&id, true).unwrap();
    assert!(ExtStore::open(&config).list()[0].enabled);

    // The folder leaves `extensions/` at once; the caller deletes it off the main thread.
    let trash = store.uninstall(&id).unwrap();
    assert!(!config.join("extensions/sample").exists());
    assert!(trash.starts_with(staging_dir(&config)));
    assert!(trash.join(MANIFEST_FILE).is_file());
    assert!(store.list().is_empty());
    assert!(ExtStore::open(&config).list().is_empty());
    let unknown = ExtId::new("nope").unwrap();
    assert!(store.set_enabled(&unknown, true).is_err());
}

#[test]
fn data_survives_uninstall_and_reinstall() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let mut store = ExtStore::open(&config);
    let staged = store.stage(&Source::Folder(sample(tmp.path()))).unwrap();
    let id = store.commit(staged, perms()).unwrap();
    let data = store.list()[0].data.clone();
    assert!(data.ends_with("extension-data/sample"));
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("state.txt"), "x").unwrap();

    store.uninstall(&id).unwrap();
    let staged = store.stage(&Source::Folder(sample(tmp.path()))).unwrap();
    store.commit(staged, perms()).unwrap();
    assert_eq!(
        std::fs::read_to_string(data.join("state.txt")).unwrap(),
        "x"
    );
    assert_eq!(store.list()[0].data, data);
}

/// R40: the temp file is recreated with create_new after removing whatever held its name (here
/// a link to a file outside), then renamed over `extensions.json`.
#[test]
fn extensions_json_is_written_atomically() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    std::fs::create_dir_all(&config).unwrap();
    std::fs::write(config.join("extensions.json"), "{ corrupt").unwrap();
    let mut store = ExtStore::open(&config);
    assert!(store.list().is_empty(), "a corrupt file reads as empty");

    let outside = tmp.path().join("outside.txt");
    std::fs::write(&outside, "keep").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&outside, config.join("extensions.json.tmp")).unwrap();
    #[cfg(not(unix))]
    std::fs::write(config.join("extensions.json.tmp"), "stale").unwrap();

    let staged = store.stage(&Source::Folder(sample(tmp.path()))).unwrap();
    store.commit(staged, perms()).unwrap();

    assert_eq!(std::fs::read_to_string(&outside).unwrap(), "keep");
    assert!(!config.join("extensions.json.tmp").exists());
    assert_eq!(ExtStore::open(&config).list().len(), 1);
}

/// Review Focus 2: no symlink is copied (one pointing outside is the escape; v1 refuses them
/// all), and neither is anything that is not a regular file or folder.
#[cfg(unix)]
#[test]
fn install_refuses_escaping_paths() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let store = ExtStore::open(&config);
    std::fs::write(tmp.path().join("secret"), "s").unwrap();

    let src = sample(tmp.path());
    std::os::unix::fs::symlink(tmp.path().join("secret"), src.join("bin/leak")).unwrap();
    let error = store.stage(&Source::Folder(src.clone())).err().unwrap();
    assert!(
        matches!(&error, InstallError::Refused { path, .. } if path.ends_with("bin/leak")),
        "{error:?}"
    );
    assert!(error.to_string().contains("symbolic link"), "{error}");
    assert!(
        leftovers(&config).is_empty(),
        "a refused stage leaves nothing"
    );

    // A link that stays inside is refused too.
    std::fs::remove_file(src.join("bin/leak")).unwrap();
    std::os::unix::fs::symlink("run", src.join("bin/alias")).unwrap();
    assert!(matches!(
        store.stage(&Source::Folder(src.clone())),
        Err(InstallError::Refused { .. })
    ));
    std::fs::remove_file(src.join("bin/alias")).unwrap();

    // A FIFO would block the copy forever.
    let fifo = src.join("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success());
    let error = store.stage(&Source::Folder(src)).err().unwrap();
    assert!(
        matches!(&error, InstallError::Refused { path, .. } if *path == fifo),
        "{error:?}"
    );
    assert!(leftovers(&config).is_empty());
    assert!(!config.join("extensions").join("sample").exists());
}

#[test]
fn install_refuses_more_than_100_mb() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let src = sample(tmp.path());
    // Sparse: no disk is used, and the size is refused before anything is copied.
    let big = std::fs::File::create(src.join("big.bin")).unwrap();
    big.set_len(100 * 1024 * 1024 + 1).unwrap();
    let staged = ExtStore::open(&config).stage(&Source::Folder(src));
    assert!(matches!(staged, Err(InstallError::TooLarge)), "{staged:?}");
    assert!(leftovers(&config).is_empty());
}

#[test]
fn stage_needs_a_valid_manifest() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let store = ExtStore::open(&config);
    let src = sample(tmp.path());
    std::fs::write(
        src.join(MANIFEST_FILE),
        "id = \"../x\"\nname = \"n\"\nversion = \"1\"\n",
    )
    .unwrap();
    assert!(matches!(
        store.stage(&Source::Folder(src.clone())),
        Err(InstallError::Manifest(ManifestError::BadId(_)))
    ));
    std::fs::remove_file(src.join(MANIFEST_FILE)).unwrap();
    assert!(matches!(
        store.stage(&Source::Folder(src)),
        Err(InstallError::NoManifest)
    ));
    assert!(leftovers(&config).is_empty());
}

#[test]
fn git_source_rejects_non_https_urls() {
    for url in [
        "http://example.com/x.git",
        "file:///tmp/x",
        "ssh://git@example.com/x.git",
        "/tmp/local",
        "../relative",
        "-oProxyCommand=touch /tmp/pwned",
        "--upload-pack=touch /tmp/pwned",
        "ext::sh -c touch% /tmp/pwned",
        "https://",
        "git@",
        "git@-oProxyCommand=x:repo",
        "git@foo/bar",
        "git@foo/bar:x",
        "git@:path",
        "git@host:",
        "https://-host/x.git",
        "https://example.com/x y.git",
        "https://example.com/x\n.git",
        "https://example.com/x\t.git",
        "",
    ] {
        assert!(
            matches!(check_git_source(url, None), Err(InstallError::BadUrl(_))),
            "{url:?} accepted"
        );
    }
    for url in [
        "https://github.com/a/b.git",
        "git@github.com:a/b.git",
        "git@host:a",
    ] {
        assert!(check_git_source(url, None).is_ok(), "{url:?} refused");
    }
    for rev in ["", "-b", "--upload-pack=x", "v1 x", "v1\n"] {
        assert!(
            matches!(
                check_git_source("https://github.com/a/b.git", Some(rev)),
                Err(InstallError::BadRev(_))
            ),
            "{rev:?} accepted"
        );
    }
    assert!(check_git_source("https://github.com/a/b.git", Some("v1.2.0")).is_ok());

    // Refused before git runs: nothing is staged.
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let source = Source::Git {
        url: "file:///tmp/x".into(),
        rev: None,
    };
    assert!(matches!(
        ExtStore::open(&config).stage(&source),
        Err(InstallError::BadUrl(_))
    ));
    assert!(leftovers(&config).is_empty());
}

#[test]
fn git_clone_command_is_fixed() {
    let (dest, cwd) = (Path::new("/tmp/stage/clone"), Path::new("/tmp/stage"));
    let args = |rev| {
        let command = git_clone_command("https://h/r.git", rev, dest, cwd);
        assert_eq!(command.get_program(), "git");
        assert_eq!(command.get_current_dir(), Some(cwd));
        let envs: Vec<_> = command.get_envs().collect();
        for (key, value) in [
            ("GIT_TERMINAL_PROMPT", "0"),
            ("GIT_LFS_SKIP_SMUDGE", "1"),
            ("GIT_ALLOW_PROTOCOL", "https:ssh"),
            ("GIT_CONFIG_NOSYSTEM", "1"),
            // R56: ssh never asks for a host key or a passphrase on /dev/tty.
            ("GIT_SSH_COMMAND", "ssh -o BatchMode=yes"),
        ] {
            assert!(
                envs.contains(&(key.as_ref(), Some(value.as_ref()))),
                "{key}={value} not in {envs:?}"
            );
        }
        (command.get_args())
            .map(|a| a.to_str().unwrap().to_string())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        args(None),
        [
            "clone",
            "--depth",
            "1",
            "--",
            "https://h/r.git",
            "/tmp/stage/clone"
        ]
    );
    assert_eq!(
        args(Some("v1")),
        [
            "clone",
            "--depth",
            "1",
            "--branch",
            "v1",
            "--",
            "https://h/r.git",
            "/tmp/stage/clone"
        ]
    );
}

fn project_with(body: &str) -> tempfile::TempDir {
    let project = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(project.path().join(".wai")).unwrap();
    std::fs::write(project.path().join(".wai/extensions.toml"), body).unwrap();
    project
}

#[test]
fn project_suggestions_parse_and_skip_malformed() {
    let none = tempfile::tempdir().unwrap();
    assert_eq!(load_suggestions(none.path()), (Vec::new(), Vec::new()));

    let project = project_with(
        "[[suggest]]\nid = \"waycarbon\"\nsource = \"https://github.com/a/b.git\"\n\
         [[suggest]]\nid = \"local\"\nsource = \"tools/ext\"\n",
    );
    let (suggestions, notices) = load_suggestions(project.path());
    assert!(notices.is_empty(), "{notices:?}");
    assert_eq!(
        suggestions,
        [
            Suggestion {
                id: ExtId::new("waycarbon").unwrap(),
                source: Source::Git {
                    url: "https://github.com/a/b.git".into(),
                    rev: None
                },
            },
            Suggestion {
                id: ExtId::new("local").unwrap(),
                source: Source::Folder(project.path().join("tools/ext")),
            },
        ]
    );

    let padding = format!("# {}\n", "x".repeat(1024 * 1024));
    for body in [
        "[[suggest]]\nid = \"x\"\nsource = \"https://a/b\"\ncommand = \"sh\"\n",
        "[[suggest]]\nid = \"../x\"\nsource = \"https://a/b\"\n",
        "[[suggest]]\nid = \"x\"\nsource = \"http://a/b\"\n",
        "[[suggest]]\nid = \"x\"\nsource = \"/etc\"\n",
        "[[suggest]]\nid = \"x\"\nsource = \"../outside\"\n",
        "[[suggest]]\nid = \"x\"\n",
        "suggest = 3\n",
        padding.as_str(),
    ] {
        let project = project_with(body);
        let (suggestions, notices) = load_suggestions(project.path());
        assert!(suggestions.is_empty(), "{body:.60}");
        assert_eq!(notices.len(), 1, "{body:.60}");
        assert!(
            notices[0].starts_with("Ignored extension suggestions"),
            "{notices:?}"
        );
    }
}

/// `sample` rewritten as `version`, with `extra` as an additional file.
fn sample_v(root: &Path, version: &str, extra: &str) -> PathBuf {
    let src = sample(root);
    let manifest = std::fs::read_to_string(src.join(MANIFEST_FILE)).unwrap();
    std::fs::write(src.join(MANIFEST_FILE), manifest.replace("1.2.0", version)).unwrap();
    std::fs::write(src.join(extra), "x").unwrap();
    src
}

fn installed_sample(tmp: &Path, config: &Path) -> (ExtStore, ExtId) {
    let mut store = ExtStore::open(config);
    let old = sample_v(&tmp.join("old"), "0.1.0", "old-only");
    let id = store
        .commit(store.stage(&Source::Folder(old)).unwrap(), perms())
        .unwrap();
    store.set_enabled(&id, false).unwrap();
    (store, id)
}

#[test]
fn update_swaps_code_keeps_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let (mut store, id) = installed_sample(tmp.path(), &config);
    let data = store.list()[0].data.clone();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(data.join("marker"), "keep").unwrap();

    let new = sample_v(&tmp.path().join("new"), "0.2.0", "new-only");
    let staged = store.stage(&Source::Folder(new)).unwrap();
    let (got, trash) = store.update(staged, perms()).unwrap();

    assert_eq!(got, id);
    let check = |s: &ExtStore| {
        let l = s.list();
        assert_eq!(l[0].manifest.version, "0.2.0");
        assert!(!l[0].enabled);
    };
    check(&store);
    check(&ExtStore::open(&config));
    let dir = config.join("extensions/sample");
    assert!(dir.join("new-only").is_file());
    assert!(!dir.join("old-only").exists());
    assert_eq!(
        std::fs::read_to_string(data.join("marker")).unwrap(),
        "keep"
    );
    assert!(
        std::fs::read_to_string(config.join("extensions.json"))
            .unwrap()
            .contains("0.2.0")
    );
    assert!(trash.join("old-only").is_file());
}

#[test]
fn update_refuses_unknown_id_and_wrong_consent() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let mut empty = ExtStore::open(&config);
    let staged = empty
        .stage(&Source::Folder(sample(&tmp.path().join("a"))))
        .unwrap();
    assert!(matches!(
        empty.update(staged, perms()),
        Err(InstallError::NotInstalled(id)) if id == "sample"
    ));
    assert!(!config.join("extensions/sample").exists());

    let (mut store, _) = installed_sample(tmp.path(), &config);
    let new = sample_v(&tmp.path().join("new"), "0.2.0", "new-only");
    let staged = store.stage(&Source::Folder(new)).unwrap();
    assert!(matches!(
        store.update(staged, vec![Permission::Process]),
        Err(InstallError::Consent)
    ));
    assert_eq!(store.list()[0].manifest.version, "0.1.0");
    assert!(config.join("extensions/sample/old-only").is_file());
    assert!(!config.join("extensions/sample/new-only").exists());
}

#[cfg(unix)]
#[test]
fn failed_update_restores_the_old_folder() {
    let tmp = tempfile::tempdir().unwrap();
    let config = tmp.path().join("config");
    let (mut store, _) = installed_sample(tmp.path(), &config);
    let new = sample_v(&tmp.path().join("new"), "0.2.0", "new-only");
    let staged = store.stage(&Source::Folder(new)).unwrap();
    // `save` removes a file at this name, not a non-empty folder: it fails.
    let tmp_json = config.join("extensions.json.tmp");
    std::fs::create_dir_all(tmp_json.join("x")).unwrap();

    assert!(store.update(staged, perms()).is_err());

    let dir = config.join("extensions/sample");
    assert!(dir.join("old-only").is_file());
    assert!(!dir.join("new-only").exists());
    assert_eq!(store.list()[0].manifest.version, "0.1.0");
    assert!(
        std::fs::read_to_string(config.join("extensions.json"))
            .unwrap()
            .contains("0.1.0")
    );
}
