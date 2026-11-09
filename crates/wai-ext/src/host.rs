//! The extension host: one process per extension, spoken to in JSON-RPC 2.0 over stdio.
//!
//! A thread per extension runs its session loop; per process, one thread reads stdout (lines
//! capped at 4 MB), one drains stderr into a 20-line tail and one writes stdin, so a stuck
//! extension never blocks the loop. Every call either way is checked against what the user
//! granted. Nothing polls: the loop sleeps on its channels, with a timer only while
//! `initialize` is pending or a render waits for its slot.

use std::collections::VecDeque;
use std::io::{BufReader, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use async_channel::{Receiver, Sender};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::rpc::{self, Incoming, Line, RpcError};
use crate::{ExtId, Installed, Permission};

pub const MAX_MESSAGE: usize = 4 << 20;
pub const INITIALIZE_TIMEOUT: Duration = Duration::from_secs(10);
/// Restarts after a crash, per handle; the next crash is `Crashed`.
pub const MAX_RESTARTS: u32 = 3;
/// At most one render per 10 ms (100 per second) per extension; the excess is coalesced.
pub const RENDER_GAP: Duration = Duration::from_millis(10);
const STDERR_LINES: usize = 20;
/// Messages waiting for an extension that does not read its stdin; more are dropped.
const OUTGOING: usize = 256;
const INIT_ID: u64 = 0;

#[derive(Debug)]
pub enum ExtEvent {
    /// `initialize` answered: after the start and after each restart.
    Ready {
        id: ExtId,
    },
    Render {
        panel: String,
        tree: Value,
    },
    Notify {
        title: String,
        body: String,
    },
    OpenSession {
        agent: Option<String>,
        prompt: Option<String>,
        send: bool,
        reply: Reply,
    },
    Prompt {
        agent: String,
        session: String,
        text: String,
    },
    ListSessions {
        reply: Reply,
    },
    /// It cannot run: no process to start, or no answer to `initialize` within 10 s. Last event.
    Failed {
        id: ExtId,
        error: String,
    },
    /// It exited once more after 3 restarts. Last event.
    Crashed {
        id: ExtId,
        stderr_tail: String,
    },
}

/// What wai tells extensions about.
#[derive(Debug, Clone)]
pub enum HostEvent {
    SessionState {
        agent: String,
        session: String,
        state: String,
    },
    TurnEnded {
        agent: String,
        session: String,
        reason: String,
    },
    FileSaved {
        path: PathBuf,
    },
}

impl HostEvent {
    /// The notification and the permission it needs.
    fn message(self) -> (&'static str, Value, Option<Permission>) {
        let read = Some(Permission::SessionsRead);
        match self {
            Self::SessionState {
                agent,
                session,
                state,
            } => (
                "event/session_state",
                json!({"agent": agent, "session": session, "state": state}),
                read,
            ),
            Self::TurnEnded {
                agent,
                session,
                reason,
            } => (
                "event/turn_ended",
                json!({"agent": agent, "session": session, "reason": reason}),
                read,
            ),
            Self::FileSaved { path } => (
                "event/file_saved",
                json!({"path": path.to_string_lossy()}),
                None,
            ),
        }
    }
}

/// wai's answer to an extension's request. Dropped unanswered, it answers with an error.
pub struct Reply(Option<(Sender<Command>, u64, Value)>);

impl Reply {
    pub fn send(mut self, result: Result<Value, String>) {
        if let Some((commands, generation, id)) = self.0.take() {
            let result = result.map_err(|e| RpcError::new(rpc::HOST_ERROR, e));
            // ponytail: closed means the extension stopped; nobody waits for the answer.
            let _ = commands.try_send(Command::Respond {
                generation,
                id,
                result,
            });
        }
    }
}

impl Drop for Reply {
    fn drop(&mut self) {
        if self.0.is_some() {
            Reply(self.0.take()).send(Err("wai did not answer".into()));
        }
    }
}

impl std::fmt::Debug for Reply {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Reply")
    }
}

pub(crate) enum Command {
    /// Host → ext: a request (its answer is ignored) or a notification.
    Send {
        method: &'static str,
        params: Value,
        request: bool,
    },
    /// Answers request `id` of process `generation` (a restart starts a new one).
    Respond {
        generation: u64,
        id: Value,
        result: Result<Value, RpcError>,
    },
}

