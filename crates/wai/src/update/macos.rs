//! macOS install step: swap the running `.app` bundle for the downloaded one, keeping the previous
//! build as `<bundle>.old`, and relaunch once wai exits. Also the one-time move out of
//! `/Applications`, where a standard account cannot update.

use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context as _, bail};

const SYSTEM_APPS: &str = "/Applications";
/// Set on the copy `on_start` opens after moving, holding the folder it came from.
const RELOCATED_ENV: &str = "WAI_RELOCATED";
/// Bounds the `open` that starts the moved copy; it normally returns in well under a second.
const RELAUNCH_TIMEOUT: Duration = Duration::from_secs(30);

/// What `main` does after `on_start`.
pub enum Start {
    /// Carry on, showing the notice (if any) once the window exists.
    Continue(Option<String>),
    /// The moved copy is starting; this process must return without opening anything.
    Exit,
}

/// The `.app` directory holding `exe` (`X.app/Contents/MacOS/<bin>`). The bundle, not the binary:
/// replacing only the binary would run it against the old Info.plist and resources.
pub fn bundle_root(exe: &Path) -> Result<PathBuf, String> {
    let root = exe
        .parent()
        .filter(|d| d.file_name().is_some_and(|n| n == "MacOS"))
        .and_then(Path::parent)
        .filter(|d| d.file_name().is_some_and(|n| n == "Contents"))
        .and_then(Path::parent)
        .filter(|d| d.extension().is_some_and(|e| e == "app"))
        .ok_or("this build is not running from an installed wai.app")?;
    // Gatekeeper runs a quarantined app from a read-only copy under AppTranslocation; replacing
    // that copy updates nothing the user will open again.
    if root.to_string_lossy().contains("/AppTranslocation/") {
        return Err(
            "this copy is running from a temporary location; move wai to ~/Applications first"
                .into(),
        );
    }
    Ok(root.to_path_buf())
}

fn current_bundle() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    bundle_root(&exe.canonicalize().unwrap_or(exe))
}

/// Whether this account can add and remove entries in `dir`. A probe file, not `access(2)`: it
/// answers for the real user with ACLs applied, and it is only run when Settings looks.
fn writable(dir: &Path) -> bool {
    let probe = dir.join(format!(".wai-write-check-{}", std::process::id()));
    std::fs::write(&probe, b"").is_ok() && std::fs::remove_file(&probe).is_ok()
}

fn home_apps() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|h| Path::new(&h).join("Applications"))
}

/// Why this copy can't update itself, or `None` when it can.
pub fn blocker() -> Option<String> {
    let bundle = match current_bundle() {
        Ok(b) => b,
        Err(e) => return Some(e),
    };
    let dir = bundle.parent()?;
    if writable(dir) {
        return None;
    }
    let name = bundle.file_name()?.to_string_lossy();
    // Already in ~/Applications: telling the reader to move it there would be circular.
    if home_apps().and_then(|h| h.canonicalize().ok()).as_deref() == Some(dir) {
        return Some(format!(
            "wai can't update itself: this account can't change {}, which belongs to another user. \
             Give the app and the folder back to your account and updates will install on their own.",
            bundle.display()
        ));
    }
    // /Applications has a way out: with admin rights on, the next update installs in place and
    // the build it starts moves itself to ~/Applications (see `relocate`).
    if dir == Path::new(SYSTEM_APPS) {
        return Some(format!(
            "wai is in {}, which this account can't change, so it can't update itself. \
             Turn on admin rights in Jamf Connect and install the next update from here: wai then moves itself to ~/Applications, \
             and updates after that won't need admin. Or move {name} to ~/Applications yourself.",
            dir.display()
        ));
    }
    Some(format!(
        "wai is in {}, which this account can't change, so it can't update itself. \
         Move {name} to ~/Applications, the Applications folder in your home, and updates will install without a password.",
        dir.display()
    ))
}

fn old_of(bundle: &Path) -> PathBuf {
    let mut s = bundle.as_os_str().to_owned();
    s.push(".old");
    s.into()
}

