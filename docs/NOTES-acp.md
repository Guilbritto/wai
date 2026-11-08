# ACP notes (agent-client-protocol 5.0.0, protocol v1)

Verified against `@agentclientprotocol/claude-agent-acp@0.101.0` in `spikes/s1-acp` (see `docs/spikes/S1-acp.md`).

## Dependency

```toml
agent-client-protocol = { version = "5", features = ["process"] }   # process = AcpAgent (spawn a subprocess)
```

The SDK is runtime-agnostic: `smol::block_on` and GPUI's executors work, so tokio is not needed. Use types from
`agent_client_protocol::v1::schema::*`. `ProtocolVersion` comes from `agent_client_protocol::schema`.

## Spawning and connecting

```rust
use agent_client_protocol::{AcpAgent, Agent, schema::ProtocolVersion};
use agent_client_protocol::v1::{ConnectionTo, schema::*};

let agent = AcpAgent::from_str("npx -y @agentclientprotocol/claude-agent-acp@0.101.0")?; // shell-words parsing
agent_client_protocol::Client
    .v1()
    .on_receive_notification(async move |n: SessionNotification, _cx| { /* n.session_id, n.update */ Ok(()) },
                             agent_client_protocol::on_receive_notification!())
    .on_receive_request(async move |req: RequestPermissionRequest, responder, _cx| {
        responder.respond(RequestPermissionResponse::new(
            RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(option_id))))
        // or RequestPermissionOutcome::Cancelled
    }, agent_client_protocol::on_receive_request!())
    .connect_with(agent, async move |cx: ConnectionTo<Agent>| {
        // The connection lives only while this closure runs. In wai the closure is an actor loop that
        // receives commands from a channel and calls cx.send_request(...).
        Ok(())
    })
    .await?;
```

- Handlers are `async` closures that must be `Send`. Forward events out through a channel (`async_channel`); do not
  touch UI state inside handlers.
- Requests: `cx.send_request(Req).block_task().await?` returns the typed response.
- Errors in handlers: `responder.respond_with_error(agent_client_protocol::Error::internal_error().data(msg))`.

## Requests used by wai

| Purpose | Request |
|---|---|
| Handshake | `InitializeRequest::new(ProtocolVersion::V1).client_capabilities(ClientCapabilities::new().fs(FileSystemCapabilities::new().read_text_file(true).write_text_file(true)))` → `agent_info`, `agent_capabilities` (`load_session`, `prompt_capabilities.image`, `session_capabilities.{list,resume,close,delete}`), `auth_methods` |
| New session | `NewSessionRequest::new(cwd)`; optional `.meta(Map)`. claude-agent-acp reads `_meta.claudeCode.options` (Agent SDK options) → `session_id`, `modes { current_mode_id, available_modes }` |
| Resume | `LoadSessionRequest::new(session_id, cwd)`. It replays history as `SessionNotification`s before returning |
| Prompt | `PromptRequest::new(session_id, vec![ContentBlock::Text(TextContent::new(text))])` → `stop_reason` (`EndTurn`, `Cancelled`, …). It resolves when the turn ends; updates arrive meanwhile |
| Mode | `SetSessionModeRequest::new(session_id, "default")` |
| History | `ListSessionsRequest::new().cursor(c)` → `sessions: Vec<SessionInfo { session_id, cwd, title, updated_at }>`, `next_cursor` |
| Cancel | `CancelNotification::new(session_id)`, sent as a notification (not yet exercised against Claude) |

## Session updates seen from Claude

- `AgentMessageChunk(ContentChunk { content: ContentBlock::Text })`: streamed in small pieces.
- `AgentThoughtChunk`, `UserMessageChunk` (replay during `load_session`).
- `ToolCall { tool_call_id, kind (Execute/Edit/Read/…), title, status, content }`, then
  `ToolCallUpdate { tool_call_id, fields { status, content, title, … } }`. An Edit carries
  `ToolCallContent::Diff { path, old_text: Option<String>, new_text }` before its permission request.
- `Plan { entries }`, `UsageUpdate { used, size, cost, meta }`. Meta keys: `_claude/model`,
  `_claude/rateLimit` (`status`, reset info), `_claude/origin`.
- `SessionInfoUpdate { title, updated_at }`, `AvailableCommandsUpdate` (slash commands and skills),
  `ConfigOptionUpdate`, `CurrentModeUpdate`.

## Permission options from Claude

The option kinds are `AllowOnce` ("Yes"), `AllowAlways` ("Yes, allow all edits in <dir>/ during this session")
and `RejectOnce` ("No"). Choose an option by `kind` or `option_id`, never by position.
