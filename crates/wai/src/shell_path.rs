//! R68 (macOS): an app opened from Finder, the Dock or Launchpad gets launchd's PATH,
//! `/usr/bin:/bin:/usr/sbin:/sbin`, with no `npx` or `node`. wai then takes PATH from the user's
//! login shell, once, before anything reads it. A terminal launch keeps the terminal's.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARKER: &str = "_WAI_LOGIN_PATH_7c1e_";
const TIMEOUT: Duration = Duration::from_secs(3);
const CAP: u64 = 64 * 1024;

/// Sets PATH from the login shell when wai was not started from a terminal (no `TERM`). Must run
/// before any thread exists. On failure PATH stays as launched, and one line says why.
// ponytail: no cache. If slow shells make Finder launches slow, cache the result in the config
// dir and refresh it for the next launch.
pub fn import() {
    if std::env::var_os("TERM").is_some() {
        return;
    }
    let shell = shell(std::env::var_os("SHELL"));
    let dir = wai_settings::config_dir();
    let current = std::env::var_os("PATH").unwrap_or_default();
    let start = Instant::now();
    let found = std::fs::create_dir_all(&dir)
        .map_err(|e| e.to_string())
        .and_then(|()| login_path(&shell, &dir, TIMEOUT))
        .and_then(|path| merged(&path, &current).ok_or("no absolute folder".into()));
    match found {
        Ok(path) => {
            let n = std::env::split_paths(&path).count();
            let ms = start.elapsed().as_millis();
            eprintln!("wai: PATH from login shell ({n} entries, {ms} ms)");
            // SAFETY: `main` calls this first, before any thread exists (this function starts
            // none), so nothing can read or write the environment concurrently.
            unsafe { std::env::set_var("PATH", path) };
        }
        Err(e) => eprintln!(
            "wai: kept the launch PATH, none from {}: {e}",
            shell.display()
        ),
    }
}

