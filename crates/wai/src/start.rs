use gpui_kit::*;
use wai_agent::AgentError;
use wai_settings::shown;

use crate::agents::AgentStatus;
use crate::chat::{ChatSession, DraftEvent, DraftView, signed_out};
use crate::workspace::{Pane, Workspace};

impl Workspace {
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
        let model = self.settings.models.get(&agent).cloned();
        let draft = cx.new(|cx| DraftView::new(agent, folder, model, prompt, window, cx));
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
