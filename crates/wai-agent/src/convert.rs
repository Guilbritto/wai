//! Conversions between ACP schema types and wai's domain types.

use agent_client_protocol::schema::MaybeUndefined;
use agent_client_protocol::v1::schema as acp;
use serde::Serialize;

use crate::types::*;

/// The snake_case wire name of a schema enum (`ToolKind::Edit` → `"edit"`).
fn wire_name(value: &impl Serialize) -> String {
    match serde_json::to_value(value) {
        Ok(serde_json::Value::String(name)) => name,
        other => format!("{other:?}"),
    }
}

fn text(block: acp::ContentBlock) -> Option<String> {
    match block {
        acp::ContentBlock::Text(t) => Some(t.text),
        _ => None,
    }
}

pub(crate) fn session_event(update: acp::SessionUpdate) -> Option<SessionEvent> {
    use acp::SessionUpdate as U;
    Some(match update {
        U::UserMessageChunk(c) => SessionEvent::UserText(text(c.content)?),
        U::AgentMessageChunk(c) => SessionEvent::AgentText(text(c.content)?),
        U::AgentThoughtChunk(c) => SessionEvent::AgentThought(text(c.content)?),
        U::ToolCall(t) => SessionEvent::ToolCall(ToolCallView {
            id: t.tool_call_id.to_string(),
            title: t.title,
            kind: wire_name(&t.kind),
            status: tool_status(t.status),
            content: tool_contents(t.content),
        }),
        U::ToolCallUpdate(u) => SessionEvent::ToolCallUpdate(ToolCallPatch {
            id: u.tool_call_id.to_string(),
            title: u.fields.title,
            status: u.fields.status.map(tool_status),
            content: u.fields.content.map(tool_contents),
        }),
        U::Plan(p) => SessionEvent::Plan(
            p.entries
                .into_iter()
                .map(|e| PlanEntry {
                    status: wire_name(&e.status),
                    content: e.content,
                })
                .collect(),
        ),
        U::UsageUpdate(u) => SessionEvent::Usage {
            used: u.used,
            size: u.size,
            cost_usd: u.cost.filter(|c| c.currency == "USD").map(|c| c.amount),
            rate_limit: u.meta.and_then(|mut m| m.remove("_claude/rateLimit")),
        },
        U::SessionInfoUpdate(i) => match i.title {
            MaybeUndefined::Value(title) => SessionEvent::Title(title),
            _ => return None,
        },
        U::CurrentModeUpdate(m) => SessionEvent::ModeChanged(m.current_mode_id.to_string()),
        U::AvailableCommandsUpdate(c) => SessionEvent::Commands(
            c.available_commands
                .into_iter()
                .map(|c| CommandInfo {
                    name: c.name,
                    description: c.description,
                })
                .collect(),
        ),
        _ => return None,
    })
}

fn tool_status(status: acp::ToolCallStatus) -> ToolStatus {
    match status {
        acp::ToolCallStatus::InProgress => ToolStatus::InProgress,
        acp::ToolCallStatus::Completed => ToolStatus::Completed,
        acp::ToolCallStatus::Failed => ToolStatus::Failed,
        _ => ToolStatus::Pending,
    }
}

fn tool_contents(content: Vec<acp::ToolCallContent>) -> Vec<ToolContent> {
    content.into_iter().filter_map(tool_content).collect()
}

fn tool_content(content: acp::ToolCallContent) -> Option<ToolContent> {
    match content {
        acp::ToolCallContent::Content(c) => text(c.content).map(ToolContent::Text),
        acp::ToolCallContent::Diff(d) => Some(ToolContent::Diff(file_diff(d))),
        acp::ToolCallContent::Terminal(t) => Some(ToolContent::Terminal(t.terminal_id.to_string())),
        _ => None,
    }
}

fn file_diff(d: acp::Diff) -> FileDiff {
    FileDiff {
        path: d.path,
        old_text: d.old_text,
        new_text: d.new_text,
    }
}

