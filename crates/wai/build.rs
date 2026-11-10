//! `WAI_BUILD`: the short hash of the commit wai is built from; empty outside a git checkout or
//! without `git`. The top bar shows it beside the version, so each build can be told apart.
//! No `--dirty`: release.yml edits Cargo.toml before building when it signs.

use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    out.status.success().then_some(text)
}

fn main() {
    let build = git(&["describe", "--always", "--exclude=*"]).unwrap_or_default();
    println!("cargo:rustc-env=WAI_BUILD={build}");
    // A checkout writes HEAD; a commit, the index (a worktree's own). Nothing else reruns this;
    // outside a checkout, cargo's default (any file of the crate) stands.
    for file in ["HEAD", "index"] {
        if let Some(path) = git(&["rev-parse", "--git-path", file]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
}
