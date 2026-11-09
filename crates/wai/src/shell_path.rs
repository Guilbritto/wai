//! R68 (macOS): an app opened from Finder, the Dock or Launchpad gets launchd's PATH,
//! `/usr/bin:/bin:/usr/sbin:/sbin`, with no `npx` or `node`. wai then takes PATH from the user's
//! login shell before anything reads it. A terminal launch keeps the terminal's.
//! R69: the shell's answer is cached in the config dir. A launch uses the cache and refreshes it in
//! the background for the next one; only a launch without a usable cache waits for the shell.

use std::ffi::{OsStr, OsString};
use std::fs::File;
use std::io::{Read as _, Seek as _, SeekFrom, Write as _};
use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _};
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const MARKER: &str = "_WAI_LOGIN_PATH_7c1e_";
const TIMEOUT: Duration = Duration::from_secs(3);
const CAP: u64 = 64 * 1024;
const CACHE: &str = "shell-path";

/// Sets PATH from the cache or the login shell when wai was not started from a terminal (no
/// `TERM`). Must run before any thread exists. On failure PATH stays as launched, and one line
/// says why.
pub fn import() {
    if std::env::var_os("TERM").is_some() {
        return;
    }
    let shell = shell(std::env::var_os("SHELL"));
    let dir = wai_settings::config_dir();
    let current = std::env::var_os("PATH").unwrap_or_default();
    let start = Instant::now();
    let (path, cached) = match resolve(&shell, &dir, &current, TIMEOUT) {
        Ok(found) => found,
        Err(e) => {
            return eprintln!(
                "wai: kept the launch PATH, none from {}: {e}",
                shell.display()
            );
        }
    };
    let n = std::env::split_paths(&path).count();
    match cached {
        true => eprintln!("wai: PATH from cache ({n} entries)"),
        false => {
            let ms = start.elapsed().as_millis();
            eprintln!("wai: PATH from login shell ({n} entries, {ms} ms)");
        }
    }
    // SAFETY: `main` calls this first, before any thread exists, and the refresh below starts
    // after this line, so nothing can read or write the environment concurrently.
    unsafe { std::env::set_var("PATH", path) };
    if cached {
        // ponytail: the refreshed PATH is for the next launch; this one keeps the cached PATH
        // (no `set_var` once threads run).
        std::thread::spawn(move || {
            if let Err(e) = ask_shell(&shell, &dir, &current, TIMEOUT) {
                eprintln!(
                    "wai: PATH cache not refreshed from {}: {e}",
                    shell.display()
                );
            }
        });
    }
}

/// The PATH to set and whether it came from the cache (no wait) rather than the shell, asked now.
fn resolve(
    shell: &Path,
    dir: &Path,
    current: &OsStr,
    timeout: Duration,
) -> Result<(OsString, bool), String> {
    if let Some(path) = read_cache(&dir.join(CACHE)).and_then(|p| merged(&p, current)) {
        return Ok((path, true));
    }
    let found = ask_shell(shell, dir, current, timeout)?;
    Ok((merged(&found, current).ok_or("no absolute folder")?, false))
}

/// The login shell's PATH, cached for the next launch. A cache that cannot be written only costs
/// speed, so that is one line, not an error. Never touches this process's environment.
fn ask_shell(
    shell: &Path,
    dir: &Path,
    launch: &OsStr,
    timeout: Duration,
) -> Result<String, String> {
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    let found = login_path(shell, dir, launch, timeout)?;
    merged(&found, OsStr::new("")).ok_or("no absolute folder")?;
    let cache = dir.join(CACHE);
    if let Err(e) = write_cache(&cache, &found) {
        eprintln!("wai: cannot write {}: {e}", cache.display());
    }
    Ok(found)
}

/// The cached PATH, if only this user could have written it: their regular file (not a symlink),
/// not group or world writable, at most `CAP`.
// ponytail: checked by path, then read. Whoever could swap it in between can already write the
// config dir, settings.toml included.
fn read_cache(cache: &Path) -> Option<String> {
    let meta = std::fs::symlink_metadata(cache).ok()?;
    let mine = meta.uid() == rustix::process::getuid().as_raw();
    if !(meta.is_file() && mine && meta.mode() & 0o022 == 0 && meta.len() <= CAP) {
        return None;
    }
    std::fs::read_to_string(cache).ok()
}

/// Replaces `cache` at once: a new 0600 file beside it, then a rename.
// ponytail: no fsync. A crash can leave the cache empty; it names no folder, so that launch asks
// the shell.
fn write_cache(cache: &Path, path: &str) -> std::io::Result<()> {
    let tmp = cache.with_file_name(format!(".{CACHE}-{}.tmp", std::process::id()));
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&tmp)?;
    let written = file
        .write_all(path.as_bytes())
        .and_then(|()| std::fs::rename(&tmp, cache));
    if written.is_err() {
        std::fs::remove_file(&tmp).ok();
    }
    written
}

/// `$SHELL` when it is an absolute path to a program, else zsh (macOS's default).
fn shell(var: Option<OsString>) -> PathBuf {
    var.map(PathBuf::from)
        .filter(|s| s.is_absolute() && wai_settings::executable(s))
        .unwrap_or_else(|| "/bin/zsh".into())
}

