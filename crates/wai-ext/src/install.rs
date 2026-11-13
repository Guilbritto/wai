//! Staging an extension before consent: a folder is copied, or a git repository cloned, into a
//! temp dir under the config dir, and its manifest is parsed. Nothing is installed or enabled
//! here: `ExtStore::commit` does that once the user consents.
//!
//! What a stage refuses (Review Focus 2):
//! - every symbolic link, wherever it points (v1): with none, the manifest's lexical path checks
//!   are the real ones;
//! - anything that is not a regular file or a folder (a FIFO blocks the copy, a device never
//!   ends);
//! - more than 100 MB or 10,000 entries;
//! - git URLs other than `https://` and `git@host:path`. git runs with a fixed argv, never a
//!   shell.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::Deserialize;

use crate::{ExtId, Manifest, ManifestError, parse_manifest, relative_inside};

pub const MANIFEST_FILE: &str = "wai-extension.toml";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Folder(PathBuf),
    Git { url: String, rev: Option<String> },
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Folder(dir) => f.write_str(&wai_settings::shown(dir)),
            Source::Git { url, rev: None } => f.write_str(url),
            Source::Git {
                url,
                rev: Some(rev),
            } => write!(f, "{url} at {rev}"),
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum InstallError {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Manifest(#[from] ManifestError),
    #[error("refused {}: {why}", path.display())]
    Refused { path: PathBuf, why: &'static str },
    #[error("the extension is larger than 100 MB or 10,000 files")]
    TooLarge,
    #[error("unsupported git URL {0:?}: use https:// or git@")]
    BadUrl(String),
    #[error("invalid git revision {0:?}")]
    BadRev(String),
    #[error("git clone failed: {0}")]
    Git(String),
    #[error("no wai-extension.toml in the extension folder")]
    NoManifest,
    #[error("{0} is already installed; install it again to update it")]
    AlreadyInstalled(String),
    #[error("{0} is not installed")]
    NotInstalled(String),
    #[error("an extension is installed only with every permission it asks for")]
    Consent,
}

/// An extension copied into the staging area, not installed. Dropping it removes the copy:
/// drop it off the main thread.
#[derive(Debug)]
pub struct Staged {
    manifest: Manifest,
    source: Source,
    /// `<staging>/<n>`; the extension is its `ext` folder.
    root: PathBuf,
}

impl Staged {
    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    /// Where it was staged from: users tell an extension from a fork with the same id by it.
    pub fn source(&self) -> &Source {
        &self.source
    }

    /// The staged extension's folder.
    pub fn dir(&self) -> PathBuf {
        self.root.join("ext")
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// Each extension's own folder, `<root>/<id>`; nothing here touches it.
pub(crate) fn data_root(config_dir: &Path) -> PathBuf {
    config_dir.join("extension-data")
}

pub(crate) fn extensions_dir(config_dir: &Path) -> PathBuf {
    config_dir.join("extensions")
}

/// Where stages wait for consent; wai clears it at launch. Ids never start with a dot, so it is
/// never an extension's folder.
pub fn staging_dir(config_dir: &Path) -> PathBuf {
    extensions_dir(config_dir).join(".staging")
}

struct Budget {
    bytes: u64,
    entries: usize,
}

const LIMITS: Budget = Budget {
    bytes: 100 * 1024 * 1024,
    entries: 10_000,
};

/// A new, unused path in the staging area (the folder is not created).
pub(crate) fn staging_path(config_dir: &Path) -> std::io::Result<PathBuf> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let parent = staging_dir(config_dir);
    std::fs::create_dir_all(&parent)?;
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    Ok(parent.join(format!("{}-{n}", std::process::id())))
}

pub(crate) fn stage(config_dir: &Path, source: &Source) -> Result<Staged, InstallError> {
    if let Source::Git { url, rev } = source {
        check_git_source(url, rev.as_deref())?;
    }
    let root = staging_path(config_dir)?;
    std::fs::create_dir(&root)?;
    match fill(&root, source) {
        Ok(manifest) => Ok(Staged {
            manifest,
            source: source.clone(),
            root,
        }),
        Err(e) => {
            std::fs::remove_dir_all(&root).ok();
            Err(e)
        }
    }
}

fn fill(root: &Path, source: &Source) -> Result<Manifest, InstallError> {
    let ext = root.join("ext");
    match source {
        Source::Folder(dir) => copy_tree(dir, &ext, LIMITS)?,
        Source::Git { url, rev } => {
            let clone = root.join("clone");
            git_clone(url, rev.as_deref(), &clone, root)?;
            std::fs::remove_dir_all(clone.join(".git"))?;
            // The checked copy: a repository can hold links too.
            copy_tree(&clone, &ext, LIMITS)?;
            std::fs::remove_dir_all(&clone)?;
        }
    }
    let text =
        wai_settings::read_config(&ext.join(MANIFEST_FILE))?.ok_or(InstallError::NoManifest)?;
    Ok(parse_manifest(&text, &ext)?)
}

fn copy_tree(src: &Path, dst: &Path, mut budget: Budget) -> Result<(), InstallError> {
    copy_dir(src, dst, &mut budget)
}

fn copy_dir(src: &Path, dst: &Path, budget: &mut Budget) -> Result<(), InstallError> {
    std::fs::create_dir(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let from = entry.path();
        budget.entries = (budget.entries.checked_sub(1)).ok_or(InstallError::TooLarge)?;
        // The entry itself, never what a link points to.
        let kind = entry.file_type()?;
        if kind.is_dir() {
            copy_dir(&from, &dst.join(entry.file_name()), budget)?;
        } else if kind.is_file() {
            let len = entry.metadata()?.len();
            budget.bytes = (budget.bytes.checked_sub(len)).ok_or(InstallError::TooLarge)?;
            // ponytail: an entry swapped for a link or a FIFO between the check and the copy
            // still gets through; the folder is one the user picked, the clone is wai's own.
            std::fs::copy(&from, dst.join(entry.file_name()))?;
        } else {
            let why = if kind.is_symlink() {
                "symbolic links are not allowed in extensions"
            } else {
                "only regular files and folders are allowed in extensions"
            };
            return Err(InstallError::Refused { path: from, why });
        }
    }
    Ok(())
}

/// `https://<host>…` or `git@<host>:<path>` URLs with no whitespace or control characters,
/// whose host does not start with `-`; a revision that does not start with `-` either (git or
/// ssh would read it as an option).
pub fn check_git_source(url: &str, rev: Option<&str>) -> Result<(), InstallError> {
    let clean = |s: &str| !s.chars().any(|c| c.is_whitespace() || c.is_control());
    let host_ok = |host: &str| !host.is_empty() && !host.starts_with('-');
    let ok = if let Some(rest) = url.strip_prefix("https://") {
        host_ok(rest)
    } else if let Some(rest) = url.strip_prefix("git@") {
        // scp-like: a `:` with no `/` before it.
        rest.split_once(':')
            .is_some_and(|(host, path)| host_ok(host) && !host.contains('/') && !path.is_empty())
    } else {
        false
    };
    if !ok || !clean(url) {
        return Err(InstallError::BadUrl(url.to_string()));
    }
    if let Some(rev) = rev
        && (rev.is_empty() || rev.starts_with('-') || !clean(rev))
    {
        return Err(InstallError::BadRev(rev.to_string()));
    }
    Ok(())
}

/// `git clone --depth 1 [--branch <rev>] -- <url> <dest>`: after `--` the URL is never an
/// option. R18: it runs from `cwd` (the staging folder), never from a repository. No prompt can
/// wait on a terminal (ssh's host-key and passphrase questions included: `BatchMode` fails
/// instead), only https and ssh are spoken, LFS filters do not run, and the system gitconfig is
/// not read.
pub fn git_clone_command(url: &str, rev: Option<&str>, dest: &Path, cwd: &Path) -> Command {
    let mut args: Vec<OsString> = vec!["clone".into(), "--depth".into(), "1".into()];
    if let Some(rev) = rev {
        args.extend(["--branch".into(), rev.into()]);
    }
    args.extend(["--".into(), url.into(), dest.into()]);
    let mut command = Command::new("git");
    command
        .args(args)
        .current_dir(cwd)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_LFS_SKIP_SMUDGE", "1")
        .env("GIT_ALLOW_PROTOCOL", "https:ssh")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes")
        .stdin(Stdio::null());
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000); // CREATE_NO_WINDOW
    command
}

fn git_clone(url: &str, rev: Option<&str>, dest: &Path, cwd: &Path) -> Result<(), InstallError> {
    let out = git_clone_command(url, rev, dest, cwd)
        .output()
        .map_err(|e| InstallError::Git(format!("cannot run git: {e}")))?;
    if out.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    let last = stderr.trim().lines().last().map(str::to_string);
    Err(InstallError::Git(last.unwrap_or(out.status.to_string())))
}

/// A project's suggestion: shown to the user, never installed by itself (R15).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub id: ExtId,
    pub source: Source,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SuggestFile {
    #[serde(default)]
    suggest: Vec<RawSuggestion>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawSuggestion {
    id: String,
    source: String,
}

/// `<project>/.wai/extensions.toml`: `[[suggest]]` tables with an `id` and a `source`, an
/// `https://` or `git@` URL or a folder inside the project. A file that does not load (or is
/// over 1 MB) is skipped whole, with a notice.
pub fn load_suggestions(project: &Path) -> (Vec<Suggestion>, Vec<String>) {
    let path = project.join(".wai").join("extensions.toml");
    match read_suggestions(&path, project) {
        Ok(suggestions) => (suggestions, Vec::new()),
        Err(e) => {
            let notice = format!("Ignored extension suggestions in {}: {e}", path.display());
            (Vec::new(), vec![notice])
        }
    }
}

fn read_suggestions(path: &Path, project: &Path) -> Result<Vec<Suggestion>, String> {
    let Some(text) = wai_settings::read_config(path).map_err(|e| e.to_string())? else {
        return Ok(Vec::new());
    };
    let file: SuggestFile = toml::from_str(&text).map_err(|e| e.to_string())?;
    let suggestion = |raw: RawSuggestion| {
        let id = ExtId::new(&raw.id).map_err(|e| e.to_string())?;
        let source = if raw.source.contains("://") || raw.source.starts_with("git@") {
            check_git_source(&raw.source, None).map_err(|e| e.to_string())?;
            Source::Git {
                url: raw.source,
                rev: None,
            }
        } else if relative_inside(&raw.source) {
            Source::Folder(project.join(&raw.source))
        } else {
            return Err(format!(
                "source {:?} must be an https:// or git@ URL or a folder inside the project",
                raw.source
            ));
        };
        Ok(Suggestion { id, source })
    };
    file.suggest.into_iter().map(suggestion).collect()
}

#[cfg(test)]
mod tests {
    use super::{Budget, InstallError, copy_tree};

    #[test]
    fn copy_caps_entries_and_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let src = tmp.path().join("src");
        std::fs::create_dir_all(src.join("d")).unwrap();
        std::fs::write(src.join("d/a"), "12345").unwrap();
        std::fs::write(src.join("b"), "12345").unwrap();
        // Three entries (d, d/a, b) and 10 bytes.
        let copy = |name: &str, bytes, entries| {
            copy_tree(&src, &tmp.path().join(name), Budget { bytes, entries })
        };
        assert!(copy("fits", 10, 3).is_ok());
        assert_eq!(
            std::fs::read(tmp.path().join("fits/d/a")).unwrap(),
            b"12345"
        );
        assert!(matches!(
            copy("entries", 10, 2),
            Err(InstallError::TooLarge)
        ));
        assert!(matches!(copy("bytes", 9, 3), Err(InstallError::TooLarge)));
    }
}
