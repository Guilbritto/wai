//! A scripted ACP agent for wai's tests. Every flag is optional:
//!
//! - `--reply <text>`: the turn's answer, streamed one word per chunk
//!   ("one", " two", " three"); default `Hello from fake agent`.
//! - `--reply-file <path>`: like `--reply`, read from a file (multi-line markdown).
//!   With `--ask-permission` it streams before the tool call.
//! - `--chunk-delay-ms <n>`: pause before each chunk.
//! - `--ask-permission`: send an Edit tool call for `notes.md`, ask permission and
//!   reply `chose:<option_id>` or `chose:cancelled`.
//! - `--ask-question`: ask a form (a required single choice `color`: `red`/`blue`, and an
//!   optional text `note`) and reply `answered:<json>`, `answered:declined` or
//!   `answered:cancelled`. Like claude-agent-acp, it asks only a client that advertises form
//!   elicitation, and replies `answered:unsupported` otherwise.
//! - `--withdraw-question`: with `--ask-question`, first ask the same form and withdraw it at
//!   once (`$/cancel_request`).
//! - `--ask-unsupported-question`: `--ask-question` with one more field `shape` of an unknown
//!   schema type (`_shape`).
//! - `--subagent`: before the reply, run a subagent `Explore the code` as a child session
//!   `<session>-sub`: announced `running` on the parent; on the child a Read tool call `sub-read`
//!   (`Read lib.rs`), a one-entry plan (`Sub step`), the text `sub text`, then the call completed;
//!   then on the parent its reply `found it` (a `session_message`) and `idle` (`end_turn`). Like
//!   claude-agent-acp, only for a client that advertised `subagents`. With `--ask-permission` or
//!   `--ask-question`, the subagent asks (on the child, after `sub-read` starts; `requires_action`
//!   on the parent before, `running` once answered), and the turn still replies `chose:…` or
//!   `answered:…`.
//! - `--background`: with `--subagent`, the subagent outlives the turn: the turn streams its reply
//!   and ends right after the subagent's tool call starts (with `--ask-permission`, the request is
//!   on the wire before the first reply chunk, and the turn does not wait for it). The rest
//!   follows once that request is answered, or 100 ms after the turn ends without one.
//! - `--withdraw-permission`: with `--ask-permission`, first send the same permission request and
//!   withdraw it at once (`$/cancel_request`).
//! - `--crash-after <n>`: after n chunks print `fake agent crashed` to stderr and exit 3.
//! - `--require-auth`: `session/new` fails with the ACP auth-required error.
//! - `--require-auth-on-prompt`: `session/prompt` fails with the ACP auth-required error.
//! - `--fail-fork`: `session/fork` fails with an internal error.
//! - `--no-load`: do not advertise `loadSession`.
//! - `--flood <bytes>`: start the turn with one tool call carrying that many bytes of `x\n`.
//! - `--steps <n>`: start the turn with a thought (`Looking around`), then n Read tool calls
//!   titled `Read README.md`, each in progress for one `--chunk-delay-ms` and then completed
//!   with the output `step <i> output`.
//! - `--edit <path>`, `--write <path>`, `--edit-hunk <path>` (repeatable, run in order): start the
//!   turn with file edits, each like claude-agent-acp's: a pending Edit tool call (diff and
//!   location), a permission request (`allow-once` writes the file, `reject-once` fails the call),
//!   then a completed (or failed) update.
//!   - `--edit`: appends `edited by the fake agent` (creating the file if missing); the diff holds
//!     the whole file.
//!   - `--write`: replaces the file with `written by the fake agent`; the diff has no old text,
//!     like a Write, even when the file existed.
//!   - `--edit-hunk`: appends ` // edited` to the file's middle line; the call starts with that
//!     line as the snippet, and completes with the hunk (3 lines of context, no trailing newline)
//!     and its first line as the location.
//! - `--sessions <n>`: `session/list` returns n sessions titled `Session <i>`, 10 per page.
//! - `--sessions-cwd <path>`: the cwd of those sessions; default `/tmp`.
//! - `--pid-file <path>`: write the process id there at start.
//! - `--cwd-file <path>`: write the process working directory there at start.
//! - `--echo-mcp`: each turn answers with the MCP servers its session got at `session/new`,
//!   `session/load` or `session/fork`: `mcp:<name>=<command>,…` (`mcp:` when none).
//!
//! - `--models`: `session/new`, `load` and `fork` return a `model` option (`fake-small`,
//!   `fake-large`) and an `effort` option (`low`, `high`); `session/set_config_option` stores the
//!   value per session, and each turn answers `model:<current model id>`.
//! - `--config-mode`: with `--models`, a `mode` option (`ask`, `code`) joins them.
//! - `--reject-config`: `session/set_config_option` fails.
//! - `--steering`: advertise `_meta.steering.supported` and take `_session/steering`: during a
//!   turn it answers `injected`, and the turn's reply ends with ` Steered: <text>` for each one
//!   (once the reply streamed; use `--chunk-delay-ms` to keep the turn running); with no turn
//!   running it answers `promptRequired`.
//!
//! `session/load` replays `previous prompt` and `previous answer`; for an id starting with
//! `missing` it fails with the ACP session-not-found error instead.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use agent_client_protocol::v1::{ConnectionTo, schema as acp};
use agent_client_protocol::{Agent, Client, Error, Stdio, UntypedMessage};

