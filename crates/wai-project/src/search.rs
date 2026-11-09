//! Project search: a gitignore-aware text search on a background thread, streamed in batches.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use grep_regex::{RegexMatcher, RegexMatcherBuilder};
use grep_searcher::sinks::Lossy;
use grep_searcher::{BinaryDetection, SearcherBuilder};
use ignore::WalkBuilder;

pub const MAX_MATCHES: usize = 10_000;
/// Larger files are skipped: they are rarely source, and they slow every search.
const MAX_FILE_SIZE: u64 = 20 * 1024 * 1024;
/// A match line is cut here, so one minified file cannot fill the list with megabytes.
const MAX_LINE_CHARS: usize = 300;
/// Batches the worker may run ahead of the reader: a cancelled search stops within these.
const QUEUED_BATCHES: usize = 8;

#[derive(Clone, Debug)]
pub struct SearchQuery {
    pub pattern: String,
    pub regex: bool,
    pub case_sensitive: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Match {
    pub path: PathBuf,
    /// 1-based.
    pub line: u32,
    pub text: String,
}

/// Dropping it cancels the search.
pub struct SearchHandle(Arc<AtomicBool>);

impl Drop for SearchHandle {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Relaxed);
    }
}

/// A search's handle and its batches.
pub type Search = (SearchHandle, async_channel::Receiver<Vec<Match>>);

/// Searches the files under `root` that `.gitignore` leaves in, on a new thread. One batch per
/// file with matches arrives on the channel, which closes when the search ends: all files seen,
/// `MAX_MATCHES` found, the handle dropped or the receiver gone. An empty pattern finds nothing;
/// `None` means the pattern is not a valid regular expression.
pub fn search(root: &Path, q: SearchQuery) -> Option<Search> {
    search_in(root, q, true)
}

/// `search`, with `global_ignores` choosing whether git's global excludes file applies (read
/// from `~/.gitconfig`: tests pass `false`).
pub fn search_in(root: &Path, q: SearchQuery, global_ignores: bool) -> Option<Search> {
    // ponytail: compiled on the caller's (UI) thread to say "invalid" at once; the regex
    // crate's size limits bound a pathological pattern.
    let matcher = RegexMatcherBuilder::new()
        .case_insensitive(!q.case_sensitive)
        .fixed_strings(!q.regex)
        .build(&q.pattern)
        .ok()?;
    let cancel = Arc::new(AtomicBool::new(false));
    let (tx, rx) = async_channel::bounded(QUEUED_BATCHES);
    let (root, flag) = (root.to_path_buf(), cancel.clone());
    if !q.pattern.is_empty() {
        std::thread::spawn(move || run(&root, &matcher, global_ignores, &flag, &tx));
    }
    Some((SearchHandle(cancel), rx))
}

fn run(
    root: &Path,
    matcher: &RegexMatcher,
    global_ignores: bool,
    cancel: &AtomicBool,
    tx: &async_channel::Sender<Vec<Match>>,
) {
    let mut searcher = SearcherBuilder::new()
        .line_number(true)
        .binary_detection(BinaryDetection::quit(0))
        .build();
    let walk = WalkBuilder::new(root)
        .hidden(false)
        .git_global(global_ignores)
        .current_dir(root)
        .sort_by_file_name(|a, b| a.cmp(b))
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    let mut total = 0;
    for entry in walk.flatten() {
        if cancel.load(Ordering::Relaxed) || total >= MAX_MATCHES {
            return;
        }
        let is_file = entry.file_type().is_some_and(|t| t.is_file());
        if !is_file || entry.metadata().is_ok_and(|m| m.len() > MAX_FILE_SIZE) {
            continue;
        }
        let mut batch = Vec::new();
        let sink = Lossy(|line, text: &str| {
            batch.push(Match {
                path: entry.path().to_path_buf(),
                line: line as u32,
                text: text.trim().chars().take(MAX_LINE_CHARS).collect(),
            });
            Ok(total + batch.len() < MAX_MATCHES && !cancel.load(Ordering::Relaxed))
        });
        // An unreadable file is left out, as the tree leaves out an unreadable entry.
        let _ = searcher.search_path(matcher, entry.path(), sink);
        if batch.is_empty() {
            continue;
        }
        total += batch.len();
        if tx.send_blocking(batch).is_err() {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    fn query(pattern: &str) -> SearchQuery {
        SearchQuery {
            pattern: pattern.into(),
            regex: false,
            case_sensitive: false,
        }
    }

    fn collect(root: &Path, q: SearchQuery) -> Vec<Match> {
        let (_handle, rx) = search_in(root, q, false).expect("a valid pattern");
        let mut all = Vec::new();
        while let Ok(batch) = rx.recv_blocking() {
            all.extend(batch);
        }
        all
    }

    fn project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join(".git")).unwrap();
        dir
    }

    #[test]
    fn search_finds_matches_respecting_gitignore() {
        let dir = project();
        let root = dir.path();
        fs::create_dir(root.join("target")).unwrap();
        fs::write(root.join(".gitignore"), "target/\n").unwrap();
        fs::write(root.join("a.rs"), "fn one() {}\nlet x = 1;\nfn Two() {}\n").unwrap();
        fs::write(root.join("target/out.rs"), "fn ignored() {}\n").unwrap();
        fs::write(root.join("bin.dat"), b"fn \0 binary").unwrap();

        let found = collect(root, query("fn "));
        let lines: Vec<_> = found.iter().map(|m| (m.line, m.text.as_str())).collect();
        assert_eq!(lines, [(1, "fn one() {}"), (3, "fn Two() {}")]);
        assert!(found.iter().all(|m| m.path == root.join("a.rs")));

        let case = SearchQuery {
            case_sensitive: true,
            ..query("fn t")
        };
        assert!(collect(root, case).is_empty());
        let re = SearchQuery {
            regex: true,
            ..query(r"fn \w+\(")
        };
        assert_eq!(collect(root, re).len(), 2);
        // A literal pattern is not a regex.
        fs::write(root.join("b.txt"), "a.b\naxb\n").unwrap();
        assert_eq!(collect(root, query("a.b")).len(), 1);
        assert!(collect(root, query("")).is_empty());
    }

    #[test]
    fn search_caps_and_cancels() {
        let dir = project();
        let root = dir.path();
        // 200 files x 100 matching lines = 20,000 matches.
        for n in 0..200 {
            fs::write(root.join(format!("f{n}.txt")), "hit\n".repeat(100)).unwrap();
        }
        let found = collect(root, query("hit"));
        assert_eq!(found.len(), MAX_MATCHES);

        let (handle, rx) = search_in(root, query("hit"), false).unwrap();
        let first = rx.recv_blocking().unwrap();
        drop(handle);
        let mut total = first.len();
        while let Ok(batch) = rx.recv_blocking() {
            total += batch.len();
        }
        assert!(total < MAX_MATCHES, "cancelled search kept going: {total}");
    }
}
