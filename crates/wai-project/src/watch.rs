use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher as _};

/// At most one batch per this long.
const DEBOUNCE: Duration = Duration::from_millis(100);
/// A bigger batch (`npm install`, a branch switch) becomes a `Rescan`: re-listing the open
/// folders is cheaper than walking that many paths.
const MAX_BATCH: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FsChange {
    /// Paths created, changed, removed or renamed, each once, ignored ones included.
    Changed(Vec<PathBuf>),
    /// Events were lost or too many came at once: anything may have changed.
    Rescan,
}

/// Watches a project folder and everything below it; dropping it stops the watch and closes the
/// channel.
pub struct ProjectWatcher {
    _watcher: RecommendedWatcher,
}

impl ProjectWatcher {
    /// Paths come as the OS reports them: pass a canonical `root` to compare them with it.
    pub fn start(
        root: &Path,
    ) -> anyhow::Result<(ProjectWatcher, async_channel::Receiver<FsChange>)> {
        let (events_tx, events) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(events_tx)?;
        watcher.watch(root, RecursiveMode::Recursive)?;
        let (tx, rx) = async_channel::unbounded();
        std::thread::Builder::new()
            .name("wai-watch".into())
            .spawn(move || debounce(events, tx))?;
        Ok((ProjectWatcher { _watcher: watcher }, rx))
    }
}

/// Blocks until an event comes, gathers what follows for `DEBOUNCE`, sends it as one batch.
/// Ends when the watcher or the receiver is dropped.
fn debounce(events: mpsc::Receiver<notify::Result<Event>>, tx: async_channel::Sender<FsChange>) {
    while let Ok(first) = events.recv() {
        let deadline = Instant::now() + DEBOUNCE;
        let (mut paths, mut rescan) = (BTreeSet::new(), false);
        add(first, &mut paths, &mut rescan);
        while let Ok(event) =
            events.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        {
            add(event, &mut paths, &mut rescan);
        }
        let change = if rescan || paths.len() > MAX_BATCH {
            FsChange::Rescan
        } else if paths.is_empty() {
            continue;
        } else {
            FsChange::Changed(paths.into_iter().collect())
        };
        if tx.send_blocking(change).is_err() {
            return;
        }
    }
}

fn add(event: notify::Result<Event>, paths: &mut BTreeSet<PathBuf>, rescan: &mut bool) {
    match event {
        // Reads change nothing, and listing a folder reads it: without this, a re-list on Linux
        // would report itself.
        Ok(event) if matches!(event.kind, EventKind::Access(_)) => {}
        Ok(event) if event.need_rescan() => *rescan = true,
        Ok(event) => paths.extend(event.paths),
        // A watch error may have lost events.
        Err(_) => *rescan = true,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;
    use std::time::Duration;

    use super::*;

    #[test]
    fn watcher_reports_changes_debounced() {
        let dir = tempfile::tempdir().unwrap();
        // FSEvents reports real paths: `/var` is `/private/var` on macOS.
        let root = dir.path().canonicalize().unwrap();
        let (_watcher, changes) = ProjectWatcher::start(&root).unwrap();

        let created: BTreeSet<_> = (0..50).map(|i| root.join(format!("f{i}.txt"))).collect();
        for path in &created {
            std::fs::write(path, "x").unwrap();
        }
        std::thread::sleep(Duration::from_secs(2));

        let (mut batches, mut seen) = (0, BTreeSet::new());
        while let Ok(change) = changes.try_recv() {
            match change {
                FsChange::Changed(paths) => {
                    batches += 1;
                    seen.extend(paths);
                }
                FsChange::Rescan => panic!("50 files need no rescan"),
            }
        }
        assert!((1..=5).contains(&batches), "{batches} batches");
        let missing: Vec<_> = created.difference(&seen).collect();
        assert!(missing.is_empty(), "missing {missing:?}");
    }
}