const PAGE: usize = 10;

/// A request the user answers, as the turn reports it (`chose:…`, `answered:…`).
type Asked = Pin<Box<dyn Future<Output = Result<String, Error>> + Send>>;

/// `--subagent`: the child session and, when it asked, its open request.
struct Sub {
    child: acp::SessionId,
    asked: Option<Asked>,
}

#[derive(Clone, Copy)]
enum EditKind {
    Append,
    Write,
    Hunk,
}

struct Opts {
    reply: String,
    reply_file: bool,
    chunk_delay: Duration,
    ask_permission: bool,
    plan: bool,
    subagent: bool,
    background: bool,
    withdraw_permission: bool,
    ask_question: bool,
    withdraw_question: bool,
    unsupported_question: bool,
    crash_after: Option<usize>,
    require_auth: bool,
    require_auth_on_prompt: bool,
    fail_fork: bool,
    no_load: bool,
    flood: Option<usize>,
    steps: usize,
    edits: Vec<(EditKind, PathBuf)>,
    sessions: usize,
    sessions_cwd: PathBuf,
    pid_file: Option<PathBuf>,
    cwd_file: Option<PathBuf>,
    echo_mcp: bool,
    models: bool,
    config_mode: bool,
    reject_config: bool,
    steering: bool,
}

fn parse_args() -> Opts {
    let mut opts = Opts {
        reply: "Hello from fake agent".into(),
        reply_file: false,
        chunk_delay: Duration::ZERO,
        ask_permission: false,
        plan: false,
        subagent: false,
        background: false,
        withdraw_permission: false,
        ask_question: false,
        withdraw_question: false,
        unsupported_question: false,
        crash_after: None,
        require_auth: false,
        require_auth_on_prompt: false,
        fail_fork: false,
        no_load: false,
        flood: None,
        steps: 0,
        edits: Vec::new(),
        sessions: 0,
        sessions_cwd: "/tmp".into(),
        pid_file: None,
        cwd_file: None,
        echo_mcp: false,
        models: false,
        config_mode: false,
        reject_config: false,
        steering: false,
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
            "--reply-file" => {
                let path = value();
                opts.reply = std::fs::read_to_string(&path)
                    .unwrap_or_else(|e| panic!("--reply-file {path}: {e}"));
                opts.reply_file = true;
            }
            "--chunk-delay-ms" => opts.chunk_delay = Duration::from_millis(number(value()) as u64),
            "--ask-permission" => opts.ask_permission = true,
            "--plan" => opts.plan = true,
            "--subagent" => opts.subagent = true,
            "--background" => opts.background = true,
            "--withdraw-permission" => opts.withdraw_permission = true,
            "--ask-question" => opts.ask_question = true,
            "--withdraw-question" => opts.withdraw_question = true,
            "--ask-unsupported-question" => {
                opts.ask_question = true;
                opts.unsupported_question = true;
            }
            "--crash-after" => opts.crash_after = Some(number(value())),
            "--require-auth" => opts.require_auth = true,
            "--require-auth-on-prompt" => opts.require_auth_on_prompt = true,
            "--fail-fork" => opts.fail_fork = true,
            "--no-load" => opts.no_load = true,
            "--flood" => opts.flood = Some(number(value())),
            "--steps" => opts.steps = number(value()),
            "--edit" => opts.edits.push((EditKind::Append, value().into())),
            "--write" => opts.edits.push((EditKind::Write, value().into())),
            "--edit-hunk" => opts.edits.push((EditKind::Hunk, value().into())),
            "--sessions" => opts.sessions = number(value()),
            "--sessions-cwd" => opts.sessions_cwd = value().into(),
            "--pid-file" => opts.pid_file = Some(value().into()),
            "--cwd-file" => opts.cwd_file = Some(value().into()),
            "--echo-mcp" => opts.echo_mcp = true,
            "--models" => opts.models = true,
            "--config-mode" => opts.config_mode = true,
            "--reject-config" => opts.reject_config = true,
            "--steering" => opts.steering = true,
            other => panic!("unknown flag {other}"),
        }
    }
    opts
}

