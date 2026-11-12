//! One wai per state folder: an advisory lock on `wai.lock`, released when the process ends.

use std::fs::{File, OpenOptions, TryLockError};
use std::path::Path;
use std::time::{Duration, Instant};

/// Long enough for the old copy of an update hand-off to finish exiting.
pub const LOCK_WAIT: Duration = Duration::from_secs(3);

/// Owns the locked file; dropping it releases the lock.
pub struct Guard(#[allow(dead_code)] File);

pub enum Acquired {
    Held(Guard),
    /// Another process holds the lock after the wait.
    Taken,
    /// The file cannot be created or locked, for a reason other than a holder.
    Unusable(String),
}

pub fn acquire(lock_path: &Path, wait: Duration) -> Acquired {
    let file = match OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(lock_path)
    {
        Ok(file) => file,
        Err(e) => return Acquired::Unusable(format!("{}: {e}", lock_path.display())),
    };
    let start = Instant::now();
    loop {
        match file.try_lock() {
            Ok(()) => return Acquired::Held(Guard(file)),
            Err(TryLockError::WouldBlock) if start.elapsed() < wait => {
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(TryLockError::WouldBlock) => return Acquired::Taken,
            Err(TryLockError::Error(e)) => {
                return Acquired::Unusable(format!("{}: {e}", lock_path.display()));
            }
        }
    }
}

/// Tells the user, in a native alert, that wai is already running.
pub fn tell_taken(project: &Path) {
    let folder = project.file_name().map_or_else(
        || project.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    eprintln!("wai: already running; quit it first, then open {folder}");
    #[cfg(target_os = "macos")]
    {
        // The folder travels as an argv item, never inside the script.
        std::process::Command::new("osascript")
            .args([
                "-e",
                "on run argv",
                "-e",
                r#"display alert "wai is already running" message ("Quit it first, then open " & item 1 of argv & ".") giving up after 30"#,
                "-e",
                "end run",
            ])
            .arg(&folder)
            .output()
            .ok();
    }
    #[cfg(not(target_os = "macos"))]
    if let Err(e) = notify_rust::Notification::new()
        .summary("wai is already running")
        .body(&format!("Quit it first, then open {folder}."))
        .show()
    {
        eprintln!("wai: notification failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lock_path(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("wai-instance-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(name)
    }

    #[test]
    fn second_acquire_is_taken() {
        let p = lock_path("second.lock");
        let first = acquire(&p, Duration::ZERO);
        assert!(matches!(first, Acquired::Held(_)));
        assert!(matches!(acquire(&p, Duration::ZERO), Acquired::Taken));
    }

    #[test]
    fn waits_for_the_holder_to_exit() {
        let p = lock_path("wait.lock");
        let held = acquire(&p, Duration::ZERO);
        assert!(matches!(held, Acquired::Held(_)));
        let holder = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            drop(held);
        });
        assert!(matches!(acquire(&p, LOCK_WAIT), Acquired::Held(_)));
        holder.join().unwrap();
    }

    #[test]
    fn a_dead_holder_leaves_no_lock() {
        let p = lock_path("dead.lock");
        drop(acquire(&p, Duration::ZERO));
        assert!(p.exists());
        assert!(matches!(acquire(&p, Duration::ZERO), Acquired::Held(_)));
    }

    #[test]
    fn unusable_lock_does_not_block() {
        let file = lock_path("regular-file");
        std::fs::write(&file, "").unwrap();
        let p = file.join("wai.lock");
        assert!(matches!(acquire(&p, Duration::ZERO), Acquired::Unusable(_)));
    }

    #[cfg(unix)]
    #[test]
    fn a_child_does_not_keep_the_lock() {
        let p = lock_path("child.lock");
        let held = acquire(&p, Duration::ZERO);
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 2"])
            .spawn()
            .unwrap();
        drop(held);
        let again = acquire(&p, Duration::ZERO);
        child.kill().ok();
        child.wait().ok();
        assert!(matches!(again, Acquired::Held(_)));
    }
}
