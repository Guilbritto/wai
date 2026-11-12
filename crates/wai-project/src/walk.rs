use std::path::Path;

use ignore::WalkBuilder;
use ignore::gitignore::{Gitignore, GitignoreBuilder};

fn in_repo(root: &Path) -> bool {
    root.ancestors().any(|a| a.join(".git").exists())
}

/// A walker from `start` inside the project `root`. `.gitignore` applies even when the project is
/// not in a git repository; there, folders above `root` are not read (git would not read them).
pub(crate) fn walker(start: &Path, root: &Path, global_ignores: bool) -> WalkBuilder {
    let mut builder = WalkBuilder::new(start);
    builder
        .hidden(false)
        .git_global(global_ignores)
        .current_dir(root)
        .require_git(false)
        .parents(in_repo(root));
    builder
}

/// The `.gitignore` of each folder from `root` down to, excluding, `dir`: what a walk starting at
/// `dir` misses outside a git repository. Empty inside one, where `parents(true)` covers it.
pub(crate) fn inherited(root: &Path, dir: &Path) -> Vec<Gitignore> {
    if in_repo(root) {
        return Vec::new();
    }
    let mut folders: Vec<_> = dir
        .ancestors()
        .skip(1)
        .take_while(|a| a.starts_with(root))
        .collect();
    folders.reverse();
    folders
        .into_iter()
        .map(|f| {
            let mut b = GitignoreBuilder::new(f);
            let _ = b.add(f.join(".gitignore"));
            b.build().unwrap_or_else(|_| Gitignore::empty())
        })
        .collect()
}

/// Whether the inherited rules hide `path`: the deepest folder with a matching rule decides, and a
/// whitelist rule un-ignores.
pub(crate) fn hidden_by(inherited: &[Gitignore], path: &Path, is_dir: bool) -> bool {
    inherited
        .iter()
        .rev()
        .map(|g| g.matched_path_or_any_parents(path, is_dir))
        .find(|m| !m.is_none())
        .is_some_and(|m| m.is_ignore())
}
