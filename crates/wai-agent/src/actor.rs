//! The connection thread: one ACP connection per agent, driven by a command loop.

use std::collections::HashMap;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::{ConnectionTo, Responder, schema as acp};
use agent_client_protocol::{
    AcpAgent, AcpAgentConfig, Agent, Client, Error, ErrorCode, JsonRpcRequest, RequestCancellation,
    UntypedMessage, is_connection_terminated, is_incoming_transport_closed,
};
use async_channel::{Receiver, Sender};

use crate::convert;
use crate::subagents::Subagents;
use crate::types::*;

type Reply<T> = Sender<Result<T, AgentError>>;
type Page = (Vec<SessionSummary>, Option<String>);

enum Command {
    NewSession {
        cwd: PathBuf,
        mcp: Vec<McpServer>,
        reply: Reply<SessionStarted>,
    },
    LoadSession {
        id: SessionId,
        cwd: PathBuf,
        mcp: Vec<McpServer>,
        reply: Reply<SessionStarted>,
    },
    ForkSession {
        id: SessionId,
        cwd: PathBuf,
        mcp: Vec<McpServer>,
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
    SetConfig {
        session: SessionId,
        option: String,
        value: String,
        reply: Reply<Vec<ConfigOption>>,
    },
    Prompt {
        session: SessionId,
        blocks: Vec<PromptBlock>,
    },
    Steer {
        session: SessionId,
        blocks: Vec<PromptBlock>,
        reply: Reply<Result<bool, AgentError>>,
    },
    Cancel {
        session: SessionId,
    },
}

/// A cheap, cloneable handle to one agent process.
#[derive(Clone)]
pub struct AgentHandle {
    commands: Sender<Command>,
    broker: Arc<Broker>,
    /// Closed by `shutdown` (or once every handle is gone): the agent is killed.
    stop: Sender<()>,
    /// Closes when the connection thread ends, after `Exited`.
    done: Receiver<()>,
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
            subagents: Mutex::default(),
        });
        let (stop, stop_rx) = async_channel::bounded(1);
        let (done_tx, done) = async_channel::bounded::<()>(1);
        let thread_broker = broker.clone();
        std::thread::spawn(move || {
            let _done = done_tx;
            smol::block_on(run(spec, command_rx, stop_rx, thread_broker))
        });
        let handle = AgentHandle {
            commands,
            broker,
            stop,
            done,
        };
        (handle, event_rx)
    }

    /// `mcp`: the MCP servers the agent starts for the session (each of these calls takes them).
    pub async fn new_session(
        &self,
        cwd: PathBuf,
        mcp: Vec<McpServer>,
    ) -> Result<SessionStarted, AgentError> {
        self.request(|reply| Command::NewSession { cwd, mcp, reply })
            .await
    }

    pub async fn load_session(
        &self,
        id: SessionId,
        cwd: PathBuf,
        mcp: Vec<McpServer>,
    ) -> Result<SessionStarted, AgentError> {
        self.request(|reply| Command::LoadSession {
            id,
            cwd,
            mcp,
            reply,
        })
        .await
    }

    /// Copies session `id` into a new one (the agent must advertise `AgentCaps::fork`). The
    /// copy is not replayed here: `load_session` of the returned id does that.
    pub async fn fork_session(
        &self,
        id: SessionId,
        cwd: PathBuf,
        mcp: Vec<McpServer>,
    ) -> Result<SessionStarted, AgentError> {
        self.request(|reply| Command::ForkSession {
            id,
            cwd,
            mcp,
            reply,
        })
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

    pub async fn set_config(
        &self,
        session: SessionId,
        option: String,
        value: String,
    ) -> Result<Vec<ConfigOption>, AgentError> {
        self.request(|reply| Command::SetConfig {
            session,
            option,
            value,
            reply,
        })
        .await
    }

    /// Starts a turn. Its completion is reported as `AgentEvent::TurnEnded`.
    pub fn prompt(&self, session: SessionId, blocks: Vec<PromptBlock>) {
        self.send(Command::Prompt { session, blocks });
    }

    /// Sends `blocks` into the session's running turn (`_session/steering`; the agent must
    /// advertise `AgentCaps::steering`). `true`: the turn took them; `false`: no turn runs, so
    /// they must go as a prompt.
    pub async fn steer(
        &self,
        session: SessionId,
        blocks: Vec<PromptBlock>,
    ) -> Result<bool, AgentError> {
        self.request(|reply| Command::Steer {
            session,
            blocks,
            reply,
        })
        .await?
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

    /// Answers a pending question.
    pub fn answer_question(
        &self,
        id: QuestionId,
        answer: QuestionAnswer,
    ) -> Result<(), BrokerError> {
        self.broker.answer_question(id, answer)
    }

    /// Kills the agent now (its whole process group on unix): the SDK's graceful exit waits
    /// seconds, and a quitting app has about 200 ms. Resolves once it is gone, after `Exited`.
    pub fn shutdown(&self) -> impl Future<Output = ()> + use<> {
        self.stop.close();
        let done = self.done.clone();
        async move {
            // ponytail: it only ever closes.
            let _ = done.recv().await;
        }
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
    parked: Parked,
}

/// A request of the agent waiting for the user. `_watch` is dropped with the request: it ends
/// the watch for the agent withdrawing it.
enum Parked {
    Permission {
        responder: Responder<acp::RequestPermissionResponse>,
        _watch: Sender<()>,
    },
    Question {
        responder: Responder<acp::CreateElicitationResponse>,
        _watch: Sender<()>,
    },
}

impl Parked {
    /// Answers "cancelled" in the request's own terms; returns the event that reports it.
    fn cancel(self, id: u64) -> AgentEvent {
        // ponytail: a failed respond means the connection is closing; `Exited` reports it.
        match self {
            Parked::Permission { responder, .. } => {
                let _ = responder.respond(acp::RequestPermissionResponse::new(
                    acp::RequestPermissionOutcome::Cancelled,
                ));
                AgentEvent::PermissionResolved { id }
            }
            Parked::Question { responder, .. } => {
                let _ = responder.respond(convert::question_response(QuestionAnswer::Cancel));
                AgentEvent::QuestionResolved { id }
            }
        }
    }
}

/// Shared between the handle and the connection thread: the event sender and the
/// permission requests and questions waiting for an answer (one id sequence for both). A
/// request is parked under the session that asked: a subagent's survives its root's Stop.
struct Broker {
    agent: String,
    events: Sender<AgentEvent>,
    next_id: AtomicU64,
    pending: Mutex<HashMap<u64, Pending>>,
    /// Never locked together with `pending`.
    subagents: Mutex<Subagents>,
}

impl Broker {
    fn emit(&self, event: AgentEvent) {
        // ponytail: the channel is unbounded, so this fails only when nobody listens anymore.
        let _ = self.events.try_send(event);
    }

    /// A session's update, emitted for the open session it belongs to (a subagent's: its root).
    fn on_update(&self, n: acp::SessionNotification) {
        let routed = self
            .subagents
            .lock()
            .expect("broker poisoned")
            .route(n.session_id.to_string(), n.update);
        if let Some((session, event)) = routed {
            self.emit(AgentEvent::Update { session, event });
        }
    }

    fn park(&self, id: u64, session: SessionId, parked: Parked, event: AgentEvent) {
        self.pending
            .lock()
            .expect("broker poisoned")
            .insert(id, Pending { session, parked });
        self.emit(event);
    }

    /// A subagent's request shows in its root session, tagged with the subagent. Returns the
    /// session that asked, the one the request is parked under.
    fn attribute(&self, session: &mut SessionId, subagent: &mut Option<SessionId>) -> SessionId {
        let asked = session.clone();
        let subagents = self.subagents.lock().expect("broker poisoned");
        if let Some(root) = subagents.root(&asked) {
            *session = root.clone();
            *subagent = Some(asked.clone());
        }
        asked
    }

    /// Runs `cancel` if the agent withdraws the request (`$/cancel_request`) before `answered`
    /// closes (the request left the broker another way).
    fn watch_withdrawal(
        self: &Arc<Self>,
        cx: &ConnectionTo<Agent>,
        withdrawn: RequestCancellation,
        answered: Receiver<()>,
        cancel: impl FnOnce(&Broker) + Send + 'static,
    ) -> Result<(), Error> {
        let broker = self.clone();
        cx.spawn(async move {
            let withdrawn = async {
                withdrawn.cancelled().await;
                true
            };
            let answered = async {
                let _ = answered.recv().await;
                false
            };
            if smol::future::or(withdrawn, answered).await {
                cancel(&broker);
            }
            Ok(())
        })
    }

    /// Parks a permission request until the user answers. If the agent withdraws it
    /// (`$/cancel_request`, as it does for a foreground subagent it stops), it resolves as
    /// cancelled.
    fn park_permission(
        self: &Arc<Self>,
        request: acp::RequestPermissionRequest,
        responder: Responder<acp::RequestPermissionResponse>,
        cx: &ConnectionTo<Agent>,
    ) -> Result<(), Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut request = convert::permission_request(id, request);
        let asked = self.attribute(&mut request.session, &mut request.subagent);
        let withdrawn = responder.cancellation();
        let (watch, answered) = async_channel::bounded::<()>(1);
        let parked = Parked::Permission {
            responder,
            _watch: watch,
        };
        self.park(id, asked, parked, AgentEvent::PermissionRequested(request));
        self.watch_withdrawal(cx, withdrawn, answered, move |broker| {
            // ponytail: Stale means it was answered meanwhile.
            let _ = broker.answer(id, None);
        })
    }

    /// Parks a session's form until the user answers; anything else is declined at once (a
    /// session's form wai cannot show says so first). If the agent withdraws it
    /// (`$/cancel_request`), it resolves as cancelled.
    fn park_question(
        self: &Arc<Self>,
        request: acp::CreateElicitationRequest,
        responder: Responder<acp::CreateElicitationResponse>,
        cx: &ConnectionTo<Agent>,
    ) -> Result<(), Error> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let mut question = match convert::question_request(id, request) {
            Ok(question) => question,
            Err(session) => {
                if let Some(mut session) = session {
                    self.attribute(&mut session, &mut None);
                    self.emit(AgentEvent::QuestionUnsupported { session });
                }
                return responder.respond(convert::question_response(QuestionAnswer::Decline));
            }
        };
        let asked = self.attribute(&mut question.session, &mut question.subagent);
        let withdrawn = responder.cancellation();
        let (watch, answered) = async_channel::bounded::<()>(1);
        let parked = Parked::Question {
            responder,
            _watch: watch,
        };
        self.park(id, asked, parked, AgentEvent::QuestionAsked(question));
        self.watch_withdrawal(cx, withdrawn, answered, move |broker| {
            // ponytail: Stale means it was answered meanwhile.
            let _ = broker.answer_question(id, QuestionAnswer::Cancel);
        })
    }

    /// Takes request `id` out of the broker if it is still pending and is a question (or not).
    fn take(&self, id: u64, question: bool) -> Option<Parked> {
        self.pending
            .lock()
            .expect("broker poisoned")
            .extract_if(|k, p| *k == id && matches!(p.parked, Parked::Question { .. }) == question)
            .next()
            .map(|(_, pending)| pending.parked)
    }

    fn answer(&self, id: PermissionRequestId, choice: Option<String>) -> Result<(), BrokerError> {
        let Some(Parked::Permission { responder, .. }) = self.take(id, false) else {
            return Err(BrokerError::Stale);
        };
        let outcome = match choice {
            Some(option) => {
                acp::RequestPermissionOutcome::Selected(acp::SelectedPermissionOutcome::new(option))
            }
            None => acp::RequestPermissionOutcome::Cancelled,
        };
        let sent = responder.respond(acp::RequestPermissionResponse::new(outcome));
        self.emit(AgentEvent::PermissionResolved { id });
        // The connection is gone, so the request no longer exists on the agent side either.
        sent.map_err(|_| BrokerError::Stale)
    }

    fn answer_question(&self, id: QuestionId, answer: QuestionAnswer) -> Result<(), BrokerError> {
        let Some(Parked::Question { responder, .. }) = self.take(id, true) else {
            return Err(BrokerError::Stale);
        };
        let sent = responder.respond(convert::question_response(answer));
        self.emit(AgentEvent::QuestionResolved { id });
        sent.map_err(|_| BrokerError::Stale)
    }

    /// Answers "cancelled" to every pending request that matches `session` (all when `None`).
    fn cancel_pending(&self, session: Option<&str>) {
        let cancelled: Vec<_> = self
            .pending
            .lock()
            .expect("broker poisoned")
            .extract_if(|_, p| session.is_none_or(|s| p.session == s))
            .collect();
        for (id, pending) in cancelled {
            self.emit(pending.parked.cancel(id));
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

async fn run(
    spec: AgentSpec,
    commands: Receiver<Command>,
    stop: Receiver<()>,
    broker: Arc<Broker>,
) {
    // A stop drops the connection, and with it the process guard that kills the agent.
    let stopped = async {
        let _ = stop.recv().await;
        Ok(true)
    };
    let result = smol::future::or(connect(&spec, &commands, &broker), stopped).await;
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

fn find(config: &AcpAgentConfig) -> Option<PathBuf> {
    wai_settings::find_program(config.command())
}

/// The program `connect` would spawn for `command`; `None` when it does not parse or is not on `PATH`.
pub fn program(command: &str) -> Option<PathBuf> {
    find(&AcpAgent::from_str(command).ok()?.into_config())
}

/// Runs the connection; `Ok(true)` means wai asked it to stop.
async fn connect(
    spec: &AgentSpec,
    commands: &Receiver<Command>,
    broker: &Arc<Broker>,
) -> Result<bool, Error> {
    // ponytail: the SDK spawns and kills the agent; on Windows it kills only that process (no
    // job object), so what an `npx` agent started outlives it until its stdin closes.
    let config = AcpAgent::from_str(&spec.command)?.into_config();
    let Some(found) = find(&config) else {
        let missing = format!("{} is not on PATH", config.command().display());
        return Err(Error::into_internal_error(std::io::Error::other(missing)));
    };
    let agent = AcpAgent::new(
        AcpAgentConfig::new(found)
            .args(config.arguments().iter().cloned())
            .envs(config.environment().clone()),
    );
    let updates = broker.clone();
    let permissions = broker.clone();
    let questions = broker.clone();
    Client
        .v1()
        .name(format!("wai-agent:{}", spec.name))
        .on_receive_notification(
            async move |n: acp::SessionNotification, _cx| {
                updates.on_update(n);
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |request: acp::RequestPermissionRequest, responder, cx| {
                // Parked until the user answers; the dispatch loop moves on at once.
                permissions.park_permission(request, responder, &cx)
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |request: acp::CreateElicitationRequest, responder, cx| {
                questions.park_question(request, responder, &cx)
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
                .client_info(acp::Implementation::new("wai", env!("CARGO_PKG_VERSION")))
                // Forms only: wai opens no URLs for an agent. With it, Claude asks its
                // AskUserQuestion through `elicitation/create`. With `subagents`, each of its
                // subagents comes as a child session (routed to its root by `Subagents`).
                .client_capabilities(
                    acp::ClientCapabilities::new()
                        .elicitation(
                            acp::ElicitationCapabilities::new()
                                .form(acp::ElicitationFormCapabilities::new()),
                        )
                        .subagents(acp::SubagentCapabilities::new()),
                ),
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
            fork: caps.session_capabilities.fork.is_some(),
            images: caps.prompt_capabilities.image,
            steering: init
                .meta
                .as_ref()
                .and_then(|m| m.get("steering")?.get("supported"))
                == Some(&serde_json::Value::Bool(true)),
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
            Command::NewSession { cwd, mcp, reply } => request(
                &cx,
                broker,
                acp::NewSessionRequest::new(cwd).mcp_servers(convert::mcp_servers(mcp)),
                reply,
                |r| convert::session_started(r.session_id.to_string(), r.modes, r.config_options),
            )?,
            Command::LoadSession {
                id,
                cwd,
                mcp,
                reply,
            } => request(
                &cx,
                broker,
                acp::LoadSessionRequest::new(id.clone(), cwd)
                    .mcp_servers(convert::mcp_servers(mcp)),
                reply,
                |r| convert::session_started(id, r.modes, r.config_options),
            )?,
            Command::ForkSession {
                id,
                cwd,
                mcp,
                reply,
            } => request(
                &cx,
                broker,
                acp::ForkSessionRequest::new(id, cwd).mcp_servers(convert::mcp_servers(mcp)),
                reply,
                |r| convert::session_started(r.session_id.to_string(), r.modes, r.config_options),
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
            Command::SetConfig {
                session,
                option,
                value,
                reply,
            } => request(
                &cx,
                broker,
                acp::SetSessionConfigOptionRequest::new(
                    session,
                    option,
                    acp::SessionConfigValueId::new(value),
                ),
                reply,
                |r| convert::config_options(r.config_options),
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
            Command::Steer {
                session,
                blocks,
                reply,
            } => {
                let blocks: Vec<_> = blocks.into_iter().map(convert::content_block).collect();
                // An idle session leaves the message to wai, which sends it as a prompt.
                let params = serde_json::json!({
                    "sessionId": session,
                    "prompt": blocks,
                    "_meta": {"steering": {"idleBehavior": "promptRequired"}},
                });
                let steer = UntypedMessage::new("_session/steering", params)?;
                request(&cx, broker, steer, reply, convert::steered)?
            }
            Command::Cancel { session } => {
                broker.cancel_pending(Some(&session));
                cx.send_notification(acp::CancelNotification::new(session))?;
            }
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
