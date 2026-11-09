//! The command palette (⌘⇧P): wai's own actions, the templates, the enabled extensions'
//! commands and a new session per agent, filtered as the user types.

use std::rc::Rc;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::command::{Command, CommandGroup, CommandItem, CommandState};
use gpui_kit::*;
use wai_ext::panel::clamp;
use wai_ext::{CommandSpec, ExtId};

use crate::app::{NewSession, OpenFolder, OpenPalette, ToggleTerminal};
use crate::sidebar::SidebarTab;
use crate::workspace::Workspace;

/// A label an extension wrote (a title, a template or agent name), one line.
pub(crate) const TITLE_CHARS: usize = 80;

#[derive(Clone)]
pub(crate) enum Entry {
    NewSession,
    ToggleTerminal,
    OpenFolder,
    Tab(SidebarTab),
    Extensions,
    Template(usize),
    Command(ExtId, CommandSpec),
    NewSessionWith(String),
}

/// A heading and its rows, each a label and what it runs.
pub(crate) type Group = (&'static str, Vec<(SharedString, Entry)>);

impl Workspace {
    /// The palette's groups in order; empty ones are left out.
    pub(crate) fn palette(&self, cx: &App) -> Vec<Group> {
        let wai = vec![
            ("New session".into(), Entry::NewSession),
            ("Toggle terminal".into(), Entry::ToggleTerminal),
            ("Open folder…".into(), Entry::OpenFolder),
            ("Search".into(), Entry::Tab(SidebarTab::Search)),
            ("Files".into(), Entry::Tab(SidebarTab::Files)),
            ("Agents".into(), Entry::Tab(SidebarTab::Agents)),
            ("Extensions".into(), Entry::Extensions),
        ];
        let templates = (self.all_templates(cx).into_iter().enumerate())
            .map(|(ix, t)| (clamp(&t.name, TITLE_CHARS).into(), Entry::Template(ix)));
        let commands = (self.ext_host.read(cx).commands().into_iter()).map(|(ext, command)| {
            let label = clamp(&command.title, TITLE_CHARS).into();
            (label, Entry::Command(ext, command))
        });
        let agents = (self.registry.read(cx).names().into_iter()).map(|a| {
            let label = clamp(&format!("New session with {a}"), TITLE_CHARS).into();
            (label, Entry::NewSessionWith(a))
        });
        let groups = [
            ("wai", wai),
            ("Templates", templates.collect()),
            ("Commands", commands.collect()),
            ("Agents", agents.collect()),
        ];
        groups.into_iter().filter(|(_, g)| !g.is_empty()).collect()
    }

    /// Opens the palette over the window; Enter (or a click) runs the row and closes it,
    /// Escape closes it.
    pub(crate) fn open_palette(
        &mut self,
        _: &OpenPalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if window.has_active_dialog(cx) {
            return;
        }
        let groups = Rc::new(self.palette(cx));
        let state = cx.new(|cx| CommandState::new(window, cx));
        let (this, palette) = (cx.entity().downgrade(), state.clone());
        window.open_dialog(cx, move |dialog, _, _| {
            let (groups, this) = (groups.clone(), this.clone());
            let command =
                (groups.iter()).fold(Command::new(&palette), |command, (heading, rows)| {
                    let items = rows
                        .iter()
                        .map(|(label, _)| CommandItem::new().label(label.clone()));
                    command.group(CommandGroup::new().label(*heading).items(items))
                });
            let command = command
                .bordered(false)
                // Room for the extensions' groups below wai's own.
                .max_h(px(480.))
                .placeholder("Run a command…")
                .on_confirm(move |ix, window, cx| {
                    let entry = (groups.get(ix.section))
                        .and_then(|(_, rows)| rows.get(ix.row))
                        .map(|(_, entry)| entry.clone());
                    window.close_dialog(cx);
                    if let Some(entry) = entry {
                        this.update(cx, |this, cx| this.run_entry(entry, window, cx))
                            .ok();
                    }
                });
            dialog.close_button(false).child(command)
        });
        state.update(cx, |s, cx| s.focus(window, cx));
    }

    fn run_entry(&mut self, entry: Entry, window: &mut Window, cx: &mut Context<Self>) {
        match entry {
            Entry::NewSession => self.new_session(&NewSession, window, cx),
            Entry::ToggleTerminal => self.toggle_terminal(&ToggleTerminal, window, cx),
            Entry::OpenFolder => self.open_folder(&OpenFolder, window, cx),
            Entry::Tab(tab) => {
                self.sidebar_tab = tab;
                if tab == SidebarTab::Search {
                    let focus = self.search.read(cx).query_focus(cx);
                    window.focus(&focus, cx);
                }
                cx.notify();
            }
            Entry::Extensions => {
                self.show_extensions(window, cx);
            }
            Entry::Template(ix) => self.start_template(ix, window, cx),
            Entry::Command(ext, command) => self.run_ext_command(&ext, command, window, cx),
            Entry::NewSessionWith(agent) => {
                self.start_session(agent, None, false, None, window, cx)
            }
        }
    }
}

/// The real-process tests: extensions are `/bin/sh` scripts.
#[cfg(all(test, unix))]
mod tests {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use serde_json::json;
    use wai_settings::Template;

    use crate::ext_host::tests::{bodies, install, log_path, logged, workspace_at};
    use crate::sidebar::SidebarTab;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{fake_settings, wait_until};

    const MANIFEST: &str = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.0.0\"\n\
        permissions = [\"process\"]\n[process]\ncommand = \"bin/run\"\n\
        [[contributes.templates]]\nname = \"Triage\"\nprompt = \"Triage the new issues\"\n\
        [[contributes.commands]]\nid = \"review\"\ntitle = \"Review the diff\"\n\
        prompt = \"Review the open diff\"\n\
        [[contributes.commands]]\nid = \"sync\"\ntitle = \"Sync cards\"\n\
        [[contributes.agents]]\nname = \"echo\"\ncommand = \"bin/agent\"\n";

    fn labels(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Vec<(String, Vec<String>)> {
        workspace.read_with(cx, |w, cx| {
            let groups = w.palette(cx).into_iter();
            let label = |(heading, rows): (&str, Vec<_>)| {
                let rows = rows
                    .into_iter()
                    .map(|(l, _): (gpui_kit::SharedString, _)| l.to_string());
                (heading.to_string(), rows.collect())
            };
            groups.map(label).collect()
        })
    }

    fn dialog(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.has_active_dialog(cx))
    }

    /// ⌘⇧P, `query`, Enter: runs the first row that matches.
    fn run(query: &str, cx: &mut VisualTestContext) {
        cx.simulate_keystrokes("secondary-shift-p");
        assert!(dialog(cx), "the palette opens");
        cx.simulate_input(query);
        cx.simulate_keystrokes("enter");
        assert!(!dialog(cx), "Enter closes the palette");
    }

    /// Built-ins, then the user's and the extensions' templates, the extensions' commands, and a
    /// new session per agent (the settings' and the extensions').
    #[gpui_kit::test]
    fn palette_lists_builtin_templates_and_ext_commands(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, MANIFEST, "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        workspace.update(cx, |w, _| {
            w.templates = vec![Template {
                name: "Release notes".into(),
                agent: None,
                prompt: "Write them".into(),
            }]
        });

        let strings = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(
            labels(&workspace, cx),
            [
                (
                    "wai".into(),
                    strings(&[
                        "New session",
                        "Toggle terminal",
                        "Open folder…",
                        "Search",
                        "Files",
                        "Agents",
                        "Extensions"
                    ])
                ),
                ("Templates".into(), strings(&["Release notes", "Triage"])),
                (
                    "Commands".into(),
                    strings(&["Review the diff", "Sync cards"])
                ),
                (
                    "Agents".into(),
                    strings(&["New session with fake", "New session with sample/echo"])
                ),
            ]
        );

        run("files", cx);
        assert_eq!(
            workspace.read_with(cx, |w, _| w.sidebar_tab),
            SidebarTab::Files
        );
        run("extensions", cx);
        assert_eq!(
            workspace.read_with(cx, |w, _| w.sidebar_tab),
            SidebarTab::Extensions
        );
        cx.simulate_keystrokes("secondary-shift-p");
        cx.simulate_keystrokes("escape");
        assert!(!dialog(cx), "Escape closes it");
    }

    /// A command without a prompt goes to the extension's process as `command/run`.
    #[gpui_kit::test]
    fn process_command_sends_command_run(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, MANIFEST, "", "");
        let (_, cx) = workspace_at(&config, fake_settings(""), cx);
        let log = log_path(tmp.path(), "sample");
        logged(&log, "initialize", cx, |l| {
            !bodies(l, "initialize").is_empty()
        });

        run("sync cards", cx);
        let lines = logged(&log, "command/run", cx, |l| {
            !bodies(l, "command/run").is_empty()
        });
        assert_eq!(bodies(&lines, "command/run"), [json!({"id": "sync"})]);
    }

    /// A prompt-only command (no process needed) starts a session with its prompt pre-filled,
    /// not sent, like a template.
    #[gpui_kit::test]
    fn prompt_only_command_prefills(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"prompts\"\nname = \"Prompts\"\nversion = \"1.0.0\"\n\
            [[contributes.commands]]\nid = \"review\"\ntitle = \"Review the diff\"\n\
            prompt = \"Review the open diff\"\n";
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join(wai_ext::MANIFEST_FILE), manifest).unwrap();
        let mut store = wai_ext::ExtStore::open(&config);
        let staged = store.stage(&wai_ext::Source::Folder(src)).unwrap();
        store.commit(staged, vec![]).unwrap();
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);

        run("review the diff", cx);
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        let session = workspace.read_with(cx, |w, _| w.panes[0].session().cloned().unwrap());
        assert_eq!(
            session.read_with(cx, |s, cx| s.draft(cx)),
            "Review the open diff"
        );
        assert!(session.read_with(cx, |s, _| s.model.items.is_empty()));
    }
}
