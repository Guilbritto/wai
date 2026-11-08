//! The connection thread: one ACP connection per agent, driven by a command loop.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::{ConnectionTo, Responder, schema as acp};
use agent_client_protocol::{
    AcpAgent, Agent, Client, Error, ErrorCode, JsonRpcRequest, is_connection_terminated,
    is_incoming_transport_closed,
};
use async_channel::{Receiver, Sender};

use crate::convert;
use crate::types::*;

type Reply<T> = Sender<Result<T, AgentError>>;
type Page = (Vec<SessionSummary>, Option<String>);

enum Command {
    NewSession {
        cwd: PathBuf,
        reply: Reply<SessionStarted>,
    },
    LoadSession {
        id: SessionId,
        cwd: PathBuf,
        reply: Reply<SessionStarted>,
    },
    ListSessions {
        cursor: Option<String>,
        reply: Reply<Page>,
    },
    SetMode {
        session: SessionId,
        mode: String,
        reply: Reply<()>,
    },
    Prompt {
        session: SessionId,
        blocks: Vec<PromptBlock>,
    },
    Cancel {
        session: SessionId,
    },
    Shutdown,
}

/// A cheap, cloneable handle to one agent process.
#[derive(Clone)]
pub struct AgentHandle {
    commands: Sender<Command>,
    broker: Arc<Broker>,
}

impl AgentHandle {
    /// Starts the agent on its own thread. Events arrive on the returned receiver,
    /// beginning with `Ready` and ending with `Exited`.
    pub fn spawn(spec: AgentSpec) -> (AgentHandle, Receiver<AgentEvent>) {
        let (commands, command_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        let broker = Arc::new(Broker {
            agent: spec.name.clone(),
            events,
            next_id: AtomicU64::new(1),
            pending: Mutex::default(),
        });
        let thread_broker = broker.clone();
        std::thread::spawn(move || smol::block_on(run(spec, command_rx, thread_broker)));
        (AgentHandle { commands, broker }, event_rx)
    }

    pub async fn new_session(&self, cwd: PathBuf) -> Result<SessionStarted, AgentError> {
        self.request(|reply| Command::NewSession { cwd, reply })
            .await
    }

    pub async fn load_session(
        &self,
        id: SessionId,
        cwd: PathBuf,
    ) -> Result<SessionStarted, AgentError> {
        self.request(|reply| Command::LoadSession { id, cwd, reply })
            .await
    }

    /// Lists every session the agent knows, following `next_cursor` to the end.
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, AgentError> {
        let mut all = Vec::new();
        let mut cursor = None;
        loop {
            let (page, next) = self
                .request(|reply| Command::ListSessions { cursor, reply })
                .await?;
            all.extend(page);
            match next {
                Some(next) => cursor = Some(next),
                None => return Ok(all),
            }
        }
    }

    pub async fn set_mode(&self, session: SessionId, mode: String) -> Result<(), AgentError> {
        self.request(|reply| Command::SetMode {
            session,
            mode,
            reply,
        })
        .await
    }

    /// Starts a turn. Its completion is reported as `AgentEvent::TurnEnded`.
    pub fn prompt(&self, session: SessionId, blocks: Vec<PromptBlock>) {
        self.send(Command::Prompt { session, blocks });
    }

    pub fn cancel(&self, session: SessionId) {
        self.send(Command::Cancel { session });
    }

    /// Answers a pending permission request; `None` cancels it.
    pub fn answer_permission(
        &self,
        id: PermissionRequestId,
        choice: Option<String>,
    ) -> Result<(), BrokerError> {
        self.broker.answer(id, choice)
    }

    pub fn shutdown(&self) {
        self.send(Command::Shutdown);
    }

    fn send(&self, command: Command) {
        // ponytail: a closed channel means the agent already exited and `Exited` was emitted.
        let _ = self.commands.try_send(command);
    }

    async fn request<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, AgentError> {
        let (reply, response) = async_channel::bounded(1);
        self.commands
            .send(command(reply))
            .await
            .map_err(|_| AgentError::Exited)?;
        // A dropped reply means the connection ended before the agent answered.
        response.recv().await.unwrap_or(Err(AgentError::Exited))
    }
}

struct Pending {
    session: SessionId,
    responder: Responder<acp::RequestPermissionResponse>,
}

/// Shared between the handle and the connection thread: the event sender and the
/// permission requests waiting for an answer.
struct Broker {
    agent: String,
    events: Sender<AgentEvent>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<PermissionRequestId, Pending>>,
}

impl Broker {
    fn emit(&self, event: AgentEvent) {
        // ponytail: the channel is unbounded, so this fails only when nobody listens anymore.
        let _ = self.events.try_send(event);
    }

