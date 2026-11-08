use std::path::PathBuf;

pub type SessionId = String;
pub type PermissionRequestId = u64;
pub type QuestionId = u64;

#[derive(Debug, Clone, PartialEq)]
pub struct AgentSpec {
    pub name: String,
    pub command: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionStarted {
    pub id: SessionId,
    pub modes: Vec<ModeInfo>,
    pub current_mode: Option<String>,
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
    pub images: bool,
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
    Commands(Vec<CommandInfo>),
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallView {
    pub id: String,
    pub title: String,
    pub kind: String,
    pub status: ToolStatus,
    pub content: Vec<ToolContent>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCallPatch {
    pub id: String,
    pub title: Option<String>,
    pub status: Option<ToolStatus>,
    pub content: Option<Vec<ToolContent>>,
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
}

#[derive(Debug, Clone, PartialEq)]
pub struct PermissionRequest {
    pub id: PermissionRequestId,
    pub session: SessionId,
    pub tool_call_id: String,
    pub title: String,
    pub diff: Option<FileDiff>,
    pub options: Vec<PermissionChoice>,
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
