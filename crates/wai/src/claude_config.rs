//! The workspace's side of the Claude pages in Settings: reading `~/.claude` off the UI thread
//! and writing a plugin's switch.

use std::path::PathBuf;
use std::time::Duration;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use wai_settings::claude::{Stamp, WriteError, parse_mcp_list, read_claude, set_plugin_enabled};

use crate::settings_view::claude::McpCheck;
use crate::workspace::Workspace;

/// Runs `claude mcp list` in `cwd` and returns its output; a hung or failing run is an `Err`.
pub(crate) async fn run_mcp_list(
    program: PathBuf,
    cwd: PathBuf,
    timeout: Duration,
) -> Result<String, String> {
    let run = async {
        let out = smol::process::Command::new(program)
            .args(["mcp", "list"])
            .current_dir(cwd)
            .stdin(smol::process::Stdio::null())
            .kill_on_drop(true)
            .output()
            .await
            .map_err(|e| e.to_string())?;
        if out.status.success() {
            return Ok(String::from_utf8_lossy(&out.stdout).into_owned());
        }
        let err = String::from_utf8_lossy(&out.stderr);
        let last = err.lines().rev().find(|l| !l.trim().is_empty());
        Err(last.unwrap_or("claude mcp list failed").trim().to_string())
    };
    let timed_out = async {
        smol::Timer::after(timeout).await;
        Err(format!("claude mcp list took over {}s", timeout.as_secs()))
    };
    smol::future::or(run, timed_out).await
}

impl Workspace {
    /// Reads Claude's files in the background and hands them to the open modal.
    pub(crate) fn refresh_claude(&mut self, cx: &mut Context<Self>) {
        let (home, project) = (self.claude_home.clone(), self.project.clone());
        self.claude_task = Some(cx.spawn(async move |this, cx| {
            let data = cx
                .background_spawn(async move { read_claude(&home, &project) })
                .await;
            this.update(cx, |this, cx| {
                if let Some(view) = this.settings_view() {
                    view.update(cx, |v, cx| v.set_claude(data, cx));
                }
            })
            .ok();
        }));
    }

    /// Runs `claude mcp list` in the project and shows each server's status in the open modal.
    pub(crate) fn check_mcp(&mut self, cx: &mut Context<Self>) {
        let set = |this: &mut Self, check: McpCheck, cx: &mut Context<Self>| {
            if let Some(view) = this.settings_view() {
                view.update(cx, |v, cx| v.set_mcp_check(check, cx));
            }
        };
        let program = (self.claude_program.clone())
            .or_else(|| wai_settings::find_program("claude".as_ref()))
            .filter(|p| p.is_file());
        let Some(program) = program else {
            self.mcp_task = None;
            set(self, McpCheck::Failed("Needs the claude CLI".into()), cx);
            return;
        };
        set(self, McpCheck::Running, cx);
        let project = self.project.clone();
        self.mcp_task = Some(cx.spawn(async move |this, cx| {
            let out = run_mcp_list(program, project, Duration::from_secs(60)).await;
            this.update(cx, |this, cx| {
                let check = match out {
                    Ok(out) => McpCheck::Done(parse_mcp_list(&out)),
                    Err(e) => McpCheck::Failed(format!("Check failed: {e}")),
                };
                set(this, check, cx);
            })
            .ok();
        }));
    }

