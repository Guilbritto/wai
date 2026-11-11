//! The workspace's side of the Claude pages in Settings: reading `~/.claude` off the UI thread
//! and writing a plugin's switch.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::*;
use wai_settings::claude::{Stamp, WriteError, read_claude, set_plugin_enabled};

use crate::workspace::Workspace;

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
    use crate::workspace::Workspace;
    use crate::workspace::tests::{click, open};

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
        let groups = crate::settings_view::claude::mcp_groups(&wai_settings::claude::read_claude(
            &home, &project,
        ));
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
