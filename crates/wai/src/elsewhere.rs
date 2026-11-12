//! Is a Claude conversation open in another process (a terminal `claude --resume`, another wai)?

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, SystemTime};

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_agent::SessionSummary;

use crate::workspace::Workspace;

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

impl Workspace {
    /// Ids this wai ended within RECENT: their transcripts' fresh writes are its own.
    pub(crate) fn quiet(&mut self) -> HashSet<String> {
        self.ended.retain(|_, at| at.elapsed() < RECENT);
        self.ended.keys().cloned().collect()
    }

    /// Runs `then` at once, or after the owner chose "Open anyway" when `summary`'s Claude
    /// conversation is open in another process. A session open here is never checked.
    pub(crate) fn check_elsewhere(
        &mut self,
        summary: &SessionSummary,
        then: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.default_agent().as_deref() != Some("claude")
            || self.session("claude", &summary.id, cx).is_some()
        {
            return then(self, window, cx);
        }
        let (home, quiet, summary) = (self.claude_home.clone(), self.quiet(), summary.clone());
        let ids = [summary.id.clone()];
        cx.spawn_in(window, async move |this, cx| {
            let found = cx
                .background_spawn(async move { open_elsewhere(&home, &ids, &quiet) })
                .await;
            this.update_in(cx, |this, window, cx| {
                if found.is_empty() {
                    then(this, window, cx)
                } else {
                    this.warn_elsewhere(summary, then, window, cx)
                }
            })
            .ok();
        })
        .detach();
    }

    fn warn_elsewhere(
        &mut self,
        summary: SessionSummary,
        then: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let then = Rc::new(then);
        let this = cx.entity().downgrade();
        let forkable = self.can_fork("claude", cx);
        window.open_dialog(cx, move |dialog, _, _| {
            let (this, then, summary) = (this.clone(), then.clone(), summary.clone());
            let heading = div()
                .id("confirm-heading")
                .debug_selector(|| "confirm-title".into())
                .aria_label("This conversation is open elsewhere")
                .test_support()
                .child("This conversation is open elsewhere");
            let (open_this, fork_this) = (this.clone(), this);
            dialog
                .title(heading)
                .child(
                    "Claude Code is running it in another window or terminal, or wrote to it in \
                     the last 2 minutes. Opening it here too can mix the two sessions' turns.",
                )
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(Button::new("open-anyway").label("Open anyway").on_click(
                            move |_, window, cx| {
                                window.close_dialog(cx);
                                open_this.update(cx, |w, cx| then(w, window, cx)).ok();
                            },
                        ))
                        .when(forkable, |footer| {
                            let (id, title, cwd): (_, _, PathBuf) = (
                                summary.id.clone(),
                                summary.title.clone(),
                                summary.cwd.clone(),
                            );
                            let this = fork_this.clone();
                            footer.child(Button::new("confirm").primary().label("Fork").on_click(
                                move |_, window, cx| {
                                    window.close_dialog(cx);
                                    let (id, title, cwd) = (id.clone(), title.clone(), cwd.clone());
                                    this.update(cx, |w, cx| {
                                        w.fork("claude".into(), id, title, cwd, window, cx)
                                    })
                                    .ok();
                                },
                            ))
                        }),
                )
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::prelude::v1::test; // not gpui_kit's, which `super::*` brings in

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
        let found = open_elsewhere(home.path(), std::slice::from_ref(&id), &HashSet::new());
        let _ = child.kill();
        let _ = child.wait();
        assert!(found.contains(&id));
    }

    use crate::workspace::tests::{
        click, history_settings, load_history, open, reveal, wait_until,
    };
    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    use gpui_kit::{TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    fn claude_history(n: usize) -> Settings {
        let mut settings = history_settings(n);
        let fake = settings.agents.remove("fake").unwrap();
        settings.agents.insert("claude".into(), fake);
        settings.default_agent = Some("claude".into());
        settings
    }

    fn fresh(home: &Path, id: &str) {
        let dir = home.join("projects/-p");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(format!("{id}.jsonl")), "{}\n").unwrap();
    }

    fn asking(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.render_frame(cx));
        cx.debug_bounds("confirm-title").is_some()
    }

    fn setup(
        cx: &mut TestAppContext,
        n: usize,
    ) -> (Entity<Workspace>, &mut VisualTestContext, tempfile::TempDir) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (workspace, cx) = open(cx, claude_history(n));
        let home = tmp.path().join("home");
        workspace.update(cx, |w, _| w.claude_home = home);
        load_history(&workspace, n, cx);
        (workspace, cx, tmp)
    }

    fn ids(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<String> {
        workspace.read_with(cx, |w, cx| {
            w.sessions.iter().map(|s| s.read(cx).id.clone()).collect()
        })
    }

    #[gpui_kit::test]
    fn resume_asks_when_the_conversation_is_open_elsewhere(cx: &mut TestAppContext) {
        let (workspace, cx, tmp) = setup(cx, 2);
        fresh(&tmp.path().join("home"), "session-0");
        click(("history", 0usize), cx);
        wait_until(cx, "the warning", asking);
        cx.update(|window, _| window.find_by_label("This conversation is open elsewhere"));
        assert!(workspace.read_with(cx, |w, _| w.sessions.is_empty()));
        click("open-anyway", cx);
        wait_until(cx, "the session", |cx| ids(&workspace, cx) == ["session-0"]);
    }

    #[gpui_kit::test]
    fn fork_from_the_warning_opens_a_copy(cx: &mut TestAppContext) {
        let (workspace, cx, tmp) = setup(cx, 2);
        fresh(&tmp.path().join("home"), "session-0");
        click(("history", 0usize), cx);
        wait_until(cx, "the warning", asking);
        click("confirm", cx);
        wait_until(cx, "the fork", |cx| {
            let ids = ids(&workspace, cx);
            ids.len() == 1 && ids[0] != "session-0"
        });
    }

    #[gpui_kit::test]
    fn cancel_leaves_it_closed(cx: &mut TestAppContext) {
        let (workspace, cx, tmp) = setup(cx, 2);
        fresh(&tmp.path().join("home"), "session-0");
        click(("history", 0usize), cx);
        wait_until(cx, "the warning", asking);
        click("cancel", cx);
        cx.run_until_parked();
        assert!(workspace.read_with(cx, |w, _| w.sessions.is_empty()));
        assert!(!asking(cx));
    }

    #[gpui_kit::test]
    fn a_conversation_this_wai_closed_does_not_ask(cx: &mut TestAppContext) {
        let (workspace, cx, tmp) = setup(cx, 2);
        click(("history", 1usize), cx);
        wait_until(cx, "the session", |cx| ids(&workspace, cx) == ["session-1"]);
        reveal(("open-close", 0usize), cx);
        click(("open-close", 0usize), cx);
        wait_until(cx, "it closed", |cx| ids(&workspace, cx).is_empty());
        fresh(&tmp.path().join("home"), "session-1");
        click(("history", 1usize), cx);
        wait_until(cx, "the session", |cx| {
            assert!(!asking(cx), "it asked");
            ids(&workspace, cx) == ["session-1"]
        });
    }
}
