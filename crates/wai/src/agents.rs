use std::collections::{BTreeMap, HashMap};

use gpui_kit::*;
use wai_agent::{
    AgentCaps, AgentEvent, AgentHandle, AgentSpec, McpServer, PermissionRequest, QuestionRequest,
    SessionEvent, StopReason,
};
use wai_settings::AgentConfig;

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
    /// Request ids restart at 1 per agent: only `agent`'s sessions hold this one.
    PermissionResolved {
        agent: String,
        id: u64,
    },
    Question {
        agent: String,
        request: QuestionRequest,
    },
    QuestionResolved {
        agent: String,
        id: u64,
    },
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
    /// From `Ready`: the adapter's name and version, for Settings.
    adapter: Option<(String, String)>,
    _pump: Task<()>,
}

/// One agent process per configured name, spawned on first use: the settings' agents, then the
/// enabled extensions' (`<ext-id>/<name>`).
pub struct AgentRegistry {
    configs: BTreeMap<String, AgentConfig>,
    extension: BTreeMap<String, AgentConfig>,
    /// The enabled extensions' MCP servers: every new, loaded or forked session gets them.
    mcp: Vec<McpServer>,
    agents: HashMap<String, Agent>,
}

impl EventEmitter<RegistryEvent> for AgentRegistry {}

impl AgentRegistry {
    /// `configs`: only approved commands (see `Workspace::enforce`).
    pub fn new(configs: BTreeMap<String, AgentConfig>) -> Self {
        Self {
            configs,
            extension: BTreeMap::new(),
            mcp: Vec::new(),
            agents: HashMap::new(),
        }
    }

    /// Running agents keep their process: the next spawn uses the new command.
    pub fn set_configs(&mut self, configs: BTreeMap<String, AgentConfig>) {
        self.configs = configs;
    }

    pub fn configs(&self) -> &BTreeMap<String, AgentConfig> {
        &self.configs
    }

    pub fn command(&self, name: &str) -> Option<&str> {
        let config = self.configs.get(name).or_else(|| self.extension.get(name));
        config.map(|c| c.command.as_str())
    }

    pub fn knows(&self, name: &str) -> bool {
        self.configs.contains_key(name) || self.extension.contains_key(name)
    }

    /// Every agent's name: the settings' first.
    pub fn names(&self) -> Vec<String> {
        let extension = self
            .extension
            .keys()
            .filter(|n| !self.configs.contains_key(*n));
        self.configs.keys().chain(extension).cloned().collect()
    }

    pub fn mcp_servers(&self) -> Vec<McpServer> {
        self.mcp.clone()
    }

    /// What the enabled extensions contribute now. An extension agent that goes, or changes its
    /// command, is shut down: its sessions see it exit, and it never starts again unless it comes
    /// back.
    pub fn set_extensions(&mut self, agents: BTreeMap<String, AgentConfig>, mcp: Vec<McpServer>) {
        for (name, config) in &self.extension {
            let gone = agents.get(name) != Some(config) && !self.configs.contains_key(name);
            if let Some(agent) = self.agents.get(name).filter(|_| gone) {
                // Closes its stop channel now; its `Exited` arrives through the pump.
                drop(agent.handle.shutdown());
            }
        }
        self.extension = agents;
        self.mcp = mcp;
    }

    /// The agent's handle. Spawns it (again, once it has exited) and starts its event pump.
    /// `None` when no agent has that name (in the settings or an enabled extension).
    pub fn handle(&mut self, name: &str, cx: &mut Context<Self>) -> Option<AgentHandle> {
        if !self.knows(name) {
            return None;
        }
        if let Some(agent) = self.agents.get(name)
            && !matches!(agent.status, AgentStatus::Failed(_))
        {
            return Some(agent.handle.clone());
        }
        let config = self.configs.get(name).or_else(|| self.extension.get(name));
        let command = config?.command.clone();
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
                adapter: None,
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

    /// The adapter's `(name, version)`, from `Ready` until the process exits.
    pub fn adapter(&self, id: &str) -> Option<&(String, String)> {
        self.agents.get(id)?.adapter.as_ref()
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
            AgentEvent::Ready {
                name,
                version,
                caps,
            } => {
                if let Some(entry) = self.agents.get_mut(agent) {
                    entry.caps = Some(caps);
                    entry.adapter = Some((name, version));
                }
                AgentStatus::Ready(caps)
            }
            AgentEvent::AuthRequired { .. } => AgentStatus::SignedOut,
            AgentEvent::Exited { error } => {
                // A dead process is never ready again: `mark_ready` (say, for a `TurnEnded` of
                // the same batch, routed after it) must not hide the exit.
                if let Some(entry) = self.agents.get_mut(agent) {
                    entry.caps = None;
                    entry.adapter = None;
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
                return cx.emit(RegistryEvent::PermissionResolved {
                    agent: agent.into(),
                    id,
                });
            }
            AgentEvent::QuestionAsked(request) => {
                return cx.emit(RegistryEvent::Question {
                    agent: agent.into(),
                    request,
                });
            }
            AgentEvent::QuestionResolved { id } => {
                return cx.emit(RegistryEvent::QuestionResolved {
                    agent: agent.into(),
                    id,
                });
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

#[cfg(test)]
mod tests {
    use gpui_kit::{AppContext as _, TestAppContext};

    use super::{AgentCaps, AgentEvent, AgentRegistry};
    use crate::workspace::tests::fake_settings;

    fn caps() -> AgentCaps {
        AgentCaps {
            load_session: false,
            list_sessions: false,
            fork: false,
            close: false,
            images: false,
            steering: false,
        }
    }

    fn ready() -> AgentEvent {
        AgentEvent::Ready {
            name: "wai-fake-agent".into(),
            version: "1.2.3".into(),
            caps: caps(),
        }
    }

    #[gpui_kit::test]
    fn ready_keeps_the_adapter_version(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let registry = cx.new(|_| AgentRegistry::new(fake_settings("").agents));
        registry.update(cx, |r, cx| {
            r.handle("fake", cx).unwrap();
            assert_eq!(r.adapter("fake"), None);
            r.apply("fake", ready(), cx);
            let expected = ("wai-fake-agent".to_string(), "1.2.3".to_string());
            assert_eq!(r.adapter("fake"), Some(&expected));
        });
    }

    #[gpui_kit::test]
    fn exit_clears_the_adapter_version(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let registry = cx.new(|_| AgentRegistry::new(fake_settings("").agents));
        registry.update(cx, |r, cx| {
            r.handle("fake", cx).unwrap();
            r.apply("fake", ready(), cx);
            r.apply("fake", AgentEvent::Exited { error: None }, cx);
            assert_eq!(r.adapter("fake"), None);
        });
    }
}