struct Fake {
    opts: Opts,
    next_session: AtomicU64,
    cancelled: Mutex<HashSet<String>>,
    /// The client advertised `elicitation.form` at initialize.
    forms: AtomicBool,
    /// The client advertised `subagents` at initialize.
    subagents: AtomicBool,
    /// Per session: the MCP servers it got, as `--echo-mcp` reports them.
    mcp: Mutex<HashMap<String, String>>,
    /// `--models`: per session, the chosen value of each config option.
    config: Mutex<HashMap<String, HashMap<String, String>>>,
    /// Per session with a turn running: the texts steered into it.
    steered: Mutex<HashMap<String, Vec<String>>>,
}

impl Fake {
    /// `--echo-mcp`: what session `session` got.
    fn record_mcp(&self, session: &acp::SessionId, servers: &[acp::McpServer]) {
        let names: Vec<String> = servers
            .iter()
            .map(|s| match s {
                acp::McpServer::Stdio(s) => format!("{}={}", s.name, s.command.display()),
                other => format!("{other:?}"),
            })
            .collect();
        let mut mcp = self.mcp.lock().expect("mcp poisoned");
        mcp.insert(session.to_string(), names.join(","));
    }

    /// `--models`: the config options of `session`, holding what it chose.
    fn config_options(&self, session: &acp::SessionId) -> Option<Vec<acp::SessionConfigOption>> {
        use acp::SessionConfigOptionCategory as C;
        if !self.opts.models {
            return None;
        }
        let chosen = self.config.lock().expect("config poisoned");
        let current = |id: &str, default: &str| {
            let value = chosen.get(&*session.0).and_then(|c| c.get(id));
            value.map_or(default, String::as_str).to_string()
        };
        let select = |id, name, category, default, values: [(&'static str, &'static str); 2]| {
            let values: Vec<_> = values
                .into_iter()
                .map(|(v, n)| acp::SessionConfigSelectOption::new(v, n))
                .collect();
            acp::SessionConfigOption::select(id, name, current(id, default), values)
                .category(category)
        };
        let mode = self.opts.config_mode.then(|| {
            select(
                "mode",
                "Mode",
                C::Mode,
                "ask",
                [("ask", "Ask"), ("code", "Code")],
            )
        });
        Some(
            vec![
                select(
                    "model",
                    "Model",
                    C::Model,
                    "fake-small",
                    [("fake-small", "Fake Small"), ("fake-large", "Fake Large")],
                ),
                select(
                    "effort",
                    "Effort",
                    C::ThoughtLevel,
                    "low",
                    [("low", "Low"), ("high", "High")],
                ),
            ]
            .into_iter()
            .chain(mode)
            .collect(),
        )
    }

    fn is_cancelled(&self, session: &acp::SessionId) -> bool {
        self.cancelled
            .lock()
            .expect("cancel set poisoned")
            .contains(&*session.0)
    }