pub(crate) fn permission_request(
    id: PermissionRequestId,
    req: acp::RequestPermissionRequest,
) -> PermissionRequest {
    let fields = req.tool_call.fields;
    PermissionRequest {
        id,
        session: req.session_id.to_string(),
        tool_call_id: req.tool_call.tool_call_id.to_string(),
        title: fields.title.unwrap_or_default(),
        diff: fields.content.into_iter().flatten().find_map(|c| match c {
            acp::ToolCallContent::Diff(d) => Some(file_diff(d)),
            _ => None,
        }),
        options: req
            .options
            .into_iter()
            .map(|o| PermissionChoice {
                id: o.option_id.to_string(),
                name: o.name,
                kind: match o.kind {
                    acp::PermissionOptionKind::AllowOnce => ChoiceKind::AllowOnce,
                    acp::PermissionOptionKind::AllowAlways => ChoiceKind::AllowAlways,
                    acp::PermissionOptionKind::RejectAlways => ChoiceKind::RejectAlways,
                    // ponytail: unknown future kinds are shown as the most conservative choice.
                    _ => ChoiceKind::RejectOnce,
                },
            })
            .collect(),
    }
}

pub(crate) fn stop_reason(reason: acp::StopReason) -> StopReason {
    match reason {
        acp::StopReason::EndTurn => StopReason::EndTurn,
        acp::StopReason::Cancelled => StopReason::Cancelled,
        acp::StopReason::MaxTokens => StopReason::MaxTokens,
        acp::StopReason::Refusal => StopReason::Refusal,
        other => StopReason::Other(wire_name(&other)),
    }
}

pub(crate) fn session_started(
    id: SessionId,
    modes: Option<acp::SessionModeState>,
) -> SessionStarted {
    let (modes, current_mode) = match modes {
        Some(m) => (
            m.available_modes
                .into_iter()
                .map(|m| ModeInfo {
                    id: m.id.to_string(),
                    name: m.name,
                })
                .collect(),
            Some(m.current_mode_id.to_string()),
        ),
        None => (Vec::new(), None),
    };
    SessionStarted {
        id,
        modes,
        current_mode,
    }
}

pub(crate) fn session_summary(info: acp::SessionInfo) -> SessionSummary {
    SessionSummary {
        id: info.session_id.to_string(),
        cwd: info.cwd,
        title: info.title,
        updated_at: info.updated_at,
    }
}

pub(crate) fn content_block(block: PromptBlock) -> acp::ContentBlock {
    match block {
        PromptBlock::Text(t) => acp::ContentBlock::Text(acp::TextContent::new(t)),
        PromptBlock::Image { mime, base64 } => {
            acp::ContentBlock::Image(acp::ImageContent::new(base64, mime))
        }
        PromptBlock::Resource { path } => {
            let name = path.file_name().map_or_else(
                || path.display().to_string(),
                |n| n.to_string_lossy().into_owned(),
            );
            // ponytail: relative paths are not valid file URLs; fall back to a raw `file://` string.
            let uri = url::Url::from_file_path(&path)
                .map_or_else(|()| format!("file://{}", path.display()), String::from);
            acp::ContentBlock::ResourceLink(acp::ResourceLink::new(name, uri))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resource_becomes_file_uri_link() {
        let block = content_block(PromptBlock::Resource {
            path: std::env::temp_dir().join("a b.md"),
        });
        let acp::ContentBlock::ResourceLink(link) = block else {
            panic!("{block:?}")
        };
        assert_eq!(link.name, "a b.md");
        assert!(link.uri.starts_with("file:///"), "{}", link.uri);
        assert!(link.uri.ends_with("/a%20b.md"), "{}", link.uri);
    }

    #[test]
    fn image_becomes_image_block() {
        let block = content_block(PromptBlock::Image {
            mime: "image/png".into(),
            base64: "AAAA".into(),
        });
        assert_eq!(
            block,
            acp::ContentBlock::Image(acp::ImageContent::new("AAAA", "image/png"))
        );
    }
}
