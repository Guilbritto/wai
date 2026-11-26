//! The enabled extensions' processes, one `ExtHandle` each. They start once the window is open
//! (and on install or enable) and stop on disable, before uninstall and at quit. They inherit
//! wai's cwd, the config dir (R18).
//!
//! Each live one granted `panels` shows its declared panels, as sidebar tabs or main-area panes
//! opened from the activity bar. A panel outlives a
//! restart of its process (a project switch), so what the user typed stays; it goes when its
//! extension stops, fails or crashes.
//!
//! It also holds the enabled extensions' other contributions (templates, commands, agents, MCP
//! servers), shows their notifications and hands their session requests to the workspace
//! (`SessionCall`).

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::*;
use wai_agent::McpServer;
use wai_ext::panel::{ItemState, clamp};
use wai_ext::{
    Bar, CommandSpec, ExtEvent, ExtHandle, ExtId, HostEvent, Installed, PanelLocation, PanelSpec,
    Permission,
};
use wai_settings::{AgentConfig, Template};

use crate::ext_panel::{ExtPanel, PanelAction, lucide};
use crate::notify;

/// Events applied per wake of an extension's pump: a flooding one cannot hold the UI thread.
const MAX_BATCH: usize = 256;
/// At most one notification, one `sessions/open` and one `panel/open` per 2 s per extension: the
/// excess notifications are dropped, the excess opens refused.
const CALL_GAP: Duration = Duration::from_secs(2);
/// At most `EMIT_LIMIT` `events/emit` per `EMIT_WINDOW` per extension; the excess is dropped.
const EMIT_LIMIT: u32 = 20;
const EMIT_WINDOW: Duration = Duration::from_secs(1);
const TOO_MANY_OPENS: &str = "Too many sessions opened; try again later";
const TOO_MANY_SHOWS: &str = "Too many panel opens; try again later";

/// An extension's `sessions/open`, `sessions/prompt`, `sessions/list`, `sessions/show` or
/// `panel/open`, for the workspace to carry out (and answer through the event's `Reply`).
pub struct SessionCall(Cell<Option<(ExtId, ExtEvent)>>);

impl SessionCall {
    /// The extension and its request, to the first subscriber that takes it.
    pub fn take(&self) -> Option<(ExtId, ExtEvent)> {
        self.0.take()
    }
}

impl EventEmitter<SessionCall> for ExtHost {}

/// A panel's icon: a bundled Lucide one, or the extension's own SVG, drawn as a mask.
#[derive(Debug, Clone, PartialEq)]
pub enum PanelIcon {
    Lucide(Lucide),
    /// The absolute path of the file.
    Svg(SharedString),
}

/// The largest SVG icon file read.
pub(crate) const MAX_SVG_ICON: u64 = 32 * 1024;

/// `spec`'s icon: the extension's `.svg` file when it is a small regular file (no symlink),
/// else the Lucide icon of that name.
pub(crate) fn panel_icon(spec: &PanelSpec, ext_dir: &Path) -> Option<PanelIcon> {
    if let Some(rel) = spec.svg_icon() {
        let path = ext_dir.join(rel);
        let meta = std::fs::symlink_metadata(&path).ok()?;
        let ok = meta.is_file() && meta.len() <= MAX_SVG_ICON;
        return ok.then(|| PanelIcon::Svg(path.to_string_lossy().into_owned().into()));
    }
    spec.icon.as_deref().and_then(lucide).map(PanelIcon::Lucide)
}

/// A panel an extension contributes: a sidebar tab, or a pane in the main area.
pub struct Panel {
    ext: ExtId,
    id: String,
    pub title: String,
    /// The extension's name, shown in the pane's header.
    pub ext_name: SharedString,
    /// Its icon, when it has a usable one; else the rail shows the title's initials.
    pub icon: Option<PanelIcon>,
    /// A sidebar tab, a main-area pane, or a slot of the focused prompt box.
    pub location: PanelLocation,
    /// A prompt panel's width in px.
    pub width: Option<u16>,
    /// A sidebar panel shown in the main area's place (`Workspace::wide_shown`).
    pub wide: bool,
    /// A `prompt-side` panel drawn in a box of its own.
    pub frame: bool,
    pub view: Entity<ExtPanel>,
    /// Its clicks, sent to the extension as `panel/action`.
    _actions: Subscription,
}

impl Panel {
    pub fn main(&self) -> bool {
        self.location == PanelLocation::Main
    }

    /// `<ext id>/<panel id>`.
    pub fn key(&self) -> String {
        format!("{}/{}", self.ext.as_str(), self.id)
    }
}

/// An item an enabled extension contributes to a bar.
#[derive(Debug, Clone, PartialEq)]
pub struct BarItem {
    /// `<ext id>#<item id>`, apart from a panel's `<ext id>/<panel id>`.
    pub key: String,
    pub ext: ExtId,
    pub ext_name: SharedString,
    pub id: String,
    pub bar: Bar,
    pub title: String,
    /// What its live process last said, when that is drawn.
    pub state: Option<ItemState>,
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
    /// When it last showed one of its panels (`panel/open`).
    shown: Option<Instant>,
    /// The start and count of its current `events/emit` window.
    emitted: Option<(Instant, u32)>,
    /// What each item last said (`item/update`); empty again for a fresh process.
    items: HashMap<String, ItemState>,
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
    /// The last `event/session_focused`, sent again to an extension that starts. Set while the
    /// workspace renders, so not through an update.
    focused: RefCell<Option<(Option<String>, Option<String>)>>,
    /// Every broadcast, for tests that run no extension.
    #[cfg(test)]
    pub(crate) sent: RefCell<Vec<HostEvent>>,
}

