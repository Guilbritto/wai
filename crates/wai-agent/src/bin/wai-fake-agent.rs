//! A scripted ACP agent for wai's tests. Every flag is optional:
//!
//! - `--reply <text>`: the turn's answer, streamed one word per chunk
//!   ("one", " two", " three"); default `Hello from fake agent`.
//! - `--chunk-delay-ms <n>`: pause before each chunk.
//! - `--ask-permission`: send an Edit tool call for `notes.md`, ask permission and
//!   reply `chose:<option_id>` or `chose:cancelled`.
//! - `--crash-after <n>`: after n chunks print `fake agent crashed` to stderr and exit 3.
//! - `--require-auth`: `session/new` fails with the ACP auth-required error.
//! - `--require-auth-on-prompt`: `session/prompt` fails with the ACP auth-required error.
//! - `--flood <bytes>`: start the turn with one tool call carrying that many bytes of `x\n`.
//! - `--sessions <n>`: `session/list` returns n sessions titled `Session <i>`, 10 per page.
//! - `--sessions-cwd <path>`: the cwd of those sessions; default `/tmp`.
//! - `--pid-file <path>`: write the process id there at start.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::v1::{ConnectionTo, schema as acp};
use agent_client_protocol::{Agent, Client, Error, Stdio};

const PAGE: usize = 10;

struct Opts {
    reply: String,
    chunk_delay: Duration,
    ask_permission: bool,
    crash_after: Option<usize>,
    require_auth: bool,
    require_auth_on_prompt: bool,
    flood: Option<usize>,
    sessions: usize,
    sessions_cwd: PathBuf,
    pid_file: Option<PathBuf>,
}

