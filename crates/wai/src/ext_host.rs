//! The enabled extensions' processes, one `ExtHandle` each. They start once the window is open
//! (and on install or enable) and stop on disable, before uninstall and at quit. They inherit
//! wai's cwd, the config dir (R18).
//!
//! Each live one granted `panels` shows its declared panels as sidebar tabs. A panel outlives a
//! restart of its process (a project switch), so what the user typed stays; it goes when its
//! extension stops, fails or crashes.
//!
//! It also holds the enabled extensions' other contributions (templates, commands, agents, MCP
//! servers), shows their notifications and hands their session requests to the workspace
//! (`SessionCall`).

use std::cell::Cell;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use gpui_kit::*;
use wai_agent::McpServer;
use wai_ext::panel::clamp;
use wai_ext::{
    CommandSpec, ExtEvent, ExtHandle, ExtId, HostEvent, Installed, PanelSpec, Permission,
};
use wai_settings::{AgentConfig, Template};

use crate::ext_panel::{ExtPanel, PanelAction};
use crate::notify;

/// Events applied per wake of an extension's pump: a flooding one cannot hold the UI thread.
const MAX_BATCH: usize = 256;
/// At most one notification, and one `sessions/open`, per 2 s per extension: the excess
/// notifications are dropped, the excess opens refused.
const CALL_GAP: Duration = Duration::from_secs(2);
const TOO_MANY_OPENS: &str = "Too many sessions opened; try again later";

/// An extension's `sessions/open`, `sessions/prompt` or `sessions/list`, for the workspace to
/// carry out (and answer through the event's `Reply`).
pub struct SessionCall(Cell<Option<(ExtId, ExtEvent)>>);

impl SessionCall {
    /// The extension and its request, to the first subscriber that takes it.
    pub fn take(&self) -> Option<(ExtId, ExtEvent)> {
        self.0.take()
    }
}

impl EventEmitter<SessionCall> for ExtHost {}

/// A panel an extension contributes: a sidebar tab.
pub struct Panel {
    ext: ExtId,
    id: String,
    pub title: String,
    pub view: Entity<ExtPanel>,
    /// Its clicks, sent to the extension as `panel/action`.
    _actions: Subscription,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExtStatus {
    Starting,
    Ready,
    Failed(String),
    Crashed(String),
}

impl ExtStatus {
    fn live(&self) -> bool {
        matches!(self, Self::Starting | Self::Ready)
    }
}

struct Running {
    installed: Installed,
    status: ExtStatus,
    /// Dropping it kills the process.
    handle: ExtHandle,
    /// When its last notification showed.
    notified: Option<Instant>,
    /// When it last opened a session.
    opened: Option<Instant>,
    _pump: Task<()>,
}

pub struct ExtHost {
    /// Sent in `initialize`.
    project: PathBuf,
    running: HashMap<ExtId, Running>,
    /// The running extensions' panels, in the order of the list `sync` got.
    panels: Vec<Panel>,
    /// The enabled extensions, in the order of the list `sync` got: their contributions.
    enabled: Vec<Installed>,
}

impl ExtHost {
    pub fn new(project: PathBuf) -> Self {
        Self {
            project,
            running: HashMap::new(),
            panels: Vec::new(),
            enabled: Vec::new(),
        }
    }

    /// Runs exactly the enabled extensions of `list` that have a process: starts the missing
    /// ones, stops the rest. Their contributions are the enabled ones'.
    pub fn sync(&mut self, list: &[Installed], cx: &mut Context<Self>) {
        self.enabled = list.iter().filter(|i| i.enabled).cloned().collect();
        let wanted = |i: &&Installed| i.enabled && i.manifest.process.is_some();
        let wanted: Vec<_> = list.iter().filter(wanted).collect();
        self.running
            .retain(|id, _| wanted.iter().any(|i| i.manifest.id == *id));
        for installed in &wanted {
            if !self.running.contains_key(&installed.manifest.id) {
                self.start((*installed).clone(), cx);
            }
        }
        let mut old = std::mem::take(&mut self.panels);
        let live = |id| self.running.get(id).is_some_and(|r| r.status.live());
        let granted: Vec<_> = (wanted.iter())
            .filter(|i| i.granted.contains(&Permission::Panels) && live(&i.manifest.id))
            .collect();
        for installed in granted {
            let ext = &installed.manifest.id;
            for spec in &installed.manifest.contributes.panels {
                let kept = old.iter().position(|p| p.ext == *ext && p.id == spec.id);
                let panel = match kept {
                    Some(ix) => old.swap_remove(ix),
                    None => self.panel(ext.clone(), spec, cx),
                };
                self.panels.push(panel);
            }
        }
        cx.notify();
    }

    /// Kills its process now, on this thread; its contributions go (uninstall).
    pub fn stop(&mut self, id: &ExtId, cx: &mut Context<Self>) {
        self.running.remove(id);
        self.panels.retain(|p| p.ext != *id);
        self.enabled.retain(|i| i.manifest.id != *id);
        cx.notify();
    }

