//! The project on disk, without GPUI: its file tree, listed lazily, and its watcher.

mod tree;
mod watch;

pub use tree::{Entry, list_dir};
pub use watch::{FsChange, ProjectWatcher};