/// The running process, shared with the handle so `shutdown` kills it on the caller's thread.
#[derive(Default)]
struct Shared {
    stopped: bool,
    child: Option<Child>,
}

fn lock(shared: &Mutex<Shared>) -> MutexGuard<'_, Shared> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The extension and, on unix, its whole process group: what it started dies with it.
fn kill(mut child: Child) {
    #[cfg(unix)]
    {
        let pid = rustix::process::Pid::from_child(&child);
        let _ = rustix::process::kill_process_group(pid, rustix::process::Signal::KILL);
    }
    let _ = child.kill();
    // Reaped, so the pid (and its group id) is not reused while we still hold it.
    let _ = child.wait();
}

/// One extension's process. Dropping it is `shutdown`.
pub struct ExtHandle {
    granted: Vec<Permission>,
    commands: Sender<Command>,
    shared: Arc<Mutex<Shared>>,
}

impl ExtHandle {
    /// Starts the extension's process on its own thread, from wai's cwd (R18: the config dir,
    /// never the extension's folder, which it gets as `WAI_EXT_DIR` and in `initialize`). Events
    /// arrive on the receiver, which closes once the extension is gone.
    pub fn spawn(installed: &Installed, project: &Path) -> (ExtHandle, Receiver<ExtEvent>) {
        let (commands, command_rx) = async_channel::unbounded();
        let (events, event_rx) = async_channel::unbounded();
        let shared = Arc::new(Mutex::new(Shared::default()));
        let handle = ExtHandle {
            granted: installed.granted.clone(),
            commands: commands.clone(),
            shared: shared.clone(),
        };
        let id = installed.manifest.id.clone();
        match Ext::new(installed, project) {
            Ok(ext) => {
                let name = format!("ext-{}", id.as_str());
                std::thread::Builder::new()
                    .name(name)
                    .spawn(move || {
                        smol::block_on(supervise(ext, shared, commands, command_rx, events))
                    })
                    .expect("cannot start an extension thread");
            }
            Err(error) => {
                let _ = events.try_send(ExtEvent::Failed { id, error });
            }
        }
        (handle, event_rx)
    }

    /// Sent only when the extension was granted what the event needs.
    pub fn notify_event(&self, event: HostEvent) {
        let (method, params, need) = event.message();
        if need.is_none_or(|p| self.granted.contains(&p)) {
            self.send(method, params, false);
        }
    }

    pub fn run_command(&self, id: &str) {
        self.send("command/run", json!({"id": id}), true);
    }

    pub fn panel_action(&self, panel: &str, action: &str, values: Value) {
        if self.granted.contains(&Permission::Panels) {
            let params = json!({"panel": panel, "action_id": action, "values": values});
            self.send("panel/action", params, true);
        }
    }

    /// Kills the process (its group on unix) now, on this thread; nothing restarts it.
    pub fn shutdown(&self) {
        let mut shared = lock(&self.shared);
        shared.stopped = true;
        if let Some(child) = shared.child.take() {
            kill(child);
        }
        drop(shared);
        self.commands.close();
    }

    fn send(&self, method: &'static str, params: Value, request: bool) {
        // ponytail: closed means the extension is gone; there is nobody to tell.
        let _ = self.commands.try_send(Command::Send {
            method,
            params,
            request,
        });
    }
}

impl Drop for ExtHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// What the session loop needs from the manifest and the consent.
struct Ext {
    id: ExtId,
    program: PathBuf,
    args: Vec<String>,
    dir: PathBuf,
    granted: Vec<Permission>,
    panels: Vec<String>,
    init: Value,
}

impl Ext {
    fn new(installed: &Installed, project: &Path) -> Result<Ext, String> {
        let m = &installed.manifest;
        let spec = m.process.as_ref().ok_or("it has no process")?;
        if !installed.granted.contains(&Permission::Process) {
            return Err("it may not run its own programs".into());
        }
        // A bare name is looked up on PATH; a relative path is inside the extension's folder.
        let program = if spec.command.contains(['/', '\\']) {
            installed.dir.join(&spec.command)
        } else {
            PathBuf::from(&spec.command)
        };
        let granted: Vec<String> = installed.granted.iter().map(ToString::to_string).collect();
        Ok(Ext {
            id: m.id.clone(),
            program,
            args: spec.args.clone(),
            dir: installed.dir.clone(),
            granted: installed.granted.clone(),
            panels: m.contributes.panels.iter().map(|p| p.id.clone()).collect(),
            init: json!({
                "protocol": 1,
                "wai_version": env!("CARGO_PKG_VERSION"),
                "ext_id": m.id.as_str(),
                "granted": granted,
                "project": project.to_string_lossy(),
                "ext_dir": installed.dir.to_string_lossy(),
            }),
        })
    }