    pub fn stop_all(&mut self) {
        self.running.clear();
        self.panels.clear();
    }

    /// `initialize` told them the project: the live ones start again in the new one. A failed or
    /// crashed one stays so (its 3 restarts are spent until the user re-enables it).
    pub fn set_project(&mut self, project: PathBuf, cx: &mut Context<Self>) {
        if project == self.project {
            return;
        }
        self.project = project;
        let restart: Vec<ExtId> = (self.running.iter())
            .filter(|(_, r)| r.status.live())
            .map(|(id, _)| id.clone())
            .collect();
        for id in restart {
            if let Some(old) = self.running.remove(&id) {
                self.start(old.installed, cx);
            }
        }
        cx.notify();
    }

    pub fn panels(&self) -> &[Panel] {
        &self.panels
    }

    pub fn handle(&self, id: &ExtId) -> Option<&ExtHandle> {
        self.running.get(id).map(|r| &r.handle)
    }

    /// Tells every running extension, as far as each was granted what `event` needs.
    pub fn broadcast(&self, event: HostEvent) {
        for running in self.running.values() {
            running.handle.notify_event(event.clone());
        }
    }

    /// `command/run`, to a running extension.
    pub fn run_command(&self, ext: &ExtId, id: &str) {
        if let Some(handle) = self.handle(ext) {
            handle.run_command(id);
        }
    }

    pub fn templates(&self) -> impl Iterator<Item = &Template> {
        (self.enabled.iter()).flat_map(|i| &i.manifest.contributes.templates)
    }

    /// The commands the palette offers: prompt-only ones, and the others of a live process.
    pub fn commands(&self) -> Vec<(ExtId, CommandSpec)> {
        let live = |id| self.running.get(id).is_some_and(|r| r.status.live());
        let commands = self.enabled.iter().flat_map(|i| {
            let id = &i.manifest.id;
            (i.manifest.contributes.commands.iter())
                .filter(move |c| c.prompt.is_some() || live(id))
                .map(move |c| (id.clone(), c.clone()))
        });
        commands.collect()
    }

    /// Agents and MCP servers run code: only an enabled extension granted `process` brings them
    /// (R48), never a project's `.wai/` (R15).
    fn trusted(&self) -> impl Iterator<Item = &Installed> {
        (self.enabled.iter()).filter(|i| i.granted.contains(&Permission::Process))
    }

    /// `<ext-id>/<name>`, each spawned like a settings agent, from wai's cwd (R18).
    pub fn agents(&self) -> BTreeMap<String, AgentConfig> {
        let agents = self.trusted().flat_map(|i| {
            (i.manifest.contributes.agents.iter()).map(move |a| {
                let name = format!("{}/{}", i.manifest.id.as_str(), a.name);
                // The JSON form takes the path as it is: no shell quoting to get wrong.
                let program = i.program(&a.command);
                // Lossy: `json!` on a path that is not UTF-8 would panic.
                let command = serde_json::json!({"command": program.to_string_lossy()});
                let command = command.to_string();
                (name, AgentConfig { command })
            })
        });
        agents.collect()
    }

    pub fn mcp_servers(&self) -> Vec<McpServer> {
        let servers = self.trusted().flat_map(|i| {
            (i.manifest.contributes.mcp_servers.iter()).map(move |m| McpServer {
                name: m.name.clone(),
                command: i.program(&m.command),
                args: m.args.clone(),
                env: m.env.clone(),
            })
        });
        servers.collect()
    }

    /// An OS notification, at most one per `CALL_GAP` per extension.
    fn notify(&mut self, id: &ExtId, title: &str, body: &str) {
        let Some(running) = self.running.get_mut(id) else {
            return;
        };
        if !allow(&mut running.notified, Instant::now()) {
            let title = clamp(title, 80);
            return eprintln!("wai: {} notifies too often; dropped {title:?}", id.as_str());
        }
        notify::notify(&clamp(title, 100), &clamp(body, 300));
    }

    /// Whether extension `id` may open a session now, at most once per `CALL_GAP`.
    fn may_open(&mut self, id: &ExtId) -> bool {
        (self.running.get_mut(id)).is_some_and(|r| allow(&mut r.opened, Instant::now()))
    }

    fn panel(&self, ext: ExtId, spec: &PanelSpec, cx: &mut Context<Self>) -> Panel {
        let view = cx.new(|_| ExtPanel::new());
        let (to, panel) = (ext.clone(), spec.id.clone());
        let actions = cx.subscribe(&view, move |this, _, action: &PanelAction, _| {
            if let Some(handle) = this.handle(&to) {
                handle.panel_action(&panel, &action.action, action.values.clone());
            }
        });
        Panel {
            ext,
            id: spec.id.clone(),
            title: spec.title.clone(),
            view,
            _actions: actions,
        }
    }

    pub fn status(&self, id: &ExtId) -> Option<&ExtStatus> {
        self.running.get(id).map(|r| &r.status)
    }