    async fn turn(
        self: &Arc<Self>,
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
        if self.opts.plan {
            let entry = |c: &str, s| acp::PlanEntry::new(c, acp::PlanEntryPriority::Medium, s);
            let entries = vec![
                entry("Read the code", acp::PlanEntryStatus::Completed),
                entry("Write the change", acp::PlanEntryStatus::InProgress),
                entry("Run the tests", acp::PlanEntryStatus::Pending),
            ];
            send(
                cx,
                session,
                acp::SessionUpdate::Plan(acp::Plan::new(entries)),
            )?;
        }
        if self.opts.steps > 0 {
            let thought = acp::SessionUpdate::AgentThoughtChunk(chunk("Looking around"));
            send(cx, session, thought)?;
        }
        for i in 0..self.opts.steps {
            let id = format!("step-{i}");
            let call = acp::ToolCall::new(id.clone(), "Read README.md")
                .kind(acp::ToolKind::Read)
                .status(acp::ToolCallStatus::InProgress);
            send(cx, session, acp::SessionUpdate::ToolCall(call))?;
            if !self.opts.chunk_delay.is_zero() {
                smol::Timer::after(self.opts.chunk_delay).await;
            }
            if self.is_cancelled(session) {
                return Ok(acp::StopReason::Cancelled);
            }
            let output = acp::Content::new(text_block(format!("step {i} output")));
            let fields = acp::ToolCallUpdateFields::new()
                .status(acp::ToolCallStatus::Completed)
                .content(vec![acp::ToolCallContent::Content(output)]);
            let update = acp::ToolCallUpdate::new(id, fields);
            send(cx, session, acp::SessionUpdate::ToolCallUpdate(update))?;
        }
        for (i, (kind, path)) in self.opts.edits.iter().enumerate() {
            self.edit(cx, session, &format!("edit-file-{i}"), *kind, path)
                .await?;
        }
        if self.opts.ask_permission && self.opts.reply_file {
            let reason = self.stream(cx, session, &self.opts.reply).await?;
            if reason != acp::StopReason::EndTurn {
                return Ok(reason);
            }
        }
        // --subagent: the child asks in the turn's place; --background leaves it running.
        let sub = match self.opts.subagent && self.subagents.load(Ordering::Relaxed) {
            true => Some(self.subagent_start(cx, session)?),
            false => None,
        };
        let (answer, background) = match sub {
            Some(sub) if self.opts.background => (None, Some(sub)),
            Some(sub) => {
                let finish = self
                    .clone()
                    .subagent_finish(cx.clone(), session.clone(), sub);
                (finish.await?, None)
            }
            None => (None, None),
        };
        let reply = if self.opts.models {
            let options = self.config_options(session).unwrap_or_default();
            let model = options.into_iter().next().and_then(|o| match o.kind {
                acp::SessionConfigKind::Select(s) => Some(s.current_value.to_string()),
                _ => None,
            });
            format!("model:{}", model.unwrap_or_default())
        } else if self.opts.echo_mcp {
            let mcp = self.mcp.lock().expect("mcp poisoned");
            format!("mcp:{}", mcp.get(&*session.0).map_or("", String::as_str))
        } else if let Some(answer) = answer {
            answer
        } else if background.is_some() || !(self.opts.ask_permission || self.opts.ask_question) {
            self.opts.reply.clone()
        } else if self.opts.ask_permission {
            format!("chose:{}", self.ask_permission(cx, session)?.await?)
        } else {
            format!("answered:{}", self.ask_question(cx, session)?.await?)
        };
        let reason = self.stream(cx, session, &reply).await?;
        if let Some(sub) = background {
            let (fake, cx, session) = (self.clone(), cx.clone(), session.clone());
            cx.clone()
                .spawn(async move { fake.subagent_finish(cx, session, sub).await.map(drop) })?;
        }
        Ok(reason)
    }