fn parse_args() -> Opts {
    let mut opts = Opts {
        reply: "Hello from fake agent".into(),
        chunk_delay: Duration::ZERO,
        ask_permission: false,
        crash_after: None,
        require_auth: false,
        require_auth_on_prompt: false,
        flood: None,
        sessions: 0,
        sessions_cwd: "/tmp".into(),
        pid_file: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(flag) = args.next() {
        let mut value = || {
            args.next()
                .unwrap_or_else(|| panic!("{flag} needs a value"))
        };
        let number = |v: String| -> usize {
            v.parse()
                .unwrap_or_else(|_| panic!("{flag}: bad number {v}"))
        };
        match flag.as_str() {
            "--reply" => opts.reply = value(),
            "--chunk-delay-ms" => opts.chunk_delay = Duration::from_millis(number(value()) as u64),
            "--ask-permission" => opts.ask_permission = true,
            "--crash-after" => opts.crash_after = Some(number(value())),
            "--require-auth" => opts.require_auth = true,
            "--require-auth-on-prompt" => opts.require_auth_on_prompt = true,
            "--flood" => opts.flood = Some(number(value())),
            "--sessions" => opts.sessions = number(value()),
            "--sessions-cwd" => opts.sessions_cwd = value().into(),
            "--pid-file" => opts.pid_file = Some(value().into()),
            other => panic!("unknown flag {other}"),
        }
    }
    opts
}

struct Fake {
    opts: Opts,
    next_session: AtomicU64,
    cancelled: Mutex<HashSet<String>>,
}

impl Fake {
    fn is_cancelled(&self, session: &acp::SessionId) -> bool {
        self.cancelled
            .lock()
            .expect("cancel set poisoned")
            .contains(&*session.0)
    }

    async fn turn(
        &self,
        cx: &ConnectionTo<Client>,
        session: &acp::SessionId,
    ) -> Result<acp::StopReason, Error> {
        if let Some(bytes) = self.opts.flood {
            let mut text = "x\n".repeat(bytes / 2);
            if bytes % 2 == 1 {
                text.push('x');
            }
            let call = acp::ToolCall::new("flood", "Flood")
                .kind(acp::ToolKind::Execute)
                .status(acp::ToolCallStatus::Completed)
                .content(vec![acp::ToolCallContent::Content(acp::Content::new(
                    text_block(text),
                ))]);
            send(cx, session, acp::SessionUpdate::ToolCall(call))?;
        }
        let reply = if self.opts.ask_permission {
            format!("chose:{}", self.ask_permission(cx, session).await?)
        } else {
            self.opts.reply.clone()
        };
        for (i, word) in reply.split(' ').enumerate() {
            if !self.opts.chunk_delay.is_zero() {
                smol::Timer::after(self.opts.chunk_delay).await;
            }
            if self.is_cancelled(session) {
                return Ok(acp::StopReason::Cancelled);
            }
            let text = if i == 0 {
                word.to_string()
            } else {
                format!(" {word}")
            };
            send(
                cx,
                session,
                acp::SessionUpdate::AgentMessageChunk(chunk(text)),
            )?;
            if self.opts.crash_after == Some(i + 1) {
                eprintln!("fake agent crashed");
                std::process::exit(3);
            }
        }
        Ok(acp::StopReason::EndTurn)
    }

    async fn ask_permission(
        &self,
        cx: &ConnectionTo<Client>,
        session: &acp::SessionId,
    ) -> Result<String, Error> {
        let diff = acp::ToolCallContent::Diff(acp::Diff::new("notes.md", "hi\n"));
        let call = acp::ToolCall::new("edit-1", "Write notes.md")
            .kind(acp::ToolKind::Edit)
            .status(acp::ToolCallStatus::Pending)
            .content(vec![diff.clone()]);
        send(cx, session, acp::SessionUpdate::ToolCall(call))?;

        let fields = acp::ToolCallUpdateFields::new()
            .title("Write notes.md".to_string())
            .kind(acp::ToolKind::Edit)
            .content(vec![diff]);
        let options = vec![
            acp::PermissionOption::new("allow-once", "Yes", acp::PermissionOptionKind::AllowOnce),
            acp::PermissionOption::new(
                "allow-always",
                "Yes, always",
                acp::PermissionOptionKind::AllowAlways,
            ),
            acp::PermissionOption::new("reject-once", "No", acp::PermissionOptionKind::RejectOnce),
        ];
        let request = acp::RequestPermissionRequest::new(
            session.clone(),
            acp::ToolCallUpdate::new("edit-1", fields),
            options,
        );
        let response = cx.send_request(request).block_task().await?;
        Ok(match response.outcome {
            acp::RequestPermissionOutcome::Selected(s) => s.option_id.to_string(),
            _ => "cancelled".into(),
        })
    }
}

fn text_block(text: impl Into<String>) -> acp::ContentBlock {
    acp::ContentBlock::Text(acp::TextContent::new(text))
}

fn chunk(text: impl Into<String>) -> acp::ContentChunk {
    acp::ContentChunk::new(text_block(text))
}

fn send(
    cx: &ConnectionTo<Client>,
    session: &acp::SessionId,
    update: acp::SessionUpdate,
) -> Result<(), Error> {
    cx.send_notification(acp::SessionNotification::new(session.clone(), update))
}

fn modes() -> acp::SessionModeState {
    acp::SessionModeState::new(
        "default",
        vec![
            acp::SessionMode::new("default", "Default"),
            acp::SessionMode::new("plan", "Plan"),
        ],
    )
}

fn main() -> Result<(), Error> {
    let opts = parse_args();
    if let Some(path) = &opts.pid_file {
        std::fs::write(path, std::process::id().to_string()).expect("write pid file");
    }
    let fake = Arc::new(Fake {
        opts,
        next_session: AtomicU64::new(1),
        cancelled: Mutex::default(),
    });
    smol::block_on(serve(fake))
}

async fn serve(fake: Arc<Fake>) -> Result<(), Error> {
    Agent
        .v1()
        .name("wai-fake-agent")
        .on_receive_request(
            async |request: acp::InitializeRequest, responder, _cx| {
                let caps = acp::AgentCapabilities::new()
                    .load_session(true)
                    .prompt_capabilities(acp::PromptCapabilities::new().image(true))
                    .session_capabilities(
                        acp::SessionCapabilities::new().list(acp::SessionListCapabilities::new()),
                    );
                responder.respond(
                    acp::InitializeResponse::new(request.protocol_version)
                        .agent_info(acp::Implementation::new(
                            "wai-fake-agent",
                            env!("CARGO_PKG_VERSION"),
                        ))
                        .agent_capabilities(caps),
                )
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |_request: acp::NewSessionRequest, responder, _cx| {
                    if fake.opts.require_auth {
                        return responder.respond_with_error(Error::auth_required());
                    }
                    let id = format!("fake-{}", fake.next_session.fetch_add(1, Ordering::Relaxed));
                    responder.respond(acp::NewSessionResponse::new(id).modes(modes()))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async |request: acp::LoadSessionRequest, responder, cx: ConnectionTo<Client>| {
                let session = &request.session_id;
                send(
                    &cx,
                    session,
                    acp::SessionUpdate::UserMessageChunk(chunk("previous prompt")),
                )?;
                send(
                    &cx,
                    session,
                    acp::SessionUpdate::AgentMessageChunk(chunk("previous answer")),
                )?;
                responder.respond(acp::LoadSessionResponse::new().modes(modes()))
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |request: acp::ListSessionsRequest, responder, _cx| {
                    let Ok(start) = request.cursor.as_deref().unwrap_or("0").parse::<usize>()
                    else {
                        return responder.respond_with_error(Error::invalid_params());
                    };
                    let total = fake.opts.sessions;
                    let end = (start + PAGE).min(total);
                    let sessions = (start..end)
                        .map(|i| {
                            acp::SessionInfo::new(
                                format!("session-{i}"),
                                fake.opts.sessions_cwd.clone(),
                            )
                            .title(format!("Session {i}"))
                        })
                        .collect();
                    let next = (end < total).then(|| end.to_string());
                    responder.respond(acp::ListSessionsResponse::new(sessions).next_cursor(next))
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async |_request: acp::SetSessionModeRequest, responder, _cx| {
                responder.respond(acp::SetSessionModeResponse::new())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |request: acp::PromptRequest, responder, cx: ConnectionTo<Client>| {
                    if fake.opts.require_auth_on_prompt {
                        return responder.respond_with_error(Error::auth_required());
                    }
                    fake.cancelled
                        .lock()
                        .expect("cancel set poisoned")
                        .remove(&*request.session_id.0);
                    let fake = fake.clone();
                    let turn_cx = cx.clone();
                    // The turn runs outside the dispatch loop so cancels and permission answers get through.
                    cx.spawn(async move {
                        let reason = fake.turn(&turn_cx, &request.session_id).await?;
                        responder.respond(acp::PromptResponse::new(reason))
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_notification(
            async move |notification: acp::CancelNotification, _cx| {
                fake.cancelled
                    .lock()
                    .expect("cancel set poisoned")
                    .insert(notification.session_id.to_string());
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .connect_to(Stdio::new())
        .await
}
