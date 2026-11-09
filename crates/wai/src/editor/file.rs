//! Reading and saving a file for the editor, without GPUI. Everything here blocks: callers run it
//! on the background executor.

use std::fmt;
use std::fs;
use std::io::{self, Read as _, Write as _};
use std::path::Path;
use std::time::SystemTime;

/// Above this a file opens read-only: `set_value` blocks the main thread ~350 ms per 100 MB.
pub const READ_ONLY_ABOVE: u64 = 20 * 1024 * 1024;
/// Above this a file does not open.
pub const REFUSE_ABOVE: u64 = 200 * 1024 * 1024;
/// A NUL in this many first bytes makes a file binary.
const SNIFF: u64 = 8 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Editable,
    ReadOnlyLarge,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    Binary,
    TooLarge { bytes: u64 },
    NotUtf8,
    Io(String),
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            OpenError::Binary => f.write_str("Binary file"),
            OpenError::TooLarge { .. } => f.write_str("Too large to open in wai"),
            OpenError::NotUtf8 => f.write_str("Not a UTF-8 text file"),
            OpenError::Io(e) => f.write_str(e),
        }
    }
}

impl From<io::Error> for OpenError {
    fn from(e: io::Error) -> Self {
        OpenError::Io(e.to_string())
    }
}

/// Whether `path` opens, and how: over 200 MB it does not, nor with a NUL in its first 8 KB.
pub fn classify(path: &Path) -> Result<OpenMode, OpenError> {
    let bytes = fs::metadata(path)?.len();
    if bytes > REFUSE_ABOVE {
        return Err(OpenError::TooLarge { bytes });
    }
    let mut head = Vec::new();
    fs::File::open(path)?.take(SNIFF).read_to_end(&mut head)?;
    if head.contains(&0) {
        return Err(OpenError::Binary);
    }
    Ok(if bytes > READ_ONLY_ABOVE {
        OpenMode::ReadOnlyLarge
    } else {
        OpenMode::Editable
    })
}

/// A file's modification time and size. A watcher event that finds the stamp unchanged is wai's
/// own save (or a touch) and changes nothing.
pub type Stamp = (SystemTime, u64);

pub fn stamp(path: &Path) -> Option<Stamp> {
    let meta = fs::metadata(path).ok()?;
    Some((meta.modified().ok()?, meta.len()))
}

pub struct Loaded {
    pub mode: OpenMode,
    pub text: String,
    /// Taken before the read: a change during it shows as a newer stamp later.
    pub stamp: Option<Stamp>,
}

pub fn load(path: &Path) -> Result<Loaded, OpenError> {
    let mode = classify(path)?;
    let stamp = stamp(path);
    // ponytail: text that is not UTF-8 does not open (the error says so); a lossy read-only
    // view would be the upgrade.
    let text = String::from_utf8(fs::read(path)?).map_err(|_| OpenError::NotUtf8)?;
    Ok(Loaded { mode, text, stamp })
}

/// Writes `text` to a temp file beside `path`, then renames it over `path`: a crash leaves the
/// old file or the new one, never half of one. The file keeps its permissions, and a symlink
/// keeps pointing to it. Returns the new stamp.
pub fn save(path: &Path, text: &str) -> io::Result<Option<Stamp>> {
    let target = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let name = target.file_name().ok_or(io::ErrorKind::InvalidInput)?;
    let tmp = target.with_file_name(format!(".{}.wai-save", name.to_string_lossy()));
    let written = (|| {
        // The target's mode before any content: a private file is never readable by others,
        // not even as a temp file (one left by a crash included).
        let permissions = fs::metadata(&target).ok().map(|meta| meta.permissions());
        let mut options = fs::OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        if let Some(permissions) = &permissions {
            use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
            options.mode(permissions.mode());
        }
        let mut file = options.open(&tmp)?;
        if let Some(permissions) = permissions {
            file.set_permissions(permissions)?;
        }
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, &target)
    })();
    if written.is_err() {
        fs::remove_file(&tmp).ok();
    }
    written?;
    Ok(stamp(path))
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::os::unix::fs::PermissionsExt as _;

    /// R31: the temp file takes the target's mode before any content, so a private file is never
    /// readable by others, not even for a moment (a stale temp file included).
    #[test]
    fn save_sets_the_mode_before_the_contents() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.env");
        fs::write(&path, "old").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let stale = dir.path().join(".secret.env.wai-save");
        fs::write(&stale, "stale").unwrap();
        fs::set_permissions(&stale, fs::Permissions::from_mode(0o644)).unwrap();

        super::save(&path, "new").unwrap();

        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
        assert!(!stale.exists());
    }
}