    /// `--subagent`: the child session `<session>-sub` starts its work, announced on the parent,
    /// and asks (`--ask-permission`, `--ask-question`) on the child.
    fn subagent_start(
        &self,
        cx: &ConnectionTo<Client>,
        session: &acp::SessionId,
    ) -> Result<Sub, Error> {
        let child = acp::SessionId::new(format!("{session}-sub"));
        let announce = acp::SubagentUpdate::new(child.clone())
            .title("Explore the code".to_string())
            .state(acp::StateUpdate::Running(acp::RunningStateUpdate::new()));
        send(cx, session, acp::SessionUpdate::SubagentUpdate(announce))?;
        let call = acp::ToolCall::new("sub-read", "Read lib.rs")
            .kind(acp::ToolKind::Read)
            .status(acp::ToolCallStatus::InProgress);
        send(cx, &child, acp::SessionUpdate::ToolCall(call))?;
        let entry = acp::PlanEntry::new(
            "Sub step",
            acp::PlanEntryPriority::Medium,
            acp::PlanEntryStatus::InProgress,
        );
        let plan = acp::SessionUpdate::Plan(acp::Plan::new(vec![entry]));
        send(cx, &child, plan)?;
        send(
            cx,
            &child,
            acp::SessionUpdate::AgentMessageChunk(chunk("sub text")),
        )?;
        let asked = if self.opts.ask_permission || self.opts.ask_question {
            let waiting = acp::StateUpdate::RequiresAction(acp::RequiresActionStateUpdate::new());
            send(cx, session, subagent_state(&child, waiting))?;
            Some(if self.opts.ask_permission {
                let asked = self.ask_permission(cx, &child)?;
                Box::pin(async move { Ok(format!("chose:{}", asked.await?)) }) as Asked
            } else {
                let asked = self.ask_question(cx, &child)?;
                Box::pin(async move { Ok(format!("answered:{}", asked.await?)) })
            })
        } else {
            None
        };
        Ok(Sub { child, asked })
    }