    /// Writes `enabledPlugins` unless the file changed since `seen`, then shows what is on disk.
    pub(crate) fn toggle_plugin(
        &mut self,
        id: String,
        on: bool,
        seen: Stamp,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let home = self.claude_home.clone();
        cx.spawn_in(window, async move |this, cx| {
            let result = cx
                .background_spawn(async move { set_plugin_enabled(&home, &id, on, seen) })
                .await;
            this.update_in(cx, |this, window, cx| {
                let notice = match result {
                    Ok(_) => Notification::success("Applies to new Claude sessions"),
                    Err(WriteError::Changed) => Notification::warning(
                        "Claude's settings changed since they were shown; showing them again",
                    ),
                    Err(e) => Notification::warning(format!(
                        "Could not change ~/.claude/settings.json: {e}"
                    )),
                };
                window.push_notification(notice, cx);
                this.refresh_claude(cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use crate::settings_view::Section;
    use crate::settings_view::claude::McpCheck;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{click, open, wait_until};

    fn put(path: impl AsRef<Path>, text: &str) {
        let path = path.as_ref();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// `alpha@m` (on) and `beta@m` (off), both user scope; `model` is a key the writer must keep.
    pub(crate) fn claude_fixture(home: &Path, project: &Path) {
        let h = home.display();
        std::fs::create_dir_all(project).unwrap();
        let installed = |name: &str, version: &str| {
            format!(
                r#"[{{"scope":"user","installPath":"{h}/.claude/plugins/cache/m/{name}/{version}","version":"{version}"}}]"#
            )
        };
        put(
            home.join(".claude/plugins/installed_plugins.json"),
            &format!(
                r#"{{"version":2,"plugins":{{"alpha@m":{},"beta@m":{}}}}}"#,
                installed("alpha", "1.2.0"),
                installed("beta", "abc1234")
            ),
        );
        for (name, version) in [("alpha", "1.2.0"), ("beta", "abc1234")] {
            put(
                home.join(format!(
                    ".claude/plugins/cache/m/{name}/{version}/.claude-plugin/plugin.json"
                )),
                &format!(r#"{{"name":"{name}","description":"{name} tools"}}"#),
            );
        }
        put(
            home.join(".claude/plugins/cache/m/alpha/1.2.0/.mcp.json"),
            r#"{"mcpServers":{"db":{"command":"db-mcp","env":{"KEY":"s3cr3t"}}}}"#,
        );
        let key = wai_settings::claude::project_key_for(project);
        put(
            home.join(".claude.json"),
            &format!(
                r#"{{"oauthAccount":{{"emailAddress":"s3cr3t@x.test"}},"mcpServers":{{"u":{{"command":"u-mcp"}}}},"projects":{{"{key}":{{"mcpServers":{{"l":{{"type":"sse","url":"https://l.test"}}}},"disabledMcpServers":["plugin:alpha:db"]}}}}}}"#
            ),
        );
        put(
            project.join(".mcp.json"),
            r#"{"mcpServers":{"p":{"type":"ws","url":"wss://p.test"}}}"#,
        );
        put(
            home.join(".claude/settings.json"),
            r#"{"model":"opus","enabledPlugins":{"beta@m":false}}"#,
        );
    }

    pub(crate) fn claude_ws(
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, &mut VisualTestContext, tempfile::TempDir) {
        let (workspace, cx) = open(cx, Settings::default());
        let tmp = tempfile::tempdir().unwrap();
        let (home, project) = (tmp.path().join("home"), tmp.path().join("project"));
        claude_fixture(&home, &project);
        workspace.update(cx, |w, _| {
            w.claude_home = home;
            w.project = project;
        });
        (workspace, cx, tmp)
    }

    fn open_plugins(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Plugins, None, window, cx)
        });
        cx.run_until_parked();
    }

    fn settings_json(tmp: &tempfile::TempDir) -> String {
        std::fs::read_to_string(tmp.path().join("home/.claude/settings.json")).unwrap()
    }

    fn enabled(workspace: &Entity<Workspace>, ix: usize, cx: &mut VisualTestContext) -> bool {
        let view = workspace.read_with(cx, |w, _| w.settings_view()).unwrap();
        view.read_with(cx, |v, _| v.claude().unwrap().plugins[ix].enabled)
    }

    fn open_mcp(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Mcp, None, window, cx)
        });
        cx.run_until_parked();
    }

    /// The statuses of the rows the page shows, by label.
    fn mcp_statuses(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Vec<(String, (crate::settings_view::claude::Tone, String))> {
        let view = workspace.read_with(cx, |w, _| w.settings_view()).unwrap();
        view.read_with(cx, |v, _| {
            let groups =
                crate::settings_view::claude::mcp_groups(v.claude().unwrap(), v.mcp_check());
            groups
                .into_iter()
                .flat_map(|g| g.rows)
                .filter_map(|r| Some((r.name, r.status?)))
                .collect()
        })
    }

    #[cfg(unix)]
    #[gpui_kit::test]
    fn check_status_runs_claude_mcp_list_in_the_project(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the check is a real process
        use std::os::unix::fs::PermissionsExt as _;
        let (workspace, cx, tmp) = claude_ws(cx);
        // The fixture disables plugin:alpha:db for this project; the check needs it on.
        let key = wai_settings::claude::project_key_for(&tmp.path().join("project"));
        put(
            tmp.path().join("home/.claude.json"),
            &format!(r#"{{"projects":{{"{key}":{{}}}}}}"#),
        );
        let bin = tmp.path().join("bin/claude");
        put(
            &bin,
            "#!/bin/sh\npwd > \"$0.cwd\"\necho \"$@\" > \"$0.args\"\nprintf '%s\\n' 'plugin:alpha:db: db-mcp - ✔ Connected'\n",
        );
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
        workspace.update(cx, |w, _| w.claude_program = Some(bin.clone()));
        open_mcp(&workspace, cx);
        click("mcp-check", cx);
        wait_until(cx, "the check", |cx| {
            mcp_statuses(&workspace, cx)
                .iter()
                .any(|(name, s)| name == "db" && s.1 == "Connected")
        });
        let statuses = mcp_statuses(&workspace, cx);
        let db = statuses.iter().find(|(n, _)| n == "db").unwrap();
        assert_eq!(db.1.0, crate::settings_view::claude::Tone::Ok);
        let args = std::fs::read_to_string(tmp.path().join("bin/claude.args")).unwrap();
        assert_eq!(args, "mcp list\n");
        let cwd = std::fs::read_to_string(tmp.path().join("bin/claude.cwd")).unwrap();
        assert_eq!(
            std::fs::canonicalize(cwd.trim()).unwrap(),
            std::fs::canonicalize(tmp.path().join("project")).unwrap()
        );
    }

    #[gpui_kit::test]
    fn check_status_without_the_cli_says_so(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the check is a real process
        let (workspace, cx, _tmp) = claude_ws(cx);
        open_mcp(&workspace, cx);
        click("mcp-check", cx);
        let statuses = mcp_statuses(&workspace, cx);
        assert!(!statuses.is_empty());
        for (name, (tone, text)) in statuses {
            if text == "Off for this project" {
                continue;
            }
            assert_eq!(
                (tone, text.as_str()),
                (
                    crate::settings_view::claude::Tone::Warn,
                    "Needs the claude CLI"
                ),
                "{name}"
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn mcp_list_gives_up_after_the_timeout() {
        use std::os::unix::fs::PermissionsExt as _;
        let tmp = tempfile::tempdir().unwrap();
        let script = tmp.path().join("claude");
        put(&script, "#!/bin/sh\nexec sleep 5\n");
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        let started = std::time::Instant::now();
        let out = smol::block_on(super::run_mcp_list(
            script,
            tmp.path().to_path_buf(),
            std::time::Duration::from_millis(200),
        ));
        assert_eq!(out, Err("claude mcp list took over 0s".to_string()));
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
    }

    #[gpui_kit::test]
    fn modal_opens_before_claude_is_read(cx: &mut TestAppContext) {
        let (workspace, cx, _tmp) = claude_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Plugins, None, window, cx)
        });
        let view = workspace
            .read_with(cx, |w, _| w.settings_view())
            .expect("open at once");
        assert!(
            view.read_with(cx, |v, _| v.claude().is_none()),
            "read off the UI thread"
        );
        cx.run_until_parked();
        assert_eq!(
            view.read_with(cx, |v, _| v.claude().map(|d| d.plugins.len())),
            Some(2)
        );
        assert_eq!(
            view.read_with(cx, |v, cx| v.nav_count(Section::Plugins, cx)),
            Some(2)
        );
    }

    #[gpui_kit::test]
    fn plugin_toggle_writes_enabled_plugins(cx: &mut TestAppContext) {
        let (workspace, cx, tmp) = claude_ws(cx);
        open_plugins(&workspace, cx);
        click(("plugin-toggle", 0usize), cx);
        let json: serde_json::Value = serde_json::from_str(&settings_json(&tmp)).unwrap();
        assert_eq!(json["enabledPlugins"]["alpha@m"], false);
        assert_eq!(json["model"], "opus");
        assert!(!enabled(&workspace, 0, cx));
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);
    }

    #[gpui_kit::test]
    fn changed_settings_are_not_overwritten(cx: &mut TestAppContext) {
        let (workspace, cx, tmp) = claude_ws(cx);
        open_plugins(&workspace, cx);
        put(
            tmp.path().join("home/.claude/settings.json"),
            r#"{"model":"other"}"#,
        );
        click(("plugin-toggle", 0usize), cx);
        assert_eq!(settings_json(&tmp), r#"{"model":"other"}"#);
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 1);
        assert!(enabled(&workspace, 0, cx), "no entry means on");
    }

    #[gpui_kit::test]
    fn toggles_wait_for_the_write(cx: &mut TestAppContext) {
        let (workspace, cx, _tmp) = claude_ws(cx);
        open_plugins(&workspace, cx);
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(("plugin-toggle", 0usize), cx);
            window.render_frame(cx);
        });
        let disabled = cx.update(|window, _| window.find(("plugin-toggle", 0usize)).disabled());
        assert_eq!(disabled, Some(true));
    }

    #[gpui_kit::test]
    fn coming_soon_controls_are_disabled(cx: &mut TestAppContext) {
        let (workspace, cx, _tmp) = claude_ws(cx);
        open_plugins(&workspace, cx);
        cx.update(|window, cx| window.render_frame(cx));
        cx.update(|window, _| {
            assert_eq!(window.find("soon-browse").disabled(), Some(true));
            assert_eq!(window.find("soon-plugins-menu").disabled(), Some(true));
            assert_eq!(window.find(("plugin-menu", 0usize)).disabled(), Some(true));
            assert_eq!(window.find(("soon-add", 0usize)).disabled(), Some(true));
        });
    }

    #[test]
    fn mcp_rows_carry_no_secret() {
        let tmp = tempfile::tempdir().unwrap();
        let (home, project) = (tmp.path().join("home"), tmp.path().join("project"));
        claude_fixture(&home, &project);
        let groups = crate::settings_view::claude::mcp_groups(
            &wai_settings::claude::read_claude(&home, &project),
            &McpCheck::NotRun,
        );
        assert!(!format!("{groups:?}").contains("s3cr3t"));
        assert_eq!(groups.iter().map(|g| g.rows.len()).sum::<usize>(), 5);
    }

    #[gpui_kit::test]
    fn mcp_toggle_is_coming_soon(cx: &mut TestAppContext) {
        let (workspace, cx, tmp) = claude_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Mcp, None, window, cx)
        });
        cx.run_until_parked();
        let before = std::fs::read(tmp.path().join("home/.claude.json")).unwrap();
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert_eq!(window.find(("mcp-toggle", 0usize)).disabled(), Some(true));
            assert_eq!(window.find("soon-add-server").disabled(), Some(true));
            window.click(("mcp-toggle", 0usize), cx);
        });
        cx.run_until_parked();
        assert_eq!(
            std::fs::read(tmp.path().join("home/.claude.json")).unwrap(),
            before
        );
        assert_eq!(cx.update(|window, cx| window.notifications(cx).len()), 0);
    }

    #[gpui_kit::test]
    fn groups_collapse_and_search_opens_them(cx: &mut TestAppContext) {
        let (workspace, cx, _tmp) = claude_ws(cx);
        open_plugins(&workspace, cx);
        click(("settings-group", 0usize), cx);
        let shown = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find(("plugin-toggle", 0usize)).is_some()
            })
        };
        assert!(!shown(cx));
        let view = workspace.read_with(cx, |w, _| w.settings_view()).unwrap();
        let search = view.read_with(cx, |v, _| v.search.clone());
        cx.update(|window, cx| search.update(cx, |s, cx| s.set_value("alpha", window, cx)));
        assert!(shown(cx));
    }
}