    fn park(
        &self,
        request: acp::RequestPermissionRequest,
        responder: Responder<acp::RequestPermissionResponse>,
    ) {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let request = convert::permission_request(id, request);
        let pending = Pending {
            session: request.session.clone(),
            responder,
        };
        self.pending
            .lock()
            .expect("permission broker poisoned")
            .insert(id, pending);
        self.emit(AgentEvent::PermissionRequested(request));
    }

    fn answer(&self, id: PermissionRequestId, choice: Option<String>) -> Result<(), BrokerError> {
        let pending = self
            .pending
            .lock()
            .expect("permission broker poisoned")
            .remove(&id)
            .ok_or(BrokerError::Stale)?;
        let outcome = match choice {
            Some(option) => {
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(option))
            }
            None => acp::RequestPermissionOutcome::Cancelled,
        };
        let sent = pending
            .responder
            .respond(acp::RequestPermissionResponse::new(outcome));
        self.emit(AgentEvent::PermissionResolved { id });
        // The connection is gone, so the request no longer exists on the agent side either.
        sent.map_err(|_| BrokerError::Stale)
    }

    /// Answers `Cancelled` to every pending request that matches `session` (all when `None`).
    fn cancel_pending(&self, session: Option<&str>) {
        let cancelled: Vec<_> = self
            .pending
            .lock()
            .expect("permission broker poisoned")
            .extract_if(|_, p| session.is_none_or(|s| p.session == s))
            .collect();
        for (id, pending) in cancelled {
            // ponytail: a failed respond means the connection is closing; `Exited` reports it.
            let _ = pending
                .responder
                .respond(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ));
            self.emit(AgentEvent::PermissionResolved { id });
        }
    }

    fn end_turn(&self, session: SessionId, reason: StopReason) {
        self.cancel_pending(Some(&session));
        self.emit(AgentEvent::TurnEnded { session, reason });
    }

    fn agent_error(&self, error: Error) -> AgentError {
        if error.code == ErrorCode::AUTH_REQUIRED {
            self.emit(AgentEvent::AuthRequired {
                agent: self.agent.clone(),
            });
            AgentError::AuthRequired
        } else if is_incoming_transport_closed(&error) || is_connection_terminated(&error) {
            AgentError::Exited
        } else {
            AgentError::Protocol(error.to_string())
        }
    }
}

async fn run(spec: AgentSpec, commands: Receiver<Command>, broker: Arc<Broker>) {
    let result = connect(&spec, &commands, &broker).await;
    // Fail every queued request and refuse new ones before announcing the exit.
    commands.close();
    while commands.try_recv().is_ok() {}
    broker.cancel_pending(None);
    let error = match result {
        Ok(true) => None,
        Ok(false) => Some("the agent closed the connection".to_string()),
        Err(e) => Some(e.to_string()),
    };
    broker.emit(AgentEvent::Exited { error });
}

