//! Installs the Claude Code plugins an extension requires, with the user's `claude` CLI, after
//! the user pressed Install. Argv only, no shell, never `-y`.

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use wai_ext::RequiredPlugin;
use wai_ext::panel::clamp;

pub const INSTALL_TIMEOUT: Duration = Duration::from_secs(300);

const REASON_CHARS: usize = 300;

pub fn install_args(p: &RequiredPlugin) -> Vec<String> {
    let args = [
        "plugin",
        "install",
        &p.name,
        "--marketplace",
        &p.marketplace,
    ];
    let tail = ["--scope", "user", "--json"];
    args.into_iter().chain(tail).map(String::from).collect()
}

/// What "Install in terminal" types: the user presses Enter.
pub fn terminal_line(p: &RequiredPlugin) -> String {
    format!(
        "claude plugin install {} --marketplace {}",
        p.name, p.marketplace
    )
}

fn command(claude: &Path, cwd: &Path) -> Command {
    let mut cmd = Command::new(claude);
    // A private marketplace must never hang on a password prompt.
    cmd.current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    cmd
}

/// The ids `claude plugin list --json` shows; empty on any failure.
pub fn installed(claude: &Path, cwd: &Path) -> Vec<String> {
    let out = command(claude, cwd)
        .args(["plugin", "list", "--json"])
        .stderr(Stdio::null())
        .output();
    let Some(out) = out.ok().filter(|o| o.status.success()) else {
        return Vec::new();
    };
    let list: Vec<serde_json::Value> = serde_json::from_slice(&out.stdout).unwrap_or_default();
    (list.iter())
        .filter_map(|p| p.get("id")?.as_str().map(String::from))
        .collect()
}

fn read(pipe: Option<impl std::io::Read + Send + 'static>) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(mut pipe) = pipe {
            // The output is small; a read error leaves what came so far.
            pipe.read_to_string(&mut text).ok();
        }
        text
    })
}

/// Blocking: run it on a background thread.
pub fn install(
    claude: &Path,
    p: &RequiredPlugin,
    cwd: &Path,
    timeout: Duration,
) -> Result<(), String> {
    let prefix = format!("{}@", p.name);
    if installed(claude, cwd)
        .iter()
        .any(|id| id.starts_with(&prefix))
    {
        return Ok(());
    }
    let mut child = (command(claude, cwd).args(install_args(p)))
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("could not run claude: {e}"))?;
    let (stdout, stderr) = (read(child.stdout.take()), read(child.stderr.take()));
    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() >= deadline => {
                // Not joined: a grandchild may still hold the pipes open.
                child.kill().ok();
                child.wait().ok();
                return Err(format!("no answer in {} minutes", timeout.as_secs() / 60));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(e) => return Err(e.to_string()),
        }
    };
    if status.success() {
        return Ok(());
    }
    let last = |text: String| {
        let line = text.lines().rev().find(|l| !l.trim().is_empty());
        line.map(|l| clamp(l, REASON_CHARS))
    };
    let (stdout, stderr) = (
        stdout.join().unwrap_or_default(),
        stderr.join().unwrap_or_default(),
    );
    Err(last(stderr)
        .or_else(|| last(stdout))
        .unwrap_or_else(|| format!("claude exited with {status}")))
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt as _;

    use super::*;

    fn plugin() -> RequiredPlugin {
        RequiredPlugin {
            name: "waycarbon".into(),
            marketplace: "waycarbon/claude-code".into(),
        }
    }

    /// A fake `claude`: `list` prints `list`, `install` runs `on_install`; every call is logged.
    fn fake(dir: &Path, list: &str, on_install: &str) -> std::path::PathBuf {
        let path = dir.join("claude");
        let script = format!(
            "#!/bin/sh\necho \"$* [$GIT_TERMINAL_PROMPT]\" >> '{log}'\n\
             case \"$2\" in list) echo '{list}' ;; install) {on_install} ;; esac\n",
            log = dir.join("log").display()
        );
        std::fs::write(&path, script).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    fn log(dir: &Path) -> String {
        std::fs::read_to_string(dir.join("log")).unwrap_or_default()
    }

    #[test]
    fn install_runs_claude_with_argv_only() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = fake(tmp.path(), "[]", "true");
        install(&claude, &plugin(), tmp.path(), INSTALL_TIMEOUT).unwrap();
        let log = log(tmp.path());
        let want =
            "plugin install waycarbon --marketplace waycarbon/claude-code --scope user --json [0]";
        assert!(log.lines().any(|l| l == want), "{log}");
        assert!(!log.contains(" -y"), "{log}");
        assert_eq!(
            terminal_line(&plugin()),
            "claude plugin install waycarbon --marketplace waycarbon/claude-code"
        );
    }

    #[test]
    fn already_installed_is_skipped() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = fake(tmp.path(), r#"[{"id":"waycarbon@waycarbon"}]"#, "exit 1");
        install(&claude, &plugin(), tmp.path(), INSTALL_TIMEOUT).unwrap();
        assert!(!log(tmp.path()).contains("plugin install"));
    }

    #[test]
    fn failure_reports_the_last_stderr_line() {
        let tmp = tempfile::tempdir().unwrap();
        let on_install = "printf 'a\\nPermission denied (publickey)\\n' >&2; exit 1";
        let claude = fake(tmp.path(), "[]", on_install);
        let err = install(&claude, &plugin(), tmp.path(), INSTALL_TIMEOUT).unwrap_err();
        assert_eq!(err, "Permission denied (publickey)");
    }

    #[test]
    fn hung_install_is_killed() {
        let tmp = tempfile::tempdir().unwrap();
        let claude = fake(tmp.path(), "[]", "sleep 30");
        let start = Instant::now();
        let result = install(&claude, &plugin(), tmp.path(), Duration::from_millis(300));
        assert!(result.is_err());
        assert!(start.elapsed() < Duration::from_secs(5));
    }
}
