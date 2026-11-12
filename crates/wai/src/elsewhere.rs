//! Is a Claude conversation open in another process (a terminal `claude --resume`, another wai)?

use std::collections::HashSet;
use std::path::Path;
use std::time::{Duration, SystemTime};

pub(crate) const RECENT: Duration = Duration::from_secs(120);

/// Of `ids`, those open in another process: one whose command line names the id, or a
/// transcript `<claude_home>/projects/*/<id>.jsonl` written within RECENT (not for `quiet`
/// ids: this wai ended them lately, so the write is its own). Blocking: background executor.
pub(crate) fn open_elsewhere(
    claude_home: &Path,
    ids: &[String],
    quiet: &HashSet<String>,
) -> HashSet<String> {
    let mut found = HashSet::new();
    if ids.is_empty() {
        return found;
    }
    let dirs: Vec<_> = std::fs::read_dir(claude_home.join("projects"))
        .map(|d| d.flatten().map(|e| e.path()).collect())
        .unwrap_or_default();
    let now = SystemTime::now();
    for id in ids.iter().filter(|id| !quiet.contains(*id)) {
        let fresh = dirs.iter().any(|d| {
            std::fs::metadata(d.join(format!("{id}.jsonl")))
                .and_then(|m| m.modified())
                .is_ok_and(|t| now.duration_since(t).unwrap_or_default() < RECENT)
        });
        if fresh {
            found.insert(id.clone());
        }
    }
    let ps = processes();
    found.extend(ids.iter().filter(|id| named_by(&ps, id)).cloned());
    found
}

/// A line of `ps` output names `id`: a token equal to it or ending in `=<id>`, and a token containing "claude".
pub(crate) fn named_by(ps: &str, id: &str) -> bool {
    let suffix = format!("={id}");
    ps.lines().any(|line| {
        let mut tokens = line.split_whitespace();
        tokens.clone().any(|t| t == id || t.ends_with(&suffix))
            && tokens.any(|t| t.contains("claude"))
    })
}

fn processes() -> String {
    if cfg!(windows) {
        return String::new();
    }
    std::process::Command::new("ps")
        .args(["-axww", "-o", "command="])
        .stdin(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).into_owned())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_process_naming_the_id_counts() {
        for yes in [
            "/u/.local/bin/claude --resume abc-1\n",
            "claude --resume=abc-1",
            "node /x/claude-agent-sdk/cli.js --session-id abc-1",
        ] {
            assert!(named_by(yes, "abc-1"), "{yes}");
        }
        for no in [
            "tail -f /h/.claude/projects/p/abc-1.jsonl",
            "grep abc-1",
            "claude --resume abc-12",
        ] {
            assert!(!named_by(no, "abc-1"), "{no}");
        }
    }

    #[test]
    fn a_fresh_transcript_counts() {
        let home = tempfile::tempdir().unwrap();
        let dir = home.path().join("projects/-p");
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("abc.jsonl");
        std::fs::write(&file, "{}\n").unwrap();
        let ids = vec!["abc".to_string()];
        let none = HashSet::new();
        assert!(open_elsewhere(home.path(), &ids, &none).contains("abc"));
        let old = SystemTime::now() - Duration::from_secs(600);
        std::fs::File::open(&file)
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert!(!open_elsewhere(home.path(), &ids, &none).contains("abc"));
        std::fs::File::open(&file)
            .unwrap()
            .set_modified(SystemTime::now())
            .unwrap();
        let quiet = HashSet::from(["abc".to_string()]);
        assert!(!open_elsewhere(home.path(), &ids, &quiet).contains("abc"));
    }

    #[cfg(unix)]
    #[test]
    fn a_running_claude_resume_counts() {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        let id = format!(
            "wai-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        );
        let mut child = std::process::Command::new("sh")
            .args(["-c", "sleep 30; :", "claude", "--resume", &id])
            .stdin(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let home = tempfile::tempdir().unwrap();
        let found = open_elsewhere(home.path(), &[id.clone()], &HashSet::new());
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.contains(&id));
    }
}