    fn start(&mut self, installed: Installed, cx: &mut Context<Self>) {
        let (handle, events) = ExtHandle::spawn(&installed, &self.project);
        let id = installed.manifest.id.clone();
        let pump = cx.spawn({
            let id = id.clone();
            async move |this, cx| {
                while let Ok(first) = events.recv().await {
                    let applied = this.update(cx, |this, cx| {
                        for event in batch(first, &events) {
                            this.apply(&id, event, cx);
                        }
                    });
                    if applied.is_err() {
                        break;
                    }
                    // A full channel answers `recv` at once: let the frame and input through.
                    smol::future::yield_now().await;
                }
            }
        });
        let running = Running {
            installed,
            status: ExtStatus::Starting,
            handle,
            notified: None,
            opened: None,
            _pump: pump,
        };
        self.running.insert(id, running);
    }

    fn apply(&mut self, id: &ExtId, event: ExtEvent, cx: &mut Context<Self>) {
        let status = match event {
            ExtEvent::Ready { .. } => ExtStatus::Ready,
            ExtEvent::Failed { error, .. } => ExtStatus::Failed(error),
            ExtEvent::Crashed { stderr_tail, .. } => ExtStatus::Crashed(stderr_tail),
            ExtEvent::Render { panel, tree } => {
                let panel = self.panels.iter().find(|p| p.ext == *id && p.id == panel);
                if let Some(panel) = panel {
                    #[cfg(feature = "demo")]
                    if let Ok(now) =
                        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                    {
                        let loading = format!("{tree:?}").contains("Loading");
                        eprintln!(
                            "wai: panel render loading={loading} at {} ms",
                            now.as_millis()
                        );
                    }
                    panel.view.update(cx, |view, cx| view.set_tree(tree, cx));
                }
                return;
            }
            ExtEvent::Notify { title, body } => return self.notify(id, &title, &body),
            ExtEvent::OpenSession { reply, .. } if !self.may_open(id) => {
                return reply.send(Err(TOO_MANY_OPENS.into()));
            }
            event @ (ExtEvent::OpenSession { .. }
            | ExtEvent::Prompt { .. }
            | ExtEvent::ListSessions { .. }) => {
                return cx.emit(SessionCall(Cell::new(Some((id.clone(), event)))));
            }
        };
        if let Some(running) = self.running.get_mut(id) {
            if !status.live() {
                self.panels.retain(|p| p.ext != *id);
            }
            running.status = status;
            cx.notify();
        }
    }
}

/// Whether a call (a notification, a session opened) at `now` may go; it then counts as the last
/// one.
fn allow(last: &mut Option<Instant>, now: Instant) -> bool {
    if last.is_some_and(|last| now < last + CALL_GAP) {
        return false;
    }
    *last = Some(now);
    true
}

/// `first` and what else waits, up to `MAX_BATCH` events: one update applies them.
fn batch<T>(first: T, rx: &smol::channel::Receiver<T>) -> Vec<T> {
    let more = std::iter::from_fn(|| rx.try_recv().ok()).take(MAX_BATCH - 1);
    std::iter::once(first).chain(more).collect()
}

/// The real-process tests: extensions are `/bin/sh` scripts.
#[cfg(all(test, unix))]
pub(crate) mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use serde_json::{Value, json};
    use wai_ext::{ExtStore, MANIFEST_FILE, Source, parse_manifest};
    use wai_settings::{AgentConfig, Settings};

    use crate::chat::{ChatItem, ChatSession, TurnState};
    use crate::workspace::tests::{fake_settings, gated, open, wait_until};
    use crate::workspace::{Pane, Workspace};

    /// A binary of this workspace, next to the test binary's `deps/`.
    fn binary(name: &str) -> PathBuf {
        let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
        let found = (std::env::current_exe().unwrap().ancestors())
            .map(|d| d.join(&name))
            .find(|p| p.is_file());
        found.unwrap_or_else(|| panic!("run `cargo build --workspace --bins` first ({name})"))
    }

