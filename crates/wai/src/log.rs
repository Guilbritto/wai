//! wai's stderr as a file: a GUI launch has no terminal, so `eprintln!` would vanish.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

const MAX_BYTES: u64 = 1024 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

/// Where stderr goes, once `init` redirected it.
#[allow(dead_code)] // read by the later task that shows the log location
pub fn path() -> Option<&'static Path> {
    PATH.get().map(PathBuf::as_path)
}

/// Moves `path` to `path.1` (replacing it) when it holds more than `max` bytes.
pub fn rotate(path: &Path, max: u64) -> std::io::Result<()> {
    match std::fs::metadata(path) {
        Ok(m) if m.len() > max => {
            let mut old = path.as_os_str().to_owned();
            old.push(".1");
            std::fs::rename(path, old)
        }
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Makes `dir/logs/wai.log` the process's stderr (macOS only). Any failure leaves stderr alone.
pub fn init(dir: &Path) -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        let path = redirect(dir).ok()?;
        PATH.set(path.clone()).ok();
        Some(path)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = dir;
        None
    }
}

#[cfg(target_os = "macos")]
fn redirect(dir: &Path) -> std::io::Result<PathBuf> {
    use std::io::Write as _;
    use std::os::unix::fs::{DirBuilderExt as _, OpenOptionsExt as _};
    let logs = dir.join("logs");
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(&logs)?;
    let path = logs.join("wai.log");
    rotate(&path, MAX_BYTES)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(&path)?;
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    writeln!(
        file,
        "--- wai {} started (unix time {secs}) ---",
        env!("CARGO_PKG_VERSION")
    )?;
    rustix::stdio::dup2_stderr(&file)?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rotate_moves_a_big_log_aside() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("wai.log");
        std::fs::write(&log, vec![b'x'; 2048]).unwrap();
        rotate(&log, 1024).unwrap();
        assert!(!log.exists());
        assert_eq!(
            std::fs::read(dir.path().join("wai.log.1")).unwrap().len(),
            2048
        );
    }

    #[test]
    fn rotate_keeps_a_small_log() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("wai.log");
        std::fs::write(&log, b"small").unwrap();
        rotate(&log, 1024).unwrap();
        assert!(log.exists());
        assert!(!dir.path().join("wai.log.1").exists());
    }

    #[test]
    fn rotate_overwrites_the_old_generation() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("wai.log");
        std::fs::write(&log, vec![b'n'; 2048]).unwrap();
        std::fs::write(dir.path().join("wai.log.1"), b"old").unwrap();
        rotate(&log, 1024).unwrap();
        assert_eq!(
            std::fs::read(dir.path().join("wai.log.1")).unwrap(),
            vec![b'n'; 2048]
        );
    }

    #[test]
    fn init_on_a_bad_dir_is_none() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("file");
        std::fs::write(&file, b"").unwrap();
        assert_eq!(init(&file), None);
    }
}
