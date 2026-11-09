//! Hot-exit: a tab's unsaved edits, kept in `<config>/recovery/<n>.json` until they are saved or
//! dropped, so a quit that never asks (Dock, logout) or a crash loses nothing. The next launch
//! reopens them. Nothing here writes the edited file itself. Everything blocks.

use std::fs;
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::file::{self, Stamp};

/// A recovery entry over this is neither written nor read (R61): it would hold up the UI thread
/// and the launch.
pub const MAX_BYTES: u64 = 50 * 1024 * 1024;

#[derive(Debug, PartialEq, Serialize, Deserialize)]
pub struct Entry {
    pub path: PathBuf,
    /// The buffer, its line ends `\n`.
    pub text: String,
    pub crlf: bool,
    /// The file's stamp the edits were made over.
    pub stamp: Option<Stamp>,
}

/// An entry the launch reopens.
pub struct Recovered {
    /// Its recovery file, which the tab goes on using.
    pub file: PathBuf,
    pub entry: Entry,
    /// The file's real path now, when it resolves.
    pub real: Option<PathBuf>,
    /// The file's stamp now; `None`: it is gone.
    pub now: Option<Stamp>,
}

/// Writes `entry` to `file` through a temp file renamed over it: a crash leaves the old entry or
/// the new one. Buffers can hold secrets, so the folder is 0700 and the file 0600. Over 50 MB
/// nothing is written (`FileTooLarge`); an older entry stays.
/// ponytail: no fsync (a full flush on macOS costs tens of ms per write); the page cache outlives
/// a crash of wai, and an entry a power loss empties is skipped at launch.
pub fn write(file: &Path, entry: &Entry) -> io::Result<()> {
    let (Some(dir), Some(name)) = (file.parent(), file.file_name()) else {
        return Err(io::ErrorKind::InvalidInput.into());
    };
    let bytes = serde_json::to_vec(entry)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::ErrorKind::FileTooLarge.into());
    }
    fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700))?;
    }
    let tmp = dir.join(format!(".{}.tmp", name.to_string_lossy()));
    let written = (|| {
        // R40: unlink whatever has the temp name (a link included), then create it anew.
        fs::remove_file(&tmp).ok();
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        options.open(&tmp)?.write_all(&bytes)?;
        fs::rename(&tmp, file)
    })();
    if written.is_err() {
        fs::remove_file(&tmp).ok();
    }
    written
}

/// The entries kept in `dir`. One whose text is the file's (nothing to recover) is deleted; one
/// over 50 MB or unreadable is left alone and logged.
pub fn load(dir: &Path) -> Vec<Recovered> {
    let Ok(read) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<_> = (read.flatten().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    files.sort();
    files.into_iter().filter_map(read_entry).collect()
}

fn read_entry(file: PathBuf) -> Option<Recovered> {
    let bytes = fs::metadata(&file).ok()?.len();
    if bytes > MAX_BYTES {
        eprintln!(
            "wai: skipped {} ({bytes} bytes, over 50 MB)",
            file.display()
        );
        return None;
    }
    let entry: Entry = match fs::read(&file).map(|raw| serde_json::from_slice(&raw)) {
        Ok(Ok(entry)) => entry,
        Ok(Err(e)) => return skip(&file, &e.to_string()),
        Err(e) => return skip(&file, &e.to_string()),
    };
    let now = file::stamp(&entry.path);
    let same = |disk: file::Loaded| disk.text == entry.text && disk.crlf == entry.crlf;
    if now.is_some() && file::load(&entry.path).is_ok_and(same) {
        fs::remove_file(&file).ok();
        return None;
    }
    let real = wai_settings::try_canonical(&entry.path).ok();
    Some(Recovered {
        file,
        entry,
        real,
        now,
    })
}

fn skip(file: &Path, why: &str) -> Option<Recovered> {
    eprintln!("wai: skipped {}: {why}", file.display());
    None
}

/// The `n` of a `<n>.json` recovery file.
pub fn number(file: &Path) -> Option<u64> {
    file.file_stem()?.to_str()?.parse().ok()
}

/// A recovery file name in `dir` that no entry has: `<n>.json` from `*next` on. Names a tab
/// holds stay below `*next` (`number`), so none is handed out twice. ponytail: one wai at a
/// time; two would share these names (and `state.json`).
pub fn next_file(dir: &Path, next: &mut u64) -> PathBuf {
    loop {
        let file = dir.join(format!("{next}.json"));
        *next += 1;
        if !file.exists() {
            return file;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{Entry, load, write};

    fn entry(path: &std::path::Path, text: &str) -> Entry {
        Entry {
            path: path.to_path_buf(),
            text: text.into(),
            crlf: false,
            stamp: super::super::file::stamp(path),
        }
    }

    /// Buffers can hold secrets: the folder is the user's alone, and so is each entry.
    #[cfg(unix)]
    #[test]
    fn recovery_files_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(".env");
        fs::write(&path, "KEY=old\n").unwrap();
        let file = dir.path().join("recovery").join("0.json");

        write(&file, &entry(&path, "KEY=secret\n")).unwrap();

        let mode = |p: &std::path::Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&file), 0o600);
        assert_eq!(mode(file.parent().unwrap()), 0o700);
        let names: Vec<_> = fs::read_dir(file.parent().unwrap())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(names, ["0.json"], "no temp file left behind");
        assert_eq!(fs::read_to_string(&path).unwrap(), "KEY=old\n");
    }

    /// A recovery file over 50 MB is left where it is and not read (a sparse file: no disk
    /// spent); the others still load.
    #[test]
    fn oversized_recovery_is_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "a").unwrap();
        let recovery = dir.path().join("recovery");
        write(&recovery.join("0.json"), &entry(&path, "mine a")).unwrap();
        let huge = recovery.join("1.json");
        fs::File::create(&huge)
            .unwrap()
            .set_len(51 * 1024 * 1024)
            .unwrap();

        let found = load(&recovery);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].entry.text, "mine a");
        assert!(huge.exists(), "left alone");
    }

    /// R61: an entry over 50 MB is not written (the launch would skip it).
    #[test]
    fn oversized_entry_is_not_written() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "a").unwrap();
        let file = dir.path().join("recovery").join("0.json");
        let big = "x".repeat(50 * 1024 * 1024 + 1);

        let error = write(&file, &entry(&path, &big)).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::FileTooLarge);
        assert!(!file.exists());
    }

    /// Edits that came back to the file's text recover nothing: the entry goes.
    #[test]
    fn recovery_matching_the_disk_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("a.txt");
        fs::write(&path, "same\n").unwrap();
        let file = dir.path().join("recovery").join("0.json");
        write(&file, &entry(&path, "same\n")).unwrap();

        assert!(load(file.parent().unwrap()).is_empty());
        assert!(!file.exists());
    }
}