    /// An executable `dir/<rel>` that runs `program` with `args` (a shell fragment).
    fn script(dir: &Path, rel: &str, program: &Path, args: &str) {
        use std::os::unix::fs::PermissionsExt as _;
        let path = dir.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            format!("#!/bin/sh\nexec '{}' {args}\n", program.display()),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// An extension in `tmp/src-<id>`: `manifest`, plus `bin/run` (`wai-fake-ext --log
    /// tmp/<id>.log <ext_flags>`) and `bin/agent` (`wai-fake-agent <agent_flags>`). Installed in
    /// `config` with every permission it asks for (and enabled), not started.
    pub(crate) fn install(
        tmp: &Path,
        config: &Path,
        manifest: &str,
        ext_flags: &str,
        agent_flags: &str,
    ) -> PathBuf {
        let id = parse_manifest(manifest, tmp).unwrap().id;
        let (src, log) = (
            tmp.join(format!("src-{}", id.as_str())),
            log_path(tmp, id.as_str()),
        );
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join(MANIFEST_FILE), manifest).unwrap();
        let flags = format!("--log '{}' {ext_flags}", log.display());
        script(&src, "bin/run", &binary("wai-fake-ext"), &flags);
        script(&src, "bin/agent", &binary("wai-fake-agent"), agent_flags);
        let mut store = ExtStore::open(config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        let granted = staged.manifest().permissions.clone();
        store.commit(staged, granted).unwrap();
        config.join("extensions").join(id.as_str())
    }

    pub(crate) fn log_path(tmp: &Path, id: &str) -> PathBuf {
        tmp.join(format!("{id}.log"))
    }

    /// The workspace with `settings`, whose config dir is `config`, its extensions started.
    pub(crate) fn workspace_at<'a>(
        config: &Path,
        settings: Settings,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        cx.executor().allow_parking(); // agents and extensions are real processes
        let (workspace, cx) = open(cx, settings);
        workspace.update(cx, |w, cx| {
            w.user_dir = config.to_path_buf();
            w.start_extensions(cx);
        });
        (workspace, cx)
    }

    /// What the fake extension reported (`{title, body}` lines), once `done` holds.
    pub(crate) fn logged(
        log: &Path,
        what: &str,
        cx: &mut VisualTestContext,
        mut done: impl FnMut(&[Value]) -> bool,
    ) -> Vec<Value> {
        // Whole lines only: the fake may be writing the last one.
        let read = || -> Vec<Value> {
            let text = std::fs::read_to_string(log).unwrap_or_default();
            (text.split_inclusive('\n').filter(|l| l.ends_with('\n')))
                .map(|l| serde_json::from_str(l).unwrap())
                .collect()
        };
        wait_until(cx, what, |_| done(&read()));
        read()
    }

    /// The bodies of the reports titled `title`, as JSON.
    pub(crate) fn bodies(lines: &[Value], title: &str) -> Vec<Value> {
        let titled = lines.iter().filter(|l| l["title"] == title);
        titled
            .map(|l| serde_json::from_str(l["body"].as_str().unwrap()).unwrap())
            .collect()
    }

    /// The reply to the fake's call `id`.
    fn reply(log: &Path, id: u64, cx: &mut VisualTestContext) -> Value {
        let lines = logged(log, "the reply", cx, |lines| {
            bodies(lines, "reply").iter().any(|r| r["id"] == id)
        });
        let replies = bodies(&lines, "reply");
        replies.into_iter().find(|r| r["id"] == id).unwrap()
    }