    /// The rest of the child's work: once its request is answered (`running` again on the
    /// parent), the call completes, the child answers the parent and goes `idle`. Returns the
    /// turn's reply for what was asked.
    async fn subagent_finish(
        self: Arc<Self>,
        cx: ConnectionTo<Client>,
        session: acp::SessionId,
        sub: Sub,
    ) -> Result<Option<String>, Error> {
        let answer = match sub.asked {
            Some(asked) => {
                let answer = asked.await?;
                let running = acp::StateUpdate::Running(acp::RunningStateUpdate::new());
                send(&cx, &session, subagent_state(&sub.child, running))?;
                Some(answer)
            }
            None => {
                if self.opts.background {
                    smol::Timer::after(Duration::from_millis(100)).await;
                }
                None
            }
        };
        let fields = acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed);
        let update = acp::ToolCallUpdate::new("sub-read", fields);
        send(&cx, &sub.child, acp::SessionUpdate::ToolCallUpdate(update))?;
        let reply = acp::SessionMessage::new("sub-reply")
            .sender_session_id(sub.child.clone())
            .recipient_session_id(session.clone())
            .content(vec![text_block("found it")]);
        send(&cx, &session, acp::SessionUpdate::SessionMessage(reply))?;
        let idle = acp::IdleStateUpdate::new().stop_reason(acp::IdleStopReason::EndTurn);
        send(
            &cx,
            &session,
            subagent_state(&sub.child, acp::StateUpdate::Idle(idle)),
        )?;
        Ok(answer)
    }

    /// Streams `reply` one word per chunk.
    async fn stream(
        &self,
        cx: &ConnectionTo<Client>,
        session: &acp::SessionId,
        reply: &str,
    ) -> Result<acp::StopReason, Error> {
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

    /// One file edit, shaped like claude-agent-acp's (see `--edit`).
    async fn edit(
        &self,
        cx: &ConnectionTo<Client>,
        session: &acp::SessionId,
        id: &str,
        kind: EditKind,
        path: &PathBuf,
    ) -> Result<(), Error> {
        let old = std::fs::read_to_string(path).ok();
        // The diff the call starts with, the file written, and the hunk the update reports.
        let (start, new, hunk) = match kind {
            EditKind::Append => {
                let new = format!("{}edited by the fake agent\n", old.as_deref().unwrap_or(""));
                (
                    acp::Diff::new(path.clone(), new.clone()).old_text(old),
                    new,
                    None,
                )
            }
            EditKind::Write => {
                let new = "written by the fake agent\n".to_string();
                (acp::Diff::new(path.clone(), new.clone()), new, None)
            }
            EditKind::Hunk => {
                let old = old.expect("--edit-hunk needs an existing file");
                let old_lines: Vec<&str> = old.lines().collect();
                let n = old_lines.len() / 2;
                let edited = format!("{} // edited", old_lines[n]);
                let mut lines = old_lines.clone();
                lines[n] = &edited;
                let (from, to) = (n.saturating_sub(3), (n + 3).min(lines.len() - 1));
                let old_hunk = old_lines[from..=to].join("\n");
                let new_hunk = lines[from..=to].join("\n");
                let snippet = acp::Diff::new(path.clone(), edited.clone()).old_text(old_lines[n]);
                let mut new = lines.join("\n");
                if old.ends_with('\n') {
                    new.push('\n');
                }
                let hunk = acp::Diff::new(path.clone(), new_hunk).old_text(old_hunk);
                (snippet, new, Some((hunk, from as u32 + 1)))
            }
        };
        let title = format!("Edit {}", path.file_name().unwrap_or_default().display());
        let call = acp::ToolCall::new(id.to_string(), title.clone())
            .kind(acp::ToolKind::Edit)
            .status(acp::ToolCallStatus::Pending)
            .content(vec![acp::ToolCallContent::Diff(start)])
            .locations(vec![acp::ToolCallLocation::new(path.clone())]);
        send(cx, session, acp::SessionUpdate::ToolCall(call))?;

        let options = vec![
            acp::PermissionOption::new("allow-once", "Yes", acp::PermissionOptionKind::AllowOnce),
            acp::PermissionOption::new("reject-once", "No", acp::PermissionOptionKind::RejectOnce),
        ];
        let fields = acp::ToolCallUpdateFields::new().title(title);
        let request = acp::RequestPermissionRequest::new(
            session.clone(),
            acp::ToolCallUpdate::new(id.to_string(), fields),
            options,
        );
        let response = cx.send_request(request).block_task().await?;
        let allowed = matches!(
            response.outcome,
            acp::RequestPermissionOutcome::Selected(s) if s.option_id.to_string() == "allow-once"
        );
        let fields = if allowed {
            std::fs::write(path, &new).map_err(Error::into_internal_error)?;
            let done = acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Completed);
            match hunk {
                Some((hunk, line)) => done
                    .content(vec![acp::ToolCallContent::Diff(hunk)])
                    .locations(vec![acp::ToolCallLocation::new(path.clone()).line(line)]),
                None => done,
            }
        } else {
            acp::ToolCallUpdateFields::new().status(acp::ToolCallStatus::Failed)
        };
        let update = acp::ToolCallUpdate::new(id.to_string(), fields);
        send(cx, session, acp::SessionUpdate::ToolCallUpdate(update))
    }

    /// Sends the `edit-1` call and its permission request on `session`; the future is the
    /// answer: the option id, or `cancelled`.
    fn ask_permission(
        &self,
        cx: &ConnectionTo<Client>,
        session: &acp::SessionId,
    ) -> Result<Asked, Error> {
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
        if self.opts.withdraw_permission {
            // Dropping the sent request withdraws it with `$/cancel_request`.
            drop(cx.send_request(request.clone()));
        }
        let sent = cx.send_request(request);
        Ok(Box::pin(async move {
            let response = sent.block_task().await?;
            Ok(match response.outcome {
                acp::RequestPermissionOutcome::Selected(s) => s.option_id.to_string(),
                _ => "cancelled".into(),
            })
        }))
    }

    /// Sends the form on `session`; the future is the answer: its JSON, `declined`, `cancelled`
    /// or `unsupported`.
    fn ask_question(
        &self,
        cx: &ConnectionTo<Client>,
        session: &acp::SessionId,
    ) -> Result<Asked, Error> {
        if !self.forms.load(Ordering::Relaxed) {
            return Ok(Box::pin(async { Ok("unsupported".into()) }));
        }
        let mut schema = acp::ElicitationSchema::new()
            .property(
                "color",
                acp::StringPropertySchema::new()
                    .title("Color".to_string())
                    .one_of(vec![
                        acp::EnumOption::new("red", "Red"),
                        acp::EnumOption::new("blue", "Blue"),
                    ]),
                true,
            )
            .property(
                "note",
                acp::StringPropertySchema::new().title("Note".to_string()),
                false,
            );
        if self.opts.unsupported_question {
            let shape = acp::OtherElicitationPropertySchema::new("_shape", Default::default());
            schema = schema.property("shape", acp::ElicitationPropertySchema::Other(shape), false);
        }
        let form = acp::ElicitationFormMode::new(
            acp::ElicitationSessionScope::new(session.clone()),
            schema,
        );
        let request = acp::CreateElicitationRequest::new(form, "Pick a color");
        if self.opts.withdraw_question {
            // Dropping the sent request withdraws it with `$/cancel_request`.
            drop(cx.send_request(request.clone()));
        }
        let sent = cx.send_request(request);
        Ok(Box::pin(async move {
            let response = sent.block_task().await?;
            Ok(match response.action {
                acp::ElicitationAction::Accept(accept) => {
                    serde_json::to_string(&accept.content.unwrap_or_default())
                        .map_err(Error::into_internal_error)?
                }
                acp::ElicitationAction::Decline => "declined".into(),
                _ => "cancelled".into(),
            })
        }))
    }
}