    /// Starts the process unless the handle stopped: under the lock, so a `shutdown` racing a
    /// restart cannot miss the new child.
    fn start(&self, shared: &Mutex<Shared>) -> Result<Option<Pipes>, String> {
        let mut shared = lock(shared);
        if shared.stopped {
            return Ok(None);
        }
        let mut command = std::process::Command::new(&self.program);
        command
            .args(&self.args)
            .env("WAI_EXT_DIR", &self.dir)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(unix)]
        std::os::unix::process::CommandExt::process_group(&mut command, 0);
        let mut child = command
            .spawn()
            .map_err(|e| format!("cannot start {}: {e}", self.program.display()))?;
        let (stdin, stdout, stderr) =
            (child.stdin.take(), child.stdout.take(), child.stderr.take());
        let (Some(mut stdin), Some(stdout), Some(stderr)) = (stdin, stdout, stderr) else {
            kill(child);
            return Err("its pipes did not open".into());
        };
        shared.child = Some(child);
        drop(shared);

        let name = |what: &str| format!("ext-{}-{what}", self.id.as_str());
        // Bounded: a flood waits in the pipe, not in wai's memory.
        let (line_tx, lines) = async_channel::bounded(8);
        thread(name("out"), move || {
            let mut stdout = BufReader::new(stdout);
            while let Ok(Some(line)) = rpc::read_line(&mut stdout, MAX_MESSAGE) {
                if line_tx.send_blocking(line).is_err() {
                    break;
                }
            }
        });
        let (out, out_rx) = async_channel::bounded::<Vec<u8>>(OUTGOING);
        thread(name("in"), move || {
            while let Ok(bytes) = out_rx.recv_blocking() {
                if stdin
                    .write_all(&bytes)
                    .and_then(|()| stdin.flush())
                    .is_err()
                {
                    break;
                }
            }
        });
        let (tail_tx, tail) = async_channel::bounded(1);
        thread(name("err"), move || {
            let mut stderr = BufReader::new(stderr);
            let mut last = VecDeque::with_capacity(STDERR_LINES);
            while let Ok(Some(line)) = rpc::read_line(&mut stderr, 4096) {
                if last.len() == STDERR_LINES {
                    last.pop_front();
                }
                last.push_back(match line {
                    Line::Message(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    Line::TooLarge => "[line too long]".into(),
                });
            }
            let _ = tail_tx.try_send(Vec::from(last).join("\n"));
        });
        Ok(Some(Pipes { lines, out, tail }))
    }

    /// An ext→host call, checked against the granted permissions.
    fn call(
        &self,
        method: &str,
        params: Value,
        reply: impl FnOnce() -> Reply,
    ) -> Result<Call, RpcError> {
        let need = |p: Permission| {
            if self.granted.contains(&p) {
                return Ok(());
            }
            let error = format!("Permission denied: {p}");
            Err(RpcError::new(rpc::PERMISSION_DENIED, error))
        };
        Ok(match method {
            "sessions/list" => {
                need(Permission::SessionsRead)?;
                Call::Deferred(ExtEvent::ListSessions { reply: reply() })
            }
            "sessions/open" => {
                need(Permission::SessionsControl)?;
                let p: OpenParams = params_of(params)?;
                Call::Deferred(ExtEvent::OpenSession {
                    agent: p.agent,
                    prompt: p.prompt,
                    send: p.send,
                    reply: reply(),
                })
            }
            "sessions/prompt" => {
                need(Permission::SessionsControl)?;
                let p: PromptParams = params_of(params)?;
                Call::Done(ExtEvent::Prompt {
                    agent: p.agent,
                    session: p.session,
                    text: p.text,
                })
            }
            "notify" => {
                need(Permission::Notify)?;
                let p: NotifyParams = params_of(params)?;
                Call::Done(ExtEvent::Notify {
                    title: p.title,
                    body: p.body,
                })
            }
            "panel/render" => {
                need(Permission::Panels)?;
                let p: RenderParams = params_of(params)?;
                if !self.panels.contains(&p.panel) {
                    let error = format!("unknown panel {:?}", p.panel);
                    return Err(RpcError::new(rpc::INVALID_PARAMS, error));
                }
                Call::Render(p.panel, p.tree)
            }
            _ => {
                let error = format!("Method not found: {method}");
                return Err(RpcError::new(rpc::METHOD_NOT_FOUND, error));
            }
        })
    }
}

