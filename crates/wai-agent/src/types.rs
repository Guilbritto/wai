use std::path::PathBuf;

pub type SessionId = String;
pub type PermissionRequestId = u64;
pub type QuestionId = u64;

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSpec {
    pub name: String,
    /// Split like a POSIX shell line on every platform (or a JSON `{"command", "args", "env"}`):
    /// build it with `quote`.
    pub command: String,
}

/// `arg` single-quoted for `AgentSpec.command`: a Windows path keeps its backslashes, and a `'`
/// in it survives.
pub fn quote(arg: impl AsRef<std::ffi::OsStr>) -> String {
    format!(
        "'{}'",
        arg.as_ref().to_string_lossy().replace('\'', r"'\''")
    )
}

/// An MCP server the agent starts for a session, over stdio.
#[derive(Debug, Clone, PartialEq)]
pub struct McpServer {
    pub name: String,
    pub command: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionStarted {
    pub id: SessionId,
    pub modes: Vec<ModeInfo>,
    pub current_mode: Option<String>,
    pub config: Vec<ConfigOption>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigValue {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigCategory {
    Mode,
    Model,
    ThoughtLevel,
    Other,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConfigOption {
    pub id: String,
    pub name: String,
    pub category: ConfigCategory,
    pub current: String,
    pub values: Vec<ConfigValue>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ModeInfo {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionSummary {
    pub id: SessionId,
    pub cwd: PathBuf,
    pub title: Option<String>,
    pub updated_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PromptBlock {
    Text(String),
    Image { mime: String, base64: String },
    Resource { path: PathBuf },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AgentCaps {
    pub load_session: bool,
    pub list_sessions: bool,
    pub fork: bool,
    /// `AgentHandle::close_session` frees a session's resources in the agent.
    pub close: bool,
    pub images: bool,
    /// Takes a message into a running turn (`AgentHandle::steer`).
    pub steering: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    Ready {
        name: String,
        version: String,
        caps: AgentCaps,
    },
    Update {
        session: SessionId,
        event: SessionEvent,
    },
    PermissionRequested(PermissionRequest),
    PermissionResolved {
        id: PermissionRequestId,
    },
    /// An ACP form elicitation (Claude's AskUserQuestion); answer with `answer_question`.
    QuestionAsked(QuestionRequest),
    /// Answered, cancelled with the turn, or withdrawn by the agent.
    QuestionResolved {
        id: QuestionId,
    },
    /// The session's form had a field wai cannot show (an unknown schema): it was declined.
    QuestionUnsupported {
        session: SessionId,
    },
    TurnEnded {
        session: SessionId,
        reason: StopReason,
    },
    AuthRequired {
        agent: String,
    },
    Exited {
        error: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub enum SessionEvent {
    AgentText(String),
    AgentThought(String),
    UserText(String),
    ToolCall(ToolCallView),
    ToolCallUpdate(ToolCallPatch),
    Plan(Vec<PlanEntry>),
    Usage {
        used: u64,
        size: u64,
        cost_usd: Option<f64>,
        rate_limit: Option<serde_json::Value>,
    },
    Title(String),
    ModeChanged(String),
    ConfigChanged(Vec<ConfigOption>),
    Commands(Vec<CommandInfo>),
    /// A subagent of the session was announced, or changed its title or state.
    Subagent(SubagentPatch),
    /// A subagent's answer to the session.
    SubagentReply {
        id: SessionId,
        text: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubagentState {
    Running,
    NeedsYou,
    Done,
    Stopped,
    Failed,
    Ended,
    Unknown,
}

/// `None` means unchanged. ponytail: a `null` clear also reads as unchanged.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SubagentPatch {
    pub id: SessionId,
    pub title: Option<String>,
    /// ACP's `description`: Claude sends the subagent type here.
    pub description: Option<String>,
    pub state: Option<SubagentState>,
    pub used: Option<u64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallView {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: ToolStatus,
    pub content: Vec<ToolContent>,
    pub locations: Vec<Location>,
    /// The subagent session that made the call; `None` for the session's own.
    pub subagent: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallPatch {
    pub id: String,
    pub title: Option<String>,
    pub status: Option<ToolStatus>,
    pub content: Option<Vec<ToolContent>>,
    /// Replaces the call's locations as a whole, like `content`.
    pub locations: Option<Vec<Location>>,
}

/// A file a tool call touches and, when the agent knows it, the 1-based line (for an edit hunk,
/// the hunk's first line in the new file).
#[derive(Debug, Clone, PartialEq)]
pub struct Location {
    pub path: PathBuf,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ToolContent {
    Text(String),
    Diff(FileDiff),
    Terminal(String),
}

#[derive(Debug, Clone, PartialEq)]
pub struct FileDiff {
    pub path: PathBuf,
    pub old_text: Option<String>,
    pub new_text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanEntry {
    pub content: String,
    pub status: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CommandInfo {
    pub name: String,
    pub description: String,
    /// What to type after the name, when the command takes input.
    pub hint: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub id: PermissionRequestId,
    pub session: SessionId,
    pub tool_call_id: String,
    pub title: String,
    pub diff: Option<FileDiff>,
    pub options: Vec<PermissionChoice>,
    /// The subagent session that asked; then `session` is its root, the open session the card
    /// shows in.
    pub subagent: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionChoice {
    pub id: String,
    pub name: String,
    pub kind: ChoiceKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChoiceKind {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuestionRequest {
    pub id: QuestionId,
    pub session: SessionId,
    pub message: String,
    pub fields: Vec<QuestionField>,
    /// As `PermissionRequest::subagent`.
    pub subagent: Option<SessionId>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct QuestionField {
    pub key: String,
    pub title: String,
    pub description: Option<String>,
    pub required: bool,
    pub kind: FieldKind,
}

#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    SingleChoice(Vec<Choice>),
    MultiChoice(Vec<Choice>),
    Text,
    Boolean,
    Number,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Choice {
    pub value: String,
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum QuestionAnswer {
    /// The form's values by field key: strings, numbers, booleans and string arrays.
    Accept(serde_json::Map<String, serde_json::Value>),
    Decline,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StopReason {
    EndTurn,
    Cancelled,
    MaxTokens,
    Refusal,
    Other(String),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentError {
    #[error("the agent is not signed in")]
    AuthRequired,
    #[error("the agent process has exited")]
    Exited,
    #[error("agent protocol error: {0}")]
    Protocol(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum BrokerError {
    #[error("the request is no longer pending")]
    Stale,
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::str::FromStr as _;

    use agent_client_protocol::AcpAgent;

    use super::quote;

    /// The SDK splits `AgentSpec.command` like a POSIX shell line on every platform: a quoted
    /// Windows path keeps its backslashes, and a `'` in a path or an argument survives.
    #[test]
    fn agent_command_quoting() {
        for program in [
            r"C:\Users\O'Neil\wai\target\debug\wai-fake-agent.exe",
            "/Users/me/my agent/wai-fake-agent",
        ] {
            let command = format!("{} --reply {}", quote(program), quote("it's done"));
            let config = AcpAgent::from_str(&command).unwrap().into_config();
            assert_eq!(config.command(), Path::new(program));
            assert_eq!(config.arguments(), ["--reply", "it's done"]);
        }
    }
}