/// Unpacks `zip` next to `bundle` and swaps it in, keeping the previous build as `<bundle>.old`.
/// Staged beside the bundle so the swap is a same-volume rename.
pub fn install(zip: &Path, bundle: &Path) -> anyhow::Result<()> {
    let parent = bundle.parent().context("bundle has no parent folder")?;
    let stage = parent.join(format!(".wai-update-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&stage);
    let result = swap(zip, bundle, &stage);
    let _ = std::fs::remove_dir_all(&stage);
    result
}

fn swap(zip: &Path, bundle: &Path, stage: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(stage)?;
    // ditto, not a zip crate: a bundle carries symlinks and resource forks a plain walk flattens,
    // and the result would not launch.
    let out = Command::new("ditto")
        .args(["-x", "-k"])
        .arg(zip)
        .arg(stage)
        .output()?;
    if !out.status.success() {
        bail!(
            "could not unpack the downloaded archive: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let apps: Vec<PathBuf> = std::fs::read_dir(stage)?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|e| e == "app"))
        .collect();
    // Exactly one: with several, picking one is a guess made at the cost of the working copy.
    let [new] = apps.as_slice() else {
        bail!(
            "expected one .app in the downloaded archive, found {}",
            apps.len()
        );
    };
    let old = old_of(bundle);
    if old.exists() {
        std::fs::remove_dir_all(&old).context("could not clear the previous build")?;
    }
    std::fs::rename(bundle, &old).context("could not set aside the running build")?;
    if let Err(e) = std::fs::rename(new, bundle) {
        let _ = std::fs::rename(&old, bundle);
        return Err(e).context("could not move the new build into place");
    }
    Ok(())
}

/// Opens `bundle` in a new process once `pid` has exited. `open -n`, not the binary: only `open`
/// registers it with the window server as the bundle (icon, Dock entry, activation). Own process
/// group so it outlives wai.
pub fn relaunch_after_exit(bundle: &Path, pid: u32) -> std::io::Result<()> {
    Command::new("/bin/sh")
        .args([
            "-c",
            r#"while kill -0 "$1" 2>/dev/null; do sleep 0.2; done; exec open -n "$2""#,
            "sh",
        ])
        .arg(pid.to_string())
        .arg(bundle)
        .process_group(0)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map(drop)
}

fn bundle_id(bundle: &Path) -> Option<String> {
    let out = Command::new("plutil")
        .args(["-extract", "CFBundleIdentifier", "raw"])
        .arg(bundle.join("Contents/Info.plist"))
        .output()
        .ok()?;
    let id = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (out.status.success() && !id.is_empty()).then_some(id)
}

/// Moves `bundle` from `system_apps` to `home_apps`, once, right after an update, while the
/// account can still change `/Applications`.
///
/// Why then: the update that brings this code was installed in place by the build before it,
/// which only works while the user has admin rights (the Jamf Connect elevation); this build is
/// what that click starts, with the elevation still on. Why only then: `<bundle>.old`, kept by
/// `install`, is how this build knows it just replaced another, so it moves because of something
/// the user asked for. `Ok(None)` for every reason it cannot move: carry on from where it is.
pub fn relocate(
    bundle: &Path,
    system_apps: &Path,
    home_apps: &Path,
) -> anyhow::Result<Option<PathBuf>> {
    if bundle.parent() != Some(system_apps) || !old_of(bundle).exists() || !writable(system_apps) {
        return Ok(None);
    }
    let name = bundle.file_name().context("bundle has no name")?;
    std::fs::create_dir_all(home_apps)
        .with_context(|| format!("could not create {}", home_apps.display()))?;
    let dest = home_apps.join(name);
    let dest_old = old_of(&dest);

    // A copy already there is replaced only if it is wai (likely an earlier manual install), and
    // set aside first so a failed move can put it back.
    let mut set_aside = false;
    if dest.symlink_metadata().is_ok() {
        match (bundle_id(&dest), bundle_id(bundle)) {
            (Some(a), Some(b)) if a == b => {}
            _ => bail!(
                "{} is not wai, so wai was left in {}",
                dest.display(),
                system_apps.display()
            ),
        }
        std::fs::remove_dir_all(&dest_old)
            .with_context(|| format!("could not clear {}", dest_old.display()))?;
        std::fs::rename(&dest, &dest_old).with_context(|| {
            format!(
                "could not set aside the copy already in {}",
                home_apps.display()
            )
        })?;
        set_aside = true;
    }
    // One rename: both folders share the data volume on current Macs, so no half-copied app; a
    // home on another volume fails with EXDEV and wai stays put.
    if let Err(e) = std::fs::rename(bundle, &dest) {
        if set_aside {
            let _ = std::fs::rename(&dest_old, &dest);
        }
        return Err(e).with_context(|| format!("could not move wai to {}", home_apps.display()));
    }
    // The update's rollback copy goes along, replacing the older one set aside above; where it
    // can't move it is removed, since clearing it in /Applications later needs the elevation.
    let _ = std::fs::remove_dir_all(&dest_old);
    if std::fs::rename(old_of(bundle), &dest_old).is_err() {
        let _ = std::fs::remove_dir_all(old_of(bundle));
    }
    Ok(Some(dest))
}

/// Opens `dest` as a new process telling it where it came from. Waited on (bounded) because
/// `open` returns once LaunchServices has the request, and it is the only way to see a failed
/// launch before this process decides to exit.
fn open_relocated(dest: &Path) -> anyhow::Result<()> {
    let mut child = Command::new("open")
        .args(["-n", "--env"])
        .arg(format!("{RELOCATED_ENV}={SYSTEM_APPS}"))
        .arg(dest)
        .spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            ensure_ok(status)?;
            return Ok(());
        }
        if start.elapsed() > RELAUNCH_TIMEOUT {
            let _ = child.kill();
            let _ = child.wait();
            bail!("opening the moved copy timed out");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn ensure_ok(status: std::process::ExitStatus) -> anyhow::Result<()> {
    if !status.success() {
        bail!("open exited with {status}");
    }
    Ok(())
}

/// Called first in `main`: the one-time move out of `/Applications`, and the notice that goes
/// with it. A local build (version not semver) is not an installed copy, so it does nothing.
pub fn on_start() -> Start {
    if let Ok(from) = std::env::var(RELOCATED_ENV)
        && !from.is_empty()
    {
        // SAFETY: called from `main` before any other thread exists.
        unsafe { std::env::remove_var(RELOCATED_ENV) };
        return Start::Continue(Some(format!(
            "wai moved to ~/Applications. It was in {from}, where updates needed an administrator; from now on they install on their own."
        )));
    }
    if super::version(env!("CARGO_PKG_VERSION")).is_none() {
        return Start::Continue(None);
    }
    let (Ok(bundle), Some(home)) = (current_bundle(), home_apps()) else {
        return Start::Continue(None);
    };
    match relocate(&bundle, Path::new(SYSTEM_APPS), &home) {
        Ok(None) => Start::Continue(None),
        Ok(Some(dest)) => match open_relocated(&dest) {
            Ok(()) => Start::Exit,
            // Moved but the copy won't start: keep running rather than exit into no wai at all.
            Err(e) => Start::Continue(Some(format!(
                "wai moved to ~/Applications but could not reopen itself from {}: {e}. Quit wai and open it from there.",
                dest.display()
            ))),
        },
        Err(e) => Start::Continue(Some(format!(
            "wai could not move to ~/Applications: {e}. Updates here still need an administrator."
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn fake_bundle(dir: &Path, name: &str, id: &str, marker: &str) -> PathBuf {
        let app = dir.join(name);
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/MacOS/wai"), b"").unwrap();
        fs::write(app.join("marker"), marker).unwrap();
        fs::write(
            app.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>\
                 <key>CFBundleIdentifier</key><string>{id}</string></dict></plist>"
            ),
        )
        .unwrap();
        app
    }

    fn zip(dir: &Path, apps: &[&Path], out: &str) -> PathBuf {
        let stage = dir.join(format!("stage-{out}"));
        fs::create_dir_all(&stage).unwrap();
        for app in apps {
            let to = stage.join(app.file_name().unwrap());
            assert!(
                Command::new("cp")
                    .arg("-R")
                    .arg(app)
                    .arg(&to)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        let z = dir.join(out);
        let mut c = Command::new("ditto");
        c.args(["-c", "-k"]);
        if apps.is_empty() {
            fs::write(stage.join("x.txt"), "x").unwrap();
        }
        c.arg(&stage).arg(&z);
        assert!(c.status().unwrap().success());
        z
    }

    fn marker(app: &Path) -> String {
        fs::read_to_string(app.join("marker")).unwrap()
    }

    #[test]
    fn bundle_root_shapes() {
        assert_eq!(
            bundle_root(Path::new("/Applications/wai.app/Contents/MacOS/wai")).unwrap(),
            Path::new("/Applications/wai.app")
        );
        assert!(bundle_root(Path::new("/x/target/release/wai")).is_err());
        let e = bundle_root(Path::new(
            "/private/var/folders/x/AppTranslocation/y/wai.app/Contents/MacOS/wai",
        ))
        .unwrap_err();
        assert!(e.contains("temporary"));
    }

    #[test]
    fn install_swaps_and_keeps_old() {
        let t = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(t.path(), "wai.app", "com.wai", "v0");
        let src = tempfile::tempdir().unwrap();
        let new1 = fake_bundle(src.path(), "wai.app", "com.wai", "v1");
        let z1 = zip(src.path(), &[&new1], "one.zip");
        install(&z1, &bundle).unwrap();
        assert_eq!(marker(&bundle), "v1");
        assert_eq!(marker(&t.path().join("wai.app.old")), "v0");

        fs::write(new1.join("marker"), "v2").unwrap();
        let z2 = zip(src.path(), &[&new1], "two.zip");
        install(&z2, &bundle).unwrap();
        assert_eq!(marker(&bundle), "v2");
        assert_eq!(marker(&t.path().join("wai.app.old")), "v1");
        let leftovers = fs::read_dir(t.path())
            .unwrap()
            .filter(|e| {
                e.as_ref()
                    .unwrap()
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".wai-update-")
            })
            .count();
        assert_eq!(leftovers, 0);
    }

    #[test]
    fn install_rejects_a_zip_without_one_app() {
        let t = tempfile::tempdir().unwrap();
        let bundle = fake_bundle(t.path(), "wai.app", "com.wai", "v0");
        let src = tempfile::tempdir().unwrap();
        let a = fake_bundle(src.path(), "a.app", "com.wai", "a");
        let b = fake_bundle(src.path(), "b.app", "com.wai", "b");
        for z in [
            zip(src.path(), &[&a, &b], "two.zip"),
            zip(src.path(), &[], "none.zip"),
        ] {
            assert!(install(&z, &bundle).is_err());
            assert_eq!(marker(&bundle), "v0");
            assert!(!t.path().join("wai.app.old").exists());
        }
    }

    #[test]
    fn relocate_moves_after_an_update() {
        let t = tempfile::tempdir().unwrap();
        let (sys, home) = (t.path().join("sys"), t.path().join("home"));
        fs::create_dir_all(&sys).unwrap();
        fs::create_dir_all(&home).unwrap();
        let bundle = fake_bundle(&sys, "wai.app", "com.wai", "new");
        fake_bundle(&sys, "wai.app.old", "com.wai", "old");
        let dest = relocate(&bundle, &sys, &home).unwrap();
        assert_eq!(dest.as_deref(), Some(home.join("wai.app").as_path()));
        assert_eq!(marker(&home.join("wai.app")), "new");
        assert_eq!(marker(&home.join("wai.app.old")), "old");
        assert_eq!(fs::read_dir(&sys).unwrap().count(), 0);
    }

    #[test]
    fn relocate_waits_for_an_update() {
        let t = tempfile::tempdir().unwrap();
        let (sys, home) = (t.path().join("sys"), t.path().join("home"));
        fs::create_dir_all(&sys).unwrap();
        fs::create_dir_all(&home).unwrap();
        let bundle = fake_bundle(&sys, "wai.app", "com.wai", "new");
        assert_eq!(relocate(&bundle, &sys, &home).unwrap(), None);
        assert!(bundle.exists());
        assert_eq!(fs::read_dir(&home).unwrap().count(), 0);
    }

    #[test]
    fn relocate_refuses_a_foreign_bundle() {
        let t = tempfile::tempdir().unwrap();
        let (sys, home) = (t.path().join("sys"), t.path().join("home"));
        fs::create_dir_all(&sys).unwrap();
        fs::create_dir_all(&home).unwrap();
        let bundle = fake_bundle(&sys, "wai.app", "com.wai", "new");
        fake_bundle(&sys, "wai.app.old", "com.wai", "old");
        let other = fake_bundle(&home, "wai.app", "com.other", "other");
        let e = relocate(&bundle, &sys, &home).unwrap_err();
        assert!(e.to_string().contains("is not wai"), "{e}");
        assert_eq!(marker(&bundle), "new");
        assert_eq!(marker(&other), "other");
    }
}
