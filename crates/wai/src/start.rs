use std::path::PathBuf;

use gpui_kit::*;
use wai_agent::AgentError;
use wai_settings::shown;

use crate::agents::AgentStatus;
use crate::chat::{ChatSession, DraftData, DraftEvent, DraftView, signed_out};
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

    /// Starts the session of a draft and sends its text and attachments. A failed start keeps the
    /// draft as it was, with the reason above the prompt.
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
        let fail = |error: String, cx: &mut Context<Self>| {
            draft.update(cx, |d, cx| d.fail(Some(error), cx))
        };
        if !folder.is_dir() {
            return fail(
                format!("The folder {} no longer exists.", shown(&folder)),
                cx,
            );
        }
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            return fail(
                format!("No agent named {agent} in the settings or extensions."),
                cx,
            );
        };
        let mcp = self.registry.read(cx).mcp_servers();
        draft.update(cx, |d, cx| {
            d.starting = true;
            d.notice = None;
            cx.emit(DraftEvent::Changed);
            cx.notify();
        });
        *self.starting.entry(agent.clone()).or_default() += 1;
        let registry = self.registry.downgrade();
        cx.spawn_in(window, async move |this, cx| {
            let started = handle.new_session(folder.clone(), mcp).await;
            this.update_in(cx, |this, window, cx| {
                let early = this.landed(&agent, started.as_ref().ok().map(|s| s.id.as_str()));
                let started = match started {
                    Ok(started) => started,
                    Err(AgentError::AuthRequired) => {
                        this.registry
                            .update(cx, |r, cx| r.set_status(&agent, AgentStatus::SignedOut, cx));
                        let error = signed_out(&agent);
                        return draft.update(cx, |d, cx| d.fail(Some(error), cx));
                    }
                    Err(e) => {
                        let error = format!("Could not start a {agent} session: {e}");
                        return draft.update(cx, |d, cx| d.fail(Some(error), cx));
                    }
                };
                this.registry.update(cx, |r, cx| r.mark_ready(&agent, cx));
                let session = cx.new(|cx| {
                    ChatSession::new(agent.clone(), started, folder, handle, registry, window, cx)
                });
                this.land(session.clone(), early, model, window, cx);
                session.update(cx, |s, cx| s.send_with(text, attachments, cx));
                this.drafts.retain(|d| *d != draft);
                let pane = this.panes.iter().position(|p| p.draft() == Some(&draft));
                if let Some(ix) = pane {
                    this.panes[ix] = Pane::Session(session);
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
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::Settings;

    use crate::chat::{ChatItem, DraftView, TurnState};
    use crate::sidebar::canonical;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{fake_settings, open, start_now, wait_until};

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
}
