use std::path::{Path, PathBuf};

use gpui_kit::*;
use wai_agent::{AgentError, ConfigCategory, SessionStarted};
use wai_settings::{agent_label, shown};

use crate::agents::AgentStatus;
use crate::chat::{ChatEvent, ChatSession, DraftData, DraftEvent, DraftView, signed_out};
use crate::sidebar::canonical;
use crate::workspace::{Pane, Workspace};

impl Workspace {
    /// The project, then the open sessions' folders (newest first), then History's.
    pub(crate) fn known_folders(&self, cx: &App) -> Vec<PathBuf> {
        let open = self
            .sessions
            .iter()
            .rev()
            .map(|s| canonical(&s.read(cx).cwd));
        let history = self.history.iter().map(|(cwd, _)| cwd.clone());
        let mut known = vec![canonical(&self.project)];
        for folder in open.chain(history) {
            if !known.contains(&folder) {
                known.push(folder);
            }
        }
        known
    }

    pub(crate) fn draft_data(&self, cx: &App) -> DraftData {
        let registry = self.registry.read(cx);
        let agents = registry.names();
        let command = |id: &String| Some((id.clone(), registry.command(id)?.to_string()));
        DraftData {
            known: self.known_folders(cx),
            commands: agents.iter().filter_map(command).collect(),
            agents,
            models: self.models_seen.clone(),
            default_models: self.settings.models.clone(),
        }
    }

    /// Pushes fresh menu data into every draft.
    pub(crate) fn sync_drafts(&self, cx: &mut Context<Self>) {
        let data = self.draft_data(cx);
        for draft in &self.drafts {
            draft.update(cx, |d, cx| d.set_data(data.clone(), cx));
        }
    }

    /// Opens a draft of a session of `agent` in the session pane, with `prompt` typed in. It
    /// starts nothing: the agent's process spawns when the draft is sent.
    pub(crate) fn open_draft(
        &mut self,
        agent: String,
        prompt: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let folder = self.project.clone();
        let data = self.draft_data(cx);
        let draft = cx.new(|cx| DraftView::new(agent, folder, data, prompt, window, cx));
        cx.subscribe_in(
            &draft,
            window,
            |this, draft, event, window, cx| match event {
                DraftEvent::Send => this.send_draft(draft.clone(), window, cx),
                DraftEvent::Changed => cx.notify(),
            },
        )
        .detach();
        self.drafts.push(draft.clone());
        let ix = self.session_pane();
        self.panes[ix] = Pane::Draft(draft);
        self.focus_pane(ix, window, cx);
        cx.notify();
    }