fn thread(name: String, f: impl FnOnce() + Send + 'static) {
    std::thread::Builder::new()
        .name(name)
        .spawn(f)
        .expect("cannot start an extension thread");
}

struct Pipes {
    lines: Receiver<Line>,
    out: Sender<Vec<u8>>,
    /// The last stderr lines, once stderr closes.
    tail: Receiver<String>,
}

enum Call {
    /// Answered `null` now.
    Done(ExtEvent),
    /// wai answers through the event's `Reply`.
    Deferred(ExtEvent),
    Render(String, Value),
}

#[derive(Deserialize)]
struct OpenParams {
    agent: Option<String>,
    prompt: Option<String>,
    #[serde(default)]
    send: bool,
}

#[derive(Deserialize)]
struct PromptParams {
    agent: String,
    session: String,
    text: String,
}

#[derive(Deserialize)]
struct NotifyParams {
    title: String,
    #[serde(default)]
    body: String,
}

#[derive(Deserialize)]
struct RenderParams {
    panel: String,
    tree: Value,
}

fn params_of<T: for<'de> Deserialize<'de>>(params: Value) -> Result<T, RpcError> {
    serde_json::from_value(params).map_err(|e| RpcError::new(rpc::INVALID_PARAMS, e.to_string()))
}

/// Coalesces `panel/render`: one render per `RENDER_GAP` for the whole extension, each panel's
/// latest tree kept while it waits (so at most one per declared panel is held).
#[derive(Default)]
struct Renders {
    last: Option<Instant>,
    pending: VecDeque<(String, Value)>,
}

impl Renders {
    /// The render to emit now, if its slot is free; otherwise it waits.
    fn offer(&mut self, panel: String, tree: Value, now: Instant) -> Option<(String, Value)> {
        if let Some(waiting) = self.pending.iter_mut().find(|(p, _)| *p == panel) {
            waiting.1 = tree;
            return None;
        }
        if self.pending.is_empty() && self.last.is_none_or(|last| now >= last + RENDER_GAP) {
            self.last = Some(now);
            return Some((panel, tree));
        }
        self.pending.push_back((panel, tree));
        None
    }

    fn next_due(&self) -> Option<Instant> {
        let last = self.last?;
        (!self.pending.is_empty()).then(|| last + RENDER_GAP)
    }

    fn take_due(&mut self, now: Instant) -> Option<(String, Value)> {
        if self.next_due()? > now {
            return None;
        }
        self.last = Some(now);
        self.pending.pop_front()
    }
}

enum End {
    Stopped,
    Exited,
    Failed(String),
}

enum Wake {
    Line(Option<Line>),
    Command(Option<Command>),
    Timer,
}

async fn supervise(
    ext: Ext,
    shared: Arc<Mutex<Shared>>,
    commands_tx: Sender<Command>,
    commands: Receiver<Command>,
    events: Sender<ExtEvent>,
) {
    // ponytail: unbounded, so this fails only when nobody listens anymore.
    let emit = |event| {
        let _ = events.try_send(event);
    };
    let id = ext.id.clone();
    for generation in 0.. {
        let pipes = match ext.start(&shared) {
            Ok(Some(pipes)) => pipes,
            Ok(None) => return,
            Err(error) => return emit(ExtEvent::Failed { id, error }),
        };
        let end = session(&ext, &pipes, generation, &commands_tx, &commands, &emit).await;
        let child = lock(&shared).child.take();
        if let Some(child) = child {
            kill(child);
        }
        if lock(&shared).stopped {
            return;
        }
        match end {
            End::Stopped => return,
            End::Failed(error) => return emit(ExtEvent::Failed { id, error }),
            End::Exited => {}
        }
        if generation == u64::from(MAX_RESTARTS) {
            // The killed process closed stderr; a grandchild holding it gets a second.
            let tail = smol::future::or(
                async { pipes.tail.recv().await.unwrap_or_default() },
                async {
                    smol::Timer::after(Duration::from_secs(1)).await;
                    String::new()
                },
            );
            let stderr_tail = tail.await;
            return emit(ExtEvent::Crashed { id, stderr_tail });
        }
        eprintln!("wai: extension {} exited; restarting it", id.as_str());
    }
}

