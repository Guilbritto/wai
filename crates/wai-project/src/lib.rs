//! The project on disk, without GPUI: its file tree, listed lazily, its watcher and its search.

mod search;
mod tree;
mod watch;

pub use search::{MAX_MATCHES, Match, SearchHandle, SearchQuery, search, search_in};
pub use tree::{Entry, list_dir};
pub use watch::{FsChange, ProjectWatcher};