impl ExtHost {
    pub fn new(project: PathBuf) -> Self {
        Self {
            project,
            running: HashMap::new(),
            panels: Vec::new(),
            enabled: Vec::new(),
            focused: RefCell::new(None),
            #[cfg(test)]
            sent: RefCell::default(),
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
                    None => self.panel(installed, spec, cx),
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

    /// The panels with an activity bar button: not the docked ones.
    pub fn rail_panels(&self) -> Vec<&Panel> {
        (self.panels.iter())
            .filter(|p| !p.location.docked())
            .collect()
    }

    /// Extension `ext`'s panel `id`, while shown.
    pub fn panel_of(&self, ext: &ExtId, id: &str) -> Option<&Panel> {
        (self.panels.iter()).find(|p| p.ext == *ext && p.id == id)
    }

    /// Every item of the enabled extensions granted `bar`: stopped ones stay listed, without a
    /// state.
    pub fn items(&self) -> Vec<BarItem> {
        let items = self.enabled.iter().flat_map(|i| {
            let ext = &i.manifest.id;
            let running = self.running.get(ext).filter(|r| r.status.live());
            let granted = i.granted.contains(&Permission::Bar);
            (i.manifest.contributes.items.iter().filter(move |_| granted)).map(move |spec| {
                let state = running.and_then(|r| r.items.get(&spec.id));
                BarItem {
                    key: format!("{}#{}", ext.as_str(), spec.id),
                    ext: ext.clone(),
                    ext_name: clamp(&i.manifest.name, crate::extensions_view::NAME_CHARS).into(),
                    id: spec.id.clone(),
                    bar: spec.bar,
                    title: clamp(&spec.title, crate::extensions_view::NAME_CHARS),
                    state: state.filter(|s| s.shown()).cloned(),
                }
            })
        });
        items.collect()
    }

    /// `item/action`, to a running extension.
    pub fn item_action(&self, ext: &ExtId, item: &str) {
        if let Some(handle) = self.handle(ext) {
            handle.item_action(item);
        }
    }

    /// The enabled extensions whose process failed or crashed: name and error.
    pub fn stopped(&self) -> Vec<(String, String)> {
        let stopped = self.enabled.iter().filter_map(|i| {
            match self.running.get(&i.manifest.id).map(|r| &r.status) {
                Some(ExtStatus::Failed(e) | ExtStatus::Crashed(e)) => {
                    Some((i.manifest.name.clone(), e.clone()))
                }
                _ => None,
            }
        });
        stopped.collect()
    }

    pub fn handle(&self, id: &ExtId) -> Option<&ExtHandle> {
        self.running.get(id).map(|r| &r.handle)
    }

    /// Tells every running extension, as far as each was granted what `event` needs.
    pub fn broadcast(&self, event: HostEvent) {
        #[cfg(test)]
        self.sent.borrow_mut().push(event.clone());
        for running in self.running.values() {
            running.handle.notify_event(event.clone());
        }
    }

    /// `event/session_focused` when the focused session changed: `agent` and `session` of the
    /// focused pane's session, `None` when it is not one.
    pub fn focus(&self, agent: Option<String>, session: Option<String>) {
        let now = Some((agent, session));
        if *self.focused.borrow() != now {
            self.focused.replace(now);
            self.broadcast(self.focus_event().unwrap());
        }
    }

    fn focus_event(&self) -> Option<HostEvent> {
        let (agent, session) = self.focused.borrow().clone()?;
        Some(HostEvent::SessionFocused { agent, session })
    }

    /// The first panel at `location` of the running extensions, in list order.
    pub fn panel_at(&self, location: PanelLocation) -> Option<&Panel> {
        self.panels.iter().find(|p| p.location == location)
    }

    /// `command/run`, to a running extension.
    pub fn run_command(&self, ext: &ExtId, id: &str) {
        if let Some(handle) = self.handle(ext) {
            handle.run_command(id);
        }
    }

    /// The enabled extensions' theme files as (extension folder, file).
    pub fn theme_files(&self) -> Vec<(String, PathBuf)> {
        let files = self.enabled.iter().flat_map(|i| {
            let dir = i.dir.to_string_lossy().into_owned();
            (i.manifest.contributes.themes.iter()).map(move |t| (dir.clone(), i.dir.join(t)))
        });
        files.collect()
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

    /// Whether extension `id` may show a panel now, at most once per `CALL_GAP`.
    fn may_show(&mut self, id: &ExtId) -> bool {
        (self.running.get_mut(id)).is_some_and(|r| allow(&mut r.shown, Instant::now()))
    }

    fn panel(&self, installed: &Installed, spec: &PanelSpec, cx: &mut Context<Self>) -> Panel {
        let (ext, ext_name) = (
            installed.manifest.id.clone(),
            installed.manifest.name.clone(),
        );
        let ext_name = SharedString::from(ext_name);
        let foot = spec.location == PanelLocation::SidebarFoot;
        let view = cx.new(|_| {
            let mut panel = ExtPanel::new(ext_name.clone());
            panel.foot = foot;
            panel
        });
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
            ext_name,
            icon: panel_icon(spec, &installed.dir),
            location: spec.location,
            width: spec.width,
            wide: spec.wide,
            frame: spec.frame.unwrap_or(true),
            view,
            _actions: actions,
        }
    }

    pub fn status(&self, id: &ExtId) -> Option<&ExtStatus> {
        self.running.get(id).map(|r| &r.status)
    }

    fn start(&mut self, installed: Installed, cx: &mut Context<Self>) {
        let (handle, events) = ExtHandle::spawn(&installed, &self.project);
        if let Some(event) = self.focus_event() {
            handle.notify_event(event);
        }
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
            shown: None,
            emitted: None,
            items: HashMap::new(),
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
            ExtEvent::Item { item, state } => {
                if let Some(running) = self.running.get_mut(id).filter(|r| r.status.live())
                    && running.items.get(&item) != Some(&state)
                {
                    running.items.insert(item, state);
                    cx.notify();
                }
                return;
            }
            ExtEvent::Emit { name, data } => {
                let Some(running) = self.running.get_mut(id) else {
                    return;
                };
                if !allow_burst(&mut running.emitted, Instant::now()) {
                    // Once per window: a flood must not flood the log too.
                    if running.emitted.is_some_and(|(_, n)| n == EMIT_LIMIT + 1) {
                        eprintln!("wai: {} emits too often; dropping events", id.as_str());
                    }
                    return;
                }
                for (other, running) in &self.running {
                    if other != id {
                        running.handle.notify_event(HostEvent::Ext {
                            source: id.clone(),
                            name: name.clone(),
                            data: data.clone(),
                        });
                    }
                }
                return;
            }
            ExtEvent::Notify { title, body } => return self.notify(id, &title, &body),
            ExtEvent::OpenSession { reply, .. } if !self.may_open(id) => {
                return reply.send(Err(TOO_MANY_OPENS.into()));
            }
            ExtEvent::OpenPanel { reply, .. } | ExtEvent::ShowSession { reply, .. }
                if !self.may_show(id) =>
            {
                return reply.send(Err(TOO_MANY_SHOWS.into()));
            }
            event @ (ExtEvent::OpenSession { .. }
            | ExtEvent::Prompt { .. }
            | ExtEvent::ListSessions { .. }
            | ExtEvent::ReadSession { .. }
            | ExtEvent::CancelSession { .. }
            | ExtEvent::ShowSession { .. }
            | ExtEvent::AnswerPermission { .. }
            | ExtEvent::AnswerQuestion { .. }
            | ExtEvent::OpenPanel { .. }) => {
                return cx.emit(SessionCall(Cell::new(Some((id.clone(), event)))));
            }
        };
        if let Some(running) = self.running.get_mut(id) {
            if !status.live() {
                self.panels.retain(|p| p.ext != *id);
                running.items.clear();
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

/// Whether an event at `now` fits the emitter's current window of `EMIT_LIMIT` per `EMIT_WINDOW`.
fn allow_burst(window: &mut Option<(Instant, u32)>, now: Instant) -> bool {
    let (start, count) = match *window {
        Some((start, count)) if now < start + EMIT_WINDOW => (start, count + 1),
        _ => (now, 1),
    };
    *window = Some((start, count));
    count <= EMIT_LIMIT
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

    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use serde_json::{Value, json};
    use wai_ext::{ExtEvent, ExtId, ExtStore, HostEvent, MANIFEST_FILE, Source, parse_manifest};
    use wai_settings::{AgentConfig, Settings};

    use super::{PanelIcon, panel_icon};
    use crate::chat::{ChatItem, ChatSession, TurnState};
    use crate::workspace::tests::{fake_settings, gated, open, start_now, wait_until};
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
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_settings(window, cx)));
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
        let drafted = workspace.read_with(cx, |w, cx| {
            let draft = w.panes.iter().find_map(|p| p.draft());
            draft.map(|d| d.read(cx).agent.clone())
        });
        assert_eq!(drafted.as_deref(), Some("sample/echo"));
        cx.simulate_input("hi");
        cx.simulate_keystrokes("enter");
        let session = pane_session(&workspace, cx);
        assert_eq!(session.read_with(cx, |s, _| s.agent.clone()), "sample/echo");
        wait_until(cx, "the answer", |cx| {
            session.read_with(cx, |s, _| s.model.state == TurnState::Idle)
                && matches!(items(&session, cx).last(), Some(ChatItem::Agent { .. }))
        });
        let answer = match items(&session, cx).pop() {
            Some(ChatItem::Agent { text }) => text,
            other => unreachable!("{other:?}"),
        };
        assert_eq!(answer, "from the extension");
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

        let session = start_now(&workspace, cx);
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
        let second = start_now(&workspace, cx);
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

        let session = start_now(&workspace, cx);
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

    /// API 1.11 `sessions/open` with `background` takes no pane: the session is in the open list only.
    #[gpui_kit::test]
    fn sessions_open_in_background(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"opener\"\nname = \"Opener\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"sessions:control\"]\n\
            [process]\ncommand = \"bin/run\"\n";
        let calls = r#"--call 'sessions/open {"agent": "fake", "prompt": "go", "send": true, "background": true}'"#;
        install(tmp.path(), &config, manifest, calls, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let log = log_path(tmp.path(), "opener");
        let panes = workspace.read_with(cx, |w, _| w.panes.len());

        assert_eq!(
            reply(&log, 1, cx)["result"],
            json!({"agent": "fake", "session": "fake-1"})
        );
        let session = workspace.read_with(cx, |w, _| w.sessions[0].clone());
        wait_until(
            cx,
            "the answer",
            |cx| matches!(items(&session, cx).last(), Some(ChatItem::Agent { text }) if text == "Hello from fake agent"),
        );
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), panes, "no pane added");
            assert!(w.panes.iter().all(|p| p.session().is_none()), "in no pane");
        });
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
        let session = start_now(&workspace, cx);
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

        let session = start_now(&workspace, cx);
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

    /// An extension hears the events of the sources it was granted, never its own, and at most
    /// `EMIT_LIMIT` of a burst.
    #[gpui_kit::test]
    fn ext_events_reach_granted_listeners(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = |id: &str, permissions: &str| {
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\napi = \"1.7\"\n\
                 permissions = [{permissions}]\n[process]\ncommand = \"bin/run\"\n"
            )
        };
        let events = "\"process\", \"events:emitter\"";
        for (id, permissions) in [
            ("emitter", events),
            ("listener", events),
            ("deaf", "\"process\""),
        ] {
            install(tmp.path(), &config, &manifest(id, permissions), "", "");
        }
        let (workspace, cx) = workspace_at(&config, Settings::default(), cx);
        for id in ["emitter", "listener", "deaf"] {
            logged(&log_path(tmp.path(), id), "initialize", cx, |l| {
                !bodies(l, "initialize").is_empty()
            });
        }

        let emitter = ExtId::new("emitter").unwrap();
        workspace.update(cx, |w, cx| {
            w.ext_host.update(cx, |host, cx| {
                for n in 0..25 {
                    let event = ExtEvent::Emit {
                        name: "pr.review".into(),
                        data: json!({"n": n}),
                    };
                    host.apply(&emitter, event, cx);
                }
                host.broadcast(HostEvent::FileSaved {
                    path: "/fence".into(),
                });
            })
        });

        let heard = |id: &str, cx: &mut VisualTestContext| {
            let log = log_path(tmp.path(), id);
            let lines = logged(&log, id, cx, |l| !bodies(l, "event/file_saved").is_empty());
            bodies(&lines, "event/ext")
        };
        let want: Vec<_> = (0..20)
            .map(|n| json!({"source": "emitter", "name": "pr.review", "data": {"n": n}}))
            .collect();
        assert_eq!(heard("listener", cx), want);
        assert!(heard("deaf", cx).is_empty());
        assert!(heard("emitter", cx).is_empty());
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
        let terminal = workspace.read_with(cx, |w, _| w.terminal().cloned().expect("the terminal"));

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
    /// (once the user is in it; R61: it prepares beside the focused pane) it opens a draft, and the
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
            assert_eq!(w.panes[1].draft(), w.drafts.first());
            assert!(w.panes[1].draft().is_some());
        });
        std::fs::write(&gate, "").unwrap();

        let theirs = reply(&log, 1, cx)["result"].clone();
        assert_eq!(theirs["agent"], "slow");
        workspace.read_with(cx, |w, cx| {
            assert_eq!(w.sessions.len(), 1, "the extension's session opens too");
            assert_eq!(w.panes.len(), 2);
            assert!(matches!(w.panes[0], Pane::Empty(_)));
            assert!(w.panes[1].draft().is_some(), "the draft keeps its pane");
            assert_eq!(w.sessions[0].read(cx).draft(cx), "theirs");
        });
    }

    /// Extension `cards`: panel `main` in the main area with a known icon, panel `side` in the
    /// sidebar with an unknown one.
    const PANELS: &str = "id = \"cards\"\nname = \"Cards\"\nversion = \"1.0.0\"\n\
        permissions = [\"process\", \"panels\"]\n[process]\ncommand = \"bin/run\"\n\
        [[contributes.panels]]\nid = \"main\"\ntitle = \"My cards\"\nlocation = \"main\"\n\
        icon = \"git-pull-request\"\n\
        [[contributes.panels]]\nid = \"side\"\ntitle = \"Side\"\nicon = \"no-such-icon\"\n";

    /// Extension `bars`: item `prs` in the bottom bar, `ci` in the top bar.
    const ITEMS: &str = "id = \"bars\"\nname = \"Bars\"\nversion = \"1.0.0\"\n\
        permissions = [\"process\", \"bar\"]\n[process]\ncommand = \"bin/run\"\n\
        [[contributes.items]]\nid = \"prs\"\nbar = \"bottom\"\ntitle = \"PRs\"\n\
        [[contributes.items]]\nid = \"ci\"\nbar = \"top\"\ntitle = \"CI\"\n";

    const PRS_UPDATE: &str =
        r#"--call 'item/update {"item": "prs", "text": "3 PRs", "tone": "warning"}'"#;

    fn bar_items(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Vec<super::BarItem> {
        workspace.read_with(cx, |w, cx| w.ext_host.read(cx).items())
    }

    /// An item shows what its process last said; the unset one is listed without a state; a
    /// disabled extension lists none.
    #[gpui_kit::test]
    fn items_follow_the_process(cx: &mut TestAppContext) {
        use wai_ext::Bar;
        use wai_ext::panel::{ItemState, Tone};

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, ITEMS, PRS_UPDATE, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        wait_until(cx, "the item", |cx| {
            bar_items(&workspace, cx)[0].state.is_some()
        });
        let ext = parse_manifest(ITEMS, tmp.path()).unwrap().id;
        let item = |key: &str, id: &str, bar, title: &str, state| super::BarItem {
            key: key.into(),
            ext: ext.clone(),
            ext_name: "Bars".into(),
            id: id.into(),
            bar,
            title: title.into(),
            state,
        };
        let shown = ItemState {
            text: Some("3 PRs".into()),
            tone: Tone::Warning,
            ..Default::default()
        };
        assert_eq!(
            bar_items(&workspace, cx),
            [
                item("bars#prs", "prs", Bar::Bottom, "PRs", Some(shown)),
                item("bars#ci", "ci", Bar::Top, "CI", None),
            ]
        );

        disable_first(&workspace, cx);
        assert_eq!(bar_items(&workspace, cx), []);
    }

    /// Review focus 3: a restarted extension's items stay empty until it sends them again.
    #[gpui_kit::test]
    fn restart_starts_items_empty(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, ITEMS, PRS_UPDATE, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        wait_until(cx, "the item", |cx| {
            bar_items(&workspace, cx)[0].state.is_some()
        });
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let other = tmp.path().join("other");
        std::fs::create_dir_all(&other).unwrap();
        let after = cx.update(|_, cx| {
            host.update(cx, |h, cx| {
                h.set_project(other, cx);
                h.items()
            })
        });
        assert_eq!(after[0].state, None);
    }

    /// A click on an item reaches its extension as `item/action`.
    #[gpui_kit::test]
    fn item_click_reaches_the_extension(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, ITEMS, "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let ext = parse_manifest(ITEMS, tmp.path()).unwrap().id;
        wait_until(cx, "the extension", |cx| {
            host.read_with(cx, |h, _| h.status(&ext) == Some(&super::ExtStatus::Ready))
        });
        host.read_with(cx, |h, _| h.item_action(&ext, "prs"));
        let lines = logged(&log_path(tmp.path(), "bars"), "item/action", cx, |l| {
            l.iter().any(|l| l["title"] == "item/action")
        });
        assert_eq!(bodies(&lines, "item/action"), [json!({"item": "prs"})]);
    }

    /// Extension `id` with three top-bar items `a`, `b` and `c`.
    fn top_items(id: &str, name: &str) -> String {
        let items: String = ["a", "b", "c"]
            .iter()
            .map(|i| {
                format!("[[contributes.items]]\nid = \"{i}\"\nbar = \"top\"\ntitle = \"{i}\"\n")
            })
            .collect();
        format!(
            "id = \"{id}\"\nname = \"{name}\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"bar\"]\n[process]\ncommand = \"bin/run\"\n{items}"
        )
    }

    /// Only the first four top items are drawn, and a narrow window still shows every button.
    #[gpui_kit::test]
    fn top_bar_fits_four_extension_items(cx: &mut TestAppContext) {
        use crate::sidebar::tests::label;
        use gpui_kit::{px, size};

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let calls: String = ["a", "b", "c"]
            .iter()
            .map(|i| {
                format!(
                    r#"--call 'item/update {{"item": "{i}", "text": "a fairly long item text"}}' "#
                )
            })
            .collect();
        install(tmp.path(), &config, &top_items("bars", "Bars"), &calls, "");
        install(
            tmp.path(),
            &config,
            &top_items("bars2", "Bars 2"),
            &calls,
            "",
        );
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        workspace.update(cx, |w, cx| {
            w.project = std::env::temp_dir().join("x".repeat(60));
            cx.notify();
        });
        cx.simulate_resize(size(px(900.), px(700.)));
        wait_until(cx, "four items", |cx| {
            label(("topbar-item", 3usize), cx).is_some()
        });
        assert!(label(("topbar-item", 4usize), cx).is_none());
        let status = cx.debug_bounds("topbar-status").expect("status drawn");
        let actions = cx.debug_bounds("topbar-actions").expect("actions drawn");
        assert!(actions.right() <= px(900.), "{actions:?}");
        assert!(status.right() < actions.left(), "{status:?} {actions:?}");
    }

    /// A click reaches the extension; hiding the item in the bar's menu removes it, and a
    /// relaunch keeps it hidden.
    #[gpui_kit::test]
    fn extension_item_click_and_hide(cx: &mut TestAppContext, relaunch: &mut TestAppContext) {
        use crate::sidebar::tests::label;
        use crate::workspace::tests::{click, launch, menu_item, open_at};
        use wai_settings::AppState;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, ITEMS, PRS_UPDATE, "");
        let path = tmp.path().join("state.json");

        cx.executor().allow_parking();
        let (workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));
        workspace.update(cx, |w, cx| {
            w.user_dir = config.clone();
            w.start_extensions(cx);
        });
        wait_until(cx, "the item", |cx| {
            label(("bottombar-item", 0usize), cx).is_some()
        });
        click(("bottombar-item", 0usize), cx);
        let log = log_path(tmp.path(), "bars");
        let lines = logged(&log, "item/action", cx, |l| {
            l.iter().any(|l| l["title"] == "item/action")
        });
        assert_eq!(bodies(&lines, "item/action"), [json!({"item": "prs"})]);

        menu_item("bottombar", "PRs", cx);
        assert!(label(("bottombar-item", 0usize), cx).is_none());
        assert!(AppState::load(&path).hidden.contains("bars#prs"));

        relaunch.executor().allow_parking();
        let (workspace, cx) = open_at(relaunch, fake_settings(""), Some(path.clone()));
        launch(&workspace, cx);
        workspace.update(cx, |w, cx| {
            w.user_dir = config.clone();
            w.start_extensions(cx);
        });
        wait_until(cx, "the item's text", |cx| {
            bar_items(&workspace, cx)[0].state.is_some()
        });
        assert!(label(("bottombar-item", 0usize), cx).is_none());
    }

    const TEXT: &str = r#"{"type": "text", "text": "Current session"}"#;

    /// An `item/update` for `item` with a card of `n` text lines, as a fake-extension call.
    fn card_call(flag: &str, item: &str, n: usize) -> String {
        let lines = vec![TEXT; n].join(", ");
        let card = format!(r#"{{"type": "column", "children": [{lines}]}}"#);
        let update = format!(r#"{{"item": "{item}", "text": "x", "card": {card}}}"#);
        format!("{flag} 'item/update {update}'")
    }

    /// The card's bounds once it is drawn, hovering `item` first; `None` if it is not drawn.
    fn hover_card_bounds(
        item: (&'static str, usize),
        cx: &mut VisualTestContext,
    ) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
        cx.update(|window, cx| window.hover(item, cx));
        cx.executor().advance_clock(Duration::from_millis(700));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.draw(cx).clear(cx);
            window.draw(cx).clear(cx);
        });
        cx.debug_bounds("item-card")
    }

    fn item_bounds(
        item: (&'static str, usize),
        cx: &mut VisualTestContext,
    ) -> gpui_kit::Bounds<gpui_kit::Pixels> {
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.find(item).bounds()
        })
    }

    /// A bottom item's card opens above it with the extension's name last; an update with a
    /// different card redraws it, and one without removes it, each with the pointer never
    /// leaving the item.
    #[gpui_kit::test]
    fn extension_item_hover_card(cx: &mut TestAppContext) {
        use crate::workspace::tests::click;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let flags = [
            card_call("--call", "prs", 1),
            card_call("--on-action", "prs", 3),
            r#"--on-action 'item/update {"item": "prs", "text": "x"}'"#.to_string(),
        ]
        .join(" ");
        install(tmp.path(), &config, ITEMS, &flags, "");
        let (_workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let item = ("bottombar-item", 0usize);
        wait_until(cx, "the item", |cx| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find(item).is_some()
            })
        });

        let card = hover_card_bounds(item, cx).expect("the card is drawn");
        assert!(card.bottom() <= item_bounds(item, cx).top(), "opens above");
        let footer = cx
            .debug_bounds("item-card-footer")
            .expect("the extension's name");
        assert!(footer.bottom() <= card.bottom() && footer.top() > card.top());

        // The pointer stays: a second card, taller, replaces the open one.
        click(item, cx);
        wait_until(cx, "the redrawn card", |cx| {
            cx.update(|window, cx| window.render_frame(cx));
            cx.debug_bounds("item-card")
                .is_some_and(|c| c.size.height > card.size.height)
        });
        assert!(cx.debug_bounds("item-card-footer").is_some());

        // An update without a card removes the open one.
        click(item, cx);
        wait_until(cx, "the card removed", |cx| {
            cx.update(|window, cx| window.render_frame(cx));
            cx.debug_bounds("item-card").is_none()
        });
    }

    /// A top item's card opens below it, and stopping the extension removes both.
    #[gpui_kit::test]
    fn extension_top_item_hover_card_and_stop(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(
            tmp.path(),
            &config,
            ITEMS,
            &card_call("--call", "ci", 1),
            "",
        );
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let item = ("topbar-item", 0usize);
        wait_until(cx, "the top item", |cx| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find(item).is_some()
            })
        });
        let card = hover_card_bounds(item, cx).expect("the card is drawn");
        assert!(card.top() >= item_bounds(item, cx).bottom(), "opens below");

        let id = parse_manifest(ITEMS, tmp.path()).unwrap().id;
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        cx.update(|_, cx| host.update(cx, |h, cx| h.stop(&id, cx)));
        cx.run_until_parked();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.render_frame(cx);
        });
        assert!(cx.debug_bounds("item-card").is_none());
        assert!(cx.update(|window, _| { window.try_find(item).is_none() }));
    }

    /// Extension `id` with three bottom items `a`, `b` and `c`.
    fn bottom_items(id: &str, name: &str) -> String {
        top_items(id, name).replace("bar = \"top\"", "bar = \"bottom\"")
    }

    /// Several long bottom items shrink in a narrow window: the version stays in view.
    #[gpui_kit::test]
    fn bottom_bar_fits_narrow_window(cx: &mut TestAppContext) {
        use crate::sidebar::tests::label;
        use gpui_kit::{px, size};

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let text = "x".repeat(32);
        let calls: String = ["a", "b", "c"]
            .iter()
            .map(|i| format!(r#"--call 'item/update {{"item": "{i}", "text": "{text}"}}' "#))
            .collect();
        for (id, name) in [("bars", "Bars"), ("bars2", "Bars 2"), ("bars3", "Bars 3")] {
            install(tmp.path(), &config, &bottom_items(id, name), &calls, "");
        }
        let (_workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        cx.simulate_resize(size(px(900.), px(700.)));
        wait_until(cx, "the items", |cx| {
            label(("bottombar-item", 2usize), cx).is_some()
        });
        let version = cx.update(|window, _| window.find("bottombar-version").bounds());
        assert!(version.right() <= px(900.), "{version:?}");
    }

    /// API 1.6: a `prompt` panel shows inside the focused pane's prompt box and a `prompt-side`
    /// one beside it, at their widths; neither gets a rail button. The extension hears which
    /// session has the focus. Off the focused pane, or in a pane too narrow, they hide.
    #[gpui_kit::test]
    fn prompt_panels_follow_the_focused_box(cx: &mut TestAppContext) {
        use gpui_kit::{Bounds, Pixels, px, size};

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"pet\"\nname = \"Pet\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"panels\", \"sessions:read\"]\n\
            [process]\ncommand = \"bin/run\"\n\
            [[contributes.panels]]\nid = \"in\"\ntitle = \"In\"\nlocation = \"prompt\"\n\
            width = 150\n\
            [[contributes.panels]]\nid = \"out\"\ntitle = \"Out\"\nlocation = \"prompt-side\"\n";
        install(tmp.path(), &config, manifest, "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        cx.simulate_resize(size(px(1400.), px(800.)));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panels", |cx| {
            host.read_with(cx, |h, _| h.panels().len() == 2)
        });
        start_now(&workspace, cx);
        let find = |cx: &mut VisualTestContext| -> [Option<Bounds<Pixels>>; 3] {
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.render_frame(cx);
                ["prompt-box", "prompt-slot", "prompt-side"]
                    .map(|id| window.try_find(id).map(|e| e.bounds()))
            })
        };
        let [field, inside, side] = find(cx);
        let (field, inside, side) = (field.unwrap(), inside.unwrap(), side.unwrap());
        assert!((inside.size.width - px(150.)).abs() < px(0.5), "{inside:?}");
        assert!(inside.left() > field.left() && inside.right() <= field.right() + px(0.5));
        assert!((side.size.width - px(200.)).abs() < px(0.5), "{side:?}");
        assert!(side.left() >= field.right(), "{side:?} {field:?}");
        assert!(crate::sidebar::tests::label(("tab-panel", 0usize), cx).is_none());
        let log = log_path(tmp.path(), "pet");
        let focused = json!({"agent": "fake", "session": "fake-1"});
        logged(&log, "the focused session", cx, |l| {
            bodies(l, "event/session_focused").contains(&focused)
        });

        // Another pane takes the focus, and the slots.
        workspace.update(cx, |w, cx| {
            w.panes.push(Pane::Empty(None));
            w.focused_pane = 1;
            cx.notify();
        });
        assert_eq!(find(cx), [field_of(cx), None, None]);
        let none = json!({"agent": null, "session": null});
        logged(&log, "no focused session", cx, |l| {
            bodies(l, "event/session_focused").last() == Some(&none)
        });

        // Alone again, in a narrow window: no room for them.
        workspace.update(cx, |w, cx| {
            w.panes.pop();
            w.focused_pane = 0;
            cx.notify();
        });
        assert!(find(cx)[1].is_some());
        cx.simulate_resize(size(px(700.), px(800.)));
        let [field, inside, side] = find(cx);
        assert!(field.is_some() && inside.is_none() && side.is_none());
    }

    /// API 1.7: a frameless `prompt-side` panel floats. Anchored to the prompt box's bottom, a
    /// taller tree grows up over what is above: the draft's prompt box does not move.
    #[gpui_kit::test]
    fn frameless_side_panel_moves_nothing(cx: &mut TestAppContext) {
        use gpui_kit::{px, size};
        use wai_ext::PanelLocation;
        use wai_ext::panel::parse;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"pet\"\nname = \"Pet\"\nversion = \"1.0.0\"\napi = \"1.7\"\n\
            permissions = [\"process\", \"panels\"]\n\
            [process]\ncommand = \"bin/run\"\n\
            [[contributes.panels]]\nid = \"out\"\ntitle = \"Out\"\nlocation = \"prompt-side\"\n\
            frame = false\n";
        install(tmp.path(), &config, manifest, "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        cx.simulate_resize(size(px(1400.), px(800.)));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panel", |cx| {
            host.read_with(cx, |h, _| h.panel_at(PanelLocation::PromptSide).is_some())
        });
        cx.simulate_keystrokes("secondary-n");
        let slot = host.read_with(cx, |h, _| {
            h.panel_at(PanelLocation::PromptSide)
                .map(|p| p.view.clone())
                .unwrap()
        });
        let draw = |lines: usize, cx: &mut VisualTestContext| {
            let text = vec!["x"; lines].join("\n");
            let tree = parse(&json!({"type": "text", "text": text}));
            slot.update(cx, |p, cx| p.set_tree(tree, cx));
            cx.run_until_parked();
            cx.update(|window, cx| {
                window.render_frame(cx);
                ["draft-composer", "prompt-side"].map(|id| window.find(id).bounds())
            })
        };
        let [field, _] = draw(1, cx);
        let [moved, side] = draw(40, cx);
        assert_eq!(moved, field);
        assert!(side.size.height > field.size.height * 2., "{side:?}");
        assert!(
            (side.bottom() - field.bottom()).abs() < px(0.5),
            "{side:?} {field:?}"
        );
    }

    /// API 1.9: a `sidebar-foot` panel sits under every sidebar tab, with no rail button. Its
    /// height is its content's; a bubble floats above it.
    #[gpui_kit::test]
    fn sidebar_foot_shows_on_every_tab(cx: &mut TestAppContext) {
        use gpui_kit::{ElementId, px, size};
        use wai_ext::PanelLocation;
        use wai_ext::panel::parse;

        use crate::sidebar::SidebarTab;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"pet\"\nname = \"Pet\"\nversion = \"1.0.0\"\napi = \"1.9\"\n\
            permissions = [\"process\", \"panels\"]\n\
            [process]\ncommand = \"bin/run\"\n\
            [[contributes.panels]]\nid = \"foot\"\ntitle = \"Foot\"\nlocation = \"sidebar-foot\"\n";
        install(tmp.path(), &config, manifest, "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        cx.simulate_resize(size(px(1400.), px(800.)));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panel", |cx| {
            host.read_with(cx, |h, _| h.panel_at(PanelLocation::SidebarFoot).is_some())
        });
        let slot = host.read_with(cx, |h, _| {
            h.panel_at(PanelLocation::SidebarFoot)
                .map(|p| p.view.clone())
                .unwrap()
        });
        let draw = |tree: serde_json::Value, cx: &mut VisualTestContext| {
            slot.update(cx, |p, cx| p.set_tree(parse(&tree), cx));
            cx.run_until_parked();
            cx.update(|window, cx| window.render_frame(cx));
        };
        let foot = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| window.render_frame(cx));
            cx.debug_bounds("sidebar-foot")
        };

        draw(json!({"type": "text", "text": "Pipoca"}), cx);
        workspace.update(cx, |w, cx| {
            w.sidebar_tab = SidebarTab::Sessions;
            cx.notify();
        });
        assert!(foot(cx).is_some());
        workspace.update(cx, |w, cx| {
            w.sidebar_tab = SidebarTab::History;
            cx.notify();
        });
        draw(json!({"type": "text", "text": "Pipoca"}), cx);
        assert!(foot(cx).is_some());
        assert!(crate::sidebar::tests::label(("tab-panel", 0usize), cx).is_none());

        let text = json!({"type": "text", "text": "Pipoca"});
        draw(json!({"type": "column", "children": [text.clone()]}), cx);
        let plain = foot(cx).unwrap();
        draw(
            json!({"type": "column", "children": [
                {"type": "bubble", "id": "say", "text": "Oi"},
                text,
            ]}),
            cx,
        );
        let with = foot(cx).unwrap();
        assert_eq!(with.size.height, plain.size.height);
        let bubble = cx.update(|window, _| {
            window
                .find(ElementId::Name("panel-bubble:say".into()))
                .bounds()
        });
        assert!(bubble.top() < with.top(), "{bubble:?} {with:?}");

        draw(
            json!({"type": "column", "children": [
                {"type": "menu", "items": [{"id": "a", "label": "A"}]},
            ]}),
            cx,
        );
        let menu = ("ext-foot-menu", 2usize);
        let shown = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find(menu).is_some_and(|m| m.visible())
            })
        };
        assert!(!shown(cx), "hidden until hovered");
        let over = foot(cx).unwrap().origin + gpui_kit::point(px(200.), px(2.));
        cx.simulate_mouse_move(over, None, Default::default());
        assert!(shown(cx), "shown on hover");
        cx.simulate_mouse_move(gpui_kit::point(px(0.), px(0.)), None, Default::default());
        assert!(!shown(cx), "hidden again");

        workspace.update(cx, |w, cx| {
            w.sidebar_hidden = true;
            cx.notify();
        });
        assert!(foot(cx).is_none());
    }

    /// A foot taller than half the sidebar is capped at half, so the tabs keep their room.
    #[gpui_kit::test]
    fn tall_sidebar_foot_is_capped(cx: &mut TestAppContext) {
        use gpui_kit::{px, size};
        use wai_ext::PanelLocation;
        use wai_ext::panel::parse;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"pet\"\nname = \"Pet\"\nversion = \"1.0.0\"\napi = \"1.9\"\n\
            permissions = [\"process\", \"panels\"]\n\
            [process]\ncommand = \"bin/run\"\n\
            [[contributes.panels]]\nid = \"foot\"\ntitle = \"Foot\"\nlocation = \"sidebar-foot\"\n";
        install(tmp.path(), &config, manifest, "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        cx.simulate_resize(size(px(1400.), px(800.)));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panel", |cx| {
            host.read_with(cx, |h, _| h.panel_at(PanelLocation::SidebarFoot).is_some())
        });
        let slot = host.read_with(cx, |h, _| {
            h.panel_at(PanelLocation::SidebarFoot)
                .map(|p| p.view.clone())
                .unwrap()
        });
        let row = "#".repeat(64);
        let tree = json!({"type": "pixels", "rows": vec![row; 64], "scale": 8});
        slot.update(cx, |p, cx| p.set_tree(parse(&tree), cx));
        cx.run_until_parked();
        cx.update(|window, cx| window.render_frame(cx));
        let sidebar = cx.debug_bounds("sidebar").unwrap();
        let foot = cx.debug_bounds("sidebar-foot").unwrap();
        let body = cx.debug_bounds("sidebar-body").unwrap();
        assert!(foot.size.height <= sidebar.size.height / 2. + px(0.5));
        assert!(foot.size.height > px(100.), "{foot:?}");
        assert!(body.size.height > px(0.), "{body:?}");
    }

    /// The real wai-bichinho, built with `go`, walks its pixel pet in the sidebar's foot.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn bichinho_walks_in_the_sidebar_foot(cx: &mut TestAppContext) {
        use gpui_kit::{px, size};
        use wai_ext::PanelLocation;
        use wai_ext::panel::Node;

        let tmp = tempfile::tempdir().unwrap();
        let from = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../extensions/wai-bichinho");
        let src = tmp.path().join("src");
        std::fs::create_dir_all(&src).unwrap();
        for entry in std::fs::read_dir(&from).unwrap().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let go = name.ends_with(".go") && !name.ends_with("_test.go");
            if go || name == MANIFEST_FILE || name == "go.mod" {
                std::fs::copy(entry.path(), src.join(&name)).unwrap();
            }
        }
        let built = std::process::Command::new("go")
            .args(["build", "-o", "bin/wai-bichinho", "."])
            .current_dir(&src)
            .status();
        if !built.is_ok_and(|s| s.success()) {
            eprintln!("SKIPPED: `go build` is unavailable, so wai-bichinho cannot be built");
            return;
        }
        let config = tmp.path().join("config");
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        let granted = staged.manifest().permissions.clone();
        store.commit(staged, granted).unwrap();

        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        cx.simulate_resize(size(px(1400.), px(800.)));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panels", |cx| {
            host.read_with(cx, |h, _| h.panels().len() == 2)
        });
        start_now(&workspace, cx);
        let slot = host.read_with(cx, |h, _| {
            h.panel_at(PanelLocation::SidebarFoot)
                .map(|p| p.view.clone())
        });
        let slot = slot.expect("the pet's slot");
        let pixels = |cx: &mut VisualTestContext| {
            let mut rows = 0;
            if let Some(Ok(tree)) = slot.read_with(cx, |p, _| p.tree.clone()) {
                tree.walk(&mut |node| {
                    if let Node::Pixels { rows: r, .. } = node {
                        rows = r.len();
                    }
                });
            }
            rows
        };
        wait_until(cx, "the pet's pixels", |cx| pixels(cx) > 0);
        cx.update(|window, cx| {
            window.render_frame(cx);
        });
        assert!(cx.debug_bounds("sidebar-foot").is_some());
        // The sidebar shows the card's button only.
        assert!(crate::sidebar::tests::label(("tab-panel", 0usize), cx).is_some());
        assert!(crate::sidebar::tests::label(("tab-panel", 1usize), cx).is_none());
    }

    fn field_of(cx: &mut VisualTestContext) -> Option<gpui_kit::Bounds<gpui_kit::Pixels>> {
        cx.update(|window, _| window.try_find("prompt-box").map(|e| e.bounds()))
    }

    /// A panel and an item with the same id hide apart.
    #[gpui_kit::test]
    fn item_and_panel_with_one_id_hide_apart(cx: &mut TestAppContext) {
        use crate::sidebar::tests::label;
        use crate::workspace::tests::menu_item;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"bars\"\nname = \"Bars\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"panels\", \"bar\"]\n[process]\ncommand = \"bin/run\"\n\
            [[contributes.panels]]\nid = \"x\"\ntitle = \"X panel\"\n\
            [[contributes.items]]\nid = \"x\"\nbar = \"bottom\"\ntitle = \"X item\"\n";
        let call = r#"--call 'item/update {"item": "x", "text": "hi"}'"#;
        install(tmp.path(), &config, manifest, call, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        wait_until(cx, "the item", |cx| {
            label(("bottombar-item", 0usize), cx).is_some()
        });
        assert!(label(("tab-panel", 0usize), cx).is_some());
        menu_item("bottombar", "X item", cx);
        assert!(label(("bottombar-item", 0usize), cx).is_none());
        assert!(label(("tab-panel", 0usize), cx).is_some());
        let hidden = workspace.read_with(cx, |w, _| w.hidden.clone());
        assert!(hidden.contains("bars#x") && !hidden.contains("bars/x"));
    }

    /// Extension `id` with a sidebar panel `p` and a bottom item `i`.
    fn panel_and_item(id: &str) -> String {
        format!(
            "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"panels\", \"bar\"]\n[process]\ncommand = \"bin/run\"\n\
            [[contributes.panels]]\nid = \"p\"\ntitle = \"P\"\n\
            [[contributes.items]]\nid = \"i\"\nbar = \"bottom\"\ntitle = \"I\"\n"
        )
    }

    /// An extension's items sit in the order of its panel's icon in the rail.
    #[gpui_kit::test]
    fn bar_items_follow_the_rail(cx: &mut TestAppContext) {
        use wai_ext::Bar;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, &panel_and_item("alpha"), "", "");
        install(tmp.path(), &config, &panel_and_item("beta"), "", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panels", |cx| {
            host.read_with(cx, |h, _| h.panels().len() == 2)
        });
        let keys = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, cx| {
                (w.bar_items(Bar::Bottom, cx).into_iter())
                    .map(|i| i.key)
                    .collect::<Vec<_>>()
            })
        };
        let rail = workspace.read_with(cx, |w, cx| w.rail_keys(cx));
        let first = rail.iter().find(|k| k.contains('/')).unwrap().clone();
        let second = rail.iter().rfind(|k| k.contains('/')).unwrap().clone();
        let item_of = |panel: &str| format!("{}#i", panel.split('/').next().unwrap());
        assert_eq!(keys(cx), [item_of(&first), item_of(&second)]);

        workspace.update(cx, |w, cx| w.move_rail(&second, &first, cx));
        assert_eq!(keys(cx), [item_of(&second), item_of(&first)]);
    }

    /// A crashed extension is listed with its name; the live one is not.
    #[gpui_kit::test]
    fn stopped_lists_crashed_extensions(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, PANELS, "--crash", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the crash", |cx| {
            host.read_with(cx, |h, _| h.stopped().len() == 1)
        });
        assert_eq!(host.read_with(cx, |h, _| h.stopped()[0].0.clone()), "Cards");
    }

    /// A crashed extension shows in the bottom bar; a click opens the Extensions view.
    #[gpui_kit::test]
    fn stopped_extension_shows_in_the_bottom_bar(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install(tmp.path(), &config, PANELS, "--crash", "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        wait_until(cx, "the stopped item", |cx| {
            crate::sidebar::tests::label("bottombar-extensions", cx).as_deref()
                == Some("Cards stopped")
        });
        crate::workspace::tests::click("bottombar-extensions", cx);
        let view = workspace.read_with(cx, |w, _| w.settings_view().expect("settings open"));
        let section = view.read_with(cx, |v, _| v.section());
        assert_eq!(section, crate::settings_view::Section::Extensions);
    }

    /// The panes that show `view`.
    fn panes_of(
        workspace: &Entity<Workspace>,
        view: &Entity<crate::ext_panel::ExtPanel>,
        cx: &mut VisualTestContext,
    ) -> usize {
        workspace.read_with(cx, |w, _| {
            (w.panes.iter())
                .filter(|p| matches!(p, Pane::Extension(v) if v == view))
                .count()
        })
    }

    /// `PANELS` installed with `flags` and started; its main panel's view once both panels show.
    fn start_panels<'a>(
        tmp: &Path,
        flags: &str,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<crate::ext_panel::ExtPanel>,
        &'a mut VisualTestContext,
    ) {
        start_manifest(tmp, PANELS, flags, cx)
    }

    fn start_manifest<'a>(
        tmp: &Path,
        manifest: &str,
        flags: &str,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<crate::ext_panel::ExtPanel>,
        &'a mut VisualTestContext,
    ) {
        let config = tmp.join("config");
        install(tmp, &config, manifest, flags, "");
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panels", |cx| {
            host.read_with(cx, |h, _| h.panels().len() == 2)
        });
        let view = host.read_with(cx, |h, _| h.panels()[0].view.clone());
        (workspace, view, cx)
    }

    #[test]
    fn panel_icon_resolves() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let svg = "<svg viewBox=\"0 0 24 24\"><path d=\"M3 3h18v18H3z\"/></svg>";
        std::fs::write(dir.join("x.svg"), svg).unwrap();
        std::fs::create_dir(dir.join("ext")).unwrap();
        let ext = dir.join("ext");
        std::fs::write(ext.join("icon.svg"), svg).unwrap();
        std::fs::write(ext.join("big.svg"), vec![b' '; 33 * 1024]).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.join("x.svg"), ext.join("link.svg")).unwrap();
        let spec = |icon: &str| {
            let text = format!(
                "id = \"a\"\nname = \"A\"\nversion = \"1.0.0\"\npermissions = [\"panels\"]\n\
                 [[contributes.panels]]\nid = \"p\"\ntitle = \"P\"\nicon = \"{icon}\"\n"
            );
            parse_manifest(&text, &ext)
                .unwrap()
                .contributes
                .panels
                .remove(0)
        };
        let got = |icon: &str| panel_icon(&spec(icon), &ext);
        assert_eq!(
            got("icon.svg"),
            Some(PanelIcon::Svg(
                ext.join("icon.svg").to_string_lossy().into_owned().into()
            ))
        );
        assert!(matches!(got("database"), Some(PanelIcon::Lucide(_))));
        assert_eq!(got("nope"), None);
        assert_eq!(got("missing.svg"), None);
        assert_eq!(got("big.svg"), None);
        assert_eq!(got("../x.svg"), None);
        #[cfg(unix)]
        assert_eq!(got("link.svg"), None);
    }

    /// A main panel with an SVG icon shows no initials in the rail.
    #[gpui_kit::test]
    fn svg_icon_replaces_initials(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = PANELS.replace("git-pull-request", "icon.svg");
        let dir = install(tmp.path(), &config, &manifest, "--panel", "");
        std::fs::write(dir.join("icon.svg"), "<svg viewBox=\"0 0 24 24\"/>").unwrap();
        let (workspace, cx) = workspace_at(&config, fake_settings(""), cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        wait_until(cx, "the panels", |cx| {
            host.read_with(cx, |h, _| h.panels().len() == 2)
        });
        let found = |ix: usize, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.try_find(("panel-initials", ix)).is_some()
            })
        };
        assert!(!found(0, cx), "the SVG panel has no initials");
        assert!(found(1, cx), "the unknown icon keeps them");
    }

    /// A main panel's rail button opens it as a pane, once: a second click focuses that pane.
    /// It is never a sidebar tab. A known icon replaces the initials; an unknown one leaves them.
    #[gpui_kit::test]
    fn main_panel_opens_as_a_pane(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;
        use crate::sidebar::SidebarTab;

        let tmp = tempfile::tempdir().unwrap();
        let (workspace, view, cx) = start_panels(tmp.path(), "--panel", cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let icons = host.read_with(cx, |h, _| {
            (h.panels().iter())
                .map(|p| match &p.icon {
                    Some(PanelIcon::Lucide(i)) => Some(i.path().to_string()),
                    _ => None,
                })
                .collect::<Vec<_>>()
        });
        assert_eq!(icons, [Some("icons/git-pull-request.svg".into()), None]);

        click(("tab-panel", 0usize), cx);
        assert_eq!(panes_of(&workspace, &view, cx), 1);
        assert_eq!(
            workspace.read_with(cx, |w, _| w.sidebar_tab),
            SidebarTab::Sessions
        );
        cx.update(|window, cx| window.render_frame(cx));
        assert!(cx.debug_bounds("extension-pane-0").is_some());

        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.panes.push(Pane::Empty(None));
                w.focus_pane(1, window, cx);
            })
        });
        click(("tab-panel", 0usize), cx);
        assert_eq!(panes_of(&workspace, &view, cx), 1);
        assert_eq!(
            workspace.read_with(cx, |w, _| (w.panes.len(), w.focused_pane)),
            (2, 0)
        );
    }

    /// A wide panel's icon hides the main area while it is shown; its icon again, another tab, the
    /// terminal, a file, a session starting or a main panel opening bring the main area back, on
    /// the Sessions tab.
    #[gpui_kit::test]
    fn wide_panel_takes_the_main_area(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;
        use crate::sidebar::SidebarTab;

        let tmp = tempfile::tempdir().unwrap();
        let wide = PANELS.replace(
            "icon = \"no-such-icon\"\n",
            "icon = \"no-such-icon\"\nwide = true\n",
        );
        let (workspace, view, cx) = start_manifest(tmp.path(), &wide, "--panel", cx);
        let main = |cx: &mut VisualTestContext| {
            cx.run_until_parked();
            cx.update(|window, cx| window.render_frame(cx));
            cx.debug_bounds("pane-0").is_some()
        };
        let tab = |cx: &mut VisualTestContext| workspace.read_with(cx, |w, _| w.sidebar_tab);
        assert!(main(cx));

        click(("tab-panel", 1usize), cx);
        assert!(matches!(tab(cx), SidebarTab::Panel(_)));
        assert!(!main(cx), "a wide panel hides the main area");
        assert!(cx.debug_bounds("sidebar").is_some());
        click(("tab-panel", 1usize), cx);
        assert_eq!(tab(cx), SidebarTab::Sessions);
        assert!(main(cx));

        click(("tab-panel", 1usize), cx);
        click("tab-sessions", cx);
        assert!(main(cx));

        // ⌘B hides the wide panel and shows the main area, without a special case.
        click(("tab-panel", 1usize), cx);
        cx.simulate_keystrokes("secondary-b");
        assert!(main(cx));
        cx.simulate_keystrokes("secondary-b");
        assert!(!main(cx));

        // The terminal and files live in the main area: showing either leaves the wide panel.
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.toggle_terminal(&crate::app::ToggleTerminal, window, cx)
            })
        });
        assert_eq!(tab(cx), SidebarTab::Sessions);
        assert!(main(cx));
        click(("tab-panel", 1usize), cx);
        let file = tmp.path().join("notes.txt");
        std::fs::write(&file, "notes").unwrap();
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.open_file(file, None, window, cx)));
        assert_eq!(tab(cx), SidebarTab::Sessions);
        assert!(main(cx));

        click(("tab-panel", 1usize), cx);
        assert!(!main(cx));
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.start_session("fake".into(), None, false, None, window, cx)
            })
        });
        assert_eq!(tab(cx), SidebarTab::Sessions);
        assert!(main(cx));

        click(("tab-panel", 1usize), cx);
        click(("tab-panel", 0usize), cx);
        assert_eq!(tab(cx), SidebarTab::Sessions);
        assert_eq!(panes_of(&workspace, &view, cx), 1);
    }

    /// Hiding the selected sidebar panel's icon shows the sessions; a main panel's pane stays.
    #[gpui_kit::test]
    fn hiding_a_selected_panel_icon_shows_sessions(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;
        use crate::sidebar::SidebarTab;

        let tmp = tempfile::tempdir().unwrap();
        let (workspace, view, cx) = start_panels(tmp.path(), "--panel", cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let keys = host.read_with(cx, |h, _| {
            h.panels().iter().map(|p| p.key()).collect::<Vec<_>>()
        });
        click(("tab-panel", 1usize), cx);
        assert!(matches!(
            workspace.read_with(cx, |w, _| w.sidebar_tab),
            SidebarTab::Panel(_)
        ));
        workspace.update(cx, |w, cx| w.toggle_hidden(&keys[1], cx));
        assert_eq!(
            workspace.read_with(cx, |w, _| w.sidebar_tab),
            SidebarTab::Sessions
        );

        click(("tab-panel", 0usize), cx);
        assert_eq!(panes_of(&workspace, &view, cx), 1);
        workspace.update(cx, |w, cx| w.toggle_hidden(&keys[0], cx));
        assert_eq!(panes_of(&workspace, &view, cx), 1);
    }

    /// A main panel's × closes its pane and the extension keeps running; disabling the extension
    /// closes the pane, and the focus lands on a live pane.
    #[gpui_kit::test]
    fn main_pane_closes_with_its_panel(cx: &mut TestAppContext) {
        use crate::extensions_view::tests::click;

        let tmp = tempfile::tempdir().unwrap();
        let (workspace, view, cx) = start_panels(tmp.path(), "--panel", cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        click(("tab-panel", 0usize), cx);
        click(("extension-pane-close", 0usize), cx);
        assert_eq!(panes_of(&workspace, &view, cx), 0);
        assert_eq!(host.read_with(cx, |h, _| h.panels().len()), 2);

        click(("tab-panel", 0usize), cx);
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.panes.insert(0, Pane::Empty(None));
                w.panes.push(Pane::Empty(None));
                w.focus_pane(1, window, cx);
            })
        });
        let mut list = ExtStore::open(&tmp.path().join("config")).list();
        list[0].enabled = false;
        host.update(cx, |h, cx| h.sync(&list, cx));
        cx.run_until_parked();
        assert_eq!(panes_of(&workspace, &view, cx), 0);
        workspace.read_with(cx, |w, _| {
            assert_eq!(w.panes.len(), 2);
            assert!(w.focused_pane < 2);
        });
    }

    /// API 1.11 `sessions/show` shows an open session in the focused pane, like a click in the
    /// open list; it shares `panel/open`'s one per 2 s.
    #[gpui_kit::test]
    fn sessions_show_shows_and_rate_limits(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"shower\"\nname = \"Shower\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\", \"sessions:control\"]\n[process]\ncommand = \"bin/run\"\n";
        let show = r#"'sessions/show {"agent": "fake", "session": "fake-1"}'"#;
        install(
            tmp.path(),
            &config,
            manifest,
            &format!("--call {show} --call {show}"),
            "",
        );
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.executor().allow_parking();
        let session = start_now(&workspace, cx);
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.close_pane(0, window, cx)));
        workspace.read_with(cx, |w, _| {
            assert!(w.panes.iter().all(|p| p.session().is_none()))
        });
        workspace.update(cx, |w, cx| {
            w.user_dir = config.clone();
            w.start_extensions(cx);
        });
        let log = log_path(tmp.path(), "shower");

        let first = reply(&log, 1, cx);
        assert!(
            first["error"].is_null() && first["result"].is_null(),
            "{first}"
        );
        workspace.read_with(cx, |w, _| {
            let ix = w.panes.iter().position(|p| p.session() == Some(&session));
            assert_eq!(ix, Some(w.focused_pane));
        });
        assert_eq!(
            reply(&log, 2, cx)["error"]["message"],
            "Too many panel opens; try again later"
        );
    }

    /// `sessions/show` of a session that is not open is an error; without `sessions:control` it
    /// is denied.
    #[gpui_kit::test]
    fn sessions_show_refuses(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = |id: &str, permissions: &str| {
            format!(
                "id = \"{id}\"\nname = \"{id}\"\nversion = \"1.0.0\"\n\
                 permissions = [{permissions}]\n[process]\ncommand = \"bin/run\"\n"
            )
        };
        let call = r#"--call 'sessions/show {"agent": "fake", "session": "ghost"}'"#;
        let lost = manifest("lost", "\"process\", \"sessions:control\"");
        install(tmp.path(), &config, &lost, call, "");
        let blind = manifest("blind", "\"process\"");
        install(tmp.path(), &config, &blind, call, "");
        let (_workspace, cx) = workspace_at(&config, fake_settings(""), cx);

        let lost = reply(&log_path(tmp.path(), "lost"), 1, cx);
        assert_eq!(lost["error"]["message"], "No session fake ghost");
        let blind = reply(&log_path(tmp.path(), "blind"), 1, cx);
        assert_eq!(
            blind["error"],
            json!({"code": -32001, "message": "Permission denied: sessions:control"})
        );
    }

    /// `panel/open` shows a main panel beside the editor without taking its keyboard; a second
    /// one within 2 s is refused and shows nothing.
    #[gpui_kit::test]
    fn panel_open_shows_and_rate_limits(cx: &mut TestAppContext) {
        use crate::sidebar::SidebarTab;

        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let file = tmp.path().join("notes.md");
        std::fs::write(&file, "notes\n").unwrap();
        let calls =
            r#"--call 'panel/open {"panel": "main"}' --call 'panel/open {"panel": "side"}'"#;
        install(tmp.path(), &config, PANELS, calls, "");
        let (workspace, cx) = start_after(&config, fake_settings(""), cx, |workspace, cx| {
            let path = file.clone();
            cx.update(|window, cx| {
                workspace.update(cx, |w, cx| w.open_file(path, None, window, cx))
            });
            cx.run_until_parked();
        });
        let log = log_path(tmp.path(), "cards");

        assert_eq!(reply(&log, 1, cx)["result"], Value::Null);
        assert_eq!(
            reply(&log, 2, cx)["error"]["message"],
            "Too many panel opens; try again later"
        );
        cx.run_until_parked();
        workspace.read_with(cx, |w, _| {
            assert!(matches!(w.panes[..], [Pane::Editor(_), Pane::Extension(_)]));
            assert_eq!(w.focused_pane, 0);
            assert_eq!(w.sidebar_tab, SidebarTab::Sessions);
        });
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

    /// The workspace re-applies once the started extensions' theme files are known.
    #[gpui_kit::test]
    fn extension_theme_reaches_the_catalog(cx: &mut TestAppContext) {
        use crate::appearance::catalog::ThemeCatalog;
        use gpui_kit::component::ThemeMode;

        const LOOKS: &str = "id = \"looks\"\nname = \"Looks\"\nversion = \"1.0.0\"\n\
            [contributes]\nthemes = [\"t.json\"]\n";
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let dir = install(tmp.path(), &config, LOOKS, "", "");
        std::fs::write(
            dir.join("t.json"),
            r#"{"name":"set","themes":[{"name":"From ext","mode":"dark"}]}"#,
        )
        .unwrap();
        let (_workspace, cx) = start_after(&config, fake_settings(""), cx, |_, _| {});
        cx.run_until_parked();
        let found = cx.update(|_, cx| {
            ThemeCatalog::global(cx)
                .find("From ext", ThemeMode::Dark)
                .is_some()
        });
        assert!(found);
    }

    fn themed(dir: &std::path::Path, enabled: bool) -> wai_ext::Installed {
        let manifest = "id = \"looks\"\nname = \"Looks\"\nversion = \"1.0.0\"\n\
            [contributes]\nthemes = [\"themes/one.json\", \"two.json\"]\n";
        wai_ext::Installed {
            manifest: wai_ext::parse_manifest(manifest, dir).unwrap(),
            dir: dir.into(),
            data: dir.join("data"),
            enabled,
            granted: Vec::new(),
        }
    }

    #[gpui_kit::test]
    fn enabled_extension_lists_its_theme_files(cx: &mut TestAppContext) {
        let dir = std::path::Path::new("/ext/looks");
        let host =
            cx.update(|cx| gpui_kit::AppContext::new(cx, |_| super::ExtHost::new("/p".into())));
        host.update(cx, |h, cx| h.sync(&[themed(dir, true)], cx));
        let files = host.read_with(cx, |h, _| h.theme_files());
        assert_eq!(
            files,
            vec![
                ("/ext/looks".to_string(), dir.join("themes/one.json")),
                ("/ext/looks".to_string(), dir.join("two.json")),
            ]
        );
    }

    #[gpui_kit::test]
    fn disabled_extension_drops_its_themes(cx: &mut TestAppContext) {
        let dir = std::path::Path::new("/ext/looks");
        let host =
            cx.update(|cx| gpui_kit::AppContext::new(cx, |_| super::ExtHost::new("/p".into())));
        host.update(cx, |h, cx| h.sync(&[themed(dir, true)], cx));
        host.update(cx, |h, cx| h.sync(&[themed(dir, false)], cx));
        assert!(host.read_with(cx, |h, _| h.theme_files().is_empty()));
    }

    #[gpui_kit::test]
    fn theme_only_extension_starts_no_process(cx: &mut TestAppContext) {
        let dir = std::path::Path::new("/ext/looks");
        let host =
            cx.update(|cx| gpui_kit::AppContext::new(cx, |_| super::ExtHost::new("/p".into())));
        host.update(cx, |h, cx| h.sync(&[themed(dir, true)], cx));
        host.read_with(cx, |h, _| {
            assert!(h.running.is_empty());
            assert_eq!(h.theme_files().len(), 2);
        });
    }
}

#[cfg(test)]
mod limits {
    use std::time::{Duration, Instant};

    use super::{CALL_GAP, EMIT_LIMIT, EMIT_WINDOW, MAX_BATCH, allow, allow_burst, batch};

    #[test]
    fn notifications_wait_two_seconds() {
        let (mut last, now) = (None, Instant::now());
        assert!(allow(&mut last, now));
        assert!(!allow(&mut last, now + Duration::from_millis(1_999)));
        assert!(allow(&mut last, now + CALL_GAP));
        assert!(!allow(&mut last, now + CALL_GAP + Duration::from_secs(1)));
    }

    #[test]
    fn allow_burst_lets_twenty_through_per_second() {
        let (mut window, t0) = (None, Instant::now());
        for _ in 0..EMIT_LIMIT {
            assert!(allow_burst(&mut window, t0));
        }
        assert!(!allow_burst(&mut window, t0));
        assert!(!allow_burst(&mut window, t0 + Duration::from_millis(999)));
        assert!(allow_burst(&mut window, t0 + EMIT_WINDOW));
        assert_eq!(window, Some((t0 + EMIT_WINDOW, 1)));
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
            data: dir.join("data"),
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
