use std::path::{Path, PathBuf};

use gpui_kit::*;
use wai_agent::{AgentError, ConfigCategory, SessionStarted};
use wai_settings::{agent_label, shown};

use crate::agents::AgentStatus;
use crate::chat::{ChatEvent, ChatSession, DraftData, DraftEvent, DraftView, signed_out};
use crate::git::{self, Repo};
use crate::sidebar::canonical;
use crate::workspace::{Pane, Workspace, started_modes};

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
            modes: self.modes_seen.clone(),
            last_models: self.last_models.clone(),
            last_bases: self.last_bases.clone(),
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
        let mode = d.mode.clone();
        let (text, attachments) = d.message(cx);
        let (repo, worktree, branch) = (d.repo.clone(), d.worktree, d.branch.clone());
        if !folder.is_dir() {
            let error = format!("The folder {} no longer exists.", shown(&folder));
            return draft.update(cx, |d, cx| d.fail(Some(error), String::new(), window, cx));
        }
        let Some(handle) = self.registry.update(cx, |r, cx| r.handle(&agent, cx)) else {
            let error = format!("No agent named {agent} in the settings or extensions.");
            return draft.update(cx, |d, cx| d.fail(Some(error), String::new(), window, cx));
        };
        let job = match (repo, worktree, branch) {
            (None, true, _) => {
                let error = "Still reading the repository. Try again in a moment.".to_string();
                return draft.update(cx, |d, cx| d.fail(Some(error), String::new(), window, cx));
            }
            (Some(repo), true, base) => {
                let base = base.unwrap_or_else(|| repo.default.clone());
                Some(GitJob::Worktree {
                    slug: git::slug(&text),
                    repo,
                    base,
                })
            }
            (Some(repo), false, Some(branch)) if Some(&branch) != repo.current.as_ref() => {
                if let Some(error) = self.shared_folder(&draft, &repo.root, &branch, cx) {
                    return draft
                        .update(cx, |d, cx| d.fail(Some(error), String::new(), window, cx));
                }
                Some(GitJob::Checkout { branch })
            }
            _ => None,
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
        let git_step = job.as_ref().map(|job| {
            let title = match job {
                GitJob::Worktree { .. } => "Creating worktree".to_string(),
                GitJob::Checkout { branch } => format!("Checking out {branch}"),
            };
            chat.update(cx, |chat, cx| chat.step(title, cx))
        });
        let remembered_base = match &job {
            Some(GitJob::Worktree { repo, base, .. }) => Some((repo.root.clone(), base.clone())),
            _ => None,
        };
        let mut start_step = None;
        if job.is_none() {
            start_step =
                Some(chat.update(cx, |chat, cx| chat.step(format!("Starting {label}"), cx)));
            *self.starting.entry(agent.clone()).or_default() += 1;
        }
        cx.spawn_in(window, async move |this, cx| {
            let (mut folder, mut step) = (folder, start_step);
            if let Some(job) = job {
                let (base, root) = match &job {
                    GitJob::Worktree { repo, base, .. } => (base.clone(), repo.root.clone()),
                    GitJob::Checkout { branch } => (branch.clone(), PathBuf::new()),
                };
                let run = cx.background_spawn({
                    let folder = folder.clone();
                    async move {
                        match job {
                            GitJob::Worktree { repo, slug, base } => {
                                git::create_worktree(&repo, &slug, &base).map(Some)
                            }
                            GitJob::Checkout { branch } => {
                                git::checkout(&folder, &branch).map(|()| None)
                            }
                        }
                    }
                });
                let done = run.await;
                let next = this
                    .update_in(cx, |this, window, cx| {
                        let current = draft.read(cx).starting.as_ref() == Some(&chat);
                        let ix = git_step.unwrap_or_default();
                        let cwd = match done {
                            Err(error) => {
                                if current {
                                    this.revert_draft(&draft, Some(error), window, cx);
                                }
                                return None;
                            }
                            Ok(Some(wt)) => {
                                let sub = canonical(&folder);
                                let sub = sub.strip_prefix(&root).unwrap_or(Path::new(""));
                                let cwd = wt.path.join(sub);
                                // A retry reuses the worktree, even after Esc.
                                draft.update(cx, |d, cx| d.use_worktree(&folder, cwd.clone(), cx));
                                if !current {
                                    return None;
                                }
                                chat.update(cx, |chat, cx| {
                                    if let Some(why) = &wt.fetch_failed {
                                        let note = format!(
                                            "Could not fetch origin/{base} ({why}); started from the local {base}"
                                        );
                                        let note_ix = chat.step(note.clone(), cx);
                                        chat.end_step(note_ix, note, false, cx);
                                    }
                                    let title = format!(
                                        "Created worktree {} on branch {} from {}",
                                        shown(&wt.path),
                                        wt.branch,
                                        wt.from
                                    );
                                    chat.end_step(ix, title, false, cx);
                                    chat.cwd = cwd.clone();
                                });
                                cwd
                            }
                            Ok(None) => {
                                if !current {
                                    return None;
                                }
                                let title = format!("Checked out {base}");
                                chat.update(cx, |chat, cx| chat.end_step(ix, title, false, cx));
                                folder.clone()
                            }
                        };
                        *this.starting.entry(agent.clone()).or_default() += 1;
                        let title = format!("Starting {label}");
                        let step = chat.update(cx, |chat, cx| chat.step(title, cx));
                        Some((cwd, step))
                    })
                    .ok()
                    .flatten();
                let Some((cwd, next)) = next else {
                    return;
                };
                (folder, step) = (cwd, Some(next));
            }
            let step = step.unwrap_or_default();
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
                let title = started_title(
                    &label,
                    &folder,
                    &started,
                    mode.as_deref(),
                    model.as_deref(),
                );
                chat.update(cx, |chat, cx| chat.end_step(step, title, false, cx));
                this.registry.update(cx, |r, cx| r.mark_ready(&agent, cx));
                chat.update(cx, |chat, cx| chat.started(started, window, cx));
                if let Some(model) = &model {
                    this.last_models.insert(agent.clone(), model.clone());
                }
                if let Some((root, base)) = &remembered_base {
                    this.last_bases.insert(root.clone(), base.clone());
                }
                this.land(chat.clone(), early, model, window, cx);
                if let Some(mode) = mode {
                    chat.update(cx, |chat, cx| {
                        let option = (chat.model.config.iter())
                            .find(|o| o.category == ConfigCategory::Mode)
                            .map(|o| o.id.clone());
                        match option {
                            Some(option) => chat.set_option(&option, mode, cx),
                            None if chat.model.current_mode.as_ref() != Some(&mode) => {
                                chat.set_mode(mode, cx)
                            }
                            None => {}
                        }
                    });
                }
                chat.update(cx, |chat, cx| chat.send_first(cx));
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

    /// D27: why a Local branch switch is refused, when another session (or a draft that is
    /// starting) already works in `folder`.
    fn shared_folder(
        &self,
        draft: &Entity<DraftView>,
        root: &Path,
        branch: &str,
        cx: &App,
    ) -> Option<String> {
        let folder = canonical(root);
        let worktrees = folder.join(".worktrees");
        let sessions = self.sessions.iter().map(|s| s.read(cx).cwd.clone());
        let starting = (self.drafts.iter())
            .filter(|d| *d != draft)
            .filter_map(|d| d.read(cx).starting.as_ref().map(|c| c.read(cx).cwd.clone()));
        let used = sessions.chain(starting).any(|cwd| {
            let cwd = canonical(&cwd);
            cwd.starts_with(&folder) && !cwd.starts_with(&worktrees)
        });
        let name = folder.file_name().map_or_else(
            || folder.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        used.then(|| {
            format!(
                "Can't switch to {branch}: another open session works in {name}. Pick New worktree instead."
            )
        })
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

/// The git work a send does before the agent starts.
enum GitJob {
    Worktree {
        repo: Repo,
        slug: String,
        base: String,
    },
    Checkout {
        branch: String,
    },
}

/// `Started <Label> in <folder> · <mode> · <model>`, without the parts the agent did not name.
fn started_title(
    label: &str,
    folder: &Path,
    started: &SessionStarted,
    mode: Option<&str>,
    model: Option<&str>,
) -> String {
    let seen = started_modes(started);
    let current = seen.as_ref().and_then(|s| s.current.clone());
    let mode = mode.map(str::to_string).or(current);
    let mode = seen.and_then(|s| s.modes.into_iter().find(|m| Some(&m.id) == mode.as_ref()));
    let mode = mode.map(|m| m.name);
    let model = (started.config.iter())
        .find(|o| o.category == ConfigCategory::Model)
        .and_then(|o| {
            let id = model.unwrap_or(&o.current);
            o.values.iter().find(|v| v.id == id)
        })
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

    use crate::chat::{ChatItem, ChatSession, DraftView, TurnState, signed_out};
    use crate::git::tests::{cloned, git, repo_with};
    use crate::git::{self, SUBMODULE_HINT};
    use crate::sidebar::canonical;
    use crate::workspace::Workspace;
    use crate::workspace::tests::{
        fake_live, fake_settings, gated, live, open, start_now, wait_until,
    };
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
    fn cloned_folder_becomes_the_drafts_folder(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // git is a real process
        let dir = tempfile::tempdir().unwrap();
        let (bare, _) = git::tests::cloned(dir.path());
        let parent = mkdir(dir.path(), "parent");
        let (workspace, cx) = open(cx, two_agents(""));
        let draft = new_draft(&workspace, cx);
        cx.simulate_input("hi");
        draft.update(cx, |d, cx| {
            d.clone_into(bare.to_string_lossy().into_owned(), parent.clone(), cx)
        });
        draft.read_with(cx, |d, cx| assert!(!d.can_send(cx)));
        wait_until(cx, "the clone", |cx| {
            draft.read_with(cx, |d, _| {
                d.folder == canonical(&parent.join("origin")) && d.repo.is_some()
            })
        });
        draft.read_with(cx, |d, _| assert!(d.cloning.is_none()));
        leave(&workspace, cx);
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

    fn send_and_wait(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
        sessions: usize,
    ) -> Entity<ChatSession> {
        type_and_send(cx, "hi");
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == sessions)
        });
        workspace.read_with(cx, |w, _| w.sessions[sessions - 1].clone())
    }

    #[gpui_kit::test]
    fn new_draft_takes_the_last_model_used_for_its_agent(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings("--models"));
        start_now(&workspace, cx);
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| d.set_model(Some("fake-large".into()), cx));
        send_and_wait(&workspace, cx, 2);
        let next = new_draft(&workspace, cx);
        next.read_with(cx, |d, _| {
            assert_eq!(d.model.as_deref(), Some("fake-large"))
        });
        workspace.read_with(cx, |w, _| assert!(w.settings.models.is_empty()));
    }

    #[gpui_kit::test]
    fn new_draft_never_preselects_a_picked_mode(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        start_now(&workspace, cx);
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| d.set_mode(Some("plan".into()), cx));
        let session = send_and_wait(&workspace, cx, 2);
        wait_until(cx, "the mode", |cx| {
            session.read_with(cx, |s, _| s.model.current_mode.as_deref() == Some("plan"))
        });
        let next = new_draft(&workspace, cx);
        next.read_with(cx, |d, _| assert_eq!(d.mode.as_deref(), Some("default")));
    }

    #[gpui_kit::test]
    fn mode_chip_waits_for_the_agent_to_report_modes(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let shown = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find("draft-mode").is_some()
            })
        };
        new_draft(&workspace, cx);
        assert!(!shown(cx));
        start_now(&workspace, cx);
        new_draft(&workspace, cx);
        assert!(shown(cx));
    }

    #[gpui_kit::test]
    fn worktree_base_is_remembered_per_repository(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let one = repo_with(&dir.path().join("one"), &["feature"]);
        git(&one, &["checkout", "main"]);
        let two = repo_with(&dir.path().join("two"), &["other"]);
        git(&two, &["checkout", "main"]);
        let (workspace, cx, _user) = fake_live(cx, "");
        let draft = repo_draft(&workspace, cx, &one);
        draft.update(cx, |d, cx| {
            d.set_worktree(true, cx);
            d.set_branch(Some("feature".into()), cx);
        });
        send_and_wait(&workspace, cx, 1);
        let again = repo_draft(&workspace, cx, &one);
        again.update(cx, |d, cx| d.set_worktree(true, cx));
        again.read_with(cx, |d, _| assert_eq!(d.branch.as_deref(), Some("feature")));
        let other = repo_draft(&workspace, cx, &two);
        other.update(cx, |d, cx| d.set_worktree(true, cx));
        other.read_with(cx, |d, _| assert_eq!(d.branch.as_deref(), Some("main")));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn picks_never_change_settings(cx: &mut TestAppContext) {
        let fake = fake_settings("--models");
        let file = format!(
            "[agents.fake]\ncommand = \'\'\'{}\'\'\'\n",
            fake.agents["fake"].command
        );
        let (workspace, cx, dir) = live(cx, fake, Some(&file));
        let path = dir.path().join("settings.toml");
        let before = std::fs::read(&path).unwrap();
        start_now(&workspace, cx);
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| {
            d.set_model(Some("fake-large".into()), cx);
            d.set_mode(Some("plan".into()), cx);
        });
        send_and_wait(&workspace, cx, 2);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        workspace.read_with(cx, |w, _| assert!(w.settings.models.is_empty()));
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

    fn repo_draft(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
        folder: &Path,
    ) -> Entity<DraftView> {
        let draft = new_draft(workspace, cx);
        draft.update(cx, |d, cx| d.set_folder(folder.to_path_buf(), cx));
        wait_until(cx, "the repository", |cx| {
            draft.read_with(cx, |d, _| d.repo.is_some())
        });
        draft
    }

    fn type_and_send(cx: &mut VisualTestContext, text: &str) {
        cx.simulate_input(text);
        cx.simulate_keystrokes("enter");
    }

    /// The step lines of a chat: title and whether the step is complete.
    fn steps(chat: &Entity<ChatSession>, cx: &mut VisualTestContext) -> Vec<(String, bool)> {
        chat.read_with(cx, |c, _| {
            (c.model.items.iter())
                .filter_map(|item| match item {
                    ChatItem::Tool(t) => Some((t.title.clone(), t.status == ToolStatus::Completed)),
                    _ => None,
                })
                .collect()
        })
    }

    fn one_session(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ChatSession> {
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.sessions.len() == 1)
        });
        workspace.read_with(cx, |w, _| w.sessions[0].clone())
    }

    fn worktrees(root: &Path) -> usize {
        std::fs::read_dir(root.join(".worktrees")).unwrap().count()
    }

    #[gpui_kit::test]
    fn git_chips_hidden_outside_a_repository(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = new_draft(&workspace, cx);
        wait_until(cx, "the read", |cx| draft.read_with(cx, |d, _| !d.reading));
        let shown = cx.update(|window, cx| {
            window.render_frame(cx);
            (
                window.try_find("draft-workspace").is_some(),
                window.try_find("draft-branch").is_some(),
            )
        });
        assert_eq!(shown, (false, false));
    }

    #[gpui_kit::test]
    fn git_data_is_read_off_the_ui_thread(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &[]);
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = new_draft(&workspace, cx);
        draft.update(cx, |d, cx| d.set_folder(root.clone(), cx));
        draft.read_with(cx, |d, _| assert!(d.repo.is_none()));
        wait_until(cx, "the repository", |cx| {
            draft.read_with(cx, |d, _| {
                d.repo.as_ref().and_then(|r| r.current.as_deref()) == Some("main")
            })
        });
        let shown = cx.update(|window, cx| {
            window.render_frame(cx);
            (
                window.try_find("draft-workspace").is_some(),
                window.try_find("draft-branch").is_some(),
            )
        });
        assert_eq!(shown, (true, true));
    }

    #[gpui_kit::test]
    fn local_send_on_another_branch_checks_it_out(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &["feature"]);
        git(&root, &["checkout", "main"]);
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = repo_draft(&workspace, cx, &root);
        draft.update(cx, |d, cx| d.set_branch(Some("feature".into()), cx));
        type_and_send(cx, "hi");
        let session = one_session(&workspace, cx);
        session.read_with(cx, |s, _| assert_eq!(canonical(&s.cwd), root));
        let current = git::repo(&root).unwrap().current;
        assert_eq!(current.as_deref(), Some("feature"));
        let steps = steps(&session, cx);
        assert!(steps.contains(&("Checked out feature".to_string(), true)));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn local_switch_refuses_uncommitted_files(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &["feature"]);
        git(&root, &["checkout", "main"]);
        std::fs::write(root.join("wip.txt"), "wip").unwrap();
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = repo_draft(&workspace, cx, &root);
        draft.update(cx, |d, cx| d.set_branch(Some("feature".into()), cx));
        type_and_send(cx, "hi");
        wait_until(cx, "the refusal", |cx| {
            draft.read_with(cx, |d, _| d.starting.is_none() && d.notice.is_some())
        });
        draft.read_with(cx, |d, _| {
            let notice = d.notice.as_deref().unwrap();
            assert!(notice.contains("uncommitted files"), "{notice}");
        });
        let current = git::repo(&root).unwrap().current;
        assert_eq!(current.as_deref(), Some("main"));
        workspace.read_with(cx, |w, _| assert!(w.sessions.is_empty()));
    }

    #[gpui_kit::test]
    fn local_switch_refuses_a_folder_another_session_uses(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &["feature"]);
        git(&root, &["checkout", "main"]);
        let (workspace, cx, _user) = fake_live(cx, "");
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(root.clone(), window, cx))
        });
        start_now(&workspace, cx);
        let draft = new_draft(&workspace, cx);
        wait_until(cx, "the repository", |cx| {
            draft.read_with(cx, |d, _| d.repo.is_some())
        });
        draft.update(cx, |d, cx| d.set_branch(Some("feature".into()), cx));
        type_and_send(cx, "hi");
        let name = root.file_name().unwrap().to_string_lossy().into_owned();
        draft.read_with(cx, |d, _| {
            let want = format!(
                "Can't switch to feature: another open session works in {name}. Pick New worktree instead."
            );
            assert_eq!(d.notice.as_deref(), Some(want.as_str()));
        });
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn new_worktree_send_creates_it_from_the_fetched_base(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let (bare, clone) = cloned(dir.path());
        let src = dir.path().join("src");
        std::fs::write(src.join("b.txt"), "b").unwrap();
        git(&src, &["add", "."]);
        git(&src, &["commit", "-m", "second"]);
        git(&src, &["push", bare.to_str().unwrap(), "main"]);
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = repo_draft(&workspace, cx, &clone);
        draft.update(cx, |d, cx| {
            d.set_worktree(true, cx);
            d.set_branch(Some("main".into()), cx);
        });
        type_and_send(cx, "Add tag filter");
        let session = one_session(&workspace, cx);
        let want = clone.join(".worktrees/add-tag-filter");
        session.read_with(cx, |s, _| assert_eq!(canonical(&s.cwd), want));
        let head = git::head(&want).unwrap();
        assert_eq!(head.branch.as_deref(), Some("wai/add-tag-filter"));
        assert_eq!(
            git(&want, &["rev-parse", "HEAD"]),
            git(&bare, &["rev-parse", "main"])
        );
        assert!(
            steps(&session, cx)
                .iter()
                .any(|(t, _)| t.starts_with("Created worktree"))
        );
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn offline_fetch_still_starts_the_session(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let (_, clone) = cloned(dir.path());
        let missing = dir.path().join("missing.git");
        git(
            &clone,
            &["remote", "set-url", "origin", missing.to_str().unwrap()],
        );
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = repo_draft(&workspace, cx, &clone);
        draft.update(cx, |d, cx| d.set_worktree(true, cx));
        type_and_send(cx, "Work offline");
        let session = one_session(&workspace, cx);
        let steps = steps(&session, cx);
        let fetch = steps
            .iter()
            .find(|(t, _)| t.starts_with("Could not fetch origin/main"));
        assert_eq!(fetch.map(|(_, done)| *done), Some(true), "{steps:?}");
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn worktree_keeps_the_subfolder(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &[]);
        let sub = mkdir(&root, "sub");
        std::fs::write(sub.join("f.txt"), "f").unwrap();
        git(&root, &["add", "."]);
        git(&root, &["commit", "-m", "sub"]);
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = repo_draft(&workspace, cx, &sub);
        draft.update(cx, |d, cx| d.set_worktree(true, cx));
        type_and_send(cx, "sub work");
        let session = one_session(&workspace, cx);
        let want = root.join(".worktrees/sub-work/sub");
        session.read_with(cx, |s, _| assert_eq!(canonical(&s.cwd), want));
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn retry_after_a_created_worktree_reuses_it(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &[]);
        let (workspace, cx) = open(cx, fake_settings("--require-auth"));
        let draft = repo_draft(&workspace, cx, &root);
        draft.update(cx, |d, cx| d.set_worktree(true, cx));
        type_and_send(cx, "do it");
        let reverted = |cx: &mut VisualTestContext| {
            draft.read_with(cx, |d, _| d.starting.is_none() && d.notice.is_some())
        };
        wait_until(cx, "the sign-in notice", reverted);
        let wt = root.join(".worktrees/do-it");
        draft.read_with(cx, |d, _| {
            assert_eq!(d.notice.as_deref(), Some(signed_out("fake").as_str()));
            assert!(!d.worktree);
            assert_eq!(d.folder, wt);
        });
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the second refusal", reverted);
        assert_eq!(worktrees(&root), 1);
    }

    #[gpui_kit::test]
    fn escape_after_the_worktree_keeps_it_for_the_retry(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let dir = tempfile::tempdir().unwrap();
        let tmp = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &[]);
        let (settings, gate) = gated("", tmp.path());
        let (workspace, cx) = open(cx, settings);
        let draft = repo_draft(&workspace, cx, &root);
        draft.update(cx, |d, cx| d.set_worktree(true, cx));
        type_and_send(cx, "Fix it");
        wait_until(cx, "the worktree step", |cx| {
            let chat = draft.read_with(cx, |d, _| d.starting.clone());
            chat.is_some_and(|chat| {
                steps(&chat, cx)
                    .iter()
                    .any(|(t, _)| t.starts_with("Created worktree"))
            })
        });
        cx.simulate_keystrokes("escape");
        let wt = root.join(".worktrees/fix-it");
        draft.read_with(cx, |d, _| {
            assert!(d.starting.is_none());
            assert_eq!(d.folder, wt);
            assert!(!d.worktree);
        });
        std::fs::write(&gate, "").unwrap();
        cx.simulate_keystrokes("enter");
        let session = one_session(&workspace, cx);
        session.read_with(cx, |s, _| assert_eq!(canonical(&s.cwd), wt));
        assert_eq!(worktrees(&root), 1);
        leave(&workspace, cx);
    }

    #[gpui_kit::test]
    fn submodule_repo_disables_new_worktree(cx: &mut TestAppContext) {
        let dir = tempfile::tempdir().unwrap();
        let root = repo_with(&dir.path().join("r"), &[]);
        std::fs::write(root.join(".gitmodules"), "").unwrap();
        let (workspace, cx) = open(cx, fake_settings(""));
        let draft = repo_draft(&workspace, cx, &root);
        draft.update(cx, |d, cx| d.set_worktree(true, cx));
        draft.read_with(cx, |d, _| {
            assert_eq!(d.worktree_blocked(), Some(SUBMODULE_HINT));
            assert!(!d.worktree);
        });
    }
}