fn text_block(text: impl Into<String>) -> acp::ContentBlock {
    acp::ContentBlock::Text(acp::TextContent::new(text))
}

fn chunk(text: impl Into<String>) -> acp::ContentChunk {
    acp::ContentChunk::new(text_block(text))
}

/// A `subagent_update` of `child`'s state, for its parent.
fn subagent_state(child: &acp::SessionId, state: acp::StateUpdate) -> acp::SessionUpdate {
    acp::SessionUpdate::SubagentUpdate(acp::SubagentUpdate::new(child.clone()).state(state))
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
    if let Some(path) = &opts.cwd_file {
        let cwd = std::env::current_dir().expect("current dir");
        std::fs::write(path, cwd.to_string_lossy().as_bytes()).expect("write cwd file");
    }
    let fake = Arc::new(Fake {
        opts,
        next_session: AtomicU64::new(1),
        cancelled: Mutex::default(),
        forms: AtomicBool::new(false),
        subagents: AtomicBool::new(false),
        mcp: Mutex::default(),
        config: Mutex::default(),
        steered: Mutex::default(),
    });
    smol::block_on(serve(fake))
}

async fn serve(fake: Arc<Fake>) -> Result<(), Error> {
    Agent
        .v1()
        .name("wai-fake-agent")
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |request: acp::InitializeRequest, responder, _cx| {
                    let forms = request
                        .client_capabilities
                        .elicitation
                        .is_some_and(|e| e.supports_form());
                    fake.forms.store(forms, Ordering::Relaxed);
                    let subagents = request.client_capabilities.subagents.is_some();
                    fake.subagents.store(subagents, Ordering::Relaxed);
                    let caps = acp::AgentCapabilities::new()
                        .load_session(!fake.opts.no_load)
                        .prompt_capabilities(acp::PromptCapabilities::new().image(true))
                        .session_capabilities(
                            acp::SessionCapabilities::new()
                                .list(acp::SessionListCapabilities::new())
                                .fork(acp::SessionForkCapabilities::new()),
                        );
                    let meta = serde_json::json!({"steering": {"supported": true}});
                    let meta = meta.as_object().filter(|_| fake.opts.steering).cloned();
                    responder.respond(
                        acp::InitializeResponse::new(request.protocol_version)
                            .agent_info(acp::Implementation::new(
                                "wai-fake-agent",
                                env!("CARGO_PKG_VERSION"),
                            ))
                            .agent_capabilities(caps)
                            .meta(meta),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |request: acp::NewSessionRequest, responder, _cx| {
                    if fake.opts.require_auth {
                        return responder.respond_with_error(Error::auth_required());
                    }
                    let id = format!("fake-{}", fake.next_session.fetch_add(1, Ordering::Relaxed));
                    fake.record_mcp(&id.clone().into(), &request.mcp_servers);
                    let options = fake.config_options(&id.clone().into());
                    responder.respond(
                        acp::NewSessionResponse::new(id)
                            .modes(modes())
                            .config_options(options),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |request: acp::LoadSessionRequest,
                            responder,
                            cx: ConnectionTo<Client>| {
                    let session = &request.session_id;
                    if session.0.starts_with("missing") {
                        return responder.respond_with_error(
                            acp::Error::session_not_found(session.clone()).into(),
                        );
                    }
                    fake.record_mcp(session, &request.mcp_servers);
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
                    responder.respond(
                        acp::LoadSessionResponse::new()
                            .modes(modes())
                            .config_options(fake.config_options(session)),
                    )
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |request: acp::ForkSessionRequest, responder, _cx| {
                    if fake.opts.fail_fork {
                        return responder.respond_with_error(Error::internal_error());
                    }
                    if request.session_id.0.starts_with("missing") {
                        return responder.respond_with_error(
                            acp::Error::session_not_found(request.session_id).into(),
                        );
                    }
                    let id = format!("fork-{}", fake.next_session.fetch_add(1, Ordering::Relaxed));
                    fake.record_mcp(&id.clone().into(), &request.mcp_servers);
                    let options = fake.config_options(&id.clone().into());
                    responder.respond(
                        acp::ForkSessionResponse::new(id)
                            .modes(modes())
                            .config_options(options),
                    )
                }
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
            {
                let fake = fake.clone();
                async move |request: acp::SetSessionConfigOptionRequest, responder, _cx| {
                    if fake.opts.reject_config {
                        return responder.respond_with_error(Error::internal_error());
                    }
                    if let acp::SessionConfigOptionValue::ValueId { value } = request.value {
                        let mut config = fake.config.lock().expect("config poisoned");
                        let session = config.entry(request.session_id.to_string()).or_default();
                        session.insert(request.config_id.to_string(), value.to_string());
                    }
                    let options = fake.config_options(&request.session_id);
                    responder.respond(acp::SetSessionConfigOptionResponse::new(
                        options.unwrap_or_default(),
                    ))
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
                    let session = request.session_id;
                    (fake.steered.lock().expect("steered poisoned"))
                        .insert(session.to_string(), Vec::new());
                    let fake = fake.clone();
                    let turn_cx = cx.clone();
                    // The turn runs outside the dispatch loop so cancels and permission answers get through.
                    cx.spawn(async move {
                        let reason = fake.turn(&turn_cx, &session).await?;
                        let steered = (fake.steered.lock().expect("steered poisoned"))
                            .remove(&*session.0)
                            .unwrap_or_default();
                        if reason == acp::StopReason::EndTurn {
                            for text in steered {
                                let more = chunk(format!(" Steered: {text}"));
                                send(
                                    &turn_cx,
                                    &session,
                                    acp::SessionUpdate::AgentMessageChunk(more),
                                )?;
                            }
                        }
                        responder.respond(acp::PromptResponse::new(reason))
                    })
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        // Last: it takes every request the handlers above did not.
        .on_receive_request(
            {
                let fake = fake.clone();
                async move |request: UntypedMessage, responder, _cx| {
                    if !fake.opts.steering || request.method != "_session/steering" {
                        return responder.respond_with_error(Error::method_not_found());
                    }
                    let session = request.params["sessionId"].as_str().unwrap_or_default();
                    let text = request.params["prompt"][0]["text"]
                        .as_str()
                        .unwrap_or_default();
                    let mut steered = fake.steered.lock().expect("steered poisoned");
                    let outcome = match steered.get_mut(session) {
                        Some(texts) => {
                            texts.push(text.to_string());
                            "injected"
                        }
                        None => "promptRequired",
                    };
                    responder.respond(serde_json::json!({ "outcome": outcome }))
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