    /// The first session in a pane, once one shows: the user's open in the focused pane, an
    /// extension's beside it (R61).
    fn pane_session(
        workspace: &Entity<Workspace>,
        cx: &mut VisualTestContext,
    ) -> Entity<ChatSession> {
        let shown = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| w.panes.iter().find_map(|p| p.session().cloned()))
        };
        wait_until(cx, "a session in a pane", |cx| shown(cx).is_some());
        shown(cx).unwrap()
    }

    fn items(session: &Entity<ChatSession>, cx: &mut VisualTestContext) -> Vec<ChatItem> {
        session.read_with(cx, |s, _| s.model.items.clone())
    }

    /// Sends `prompt` from pane 0's box; returns the agent's answer once the turn ended.
    fn turn(session: &Entity<ChatSession>, prompt: &str, cx: &mut VisualTestContext) -> String {
        cx.simulate_input(prompt);
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the answer", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::Idle)
                && matches!(items(session, cx).last(), Some(ChatItem::Agent { .. }))
        });
        match items(session, cx).pop() {
            Some(ChatItem::Agent { text }) => text,
            other => unreachable!("{other:?}"),
        }
    }

    fn wait_gone(pid: &str) {
        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        let deadline = Instant::now() + Duration::from_secs(2);
        while alive() {
            assert!(Instant::now() < deadline, "process {pid} still alive");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// Disables extension 0 from the Extensions view.
    fn disable_first(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.show_extensions(window, cx);
            })
        });
        crate::extensions_view::tests::click(("ext-enabled", 0usize), cx);
    }

    /// An extension's agent joins the registry as `<ext-id>/<name>`, its command resolved in the
    /// extension's folder and spawned from wai's cwd (R18), and its sessions work like any
    /// agent's. Disabling the extension removes it and shuts it down: its session sees it exit.
    #[gpui_kit::test]
    fn extension_agent_appears_in_registry(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let (pid, cwd) = (tmp.path().join("agent.pid"), tmp.path().join("agent.cwd"));
        let manifest = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\"]\n\
            [[contributes.agents]]\nname = \"echo\"\ncommand = \"bin/agent\"\n";
        let flags = format!(
            "--reply 'from the extension' --pid-file '{}' --cwd-file '{}'",
            pid.display(),
            cwd.display()
        );
        let dir = install(tmp.path(), &config, manifest, "", &flags);
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let registry = workspace.read_with(cx, |w, _| w.registry.clone());
        assert_eq!(
            registry.read_with(cx, |r, _| r.names()),
            ["fake", "sample/echo"]
        );

        cx.simulate_keystrokes("secondary-shift-p");
        cx.simulate_input("sample/echo");
        cx.simulate_keystrokes("enter");
        let session = pane_session(&workspace, cx);
        assert_eq!(session.read_with(cx, |s, _| s.agent.clone()), "sample/echo");
        assert_eq!(turn(&session, "hi", cx), "from the extension");
        let spawned_in = std::fs::read_to_string(&cwd).unwrap();
        assert_eq!(
            PathBuf::from(spawned_in),
            std::env::current_dir().unwrap(),
            "spawned from wai's cwd, not {}",
            dir.display()
        );

        let pid = std::fs::read_to_string(&pid).unwrap();
        disable_first(&workspace, cx);
        assert!(!registry.read_with(cx, |r, _| r.knows("sample/echo")));
        wait_gone(&pid);
        wait_until(cx, "the session to see its agent exit", |cx| {
            session.read_with(cx, |s, _| matches!(s.model.state, TurnState::Error(_)))
        });
        let handle = registry.update(cx, |r, cx| r.handle("sample/echo", cx));
        assert!(handle.is_none(), "a removed agent never starts again");
    }

    /// R46: the enabled extensions' MCP servers go to `new_session` and `load_session` (the fake
    /// agent's `--echo-mcp` answers with what it got); a disabled extension's do not.
    #[gpui_kit::test]
    fn mcp_servers_passed_to_new_session(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\"]\n\
            [[contributes.mcp_servers]]\nname = \"jira\"\ncommand = \"bin/jira-mcp\"\n\
            args = [\"--stdio\"]\n";
        let dir = install(tmp.path(), &config, manifest, "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings("--echo-mcp"), cx);
        let jira = format!("mcp:jira={}", dir.join("bin/jira-mcp").display());

        cx.simulate_keystrokes("secondary-n");
        let session = pane_session(&workspace, cx);
        assert_eq!(turn(&session, "hi", cx), jira);

        cx.update(|window, cx| session.update(cx, |s, cx| s.load(false, window, cx)));
        wait_until(cx, "the replay", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::Idle)
        });
        assert_eq!(
            turn(&session, "again", cx),
            jira,
            "load_session gets them too"
        );

        disable_first(&workspace, cx);
        cx.simulate_keystrokes("secondary-n");
        wait_until(cx, "a second session", |cx| {
            workspace.read_with(cx, |w, _| {
                w.panes[0].session().is_some_and(|s| *s != session)
            })
        });
        let second = pane_session(&workspace, cx);
        assert_eq!(turn(&second, "hi", cx), "mcp:");
    }

    /// `extensions/wai-navegador`, installed from its folder with consent: its pinned Playwright
    /// MCP server reaches the agent's `new_session` (npx itself never runs here).
    #[gpui_kit::test]
    fn navegador_mcp_reaches_new_session(cx: &mut TestAppContext) {
        let Some(npx) = wai_settings::find_program(Path::new("npx")) else {
            // wai leaves out an MCP server whose command is not on PATH.
            return eprintln!("SKIPPED: npx is not on PATH");
        };
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions/wai-navegador");
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        let granted = staged.manifest().permissions.clone();
        store.commit(staged, granted).unwrap();

        let (workspace, cx) = workspace_at(&config, fake_settings("--echo-mcp"), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let servers = host.read_with(cx, |h, _| h.mcp_servers());
        let [server] = servers.as_slice() else {
            panic!("one MCP server: {servers:?}")
        };
        assert_eq!(server.command, Path::new("npx"));
        assert!(
            server.args[2].starts_with("@playwright/mcp@"),
            "{servers:?}"
        );

        cx.simulate_keystrokes("secondary-n");
        let session = pane_session(&workspace, cx);
        let want = format!("mcp:playwright={}", npx.display());
        assert_eq!(turn(&session, "hi", cx), want);
    }

    /// R51 `sessions/open`: `send: false` pre-fills like a template and answers the session; an
    /// unknown agent is an error (asked by a second extension: one opens once per 2 s).
    #[gpui_kit::test]
    fn sessions_open_prefills(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"opener\"\nname = \"Opener\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"sessions:control\"]\n\
            [process]\ncommand = \"bin/run\"\n";
        let calls = r#"--call 'sessions/open {"prompt": "draft me", "send": false}'"#;
        install(tmp.path(), &config, manifest, calls, "");
        let ghost = r#"--call 'sessions/open {"agent": "ghost"}'"#;
        install(
            tmp.path(),
            &config,
            &manifest.replace("opener", "ghost"),
            ghost,
            "",
        );
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let log = log_path(tmp.path(), "opener");

        let session = pane_session(&workspace, cx);
        assert_eq!(session.read_with(cx, |s, cx| s.draft(cx)), "draft me");
        assert!(items(&session, cx).is_empty(), "not sent");
        assert_eq!(
            reply(&log, 1, cx)["result"],
            json!({"agent": "fake", "session": "fake-1"})
        );
        let ghost = reply(&log_path(tmp.path(), "ghost"), 1, cx);
        assert_eq!(ghost["error"]["message"], "No agent named ghost");
    }

    /// R51 `sessions/open` with `send: true` sends the prompt: a granted permission.
    #[gpui_kit::test]
    fn sessions_open_sends(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"opener\"\nname = \"Opener\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"sessions:control\"]\n\
            [process]\ncommand = \"bin/run\"\n";
        let calls = r#"--call 'sessions/open {"agent": "fake", "prompt": "go", "send": true}'"#;
        install(tmp.path(), &config, manifest, calls, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);

        let session = pane_session(&workspace, cx);
        wait_until(
            cx,
            "the answer",
            |cx| matches!(items(&session, cx).last(), Some(ChatItem::Agent { text }) if text == "Hello from fake agent"),
        );
        let first = items(&session, cx).into_iter().next();
        assert!(matches!(first, Some(ChatItem::User { text }) if text == "go"));
        assert_eq!(session.read_with(cx, |s, cx| s.draft(cx)), "");
    }

    /// Without `sessions:control`, `sessions/open` is denied in wai-ext and opens nothing.
    #[gpui_kit::test]
    fn sessions_open_needs_permission(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"opener\"\nname = \"Opener\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\"]\n[process]\ncommand = \"bin/run\"\n";
        let calls = r#"--call 'sessions/open {"prompt": "x", "send": true}'"#;
        install(tmp.path(), &config, manifest, calls, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let log = log_path(tmp.path(), "opener");

        let denied = reply(&log, 1, cx)["error"].clone();
        assert_eq!(
            denied,
            json!({"code": -32001, "message": "Permission denied: sessions:control"})
        );
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert!(w.sessions.is_empty() && matches!(w.panes[0], Pane::Empty(_)));
        });
    }

    /// R51 `sessions/list` answers the open sessions; `sessions/prompt` sends to one.
    #[gpui_kit::test]
    fn sessions_list_and_prompt(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"driver\"\nname = \"Driver\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"sessions:read\", \"sessions:control\"]\n\
            [process]\ncommand = \"bin/run\"\n";
        let calls = r#"--call sessions/list --call 'sessions/prompt {"agent": "fake", "session": "fake-1", "text": "from the extension"}'"#;
        install(tmp.path(), &config, manifest, calls, "");
        // The session opens first; the extension starts after it.
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.executor().allow_parking();
        cx.simulate_keystrokes("secondary-n");
        let session = pane_session(&workspace, cx);
        workspace.update(cx, |w, cx| {
            w.user_dir = config.clone();
            w.start_extensions(cx);
        });
        let log = log_path(tmp.path(), "driver");

        assert_eq!(
            reply(&log, 1, cx)["result"],
            json!([{"agent": "fake", "session": "fake-1", "title": null, "state": "idle"}])
        );
        wait_until(cx, "the extension's turn", |cx| {
            matches!(items(&session, cx).last(), Some(ChatItem::Agent { .. }))
        });
        let first = items(&session, cx).into_iter().next();
        assert!(matches!(first, Some(ChatItem::User { text }) if text == "from the extension"));
    }

    /// R51 host → ext: session states and ended turns reach only an extension granted
    /// `sessions:read`; saved files reach every one.
    #[gpui_kit::test]
    fn session_events_follow_permissions(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = |id: &str, permissions: &str| {
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\n\
                 permissions = [{permissions}]\n[process]\ncommand = \"bin/run\"\n"
            )
        };
        let reader = manifest("reader", "\"process\", \"sessions:read\"");
        install(tmp.path(), &config, &reader, "", "");
        install(
            tmp.path(),
            &config,
            &manifest("blind", "\"process\""),
            "",
            "",
        );
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let initialized = |cx: &mut VisualTestContext, id: &str| {
            let log = log_path(tmp.path(), id);
            logged(&log, "initialize", cx, |l| {
                !bodies(l, "initialize").is_empty()
            });
        };
        initialized(cx, "reader");
        initialized(cx, "blind");

        cx.simulate_keystrokes("secondary-n");
        let session = pane_session(&workspace, cx);
        turn(&session, "hi", cx);
        let file = tmp.path().join("notes.md");
        std::fs::write(&file, "notes\n").unwrap();
        let path = file.clone();
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_file(path, None, window, cx)));
        cx.run_until_parked();
        cx.simulate_keystrokes("secondary-s");
        let saved = json!({"path": crate::sidebar::canonical(&file)});

        let reader = logged(
            &log_path(tmp.path(), "reader"),
            "reader's events",
            cx,
            |l| bodies(l, "event/file_saved").contains(&saved),
        );
        let states: Vec<_> = bodies(&reader, "event/session_state")
            .into_iter()
            .map(|b| b["state"].clone())
            .collect();
        assert_eq!(states, ["working", "idle"]);
        assert_eq!(
            bodies(&reader, "event/turn_ended"),
            [json!({"agent": "fake", "session": "fake-1", "reason": "end_turn"})]
        );
        let blind = logged(&log_path(tmp.path(), "blind"), "blind's events", cx, |l| {
            bodies(l, "event/file_saved").contains(&saved)
        });
        let seen: Vec<_> = blind.iter().map(|l| l["title"].clone()).collect();
        assert_eq!(seen, ["initialize", "event/file_saved"]);
    }

    /// R51: at most one notification per 2 s per extension; the excess is dropped.
    #[gpui_kit::test]
    fn notify_is_rate_limited(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"noisy\"\nname = \"Noisy\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"notify\"]\n[process]\ncommand = \"bin/run\"\n";
        let calls = r#"--call 'notify {"title": "one", "body": "1"}' --call 'notify {"title": "two"}' --call 'notify {"title": "three"}'"#;
        install(tmp.path(), &config, manifest, calls, "");
        let (_, cx) = workspace_at(&config, Settings::default(), cx);

        let log = log_path(tmp.path(), "noisy");
        logged(&log, "three answers", cx, |l| bodies(l, "reply").len() == 3);
        cx.run_until_parked();
        assert_eq!(crate::notify::sent(), [("one".into(), "1".into())]);
    }

    const OPENER: &str = "id = \"opener\"\nname = \"Opener\"\nversion = \"1.0.0\"\n\
        permissions = [\"process\", \"sessions:control\"]\n[process]\ncommand = \"bin/run\"\n";

    /// `open`, its extensions in `config` started only after `before` ran.
    fn start_after<'a>(
        config: &Path,
        settings: Settings,
        cx: &'a mut TestAppContext,
        before: impl FnOnce(&Entity<Workspace>, &mut VisualTestContext),
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        cx.executor().allow_parking(); // agents and extensions are real processes
        let (workspace, cx) = open(cx, settings);
        before(&workspace, cx);
        workspace.update(cx, |w, cx| {
            w.user_dir = config.to_path_buf();
            w.start_extensions(cx);
        });
        (workspace, cx)
    }

    /// R56: an extension's session never takes the keyboard. Typing in the terminal stays
    /// there: Enter never sends the extension's prompt.
    #[gpui_kit::test]
    fn extension_session_leaves_the_terminal_focused(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let calls = r#"--call 'sessions/open {"prompt": "draft me"}'"#;
        install(tmp.path(), &config, OPENER, calls, "");
        let (workspace, cx) = start_after(&config, fake_settings(""), cx, |_, cx| {
            cx.simulate_keystrokes("ctrl-`");
        });
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("the terminal"));

        let session = pane_session(&workspace, cx);
        cx.simulate_input("echo typed-in-terminal");
        assert_eq!(session.read_with(cx, |s, cx| s.draft(cx)), "draft me");
        wait_until(cx, "the typing in the terminal", |cx| {
            terminal.read_with(cx, |t, _| {
                (t.snapshot.rows.iter())
                    .any(|runs| runs.iter().any(|r| r.text.contains("typed-in-terminal")))
            })
        });
    }

    /// R56: nor does it take the editor's keyboard, in the pane it opens beside the editor.
    #[gpui_kit::test]
    fn extension_session_leaves_the_editor_focused(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let file = tmp.path().join("notes.md");
        std::fs::write(&file, "notes\n").unwrap();
        let calls = r#"--call 'sessions/open {"prompt": "draft me"}'"#;
        install(tmp.path(), &config, OPENER, calls, "");
        let (workspace, cx) = start_after(&config, fake_settings(""), cx, |workspace, cx| {
            let path = file.clone();
            cx.update(|window, cx| {
                workspace.update(cx, |w, cx| w.open_file(path, None, window, cx))
            });
            cx.run_until_parked();
        });

        wait_until(cx, "the session beside the editor", |cx| {
            workspace.read_with(cx, |w, _| {
                w.panes.len() == 2 && w.panes[1].session().is_some()
            })
        });
        assert_eq!(workspace.read_with(cx, |w, _| w.focused_pane), 0);
        cx.simulate_input("typed ");
        let editor = workspace.read_with(cx, |w, _| match &w.panes[0] {
            Pane::Editor(pane) => pane.clone(),
            _ => unreachable!("the editor stays in pane 0"),
        });
        let text = editor.read_with(cx, |p, cx| {
            p.tabs[p.active].read(cx).editor.read(cx).text().to_string()
        });
        assert_eq!(text, "typed notes\n");
    }

    /// R56: at most one `sessions/open` per 2 s per extension; the excess is answered with an
    /// error and opens nothing.
    #[gpui_kit::test]
    fn sessions_open_is_rate_limited(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let calls =
            r#"--call 'sessions/open {"prompt": "one"}' --call 'sessions/open {"prompt": "two"}'"#;
        install(tmp.path(), &config, OPENER, calls, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let log = log_path(tmp.path(), "opener");

        let refused = reply(&log, 2, cx);
        assert_eq!(
            refused["error"]["message"],
            "Too many sessions opened; try again later"
        );
        assert_eq!(
            reply(&log, 1, cx)["result"],
            json!({"agent": "fake", "session": "fake-1"})
        );
        cx.run_until_parked();
        assert_eq!(workspace.read_with(cx, |w, _| w.sessions.len()), 1);
    }

    /// R56: the user's ⌘N is never refused while an extension's session prepares: in that pane
    /// (once the user is in it; R61: it prepares beside the focused pane) ⌘N takes it, and the
    /// extension's session still opens (in the open list, in no pane).
    #[gpui_kit::test]
    fn new_session_takes_over_an_extensions_preparing_pane(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let calls = r#"--call 'sessions/open {"agent": "slow", "prompt": "theirs"}'"#;
        install(tmp.path(), &config, OPENER, calls, "");
        let (mut settings, gate) = gated("", tmp.path());
        // `fake` again: the extension's session prepares on the same gate.
        let fake = settings.agents["fake"].command.clone();
        let command = format!("/bin/sh -c {}", wai_agent::quote(format!("exec {fake}")));
        settings
            .agents
            .insert("slow".into(), AgentConfig { command });
        let (workspace, cx) = workspace_at(&config, settings, cx);
        let log = log_path(tmp.path(), "opener");

        wait_until(cx, "the extension's session to prepare", |cx| {
            workspace.read_with(
                cx,
                |w, _| matches!(w.panes.get(1), Some(Pane::Preparing { agent, .. }) if agent == "slow"),
            )
        });
        workspace.update(cx, |w, _| w.focused_pane = 1);
        cx.simulate_keystrokes("secondary-n");
        workspace.read_with(cx, |w, _| {
            assert!(matches!(&w.panes[1], Pane::Preparing { agent, .. } if agent == "fake"))
        });
        std::fs::write(&gate, "").unwrap();
        let mine = pane_session(&workspace, cx);
        assert_eq!(mine.read_with(cx, |s, _| s.agent.clone()), "fake");

        let theirs = reply(&log, 1, cx)["result"].clone();
        assert_eq!(theirs["agent"], "slow");
        workspace.read_with(cx, |w, cx| {
            assert_eq!(w.sessions.len(), 2, "the extension's session opens too");
            assert_eq!(w.panes.len(), 2);
            assert!(matches!(w.panes[0], Pane::Empty(_)));
            assert_eq!(w.panes[1].session(), Some(&mine), "in no pane");
            let draft = |s: &Entity<ChatSession>| s.read(cx).draft(cx);
            // Both land once the gate opens: their order is not fixed.
            let theirs = w.sessions.iter().find(|s| **s != mine).unwrap();
            assert_eq!(draft(theirs), "theirs");
        });
    }
}