/// Runs the connection; `Ok(true)` means wai asked it to stop.
async fn connect(
    spec: &AgentSpec,
    commands: &Receiver<Command>,
    broker: &Arc<Broker>,
) -> Result<bool, Error> {
    let agent = AcpAgent::from_str(&spec.command)?;
    let updates = broker.clone();
    let permissions = broker.clone();
    Client
        .v1()
        .name(format!("wai-agent:{}", spec.name))
        .on_receive_notification(
            async move |n: acp::SessionNotification, _cx| {
                if let Some(event) = convert::session_event(n.update) {
                    updates.emit(AgentEvent::Update {
                        session: n.session_id.to_string(),
                        event,
                    });
                }
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: acp::RequestPermissionRequest, responder, _cx| {
                // Parked until the user answers; the dispatch loop moves on at once.
                permissions.park(request, responder);
                Ok(())
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, async |cx: ConnectionTo<Agent>| {
            actor_loop(cx, commands, broker).await
        })
        .await
}

async fn actor_loop(
    cx: ConnectionTo<Agent>,
    commands: &Receiver<Command>,
    broker: &Arc<Broker>,
) -> Result<bool, Error> {
    let init = cx
        .send_request(
            acp::InitializeRequest::new(ProtocolVersion::V1)
                .client_info(acp::Implementation::new("wai", env!("CARGO_PKG_VERSION"))),
        )
        .block_task()
        .await?;
    let caps = init.agent_capabilities;
    let (name, version) = match init.agent_info {
        Some(info) => (info.name, info.version),
        None => (broker.agent.clone(), String::new()),
    };
    broker.emit(AgentEvent::Ready {
        name,
        version,
        caps: AgentCaps {
            load_session: caps.load_session,
            list_sessions: caps.session_capabilities.list.is_some(),
            images: caps.prompt_capabilities.image,
        },
    });

    loop {
        let next = smol::future::or(async { Some(commands.recv().await) }, async {
            cx.incoming_closed().await;
            None
        })
        .await;
        let command = match next {
            Some(Ok(command)) => command,
            // Every handle was dropped.
            Some(Err(_)) => return Ok(true),
            // The agent closed its end on its own.
            None => return Ok(false),
        };
        match command {
            Command::NewSession { cwd, reply } => {
                request(&cx, broker, acp::NewSessionRequest::new(cwd), reply, |r| {
                    convert::session_started(r.session_id.to_string(), r.modes)
                })?
            }
            Command::LoadSession { id, cwd, reply } => request(
                &cx,
                broker,
                acp::LoadSessionRequest::new(id.clone(), cwd),
                reply,
                |r| convert::session_started(id, r.modes),
            )?,
            Command::ListSessions { cursor, reply } => request(
                &cx,
                broker,
                acp::ListSessionsRequest::new().cursor(cursor),
                reply,
                |r| {
                    let page = r.sessions.into_iter().map(convert::session_summary);
                    (page.collect(), r.next_cursor)
                },
            )?,
            Command::SetMode {
                session,
                mode,
                reply,
            } => request(
                &cx,
                broker,
                acp::SetSessionModeRequest::new(session, mode),
                reply,
                |_| (),
            )?,
            Command::Prompt { session, blocks } => {
                let blocks = blocks.into_iter().map(convert::content_block).collect();
                let turn = broker.clone();
                // Ordered after the turn's updates, and it does not block the loop.
                cx.prepare_request(acp::PromptRequest::new(session.clone(), blocks))
                    .on_receiving_result(async move |result| {
                        let reason = match result {
                            Ok(response) => convert::stop_reason(response.stop_reason),
                            // claude-agent-acp reports a signed-out user here, not at session/new.
                            Err(e) => match turn.agent_error(e.clone()) {
                                AgentError::AuthRequired => {
                                    StopReason::Other("auth_required".into())
                                }
                                _ => StopReason::Other(e.to_string()),
                            },
                        };
                        turn.end_turn(session, reason);
                        Ok(())
                    })?
            }
            Command::Cancel { session } => {
                broker.cancel_pending(Some(&session));
                cx.send_notification(acp::CancelNotification::new(session))?;
            }
            Command::Shutdown => return Ok(true),
        }
    }
}

/// Sends `req` without blocking the loop; the mapped response goes to `reply`, in
/// order with the notifications that preceded it (so `load_session`'s replay
/// arrives before it returns).
fn request<Req: JsonRpcRequest, T: Send + 'static>(
    cx: &ConnectionTo<Agent>,
    broker: &Arc<Broker>,
    req: Req,
    reply: Reply<T>,
    map: impl FnOnce(Req::Response) -> T + Send + 'static,
) -> Result<(), Error> {
    let broker = broker.clone();
    cx.prepare_request(req)
        .on_receiving_result(async move |result| {
            // ponytail: a failed send means the caller stopped waiting.
            let _ = reply.try_send(result.map(map).map_err(|e| broker.agent_error(e)));
            Ok(())
        })
}