    /// Sends a draft: its chat shows at once, with the steps of the start, and the session
    /// starts behind it. A failed start, Esc or Stop gives the draft back as it was (what was
    /// typed in the chat goes after its text); a failure also says why above the prompt.
    pub(crate) fn send_draft(
        &mut self,
        draft: Entity<DraftView>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let d = draft.read(cx);
        if !d.can_send(cx) {
            return;
        }
        let (agent, folder, model) = (d.agent.clone(), d.folder.clone(), d.model.clone());
        let (text, attachments) = d.message(cx);
        if !folder.is_dir() {
            let error = format!("The folder {} no longer exists.", shown(&folder));
            return draft.update(cx, |d, cx| d.fail(Some(error), String::new(), window, cx));
        }
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            let error = format!("No agent named {agent} in the settings or extensions.");
            return draft.update(cx, |d, cx| d.fail(Some(error), String::new(), window, cx));
        };
        let mcp = self.registry.read(cx).mcp_servers();
        let registry = self.registry.downgrade();
        let chat = cx.new(|cx| {
            let (agent, folder, handle) = (agent.clone(), folder.clone(), handle.clone());
            ChatSession::pending(
                agent,
                folder,
                handle,
                registry.clone(),
                text,
                attachments,
                window,
                cx,
            )
        });
        let weak = draft.downgrade();
        cx.subscribe_in(&chat, window, move |this, _, event, window, cx| {
            let Some(draft) = weak.upgrade() else {
                return;
            };
            match event {
                ChatEvent::StopStart => this.revert_draft(&draft, None, window, cx),
                ChatEvent::Hide => {
                    let pane = this.panes.iter().position(|p| p.draft() == Some(&draft));
                    if let Some(ix) = pane {
                        this.close_pane(ix, window, cx);
                    }
                }
                _ => {}
            }
        })
        .detach();
        draft.update(cx, |d, cx| {
            d.starting = Some(chat.clone());
            d.notice = None;
            cx.emit(DraftEvent::Changed);
            cx.notify();
        });
        if self.panes[self.focused_pane].draft() == Some(&draft) {
            chat.update(cx, |chat, cx| chat.focus(window, cx));
        }
        let label = agent_label(&agent);
        let step = chat.update(cx, |chat, cx| chat.step(format!("Starting {label}"), cx));
        *self.starting.entry(agent.clone()).or_default() += 1;
        cx.spawn_in(window, async move |this, cx| {
            let started = handle.new_session(folder.clone(), mcp).await;
            this.update_in(cx, |this, window, cx| {
                let early = this.landed(&agent, started.as_ref().ok().map(|s| s.id.as_str()));
                // Esc, Stop or a discarded draft ended the send while the agent worked.
                let current = draft.read(cx).starting.as_ref() == Some(&chat);
                let started = match started {
                    Ok(started) if !current => {
                        return this.close_in_agent(agent, started.id, cx);
                    }
                    Err(_) if !current => return,
                    Ok(started) => started,
                    Err(AgentError::AuthRequired) => {
                        this.registry
                            .update(cx, |r, cx| r.set_status(&agent, AgentStatus::SignedOut, cx));
                        let error = Some(signed_out(&agent));
                        return this.revert_draft(&draft, error, window, cx);
                    }
                    Err(e) => {
                        let error = Some(format!("Could not start a {agent} session: {e}"));
                        return this.revert_draft(&draft, error, window, cx);
                    }
                };
                let title = started_title(&label, &folder, &started);
                chat.update(cx, |chat, cx| chat.end_step(step, title, false, cx));
                this.registry.update(cx, |r, cx| r.mark_ready(&agent, cx));
                chat.update(cx, |chat, cx| chat.started(started, window, cx));
                this.land(chat.clone(), early, model, window, cx);
                this.drafts.retain(|d| *d != draft);
                let pane = this.panes.iter().position(|p| p.draft() == Some(&draft));
                if let Some(ix) = pane {
                    this.panes[ix] = Pane::Session(chat);
                    // Only the focused pane takes the caret and the workspace's follow.
                    if ix == this.focused_pane {
                        this.focus_pane(ix, window, cx);
                        this.follow_session(window, cx);
                    }
                }
                this.save_state(cx);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    /// Gives a starting draft back to editing; its chat goes away, its typed text stays.
    fn revert_draft(
        &mut self,
        draft: &Entity<DraftView>,
        error: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(chat) = draft.read(cx).starting.clone() else {
            return;
        };
        let leftover = chat.read(cx).leftover(cx);
        draft.update(cx, |d, cx| d.fail(error, leftover, window, cx));
        if self.panes[self.focused_pane].draft() == Some(draft) {
            draft.update(cx, |d, cx| d.focus(window, cx));
        }
    }
}

/// `Started <Label> in <folder> · <mode> · <model>`, without the parts the agent did not name.
fn started_title(label: &str, folder: &Path, started: &SessionStarted) -> String {
    let mode = (started.modes.iter())
        .find(|m| Some(&m.id) == started.current_mode.as_ref())
        .map(|m| m.name.clone());
    let model = (started.config.iter())
        .find(|o| o.category == ConfigCategory::Model)
        .and_then(|o| o.values.iter().find(|v| v.id == o.current))
        .map(|v| v.name.clone());
    let head = format!("Started {label} in {}", shown(folder));
    let parts = std::iter::once(head).chain(mode).chain(model);
    parts.collect::<Vec<_>>().join(" · ")
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use gpui_kit::test::{TestQueryExt as _, TestWindowExt as _};
    use gpui_kit::{ElementId, Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use crate::chat::{ChatItem, DraftView, TurnState};
    use crate::sidebar::canonical;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{fake_settings, gated, open, start_now, wait_until};
    use wai_agent::ToolStatus;

    fn mkdir(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    /// `fake_settings` with a second agent `b` (also the fake) and `ghost` (not installed).
    fn two_agents(flag: &str) -> Settings {
        let mut settings = fake_settings(flag);
        let fake = settings.agents["fake"].clone();
        settings.agents.insert("b".into(), fake);
        let ghost = wai_settings::AgentConfig {
            command: "wai-no-such-program-7c1e --acp".into(),
        };
        settings.agents.insert("ghost".into(), ghost);
        settings
    }

    fn new_draft(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<DraftView> {
        cx.simulate_keystrokes("secondary-n");
        workspace.read_with(cx, |w, _| w.drafts.last().unwrap().clone())
    }

    /// Stops the project's watcher before its folder goes.
    fn leave(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project("/nonexistent".into(), window, cx))
        });
    }

    #[gpui_kit::test]
    fn folder_menu_lists_known_folders(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let a = mkdir(dir.path(), "a");
        let flag = format!("--sessions 1 --sessions-cwd {}", wai_agent::quote(&a));
        let (workspace, cx) = open(cx, fake_settings(&flag));
        start_now(&workspace, cx);
        wait_until(cx, "the history", |cx| {
            workspace.read_with(cx, |w, _| !w.history.is_empty())
        });
        let draft = new_draft(&workspace, cx);
        draft.read_with(cx, |d, _| {
            let want = [canonical(&std::env::temp_dir()), canonical(&a)];
            assert_eq!(d.data.known, want);
        });
    }

    #[gpui_kit::test]
    fn picking_a_folder_keeps_the_workspace(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let b = mkdir(dir.path(), "b");
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = new_draft(&workspace, cx);
        let project = workspace.read_with(cx, |w, _| w.project.clone());
        draft.update(cx, |d, cx| d.set_folder(b.clone(), cx));
        draft.read_with(cx, |d, _| assert_eq!(d.folder, canonical(&b)));
        workspace.read_with(cx, |w, _| assert_eq!(w.project, project));
    }

    #[gpui_kit::test]
    fn send_uses_the_drafts_folder_and_agent(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let b = mkdir(dir.path(), "b");
        let (workspace, cx) = open(cx, two_agents(""));
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| {
            d.set_agent("b".into(), cx);
            d.set_folder(b.clone(), cx);
        });
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 1)
        });
        let session = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        session.read_with(cx, |s, _| {
            assert_eq!(s.agent, "b");
            assert_eq!(canonical(&s.cwd), canonical(&b));
        });
        wait_until(cx, "the workspace to follow", |cx| {
            workspace.read_with(cx, |w, _| canonical(&w.project) == canonical(&b))
        });
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn missing_program_shows_the_notice_and_disables_send(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, two_agents(""));
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| d.set_agent("ghost".into(), cx));
        cx.simulate_input("x");
        wait_until(cx, "the installed check", |cx| {
            draft.read_with(cx, |d, _| d.installed() == Some(false))
        });
        let shown = cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find("draft-notice").is_some()
        });
        assert!(shown);
        draft.read_with(cx, |d, cx| assert!(!d.can_send(cx)));
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        workspace.read_with(cx, |w, cx| {
            assert!(w.sessions.is_empty());
            assert!(w.registry.read(cx).status("ghost").is_none());
        });
    }

    #[gpui_kit::test]
    fn changing_the_agent_resets_the_model(cx: &mut TestAppContext) {
        let mut settings = two_agents("");
        settings.models = [("b".into(), "fake-large".into())].into();
        let (workspace, cx) = open(cx, settings);
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| d.set_agent("b".into(), cx));
        draft.read_with(cx, |d, _| {
            assert_eq!(d.model.as_deref(), Some("fake-large"))
        });
        draft.update(cx, |d, cx| d.set_agent("fake".into(), cx));
        draft.read_with(cx, |d, _| assert_eq!(d.model, None));
    }

    #[gpui_kit::test]
    fn model_pick_applies_to_the_new_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--models"));
        start_now(&workspace, cx);
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| d.set_model(Some("fake-large".into()), cx));
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the second session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 2)
        });
        let session = workspace.read_with(cx, |w, _| w.sessions[1].clone());
        wait_until(cx, "the reply", |cx| {
            session.read_with(cx, |s, _| {
                let reply = s.model.items.iter().rev().find_map(|item| match item {
                    ChatItem::Agent { text } => Some(text.clone()),
                    _ => None,
                });
                s.model.state == TurnState::Idle && reply.as_deref() == Some("model:fake-large")
            })
        });
    }

    #[gpui_kit::test]
    fn model_chip_waits_for_seen_models(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings("--models"));
        new_draft(&workspace, cx);
        let shown = cx.update(|window, cx| {
            window.render_frame(cx);
            window.try_find("draft-model").is_some()
        });
        assert!(!shown);
    }

    fn send_text(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<DraftView> {
        let draft = new_draft(workspace, cx);
        cx.simulate_input("build it");
        cx.simulate_keystrokes("enter");
        draft
    }

    #[gpui_kit::test]
    fn sending_shows_the_chat_at_once(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (settings, gate) = gated("", tmp.path());
        let (workspace, cx) = open(cx, settings);
        let draft = send_text(&workspace, cx);

        let chat = draft.read_with(cx, |d, _| d.starting.clone().expect("starting"));
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes[0].draft(), Some(&draft));
            assert!(w.sessions.is_empty());
        });
        chat.read_with(cx, |c, _| {
            assert!(matches!(
                c.model.items.as_slice(),
                [
                    ChatItem::User { text },
                    ChatItem::Tool(step),
                ] if text == "build it"
                    && step.title == "Starting fake"
                    && step.status == ToolStatus::InProgress
            ));
            assert_eq!(c.model.activity().as_deref(), Some("Starting fake…"));
        });

        std::fs::write(&gate, "").unwrap();
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 1)
        });
        let session = workspace.read_with(cx, |w, _| {
            assert!(w.drafts.is_empty());
            w.sessions[0].clone()
        });
        session.read_with(cx, |s, _| {
            assert!(matches!(&s.model.items[0], ChatItem::User { text } if text == "build it"));
            let ChatItem::Tool(step) = &s.model.items[1] else {
                panic!("the second row is the step");
            };
            assert_eq!(step.status, ToolStatus::Completed);
            assert!(step.title.starts_with("Started fake in "), "{}", step.title);
        });
        wait_until(cx, "the reply", |cx| {
            session.read_with(cx, |s, _| {
                (s.model.items.iter()).any(
                    |i| matches!(i, ChatItem::Agent { text } if text == "Hello from fake agent"),
                )
            })
        });
    }

    #[gpui_kit::test]
    fn escape_before_the_session_starts_returns_to_the_draft(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (file, flag) = (tmp.path().join("closed"), "--close-file");
        let flag = format!("{flag} {}", wai_agent::quote(&file));
        let (settings, gate) = gated(&flag, tmp.path());
        let (workspace, cx) = open(cx, settings);
        let draft = send_text(&workspace, cx);
        cx.simulate_input("and tests");
        cx.simulate_keystrokes("escape");

        draft.read_with(cx, |d, cx| {
            assert!(d.starting.is_none());
            assert!(d.notice.is_none());
            assert_eq!(d.text(cx), "build it\n\nand tests");
        });
        std::fs::write(&gate, "").unwrap();
        wait_until(cx, "the close", |_| {
            std::fs::read_to_string(&file).is_ok_and(|s| s.contains("fake-1"))
        });
        workspace.read_with(cx, |w, _| assert!(w.sessions.is_empty()));
    }

    #[gpui_kit::test]
    fn starting_draft_counts_as_working(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (settings, gate) = gated("", tmp.path());
        let (workspace, cx) = open(cx, settings);
        send_text(&workspace, cx);
        workspace.read_with(cx, |w, cx| assert_eq!(w.busy_sessions(cx), 1));

        cx.simulate_keystrokes("secondary-q");
        cx.update(|window, _| window.find_by_label("1 session is still working"));
        std::fs::write(&gate, "").unwrap();
    }

    #[gpui_kit::test]
    fn untitled_session_shows_its_first_message(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        send_text(&workspace, cx);
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 1)
        });
        let session = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        cx.update(|window, cx| {
            window.render_frame(cx);
            let row = window.try_find(("open", 0usize)).unwrap();
            assert_eq!(row.label(), Some("build it"));
            let pane = window.within(ElementId::View(session.entity_id()));
            assert_eq!(pane.find("session-title").label(), Some("build it"));
        });
    }
}
