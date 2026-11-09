use std::collections::{BTreeMap, HashMap};

use gpui_kit::*;
use wai_agent::{
    AgentCaps, AgentEvent, AgentHandle, AgentSpec, PermissionRequest, QuestionRequest,
    SessionEvent, StopReason,
};
use wai_settings::{AgentConfig, Settings};

#[derive(Debug, Clone, PartialEq)]
pub enum AgentStatus {
    Starting,
    Ready(AgentCaps),
    Failed(String),
    SignedOut,
}

#[derive(Debug, Clone)]
pub enum RegistryEvent {
    Session {
        agent: String,
        session: String,
        event: SessionEvent,
    },
    Permission {
        agent: String,
        request: PermissionRequest,
    },
    PermissionResolved(u64),
    Question {
        agent: String,
        request: QuestionRequest,
    },
    QuestionResolved(u64),
    QuestionUnsupported {
        agent: String,
        session: String,
    },
    TurnEnded {
        agent: String,
        session: String,
        reason: StopReason,
    },
    StatusChanged(String),
}

struct Agent {
    handle: AgentHandle,
    status: AgentStatus,
    /// From `Ready`, kept so `mark_ready` can restore it after a sign-out.
    caps: Option<AgentCaps>,
    _pump: Task<()>,
}

/// One agent process per configured name, spawned on first use.
pub struct AgentRegistry {
    configs: BTreeMap<String, AgentConfig>,
    agents: HashMap<String, Agent>,
}

impl EventEmitter<RegistryEvent> for AgentRegistry {}

impl AgentRegistry {
    pub fn new(settings: &Settings) -> Self {
        Self {
            configs: settings.agents.clone(),
            agents: HashMap::new(),
        }
    }

    /// The agent's handle. Spawns it (again, once it has exited) and starts its event pump.
    /// `None` when no agent has that name in the settings.
    pub fn handle(&mut self, name: &str, cx: &mut Context<Self>) -> Option<AgentHandle> {
        if let Some(agent) = self.agents.get(name)
            && !matches!(agent.status, AgentStatus::Failed(_))
        {
            return Some(agent.handle.clone());
        }
        let command = self.configs.get(name)?.command.clone();
        let (handle, events) = AgentHandle::spawn(AgentSpec {
            name: name.into(),
            command,
        });
        let agent = name.to_string();
        let pump = cx.spawn(async move |this, cx| {
            while let Ok(first) = events.recv().await {
                // Everything that piled up is applied in one update.
                let batch: Vec<_> = std::iter::once(first)
                    .chain(std::iter::from_fn(|| events.try_recv().ok()))
                    .collect();
                // `Exited` is the last event: the channel stays open while a handle lives.
                let exited = batch.iter().any(|e| matches!(e, AgentEvent::Exited { .. }));
                let alive = this.update(cx, |this, cx| {
                    for event in batch {
                        this.apply(&agent, event, cx);
                    }
                });
                if exited || alive.is_err() {
                    break;
                }
            }
        });
        self.agents.insert(
            name.into(),
            Agent {
                handle: handle.clone(),
                status: AgentStatus::Starting,
                caps: None,
                _pump: pump,
            },
        );
        cx.emit(RegistryEvent::StatusChanged(name.into()));
        Some(handle)
    }

    /// Signals every agent to stop, at once (at quit); resolves once they are all gone.
    pub fn shutdown_all(&self) -> impl Future<Output = ()> + use<> {
        let stopped: Vec<_> = self.agents.values().map(|a| a.handle.shutdown()).collect();
        async move {
            for stopped in stopped {
                stopped.await;
            }
        }
    }

    pub fn status(&self, name: &str) -> Option<&AgentStatus> {
        self.agents.get(name).map(|agent| &agent.status)
    }

    pub fn set_status(&mut self, name: &str, status: AgentStatus, cx: &mut Context<Self>) {
        if let Some(agent) = self.agents.get_mut(name)
            && agent.status != status
        {
            agent.status = status;
            cx.emit(RegistryEvent::StatusChanged(name.into()));
        }
    }

    /// Back to `Ready` once the agent did real work again: call it after a successful
    /// `new_session`/`load_session` or a turn that did not end in a sign-out. No-op before `Ready`
    /// and after the process exited.
    pub fn mark_ready(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(caps) = self.agents.get(name).and_then(|agent| agent.caps) {
            self.set_status(name, AgentStatus::Ready(caps), cx);
        }
    }

    fn apply(&mut self, agent: &str, event: AgentEvent, cx: &mut Context<Self>) {
        let status = match event {
            AgentEvent::Ready { caps, .. } => {
                if let Some(entry) = self.agents.get_mut(agent) {
                    entry.caps = Some(caps);
                }
                AgentStatus::Ready(caps)
            }
            AgentEvent::AuthRequired { .. } => AgentStatus::SignedOut,
            AgentEvent::Exited { error } => {
                // A dead process is never ready again: `mark_ready` (say, for a `TurnEnded` of
                // the same batch, routed after it) must not hide the exit.
                if let Some(entry) = self.agents.get_mut(agent) {
                    entry.caps = None;
                }
                AgentStatus::Failed(error.unwrap_or_else(|| "the agent exited".into()))
            }
            AgentEvent::Update { session, event } => {
                return cx.emit(RegistryEvent::Session {
                    agent: agent.into(),
                    session,
                    event,
                });
            }
            AgentEvent::PermissionRequested(request) => {
                return cx.emit(RegistryEvent::Permission {
                    agent: agent.into(),
                    request,
                });
            }
            AgentEvent::PermissionResolved { id } => {
                return cx.emit(RegistryEvent::PermissionResolved(id));
            }
            AgentEvent::QuestionAsked(request) => {
                return cx.emit(RegistryEvent::Question {
                    agent: agent.into(),
                    request,
                });
            }
            AgentEvent::QuestionResolved { id } => {
                return cx.emit(RegistryEvent::QuestionResolved(id));
            }
            AgentEvent::QuestionUnsupported { session } => {
                return cx.emit(RegistryEvent::QuestionUnsupported {
                    agent: agent.into(),
                    session,
                });
            }
            AgentEvent::TurnEnded { session, reason } => {
                return cx.emit(RegistryEvent::TurnEnded {
                    agent: agent.into(),
                    session,
                    reason,
                });
            }
        };
        self.set_status(agent, status, cx);
    }
}
