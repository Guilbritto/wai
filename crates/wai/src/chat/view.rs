use std::path::PathBuf;

use gpui_kit::component::v_flex;
use gpui_kit::*;
use wai_agent::{PermissionRequest, SessionEvent, StopReason};

/// Stand-in until Task 1.6: shows the session title and ignores its events.
#[expect(dead_code, reason = "Task 1.6 reads agent and cwd")]
pub struct ChatSession {
    pub agent: String,
    pub id: String,
    pub cwd: PathBuf,
}

impl ChatSession {
    pub fn handle_event(&mut self, _event: &SessionEvent, _cx: &mut Context<Self>) {}

    pub fn handle_permission(&mut self, _request: PermissionRequest, _cx: &mut Context<Self>) {}

    pub fn handle_turn_ended(&mut self, _reason: &StopReason, _cx: &mut Context<Self>) {}
}

impl Render for ChatSession {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let short: String = self.id.chars().take(8).collect();
        v_flex().size_full().p_4().child(format!("Session {short}"))
    }
}
