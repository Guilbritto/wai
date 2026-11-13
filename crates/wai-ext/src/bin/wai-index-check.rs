//! Checks an extension index the way wai reads it, for CI on index pull requests:
//! `wai-index-check <index.toml>`.
//!
//! Every entry must parse, ids must be unique, and every package must download (with `curl`),
//! match its SHA-256, unpack under wai's rules and hold a manifest wai accepts, with the entry's
//! id and version. It prints one line per package and exits 1 on any failure.

use std::path::Path;
use std::process::{Command, ExitCode};

use wai_ext::index::parse_index;
use wai_ext::{ExtStore, Staged};

/// wai downloads at most this much (the unpacked limit is the same).
const MAX_BYTES: &str = "104857600";

fn download(url: &str, to: &Path) -> Result<(), String> {
    let out = Command::new("curl")
        .args(["-fsSL", "--proto", "=https", "--proto-redir", "=https"])
        .args(["--max-redirs", "5", "--max-filesize", MAX_BYTES, "-o"])
        .arg(to)
        .arg("--")
        .arg(url)
        .output()
        .map_err(|e| format!("cannot run curl: {e}"))?;
    match out.status.success() {
        true => Ok(()),
        false => Err(String::from_utf8_lossy(&out.stderr).trim().to_string()),
    }
}

/// A platform package's program, when the manifest names one inside the package (`bin/x`, not
/// `node`) and the package lacks it. On Windows wai runs `<command>.exe`.
fn missing_program(staged: &Staged, platform: &str) -> Option<String> {
    let command = &staged.manifest().process.as_ref()?.command;
    if platform == "any" || !command.contains('/') {
        return None;
    }
    let program = match platform.starts_with("windows-") {
        true => format!("{command}.exe"),
        false => command.clone(),
    };
    (!staged.dir().join(&program).is_file()).then_some(program)
}

fn main() -> ExitCode {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: wai-index-check <index.toml>");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let (listings, skipped) = match parse_index(&text) {
        Ok(parsed) => parsed,
        Err(e) => {
            eprintln!("{path}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut failed = !skipped.is_empty();
    for notice in &skipped {
        println!("FAIL {notice}");
    }
    let work = std::env::temp_dir().join(format!("wai-index-check-{}", std::process::id()));
    let store = ExtStore::open(&work.join("config"));
    let mut seen = Vec::new();
    for listing in &listings {
        let id = listing.id.as_str();
        if seen.contains(&id) {
            println!("FAIL {id}: listed twice");
            failed = true;
        }
        seen.push(id);
        for (platform, package) in &listing.packages {
            let fetch = |to: &Path| download(&package.url, to);
            let checked = (store.stage_package(&package.url, &package.sha256, fetch))
                .map_err(|e| e.to_string())
                .and_then(|staged| {
                    let m = staged.manifest();
                    if m.id != listing.id {
                        Err(format!("its manifest's id is {}", m.id.as_str()))
                    } else if m.version != listing.version {
                        Err(format!("its manifest's version is {}", m.version))
                    } else if let Some(missing) = missing_program(&staged, platform) {
                        Err(format!("it does not hold its program {missing}"))
                    } else {
                        Ok(())
                    }
                });
            match checked {
                Ok(()) => println!("ok   {id} {} {platform}", listing.version),
                Err(e) => {
                    println!("FAIL {id} {platform}: {e}");
                    failed = true;
                }
            }
        }
    }
    std::fs::remove_dir_all(&work).ok();
    println!("{} extensions, {} skipped", listings.len(), skipped.len());
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