/// One process's life: `initialize`, then calls both ways until it exits, fails or is stopped.
async fn session(
    ext: &Ext,
    pipes: &Pipes,
    generation: u64,
    commands_tx: &Sender<Command>,
    commands: &Receiver<Command>,
    emit: &impl Fn(ExtEvent),
) -> End {
    let warned = std::cell::Cell::new(false);
    let write = |msg: Value| {
        let full = matches!(
            pipes.out.try_send(rpc::encode(&msg)),
            Err(async_channel::TrySendError::Full(_))
        );
        if full && !warned.replace(true) {
            let id = ext.id.as_str();
            eprintln!("wai: extension {id} is not reading its input; dropping messages");
        }
    };
    let respond = |id: Option<Value>, result| {
        if let Some(id) = id {
            write(rpc::response(id, result));
        }
    };
    let init_deadline = Instant::now() + INITIALIZE_TIMEOUT;
    let mut ready = false;
    let mut next_id = INIT_ID + 1;
    let mut renders = Renders::default();
    write(rpc::call(Some(INIT_ID), "initialize", ext.init.clone()));
    loop {
        let now = Instant::now();
        if !ready && now >= init_deadline {
            return End::Failed("it did not answer initialize within 10 s".into());
        }
        if let Some((panel, tree)) = renders.take_due(now) {
            emit(ExtEvent::Render { panel, tree });
        }
        let deadline = [(!ready).then_some(init_deadline), renders.next_due()]
            .into_iter()
            .flatten()
            .min();
        let timer = async {
            match deadline {
                Some(deadline) => smol::Timer::at(deadline).await,
                None => std::future::pending().await,
            };
            Wake::Timer
        };
        // `race`, not `or`: a flood of lines cannot starve commands or the timer.
        let wake = smol::future::race(
            async { Wake::Line(pipes.lines.recv().await.ok()) },
            smol::future::race(async { Wake::Command(commands.recv().await.ok()) }, timer),
        )
        .await;
        let line = match wake {
            Wake::Timer => continue,
            Wake::Command(None) => return End::Stopped,
            Wake::Command(Some(Command::Send {
                method,
                params,
                request,
            })) => {
                let id = request.then(|| {
                    next_id += 1;
                    next_id - 1
                });
                write(rpc::call(id, method, params));
                continue;
            }
            Wake::Command(Some(Command::Respond {
                generation: g,
                id,
                result,
            })) => {
                if g == generation {
                    respond(Some(id), result);
                }
                continue;
            }
            Wake::Line(None) => return End::Exited,
            Wake::Line(Some(Line::TooLarge)) => {
                let error = RpcError::new(rpc::INVALID_REQUEST, "Message larger than 4 MB");
                respond(Some(Value::Null), Err(error));
                continue;
            }
            Wake::Line(Some(Line::Message(bytes))) => bytes,
        };
        let (id, method, params) = match rpc::parse(&line) {
            Err(error) => {
                respond(Some(Value::Null), Err(error));
                continue;
            }
            Ok(Incoming::Response { id, result }) => {
                if !ready && id == INIT_ID {
                    match result {
                        Ok(_) => {
                            ready = true;
                            emit(ExtEvent::Ready { id: ext.id.clone() });
                        }
                        Err(e) => return End::Failed(format!("initialize failed: {}", e.message)),
                    }
                }
                continue;
            }
            Ok(Incoming::Call { id, method, params }) => (id, method, params),
        };
        let reply = || Reply(id.clone().map(|id| (commands_tx.clone(), generation, id)));
        match ext.call(&method, params, reply) {
            Err(error) => respond(id, Err(error)),
            Ok(Call::Deferred(event)) => emit(event),
            Ok(Call::Done(event)) => {
                emit(event);
                respond(id, Ok(Value::Null));
            }
            Ok(Call::Render(panel, tree)) => {
                // Now, not the loop's `now`: that was before the wait.
                if let Some((panel, tree)) = renders.offer(panel, tree, Instant::now()) {
                    emit(ExtEvent::Render { panel, tree });
                }
                respond(id, Ok(Value::Null));
            }
        }
    }
}
