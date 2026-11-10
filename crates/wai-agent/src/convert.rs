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
            locations: locations(t.locations),
            subagent: None,
        }),
        U::ToolCallUpdate(u) => SessionEvent::ToolCallUpdate(ToolCallPatch {
            id: u.tool_call_id.to_string(),
            title: u.fields.title,
            status: u.fields.status.map(tool_status),
            content: u.fields.content.map(tool_contents),
            locations: u.fields.locations.map(locations),
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
        U::ConfigOptionUpdate(u) => SessionEvent::ConfigChanged(config_options(u.config_options)),
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

pub(crate) fn subagent_patch(u: acp::SubagentUpdate) -> SubagentPatch {
    use acp::{IdleStopReason as R, StateUpdate as S};
    let state = match u.state {
        MaybeUndefined::Value(S::Running(_)) => Some(SubagentState::Running),
        MaybeUndefined::Value(S::RequiresAction(_)) => Some(SubagentState::NeedsYou),
        MaybeUndefined::Value(S::Idle(idle)) => Some(match idle.stop_reason {
            Some(R::EndTurn) => SubagentState::Done,
            Some(R::Cancelled) => SubagentState::Stopped,
            Some(R::Error(_)) => SubagentState::Failed,
            _ => SubagentState::Ended,
        }),
        MaybeUndefined::Value(_) => Some(SubagentState::Unknown),
        _ => None,
    };
    SubagentPatch {
        id: u.session_id.to_string(),
        title: match u.title {
            MaybeUndefined::Value(title) => Some(title),
            _ => None,
        },
        state,
    }
}

/// A subagent's message to `session`: its text blocks, joined. `None` for the session's own
/// outgoing message, or one without text. ponytail: `session_message_chunk` is not read, because
/// the adapter sends whole messages on v1.
pub(crate) fn subagent_reply(m: acp::SessionMessage, session: &str) -> Option<SessionEvent> {
    let id = match m.sender_session_id {
        MaybeUndefined::Value(id) if id.0.as_ref() != session => id.to_string(),
        _ => return None,
    };
    let MaybeUndefined::Value(content) = m.content else {
        return None;
    };
    let text: Vec<String> = content.into_iter().filter_map(text).collect();
    if text.is_empty() {
        return None;
    }
    Some(SessionEvent::SubagentReply {
        id,
        text: text.join("\n"),
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

fn locations(locations: Vec<acp::ToolCallLocation>) -> Vec<Location> {
    let location = |l: acp::ToolCallLocation| Location {
        path: l.path,
        line: l.line,
    };
    locations.into_iter().map(location).collect()
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
        subagent: None,
    }
}

/// A form tied to a session. Not one: `Err(Some(session))` when a field has a schema wai cannot
/// show (ACP: an unknown schema must not render as a known control), `Err(None)` for anything
/// that is no session's form (url mode, request scope, future modes).
pub(crate) fn question_request(
    id: QuestionId,
    req: acp::CreateElicitationRequest,
) -> Result<QuestionRequest, Option<SessionId>> {
    let acp::ElicitationMode::Form(form) = req.mode else {
        return Err(None);
    };
    let acp::ElicitationScope::Session(scope) = form.scope else {
        return Err(None);
    };
    let session = scope.session_id.to_string();
    let schema = form.requested_schema;
    let required = schema.required.unwrap_or_default();
    // ponytail: the schema's map is sorted by key, so the agent's field order is lost; Claude's
    // `question_<n>` keys sort right for up to 10 questions (AskUserQuestion asks at most 4).
    let fields = schema
        .properties
        .into_iter()
        .map(|(key, property)| {
            Some(QuestionField {
                required: required.contains(&key),
                ..question_field(key, &property)?
            })
        })
        .collect::<Option<_>>();
    let Some(fields) = fields else {
        return Err(Some(session));
    };
    Ok(QuestionRequest {
        id,
        session,
        message: req.message,
        fields,
        subagent: None,
    })
}

/// `None` for a schema wai cannot show.
fn question_field(key: String, property: &acp::ElicitationPropertySchema) -> Option<QuestionField> {
    use acp::ElicitationPropertySchema as P;
    let titled = |options: &[acp::EnumOption]| {
        options
            .iter()
            .map(|o| Choice {
                value: o.value.clone(),
                label: o.title.clone(),
                description: o.description.clone(),
            })
            .collect()
    };
    let plain = |values: &[String]| {
        values
            .iter()
            .map(|v| Choice {
                value: v.clone(),
                label: v.clone(),
                description: None,
            })
            .collect()
    };
    let (title, description, kind) = match property {
        P::String(s) => (
            &s.title,
            &s.description,
            match (&s.one_of, &s.enum_values) {
                (Some(options), _) => FieldKind::SingleChoice(titled(options)),
                (None, Some(values)) => FieldKind::SingleChoice(plain(values)),
                (None, None) => FieldKind::Text,
            },
        ),
        P::Number(n) => (&n.title, &n.description, FieldKind::Number),
        P::Integer(n) => (&n.title, &n.description, FieldKind::Number),
        P::Boolean(b) => (&b.title, &b.description, FieldKind::Boolean),
        P::Array(a) => match &a.items {
            acp::MultiSelectItems::String(items) => (
                &a.title,
                &a.description,
                FieldKind::MultiChoice(plain(&items.values)),
            ),
            acp::MultiSelectItems::Titled(items) => (
                &a.title,
                &a.description,
                FieldKind::MultiChoice(titled(&items.options)),
            ),
            _ => return None,
        },
        _ => return None,
    };
    Some(QuestionField {
        title: title.clone().unwrap_or_else(|| key.clone()),
        description: description.clone(),
        key,
        required: false,
        kind,
    })
}

pub(crate) fn question_response(answer: QuestionAnswer) -> acp::CreateElicitationResponse {
    let action = match answer {
        QuestionAnswer::Accept(content) => {
            // ponytail: values ACP v1 cannot carry (objects, null) are dropped; the form never
            // produces them.
            let content = content
                .into_iter()
                .filter_map(|(key, value)| Some((key, serde_json::from_value(value).ok()?)))
                .collect::<std::collections::BTreeMap<_, _>>();
            acp::ElicitationAction::Accept(acp::ElicitationAcceptAction::new().content(content))
        }
        QuestionAnswer::Decline => acp::ElicitationAction::Decline,
        QuestionAnswer::Cancel => acp::ElicitationAction::Cancel,
    };
    acp::CreateElicitationResponse::new(action)
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

/// ACP stdio servers: every agent must support that transport. A bare command is resolved on
/// PATH first: the agent starts the server in the project, where Windows would look first. One
/// that is not on PATH is left out.
pub(crate) fn mcp_servers(servers: Vec<McpServer>) -> Vec<acp::McpServer> {
    let stdio = |s: McpServer| {
        let Some(command) = wai_settings::find_program(&s.command) else {
            eprintln!(
                "wai: MCP server {}: {} is not on PATH",
                s.name,
                s.command.display()
            );
            return None;
        };
        let env = s.env.into_iter().map(|(k, v)| acp::EnvVariable::new(k, v));
        let server = acp::McpServerStdio::new(s.name, command)
            .args(s.args)
            .env(env.collect());
        Some(acp::McpServer::Stdio(server))
    };
    servers.into_iter().filter_map(stdio).collect()
}

pub(crate) fn session_started(
    id: SessionId,
    modes: Option<acp::SessionModeState>,
    config: Option<Vec<acp::SessionConfigOption>>,
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
        config: config_options(config.unwrap_or_default()),
    }
}

/// Select options only, grouped values flattened in order; booleans are dropped.
pub(crate) fn config_options(options: Vec<acp::SessionConfigOption>) -> Vec<ConfigOption> {
    use acp::SessionConfigOptionCategory as C;
    let option = |o: acp::SessionConfigOption| {
        let acp::SessionConfigKind::Select(select) = o.kind else {
            return None;
        };
        let values = match select.options {
            acp::SessionConfigSelectOptions::Ungrouped(values) => values,
            acp::SessionConfigSelectOptions::Grouped(groups) => {
                groups.into_iter().flat_map(|g| g.options).collect()
            }
            _ => return None,
        };
        Some(ConfigOption {
            id: o.id.to_string(),
            name: o.name,
            category: match o.category {
                Some(C::Mode) => ConfigCategory::Mode,
                Some(C::Model) => ConfigCategory::Model,
                Some(C::ThoughtLevel) => ConfigCategory::ThoughtLevel,
                _ => ConfigCategory::Other,
            },
            current: select.current_value.to_string(),
            values: values
                .into_iter()
                .map(|v| ConfigValue {
                    id: v.value.to_string(),
                    name: v.name,
                })
                .collect(),
        })
    };
    options.into_iter().filter_map(option).collect()
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
    fn mcp_servers_are_stdio_with_args_and_env() {
        let server = McpServer {
            name: "jira".into(),
            command: "/opt/jira-mcp".into(),
            args: vec!["--stdio".into(), "a b".into()],
            env: vec![("MODE".into(), "test".into())],
        };
        let wire = serde_json::to_value(mcp_servers(vec![server])).unwrap();
        assert_eq!(
            wire,
            serde_json::json!([{
                "name": "jira",
                "command": "/opt/jira-mcp",
                "args": ["--stdio", "a b"],
                "env": [{"name": "MODE", "value": "test"}],
            }])
        );
    }

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

    fn elicitation(json: serde_json::Value) -> acp::CreateElicitationRequest {
        serde_json::from_value(json).unwrap()
    }

    /// The form claude-agent-acp 0.101.0 builds for AskUserQuestion
    /// (`askUserQuestionsToCreateRequest`): a titled single-select (`oneOf`), a titled
    /// multi-select (`items.anyOf`) and a free-text "Other" per question, nothing required.
    #[test]
    fn ask_user_question_form_maps_to_fields() {
        let other = "Type your own answer, or add a note to the option you chose above (optional).";
        let request = elicitation(serde_json::json!({
            "mode": "form",
            "sessionId": "s1",
            "toolCallId": "toolu_1",
            "message": "Please answer the following questions.",
            "requestedSchema": {
                "type": "object",
                "properties": {
                    "question_0": {
                        "type": "string",
                        "title": "Database",
                        "description": "Which database?",
                        "oneOf": [
                            {"const": "Postgres", "title": "Postgres", "description": "Relational"},
                            {"const": "Redis", "title": "Redis"}
                        ]
                    },
                    "question_0_custom": {"type": "string", "title": "Other", "description": other},
                    "question_1": {
                        "type": "array",
                        "description": "Which features?",
                        "items": {"anyOf": [
                            {"const": "Auth", "title": "Auth"},
                            {"const": "Search", "title": "Search",
                             "_meta": {"_claude/askUserQuestionOption": {"preview": "…"}}}
                        ]}
                    },
                    "question_1_custom": {"type": "string", "title": "Other"}
                }
            }
        }));
        let q = question_request(9, request).expect("a form for a session");
        let described = |v: &str, d: &str| Choice {
            value: v.into(),
            label: v.into(),
            description: Some(d.into()),
        };
        assert_eq!((q.id, q.session.as_str()), (9, "s1"));
        assert_eq!(q.message, "Please answer the following questions.");
        let choice = |v: &str| Choice {
            value: v.into(),
            label: v.into(),
            description: None,
        };
        assert_eq!(
            q.fields,
            [
                QuestionField {
                    key: "question_0".into(),
                    title: "Database".into(),
                    description: Some("Which database?".into()),
                    required: false,
                    kind: FieldKind::SingleChoice(vec![
                        described("Postgres", "Relational"),
                        choice("Redis")
                    ]),
                },
                QuestionField {
                    key: "question_0_custom".into(),
                    title: "Other".into(),
                    description: Some(other.into()),
                    required: false,
                    kind: FieldKind::Text,
                },
                QuestionField {
                    key: "question_1".into(),
                    title: "question_1".into(),
                    description: Some("Which features?".into()),
                    required: false,
                    kind: FieldKind::MultiChoice(vec![choice("Auth"), choice("Search")]),
                },
                QuestionField {
                    key: "question_1_custom".into(),
                    title: "Other".into(),
                    description: None,
                    required: false,
                    kind: FieldKind::Text,
                },
            ]
        );
    }

    #[test]
    fn plain_schemas_map_to_fields() {
        let request = elicitation(serde_json::json!({
            "mode": "form",
            "sessionId": "s1",
            "message": "m",
            "requestedSchema": {
                "properties": {
                    "a_size": {"type": "string", "enum": ["S", "M"]},
                    "b_tags": {"type": "array", "items": {"type": "string", "enum": ["x"]}},
                    "c_ok": {"type": "boolean", "title": "OK"},
                    "d_count": {"type": "integer", "minimum": 1},
                    "e_ratio": {"type": "number"},
                    "f_email": {"type": "string", "format": "email"}
                },
                "required": ["d_count"]
            }
        }));
        let fields = question_request(1, request).unwrap().fields;
        let kinds: Vec<_> = fields
            .iter()
            .map(|f| (f.required, f.kind.clone()))
            .collect();
        let plain = |v: &str| Choice {
            value: v.into(),
            label: v.into(),
            description: None,
        };
        assert_eq!(
            kinds,
            [
                (false, FieldKind::SingleChoice(vec![plain("S"), plain("M")])),
                (false, FieldKind::MultiChoice(vec![plain("x")])),
                (false, FieldKind::Boolean),
                (true, FieldKind::Number),
                (false, FieldKind::Number),
                (false, FieldKind::Text),
            ]
        );
        assert_eq!(fields[2].title, "OK");
    }

    /// ACP: an unknown property schema must not render as a known control.
    #[test]
    fn forms_with_unknown_schemas_are_unsupported() {
        for unknown in [
            serde_json::json!({"type": "_shape", "sides": 3}),
            serde_json::json!({"type": "array", "items": {"type": "_shape"}}),
        ] {
            let request = elicitation(serde_json::json!({
                "mode": "form",
                "sessionId": "s1",
                "message": "m",
                "requestedSchema": {"properties": {"a": {"type": "string"}, "b": unknown}}
            }));
            assert_eq!(question_request(1, request), Err(Some("s1".into())));
        }
    }

    #[test]
    fn url_and_request_scoped_elicitations_are_not_questions() {
        let url = elicitation(serde_json::json!({
            "mode": "url", "sessionId": "s1", "message": "m",
            "elicitationId": "e1", "url": "https://example.com"
        }));
        assert_eq!(question_request(1, url), Err(None));
        let request_scoped = elicitation(serde_json::json!({
            "mode": "form", "requestId": 4, "message": "m",
            "requestedSchema": {"type": "object", "properties": {}}
        }));
        assert_eq!(question_request(1, request_scoped), Err(None));
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