#[cfg(test)]
mod limits {
    use std::time::{Duration, Instant};

    use super::{CALL_GAP, MAX_BATCH, allow, batch};

    #[test]
    fn notifications_wait_two_seconds() {
        let (mut last, now) = (None, Instant::now());
        assert!(allow(&mut last, now));
        assert!(!allow(&mut last, now + Duration::from_millis(1_999)));
        assert!(allow(&mut last, now + CALL_GAP));
        assert!(!allow(&mut last, now + CALL_GAP + Duration::from_secs(1)));
    }

    /// R56: an extension folder whose path is not UTF-8 never panics: its agents' commands
    /// carry the path lossily.
    #[cfg(unix)]
    #[test]
    fn agent_commands_take_any_folder() {
        use std::os::unix::ffi::OsStrExt as _;
        use std::path::Path;

        use wai_ext::{Installed, Permission, parse_manifest};

        let dir = Path::new(std::ffi::OsStr::from_bytes(b"/tmp/ext-\xff"));
        let manifest = "id = \"odd\"\nname = \"Odd\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\"]\n\
            [[contributes.agents]]\nname = \"echo\"\ncommand = \"bin/agent\"\n";
        let mut host = super::ExtHost::new(dir.into());
        host.enabled = vec![Installed {
            manifest: parse_manifest(manifest, dir).unwrap(),
            dir: dir.into(),
            enabled: true,
            granted: vec![Permission::Process],
        }];
        let command = &host.agents()["odd/echo"].command;
        let command: serde_json::Value = serde_json::from_str(command).unwrap();
        assert_eq!(command["command"], "/tmp/ext-\u{fffd}/bin/agent");
    }

    /// R51: one wake applies at most 256 events; the rest wait for the next.
    #[test]
    fn event_drain_is_capped() {
        let (tx, rx) = smol::channel::unbounded();
        for n in 1..1_000 {
            tx.try_send(n).unwrap();
        }
        let first = batch(0, &rx);
        assert_eq!(first.len(), MAX_BATCH);
        assert_eq!(first.last(), Some(&255));
        assert_eq!(rx.len(), 1_000 - MAX_BATCH);
    }
}
