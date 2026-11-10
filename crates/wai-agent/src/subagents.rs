//! Subagent sessions (`clientCapabilities.subagents`): everything a child session reports is
//! sent to the open session it belongs to, its root. The rest of wai never sees a child id.

use std::collections::HashMap;

use agent_client_protocol::v1::schema as acp;

use crate::convert;
use crate::types::*;

#[derive(Default)]
pub(crate) struct Subagents {
    roots: HashMap<SessionId, SessionId>,
}

impl Subagents {
    /// The open session a subagent session belongs to; `None` for a session of wai's own.
    pub(crate) fn root(&self, session: &str) -> Option<&SessionId> {
        self.roots.get(session)
    }

    /// The event for wai and the open session it is for.
    pub(crate) fn route(
        &mut self,
        session: SessionId,
        update: acp::SessionUpdate,
    ) -> Option<(SessionId, SessionEvent)> {
        use acp::SessionUpdate as U;
        let root = self.roots.get(&session).cloned();
        match (update, root) {
            (U::SubagentUpdate(u), root) => {
                let root = root.unwrap_or(session);
                self.roots.insert(u.session_id.to_string(), root.clone());
                Some((root, SessionEvent::Subagent(convert::subagent_patch(u))))
            }
            (U::SessionMessage(m), None) => {
                convert::subagent_reply(m, &session).map(|e| (session, e))
            }
            (update, None) => Some((session, convert::session_event(update)?)),
            (update, Some(root)) => match convert::session_event(update)? {
                SessionEvent::ToolCall(mut v) => {
                    v.subagent = Some(session);
                    Some((root, SessionEvent::ToolCall(v)))
                }
                e @ SessionEvent::ToolCallUpdate(_) => Some((root, e)),
                // ponytail: a subagent's text, plan, usage and commands are not shown.
                _ => None,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    fn sub(id: &str, title: &str, state: acp::StateUpdate) -> acp::SessionUpdate {
        acp::SessionUpdate::SubagentUpdate(
            acp::SubagentUpdate::new(id.to_string())
                .title(title.to_string())
                .state(state),
        )
    }

    fn running() -> acp::StateUpdate {
        acp::StateUpdate::Running(acp::RunningStateUpdate::new())
    }

    fn idle(reason: Option<acp::IdleStopReason>) -> acp::StateUpdate {
        acp::StateUpdate::Idle(acp::IdleStateUpdate::new().stop_reason(reason))
    }

    fn tool_call(id: &str, title: &str) -> acp::SessionUpdate {
        acp::SessionUpdate::ToolCall(acp::ToolCall::new(id.to_string(), title.to_string()))
    }

    fn tool_call_update(id: &str, status: acp::ToolCallStatus) -> acp::SessionUpdate {
        let fields = acp::ToolCallUpdateFields::new().status(status);
        acp::SessionUpdate::ToolCallUpdate(acp::ToolCallUpdate::new(id.to_string(), fields))
    }

    fn agent_chunk(text: &str) -> acp::SessionUpdate {
        let block = acp::ContentBlock::Text(acp::TextContent::new(text));
        acp::SessionUpdate::AgentMessageChunk(acp::ContentChunk::new(block))
    }

    fn plan() -> acp::SessionUpdate {
        let entry = acp::PlanEntry::new(
            "Sub step",
            acp::PlanEntryPriority::Medium,
            acp::PlanEntryStatus::Pending,
        );
        acp::SessionUpdate::Plan(acp::Plan::new(vec![entry]))
    }

    fn call_of(routed: Option<(SessionId, SessionEvent)>) -> (SessionId, ToolCallView) {
        match routed {
            Some((root, SessionEvent::ToolCall(v))) => (root, v),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn subagent_updates_route_to_the_root() {
        let mut s = Subagents::default();
        let announced = s.route("r".into(), sub("c", "Explore", running()));
        assert_eq!(
            announced,
            Some((
                "r".into(),
                SessionEvent::Subagent(SubagentPatch {
                    id: "c".into(),
                    title: Some("Explore".into()),
                    state: Some(SubagentState::Running),
                })
            ))
        );
        let (root, v) = call_of(s.route("c".into(), tool_call("t1", "Read")));
        assert_eq!((root.as_str(), v.subagent.as_deref()), ("r", Some("c")));
        let update = s.route(
            "c".into(),
            tool_call_update("t1", acp::ToolCallStatus::Completed),
        );
        assert!(
            matches!(&update, Some((root, SessionEvent::ToolCallUpdate(_))) if root == "r"),
            "{update:?}"
        );
        assert_eq!(s.route("c".into(), agent_chunk("x")), None);
        assert_eq!(s.route("c".into(), plan()), None);
        assert_eq!(
            s.route("r".into(), agent_chunk("hi")),
            Some(("r".into(), SessionEvent::AgentText("hi".into())))
        );
    }

    /// Review focus 4: a subagent's subagent shows under the same session.
    #[test]
    fn nested_subagent_routes_to_the_root() {
        let mut s = Subagents::default();
        s.route("r".into(), sub("c", "Explore", running()));
        let announced = s.route("c".into(), sub("g", "Grep", running()));
        assert!(
            matches!(&announced, Some((root, SessionEvent::Subagent(p))) if root == "r" && p.id == "g"),
            "{announced:?}"
        );
        let (root, v) = call_of(s.route("g".into(), tool_call("t2", "Grep")));
        assert_eq!((root.as_str(), v.subagent.as_deref()), ("r", Some("g")));
        assert_eq!(s.root("g").map(String::as_str), Some("r"));
        assert_eq!(s.root("r"), None);
    }

    /// Review focus 3: interleaved calls keep their own subagent.
    #[test]
    fn parallel_subagents_stay_apart() {
        let mut s = Subagents::default();
        s.route("r".into(), sub("a", "A", running()));
        s.route("r".into(), sub("b", "B", running()));
        let tagged = |s: &mut Subagents, from: &str, id: &str| {
            call_of(s.route(from.into(), tool_call(id, "Read")))
                .1
                .subagent
        };
        assert_eq!(tagged(&mut s, "a", "t1").as_deref(), Some("a"));
        assert_eq!(tagged(&mut s, "b", "t2").as_deref(), Some("b"));
        assert_eq!(tagged(&mut s, "a", "t3").as_deref(), Some("a"));
    }

    #[test]
    fn states_and_replies_map() {
        use acp::IdleStopReason as R;
        let other_reason = R::Other(acp::OtherStopReason::new("_x", BTreeMap::new()));
        let other_state =
            acp::StateUpdate::Other(acp::OtherStateUpdate::new("_y", BTreeMap::new()));
        let rows = [
            (running(), SubagentState::Running),
            (
                acp::StateUpdate::RequiresAction(acp::RequiresActionStateUpdate::new()),
                SubagentState::NeedsYou,
            ),
            (idle(Some(R::EndTurn)), SubagentState::Done),
            (idle(Some(R::Cancelled)), SubagentState::Stopped),
            (
                idle(Some(R::Error(acp::ErrorStopReason::new()))),
                SubagentState::Failed,
            ),
            (idle(None), SubagentState::Ended),
            (idle(Some(R::MaxTokens)), SubagentState::Ended),
            (idle(Some(other_reason)), SubagentState::Ended),
            (
                acp::StateUpdate::Unknown(acp::UnknownStateUpdate::new()),
                SubagentState::Unknown,
            ),
            (other_state, SubagentState::Unknown),
        ];
        for (state, want) in rows {
            let update = acp::SubagentUpdate::new("c").state(state.clone());
            assert_eq!(
                convert::subagent_patch(update).state,
                Some(want),
                "{state:?}"
            );
        }
        assert_eq!(
            convert::subagent_patch(acp::SubagentUpdate::new("c")),
            SubagentPatch {
                id: "c".into(),
                title: None,
                state: None,
            }
        );

        let text = |t: &str| acp::ContentBlock::Text(acp::TextContent::new(t));
        let message = |from: &str| {
            acp::SessionUpdate::SessionMessage(
                acp::SessionMessage::new("m")
                    .sender_session_id(acp::SessionId::new(from))
                    .recipient_session_id(acp::SessionId::new(if from == "c" { "r" } else { "c" }))
                    .content(vec![text("found"), text("it")]),
            )
        };
        let mut s = Subagents::default();
        assert_eq!(
            s.route("r".into(), message("c")),
            Some((
                "r".into(),
                SessionEvent::SubagentReply {
                    id: "c".into(),
                    text: "found\nit".into(),
                }
            ))
        );
        assert_eq!(s.route("r".into(), message("r")), None);
    }
}