/// `$SHELL` when it is an absolute path to a program, else zsh (macOS's default).
fn shell(var: Option<OsString>) -> PathBuf {
    var.map(PathBuf::from)
        .filter(|s| s.is_absolute() && wai_settings::executable(s))
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// The PATH `shell -i -l -c` sees after its rc files, run from `dir` (the config dir, never a
/// repository: R18). A hung shell is killed with whatever it started at `timeout`.
fn login_path(shell: &Path, dir: &Path, timeout: Duration) -> Result<String, String> {
    // The marker in two halves, so an echo of this command line prints none. `printenv`, not
    // `"$PATH"`: fish joins a quoted list with spaces.
    let (head, tail) = MARKER.split_at(MARKER.len() / 2);
    let mark = format!("printf '%s%s' {head} {tail}");
    let print = format!("{mark}; /usr/bin/printenv PATH; {mark}");
    // A file, not a pipe: no reader thread before `set_var`, and nothing an rc file leaves running
    // can hold a read open. Unlinked at once; the open handles keep it.
    let name = dir.join(format!(".login-path-{}", std::process::id()));
    let open = File::options()
        .read(true)
        .write(true)
        .create(true)
        .truncate(true)
        .open(&name);
    let mut out = open.map_err(|e| format!("cannot create {}: {e}", name.display()))?;
    std::fs::remove_file(&name).ok();
    let mut command = Command::new(shell);
    command
        .args(["-i", "-l", "-c", &print])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(out.try_clone().map_err(|e| e.to_string())?)
        .stderr(Stdio::null());
    // Its own session: no terminal for an interactive shell to take over, and one group to kill.
    // SAFETY: `setsid` is async-signal-safe and touches no memory.
    unsafe { command.pre_exec(|| rustix::process::setsid().map(drop).map_err(Into::into)) };
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    let deadline = Instant::now() + timeout;
    while child.try_wait().map_err(|e| e.to_string())?.is_none() {
        if Instant::now() >= deadline {
            let pid = rustix::process::Pid::from_child(&child);
            let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
            let _ = child.kill();
            let _ = child.wait();
            return Err(format!("no answer in {timeout:?}"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let mut text = Vec::new();
    out.seek(SeekFrom::Start(0))
        .and_then(|_| out.take(CAP).read_to_end(&mut text))
        .map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&text);
    between_markers(&text)
        .map(str::to_string)
        .ok_or_else(|| "no marked PATH in its output".into())
}

/// The text between exactly two markers: rc files may print around them, never a third one.
fn between_markers(out: &str) -> Option<&str> {
    match out.split(MARKER).collect::<Vec<_>>()[..] {
        [_, path, _] => Some(path.trim_end_matches('\n')),
        _ => None,
    }
}

/// The shell's absolute folders, then the launch PATH's absolute folders it lacks, in order and
/// once each (R56: a relative entry would be the current dir). `None` if the shell gave none.
fn merged(shell: &str, current: &OsStr) -> Option<OsString> {
    let absolute = |dir: &PathBuf| dir.is_absolute();
    std::env::split_paths(shell).find(absolute)?;
    let mut dirs: Vec<PathBuf> = Vec::new();
    for dir in std::env::split_paths(shell).chain(std::env::split_paths(current)) {
        if absolute(&dir) && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    }
    std::env::join_paths(dirs).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt as _;

    fn fake_shell(dir: &Path, body: &str) -> PathBuf {
        let path = dir.join("fake-shell");
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn path_is_read_between_two_markers_only() {
        let m = MARKER;
        let noisy = format!("Welcome!\n{m}/opt/homebrew/bin:/usr/bin\n{m}bye\n");
        assert_eq!(between_markers(&noisy), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(between_markers(&format!("{m}/a:/b{m}")), Some("/a:/b"));
        assert_eq!(between_markers("Welcome!\n/usr/bin\n"), None);
        assert_eq!(between_markers(&format!("{m}/a:/b")), None);
        // More than one pair: something else printed the marker, so nothing is trusted.
        assert_eq!(between_markers(&format!("{m}/a{m}{m}/b{m}")), None);
        assert_eq!(between_markers(&format!("{m}{m}/a{m}")), None);
    }

    #[test]
    fn shell_entries_come_first_then_new_launch_entries() {
        let merge = |shell, current: &str| merged(shell, current.as_ref());
        let shell = "/opt/homebrew/bin:/usr/bin:bin::.:/opt/homebrew/bin:/usr/local/bin";
        assert_eq!(
            merge(shell, "/usr/bin:/bin:./x:/usr/sbin:/sbin"),
            Some("/opt/homebrew/bin:/usr/bin:/usr/local/bin:/bin:/usr/sbin:/sbin".into())
        );
        assert_eq!(merge("/a", ""), Some("/a".into()));
        // No absolute folder from the shell: keep the launch PATH.
        assert_eq!(merge("", "/usr/bin"), None);
        assert_eq!(merge("bin:.", "/usr/bin"), None);
    }

    #[test]
    fn shell_is_an_absolute_executable_else_zsh() {
        let zsh = PathBuf::from("/bin/zsh");
        assert_eq!(shell(Some("/bin/sh".into())), PathBuf::from("/bin/sh"));
        assert_eq!(shell(None), zsh);
        assert_eq!(shell(Some("sh".into())), zsh);
        assert_eq!(shell(Some("/nonexistent/sh".into())), zsh);
        assert_eq!(shell(Some("/etc/hosts".into())), zsh);
    }

    #[test]
    fn login_shell_runs_in_the_given_dir_and_its_path_is_read_through_noise() {
        let bin = tempfile::tempdir().unwrap();
        let cwd = tempfile::tempdir().unwrap();
        let body = r#"echo "rc noise $*"
[ "$1 $2 $3" = "-i -l -c" ] || exit 9
PATH="$(pwd -P)/bin:/usr/bin:/bin"; export PATH
eval "$4"
echo "logout noise""#;
        let shell = fake_shell(bin.path(), body);
        let found = login_path(&shell, cwd.path(), Duration::from_secs(10)).unwrap();
        let cwd_real = wai_settings::canonical(cwd.path());
        assert_eq!(found, format!("{}/bin:/usr/bin:/bin", cwd_real.display()));
        // The output file is gone (R18: and it lives in the config dir, never a repository).
        assert_eq!(std::fs::read_dir(cwd.path()).unwrap().count(), 0);
    }

    #[test]
    fn a_hanging_login_shell_is_killed_at_the_deadline() {
        let dir = tempfile::tempdir().unwrap();
        let shell = fake_shell(dir.path(), "echo started\nsleep 30");
        let start = Instant::now();
        let found = login_path(&shell, dir.path(), Duration::from_millis(300));
        assert!(
            found.as_ref().is_err_and(|e| e.contains("no answer")),
            "{found:?}"
        );
        let elapsed = start.elapsed();
        assert!(Duration::from_millis(300) <= elapsed && elapsed < Duration::from_secs(2));
    }
}
