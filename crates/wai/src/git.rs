use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub(crate) const SUBMODULE_HINT: &str = "Pick the submodule's folder to work in a worktree";

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Repo {
    pub root: PathBuf,
    pub current: Option<String>,
    pub default: String,
    pub branches: Vec<String>,
    pub dirty: usize,
    pub submodules: bool,
    pub origin: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Head {
    /// `--show-toplevel` of the folder: a linked worktree's own root.
    pub root: PathBuf,
    /// The repository's main working tree.
    pub folder: PathBuf,
    pub branch: Option<String>,
    pub worktree: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Worktree {
    pub path: PathBuf,
    pub branch: String,
    pub from: String,
    pub fetch_failed: Option<String>,
}

/// The only place a git process is built: never prompts, never reads stdin.
pub(crate) fn command(dir: &Path) -> Command {
    let mut c = Command::new("git");
    c.current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null());
    #[cfg(test)]
    c.env(
        "GIT_CONFIG_GLOBAL",
        if cfg!(windows) { "NUL" } else { "/dev/null" },
    )
    .env("GIT_CONFIG_NOSYSTEM", "1");
    c
}

fn run(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = command(dir)
        .args(args)
        .output()
        .map_err(|e| format!("could not run git: {e}"))?;
    if out.status.success() {
        return Ok(String::from_utf8_lossy(&out.stdout).trim().to_string());
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    Err(stderr
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .map_or_else(|| out.status.to_string(), |l| l.trim().to_string()))
}

fn ok(dir: &Path, args: &[&str]) -> bool {
    run(dir, args).is_ok()
}

fn current_branch(dir: &Path) -> Option<String> {
    run(dir, &["symbolic-ref", "--short", "-q", "HEAD"])
        .ok()
        .filter(|b| !b.is_empty())
}

pub(crate) fn repo(folder: &Path) -> Option<Repo> {
    let top = run(folder, &["rev-parse", "--show-toplevel"]).ok()?;
    let root = std::fs::canonicalize(top).ok()?;
    let current = current_branch(&root);
    let branches = run(
        &root,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )
    .unwrap_or_default()
    .lines()
    .map(str::to_string)
    .collect();
    let default = run(
        &root,
        &["symbolic-ref", "--short", "refs/remotes/origin/HEAD"],
    )
    .ok()
    .and_then(|b| b.strip_prefix("origin/").map(str::to_string))
    .or_else(|| {
        ["main", "master"]
            .into_iter()
            .find(|b| {
                ok(
                    &root,
                    &[
                        "rev-parse",
                        "--verify",
                        "--quiet",
                        &format!("refs/heads/{b}"),
                    ],
                )
            })
            .map(str::to_string)
    })
    .or_else(|| current.clone())
    .unwrap_or_else(|| "HEAD".to_string());
    let dirty = run(&root, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    Some(Repo {
        submodules: root.join(".gitmodules").exists(),
        origin: ok(&root, &["remote", "get-url", "origin"]),
        root,
        current,
        default,
        branches,
        dirty,
    })
}

pub(crate) fn head(folder: &Path) -> Option<Head> {
    let out = run(
        folder,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-dir",
            "--git-common-dir",
        ],
    )
    .ok()?;
    let mut lines = out.lines();
    let (top, git_dir, common) = (lines.next()?, lines.next()?, lines.next()?);
    let canon = |p: &str| std::fs::canonicalize(p).unwrap_or_else(|_| PathBuf::from(p));
    let root = canon(top);
    let worktree = git_dir != common;
    let main_dir = if worktree {
        main_folder(folder, common)
    } else {
        None
    };
    let main = main_dir.map_or_else(|| root.clone(), |m| canon(&m));
    Some(Head {
        branch: current_branch(&root),
        root,
        folder: main,
        worktree,
    })
}

/// The first entry of `worktree list` is the main working tree; for a submodule it names
/// the git dir itself, whose `core.worktree` gives the real folder.
fn main_folder(dir: &Path, common: &str) -> Option<String> {
    let list = run(dir, &["worktree", "list", "--porcelain"]).ok()?;
    let first = list.lines().find_map(|l| l.strip_prefix("worktree "))?;
    if Path::new(first) == Path::new(common) {
        run(dir, &["--git-dir", common, "rev-parse", "--show-toplevel"]).ok()
    } else {
        Some(first.to_string())
    }
}

pub(crate) fn worktree_blocked(repo: &Repo) -> Option<&'static str> {
    repo.submodules.then_some(SUBMODULE_HINT)
}

pub(crate) fn slug(text: &str) -> String {
    let words: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .take(4)
        .map(str::to_lowercase)
        .collect();
    let joined: String = words.join("-").chars().take(40).collect();
    let joined = joined.trim_end_matches('-');
    if joined.is_empty() {
        "session".to_string()
    } else {
        joined.to_string()
    }
}

pub(crate) fn create_worktree(repo: &Repo, slug: &str, base: &str) -> Result<Worktree, String> {
    let root = &repo.root;
    let remote = format!("origin/{base}");
    let pushed = ok(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("refs/remotes/{remote}"),
        ],
    );
    let fetch_failed = if repo.origin && pushed {
        run(root, &["fetch", "origin", base]).err()
    } else {
        None
    };
    let from = if fetch_failed.is_none() && pushed {
        remote
    } else {
        base.to_string()
    };
    let (branch, path) = (1..)
        .map(|n| {
            let name = if n == 1 {
                slug.to_string()
            } else {
                format!("{slug}-{n}")
            };
            (format!("wai/{name}"), root.join(".worktrees").join(name))
        })
        .find(|(branch, path)| {
            !path.exists()
                && !ok(
                    root,
                    &[
                        "rev-parse",
                        "--verify",
                        "--quiet",
                        &format!("refs/heads/{branch}"),
                    ],
                )
        })
        .expect("an unbounded range always yields");
    if !ok(root, &["check-ignore", "-q", ".worktrees/"]) {
        let common = run(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )
        .map_err(|e| format!("Could not create the worktree: {e}"))?;
        add_exclude(&PathBuf::from(common).join("info/exclude"))
            .map_err(|e| format!("Could not create the worktree: {e}"))?;
    }
    let path_arg = path.to_string_lossy().into_owned();
    run(
        root,
        &[
            "worktree",
            "add",
            "--no-track",
            "-b",
            &branch,
            &path_arg,
            &from,
        ],
    )
    .map_err(|e| format!("Could not create the worktree: {e}"))?;
    Ok(Worktree {
        path,
        branch,
        from,
        fetch_failed,
    })
}

