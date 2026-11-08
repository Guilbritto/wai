use std::io;
use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

/// The children of `dir`, a folder inside the project `root`: dirs first, then by name ignoring
/// case. `.gitignore` rules apply as git applies them (from `dir` and every folder above it, inside
/// a git repo); hidden files show, `.git` does not. Only `dir` is read, never below it, so a huge
/// ignored or collapsed folder costs nothing. An entry that cannot be read is left out; `Err` means
/// `dir` itself is gone or unreadable.
pub fn list_dir(root: &Path, dir: &Path) -> io::Result<Vec<Entry>> {
    debug_assert!(
        dir.starts_with(root),
        "{} is outside the project",
        dir.display()
    );
    std::fs::read_dir(dir)?;
    let walk = WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        // ponytail: the user's global excludes file (from ~/.gitconfig) is not read: same tree
        // on every machine and in tests. Read it if people miss their global ignores.
        .git_global(false)
        .filter_entry(|e| e.file_name() != ".git")
        .build();
    let mut entries: Vec<_> = walk
        .flatten()
        .filter(|e| e.depth() == 1)
        .map(|e| {
            let file_type = e.file_type();
            // A link to a folder expands like one.
            let is_dir = file_type.is_some_and(|t| t.is_dir())
                || (file_type.is_some_and(|t| t.is_symlink()) && e.path().is_dir());
            Entry {
                name: e.file_name().to_string_lossy().into_owned(),
                path: e.into_path(),
                is_dir,
            }
        })
        .collect();
    entries.sort_by_cached_key(|e| (!e.is_dir, e.name.to_lowercase()));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn tree_lists_lazily_and_respects_gitignore() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        for d in [
            ".git",
            "node_modules/pkg",
            "src/nested",
            "src/node_modules",
            "Docs",
        ] {
            fs::create_dir_all(root.join(d)).unwrap();
        }
        let files = [
            (".gitignore", "node_modules/\n"),
            ("node_modules/pkg/index.js", ""),
            ("src/.gitignore", "*.log\n"),
            ("src/debug.log", ""),
            ("src/main.rs", ""),
            ("src/nested/deep.rs", ""),
            ("a.txt", ""),
            ("B.txt", ""),
            ("README.md", ""),
        ];
        for (path, text) in files {
            fs::write(root.join(path), text).unwrap();
        }

        let names = |dir: &Path| -> Vec<String> {
            let entries = list_dir(root, dir).unwrap();
            for entry in &entries {
                assert_eq!(entry.path, dir.join(&entry.name), "nothing below depth 1");
                assert_eq!(entry.is_dir, entry.path.is_dir());
            }
            entries.into_iter().map(|e| e.name).collect()
        };

        // Dirs first, then case-insensitive; hidden files show, `.git` and ignored paths do not.
        assert_eq!(
            names(root),
            ["Docs", "src", ".gitignore", "a.txt", "B.txt", "README.md"]
        );
        // The root's rules apply below it, and each dir's own rules too.
        assert_eq!(
            names(&root.join("src")),
            ["nested", ".gitignore", "main.rs"]
        );
        assert!(list_dir(root, &root.join("missing")).is_err());
    }
}