/// The PATH `shell -i -l -c` sees after its rc files, run from `dir` (the config dir, never a
/// repository: R18) and started with the `launch` PATH: an imported one would carry a cached
/// folder into every refresh, so one that is gone would never leave. A hung shell is killed with
/// whatever it started at `timeout`.
fn login_path(
    shell: &Path,
    dir: &Path,
    launch: &OsStr,
    timeout: Duration,
) -> Result<String, String> {
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
        .env("PATH", launch)
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
    // ponytail: after an answer, jobs the rc files left running are not killed (killing the
    // group once the shell is reaped risks a reused id). One writing to stdout forever grows the
    // unlinked file until it exits.
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
        let found = login_path(&shell, cwd.path(), LAUNCH.as_ref(), SECOND).unwrap();
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
        let found = login_path(
            &shell,
            dir.path(),
            LAUNCH.as_ref(),
            Duration::from_millis(300),
        );
        assert!(
            found.as_ref().is_err_and(|e| e.contains("no answer")),
            "{found:?}"
        );
        let elapsed = start.elapsed();
        assert!(Duration::from_millis(300) <= elapsed && elapsed < Duration::from_secs(2));
    }

    /// A fake login shell in its own dir that puts `/shell/bin` before the PATH it inherits, as a
    /// `.zprofile` does, and leaves `ran` there,
    /// and an empty config dir.
    fn harness() -> (tempfile::TempDir, PathBuf, tempfile::TempDir) {
        let bin = tempfile::tempdir().unwrap();
        let body = "/usr/bin/touch \"$(/usr/bin/dirname \"$0\")/ran\"\nPATH=\"/shell/bin:$PATH\"; export PATH\neval \"$4\"";
        let shell = fake_shell(bin.path(), body);
        (bin, shell, tempfile::tempdir().unwrap())
    }

    fn write_cache_file(path: &Path, text: &str, mode: u32) {
        std::fs::write(path, text).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).unwrap();
    }

    fn mode(path: &Path) -> u32 {
        std::fs::symlink_metadata(path).unwrap().mode() & 0o777
    }

    const SECOND: Duration = Duration::from_secs(10);
    const LAUNCH: &str = "/usr/bin:/bin";

    #[test]
    fn a_trusted_cache_is_used_and_the_shell_is_not_asked() {
        let (bin, shell, cfg) = harness();
        write_cache_file(&cfg.path().join(CACHE), "/cached/bin:/usr/bin", 0o600);
        let found = resolve(&shell, cfg.path(), LAUNCH.as_ref(), SECOND).unwrap();
        assert_eq!(found, ("/cached/bin:/usr/bin:/bin".into(), true));
        assert!(!bin.path().join("ran").exists());
    }

    #[test]
    fn without_a_cache_the_shell_is_asked_and_its_path_cached() {
        let (bin, shell, cfg) = harness();
        let found = resolve(&shell, cfg.path(), LAUNCH.as_ref(), SECOND).unwrap();
        assert_eq!(found, ("/shell/bin:/usr/bin:/bin".into(), false));
        assert!(bin.path().join("ran").exists());
        let cache = cfg.path().join(CACHE);
        assert_eq!(
            std::fs::read_to_string(&cache).unwrap(),
            "/shell/bin:/usr/bin:/bin"
        );
        assert_eq!(mode(&cache), 0o600);
    }

    #[test]
    fn an_untrusted_cache_is_ignored_and_the_shell_is_asked() {
        let oversized = "/a:".repeat(CAP as usize / 3 + 1);
        for case in ["group-writable", "world-writable", "symlink", "oversized"] {
            let (bin, shell, cfg) = harness();
            let cache = cfg.path().join(CACHE);
            let target = bin.path().join("elsewhere");
            match case {
                "group-writable" => write_cache_file(&cache, "/cached/bin", 0o620),
                "world-writable" => write_cache_file(&cache, "/cached/bin", 0o602),
                "symlink" => {
                    write_cache_file(&target, "/cached/bin", 0o600);
                    std::os::unix::fs::symlink(&target, &cache).unwrap();
                }
                _ => write_cache_file(&cache, &oversized, 0o600),
            }
            let found = resolve(&shell, cfg.path(), LAUNCH.as_ref(), SECOND).unwrap();
            assert_eq!(found, ("/shell/bin:/usr/bin:/bin".into(), false), "{case}");
            assert!(bin.path().join("ran").exists(), "{case}");
            // Replaced by a trusted one; a symlink's target is left alone.
            assert_eq!(
                std::fs::read_to_string(&cache).unwrap(),
                "/shell/bin:/usr/bin:/bin"
            );
            assert_eq!(mode(&cache), 0o600, "{case}");
            if case == "symlink" {
                assert_eq!(std::fs::read_to_string(&target).unwrap(), "/cached/bin");
            }
        }
    }

    #[test]
    fn a_refresh_replaces_the_cache_atomically_with_mode_0600() {
        let (_bin, shell, cfg) = harness();
        let cache = cfg.path().join(CACHE);
        write_cache_file(&cache, "/old/bin", 0o600);
        let mut old = std::fs::File::open(&cache).unwrap();
        assert_eq!(
            ask_shell(&shell, cfg.path(), "/launch/bin".as_ref(), SECOND).unwrap(),
            "/shell/bin:/launch/bin"
        );
        assert_eq!(
            std::fs::read_to_string(&cache).unwrap(),
            "/shell/bin:/launch/bin"
        );
        assert_eq!(mode(&cache), 0o600);
        // A rename, not a rewrite: the old file is untouched, and no temp file is left.
        let mut text = String::new();
        old.read_to_string(&mut text).unwrap();
        assert_eq!(text, "/old/bin");
        let names: Vec<_> = std::fs::read_dir(cfg.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, [CACHE]);
    }
}