fn add_exclude(file: &Path) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let old = std::fs::read_to_string(file).unwrap_or_default();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(file)?;
    if !old.is_empty() && !old.ends_with('\n') {
        f.write_all(b"\n")?;
    }
    f.write_all(b".worktrees/\n")
}

pub(crate) fn checkout(folder: &Path, branch: &str) -> Result<(), String> {
    let dirty = run(folder, &["status", "--porcelain"])
        .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
        .unwrap_or(0);
    if dirty > 0 {
        let name = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        return Err(format!(
            "Can't switch to {branch}: {name} has {dirty} uncommitted files. Pick New worktree to work on {branch} without touching them."
        ));
    }
    run(folder, &["checkout", branch, "--"])
        .map(|_| ())
        .map_err(|e| format!("git checkout {branch} failed: {e}"))
}

/// Clones `url` into `parent`, in a folder named after the repository.
pub(crate) fn clone(url: &str, parent: &Path) -> Result<PathBuf, String> {
    let url = url.trim();
    if url.is_empty() || url.starts_with('-') {
        return Err("Enter a repository URL".into());
    }
    let name = url.trim_end_matches('/');
    let name = name.rsplit(['/', ':']).next().unwrap_or_default();
    let name = name.strip_suffix(".git").unwrap_or(name);
    if name.is_empty() || name == ".." {
        return Err("Enter a repository URL".into());
    }
    let dest = parent.join(name);
    if dest.exists() {
        return Err(format!("{} already exists", dest.display()));
    }
    run(parent, &["clone", "--", url, &dest.to_string_lossy()]).map(|_| dest)
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn git(dir: &Path, args: &[&str]) -> String {
        let out = command(dir)
            .args([
                "-c",
                "user.name=wai",
                "-c",
                "user.email=wai@example.invalid",
                "-c",
                "commit.gpgsign=false",
            ])
            .args(args)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    }

    /// Commit number `n` of a fixture, one second after the one before it.
    fn dated_commit(dir: &Path, n: usize, message: &str) {
        let date = format!("@{} +0000", 1_700_000_000 + n);
        let out = command(dir)
            .args([
                "-c",
                "user.name=wai",
                "-c",
                "user.email=wai@example.invalid",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                message,
            ])
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }

    pub(crate) fn repo_with(dir: &Path, branches: &[&str]) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        git(dir, &["init", "-b", "main"]);
        std::fs::write(dir.join("a.txt"), "a").unwrap();
        git(dir, &["add", "."]);
        dated_commit(dir, 0, "first");
        for (n, b) in branches.iter().enumerate() {
            git(dir, &["checkout", "-b", b]);
            std::fs::write(dir.join(format!("{b}.txt")), b).unwrap();
            git(dir, &["add", "."]);
            dated_commit(dir, n + 1, b);
        }
        std::fs::canonicalize(dir).unwrap()
    }

    /// A superproject with the submodule `inventories` and a linked worktree of that submodule.
    pub(crate) fn submodule_worktree(tmp: &Path) -> (PathBuf, PathBuf) {
        // Git reads the canonical \\?\ form Windows gives as a host name: hand it the plain path.
        let src = tmp.join("inv-src");
        repo_with(&src, &[]);
        let sup = repo_with(&tmp.join("super"), &[]);
        git(
            &sup,
            &[
                "-c",
                "protocol.file.allow=always",
                "submodule",
                "add",
                src.to_str().unwrap(),
                "inventories",
            ],
        );
        git(&sup, &["commit", "-m", "add submodule"]);
        let sub = sup.join("inventories");
        git(
            &sub,
            &["worktree", "add", "-b", "feat-x", ".claude/worktrees/x"],
        );
        (
            std::fs::canonicalize(&sup).unwrap(),
            std::fs::canonicalize(sub.join(".claude/worktrees/x")).unwrap(),
        )
    }

    /// A bare origin (HEAD main) and a clone of it.
    pub(crate) fn cloned(tmp: &Path) -> (PathBuf, PathBuf) {
        let src = tmp.join("src");
        repo_with(&src, &[]);
        let bare = tmp.join("origin.git");
        git(
            tmp,
            &[
                "clone",
                "--bare",
                src.to_str().unwrap(),
                bare.to_str().unwrap(),
            ],
        );
        let clone = tmp.join("clone");
        git(
            tmp,
            &["clone", bare.to_str().unwrap(), clone.to_str().unwrap()],
        );
        (bare, std::fs::canonicalize(clone).unwrap())
    }

    fn exclude_count(root: &Path) -> usize {
        let common = git(
            root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        );
        std::fs::read_to_string(PathBuf::from(common).join("info/exclude"))
            .unwrap_or_default()
            .lines()
            .filter(|l| l.trim() == ".worktrees/")
            .count()
    }

    #[test]
    fn clone_makes_a_folder_named_after_the_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let (bare, _) = cloned(tmp.path());
        let parent = tmp.path().join("parent");
        std::fs::create_dir(&parent).unwrap();
        let dest = clone(bare.to_str().unwrap(), &parent).unwrap();
        assert_eq!(dest, parent.join("origin"));
        assert!(repo(&dest).is_some());
    }

    #[test]
    fn clone_refuses_an_existing_folder() {
        let tmp = tempfile::tempdir().unwrap();
        let (bare, _) = cloned(tmp.path());
        let parent = tmp.path().join("parent");
        std::fs::create_dir_all(parent.join("origin")).unwrap();
        let err = clone(bare.to_str().unwrap(), &parent).unwrap_err();
        assert!(err.ends_with("already exists"), "{err}");
    }

    #[test]
    fn clone_refuses_an_option_as_url() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(clone("-u x", tmp.path()).is_err());
        assert!(clone("  ", tmp.path()).is_err());
        assert_eq!(std::fs::read_dir(tmp.path()).unwrap().count(), 0);
    }

    #[test]
    fn slug_takes_the_first_words() {
        assert_eq!(
            slug("Add a tag filter to the energy dashboard!"),
            "add-a-tag-filter"
        );
        assert_eq!(slug("  ?? "), "session");
        assert_eq!(
            slug("Adicionar validação de CPF"),
            "adicionar-validação-de-cpf"
        );
        assert_eq!(slug(&"x".repeat(60)).len(), 40);
    }

    #[test]
    fn not_a_repository_is_none() {
        let tmp = tempfile::tempdir().unwrap();
        assert_eq!(repo(tmp.path()), None);
    }

    #[test]
    fn repo_reads_branches_dirty_and_default() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &["feature"]);
        std::fs::write(root.join("a.txt"), "changed").unwrap();
        std::fs::write(root.join("new.txt"), "new").unwrap();
        let r = repo(&root).unwrap();
        assert_eq!(r.current.as_deref(), Some("feature"));
        assert_eq!(r.branches[0], "feature");
        assert!(r.branches.contains(&"main".to_string()));
        assert_eq!(r.dirty, 2);
        assert_eq!(r.default, "main");
        assert!(!r.origin);
        assert!(!r.submodules);
    }

    #[test]
    fn detached_head_has_no_current_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &[]);
        git(&root, &["checkout", "--detach"]);
        assert_eq!(repo(&root).unwrap().current, None);
    }

    #[test]
    fn default_branch_falls_back() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("r");
        std::fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-b", "trunk"]);
        std::fs::write(root.join("a.txt"), "a").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "first"]);
        assert_eq!(repo(&root).unwrap().default, "trunk");
    }

    #[test]
    fn default_branch_prefers_origin_head() {
        let tmp = tempfile::tempdir().unwrap();
        let src = repo_with(&tmp.path().join("src"), &[]);
        git(&src, &["branch", "-m", "main", "development"]);
        let bare = tmp.path().join("origin.git");
        git(
            tmp.path(),
            &[
                "init",
                "--bare",
                "-b",
                "development",
                bare.to_str().unwrap(),
            ],
        );
        git(&src, &["push", bare.to_str().unwrap(), "development"]);
        let clone = tmp.path().join("clone");
        git(
            tmp.path(),
            &["clone", bare.to_str().unwrap(), clone.to_str().unwrap()],
        );
        let r = repo(&clone).unwrap();
        assert_eq!(r.default, "development");
        assert!(r.origin);
    }

    #[test]
    fn submodules_block_worktrees() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &[]);
        assert_eq!(worktree_blocked(&repo(&root).unwrap()), None);
        std::fs::write(root.join(".gitmodules"), "").unwrap();
        assert_eq!(
            worktree_blocked(&repo(&root).unwrap()),
            Some(SUBMODULE_HINT)
        );
    }

    #[test]
    fn worktree_starts_from_the_fetched_base() {
        let tmp = tempfile::tempdir().unwrap();
        let (bare, clone) = cloned(tmp.path());
        let other = tmp.path().join("other");
        git(
            tmp.path(),
            &["clone", bare.to_str().unwrap(), other.to_str().unwrap()],
        );
        std::fs::write(other.join("c2.txt"), "c2").unwrap();
        git(&other, &["add", "."]);
        git(&other, &["commit", "-m", "c2"]);
        git(&other, &["push", "origin", "main"]);
        let c2 = git(&other, &["rev-parse", "HEAD"]);

        let r = repo(&clone).unwrap();
        let w = create_worktree(&r, "x", "main").unwrap();
        assert_eq!(w.path, r.root.join(".worktrees/x"));
        assert_eq!(w.branch, "wai/x");
        assert_eq!(w.from, "origin/main");
        assert_eq!(w.fetch_failed, None);
        assert_eq!(git(&w.path, &["rev-parse", "HEAD"]), c2);
        assert_eq!(exclude_count(&r.root), 1);
        create_worktree(&r, "x", "main").unwrap();
        assert_eq!(exclude_count(&r.root), 1);
        assert_eq!(git(&r.root, &["status", "--porcelain"]), "");
    }

    #[test]
    fn worktree_name_clash_gets_a_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &[]);
        let r = repo(&root).unwrap();
        create_worktree(&r, "x", "main").unwrap();
        let w = create_worktree(&r, "x", "main").unwrap();
        assert_eq!(w.path, r.root.join(".worktrees/x-2"));
        assert_eq!(w.branch, "wai/x-2");
    }

    #[test]
    fn offline_fetch_starts_from_the_local_base() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, clone) = cloned(tmp.path());
        let missing = tmp.path().join("missing");
        git(
            &clone,
            &["remote", "set-url", "origin", missing.to_str().unwrap()],
        );
        let r = repo(&clone).unwrap();
        let w = create_worktree(&r, "x", "main").unwrap();
        assert!(w.fetch_failed.is_some());
        assert_eq!(w.from, "main");
        assert!(w.path.is_dir());
    }

    #[test]
    fn never_pushed_base_does_not_report_a_fetch_failure() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, clone) = cloned(tmp.path());
        git(&clone, &["checkout", "-b", "local-only"]);
        std::fs::write(clone.join("l.txt"), "l").unwrap();
        git(&clone, &["add", "."]);
        git(&clone, &["commit", "-m", "local"]);
        let w = create_worktree(&repo(&clone).unwrap(), "x", "local-only").unwrap();
        assert_eq!(w.fetch_failed, None);
        assert_eq!(w.from, "local-only");
    }

    #[test]
    fn worktree_branch_tracks_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let (_, clone) = cloned(tmp.path());
        let w = create_worktree(&repo(&clone).unwrap(), "x", "main").unwrap();
        assert_eq!(w.from, "origin/main");
        let out = command(&clone)
            .args(["rev-parse", "--abbrev-ref", "wai/x@{upstream}"])
            .output()
            .unwrap();
        assert!(!out.status.success());
    }

    #[test]
    fn repo_with_orders_branches_by_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &["x", "y"]);
        assert_eq!(repo(&root).unwrap().branches, ["y", "x", "main"]);
    }

    #[test]
    fn ignored_worktrees_dir_leaves_exclude_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &[]);
        std::fs::write(root.join(".gitignore"), ".worktrees/\n").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "ignore"]);
        create_worktree(&repo(&root).unwrap(), "x", "main").unwrap();
        assert_eq!(exclude_count(&root), 0);
    }

    #[test]
    fn checkout_refuses_uncommitted_files() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &["feature"]);
        git(&root, &["checkout", "main"]);
        std::fs::write(root.join("a.txt"), "changed").unwrap();
        let e = checkout(&root, "feature").unwrap_err();
        assert!(e.contains("uncommitted files"), "{e}");
        assert_eq!(repo(&root).unwrap().current.as_deref(), Some("main"));
        git(&root, &["checkout", "--", "a.txt"]);
        checkout(&root, "feature").unwrap();
        assert_eq!(repo(&root).unwrap().current.as_deref(), Some("feature"));
    }

    #[test]
    fn head_tells_a_worktree_apart() {
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&tmp.path().join("r"), &[]);
        assert_eq!(
            head(&root),
            Some(Head {
                root: root.clone(),
                folder: root.clone(),
                branch: Some("main".into()),
                worktree: false
            })
        );
        let w = create_worktree(&repo(&root).unwrap(), "x", "main").unwrap();
        let h = head(&w.path).unwrap();
        assert_eq!(h.root, std::fs::canonicalize(&w.path).unwrap());
        assert_eq!(h.folder, root);
        assert_eq!(h.branch.as_deref(), Some("wai/x"));
        assert!(h.worktree);
    }

    #[test]
    fn head_names_the_submodule_of_a_submodule_worktree() {
        let tmp = tempfile::tempdir().unwrap();
        let (sup, wt) = submodule_worktree(tmp.path());
        let sub = std::fs::canonicalize(sup.join("inventories")).unwrap();
        assert_eq!(
            head(&wt),
            Some(Head {
                root: wt.clone(),
                folder: sub.clone(),
                branch: Some("feat-x".into()),
                worktree: true
            })
        );
        let h = head(&sub).unwrap();
        assert_eq!(h.root, sub);
        assert_eq!(h.folder, sub);
        assert!(!h.worktree);
    }

    #[test]
    fn detached_submodule_has_no_branch() {
        let tmp = tempfile::tempdir().unwrap();
        let (sup, _wt) = submodule_worktree(tmp.path());
        let sub = std::fs::canonicalize(sup.join("inventories")).unwrap();
        git(&sub, &["checkout", "--detach"]);
        let h = head(&sub).unwrap();
        assert_eq!(h.branch, None);
        assert_eq!(h.folder, sub);
    }
}
