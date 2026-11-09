//! The enabled extensions' processes, one `ExtHandle` each. They start once the window is open
//! (and on install or enable) and stop on disable, before uninstall and at quit. They inherit
//! wai's cwd, the config dir (R18).
//!
//! Each live one granted `panels` shows its declared panels as sidebar tabs. A panel outlives a
//! restart of its process (a project switch), so what the user typed stays; it goes when its
//! extension stops, fails or crashes.

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::*;
use wai_ext::{ExtEvent, ExtHandle, ExtId, Installed, PanelSpec, Permission};

use crate::ext_panel::{ExtPanel, PanelAction};

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
    _pump: Task<()>,
}

pub struct ExtHost {
    /// Sent in `initialize`.
    project: PathBuf,
    running: HashMap<ExtId, Running>,
    /// The running extensions' panels, in the order of the list `sync` got.
    panels: Vec<Panel>,
}

impl ExtHost {
    pub fn new(project: PathBuf) -> Self {
        Self {
            project,
            running: HashMap::new(),
            panels: Vec::new(),
        }
    }

    /// Runs exactly the enabled extensions of `list` that have a process: starts the missing
    /// ones, stops the rest.
    pub fn sync(&mut self, list: &[Installed], cx: &mut Context<Self>) {
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

    /// Kills its process now, on this thread.
    pub fn stop(&mut self, id: &ExtId, cx: &mut Context<Self>) {
        self.running.remove(id);
        self.panels.retain(|p| p.ext != *id);
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
                    // Everything that piled up is applied in one update.
                    let batch: Vec<_> = std::iter::once(first)
                        .chain(std::iter::from_fn(|| events.try_recv().ok()))
                        .collect();
                    let applied = this.update(cx, |this, cx| {
                        for event in batch {
                            this.apply(&id, event, cx);
                        }
                    });
                    if applied.is_err() {
                        break;
                    }
                }
            }
        });
        let running = Running {
            installed,
            status: ExtStatus::Starting,
            handle,
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
                    panel.view.update(cx, |view, cx| view.set_tree(tree, cx));
                }
                return;
            }
            // ponytail: notifications and sessions (3.5) are routed later; a dropped request is
            // answered "wai did not answer".
            _ => return,
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
