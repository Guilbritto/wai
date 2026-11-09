//! The enabled extensions' processes, one `ExtHandle` each. They start once the window is open
//! (and on install or enable) and stop on disable, before uninstall and at quit. They inherit
//! wai's cwd, the config dir (R18).

use std::collections::HashMap;
use std::path::PathBuf;

use gpui_kit::*;
use wai_ext::{ExtEvent, ExtHandle, ExtId, Installed};

#[derive(Debug, Clone, PartialEq)]
pub enum ExtStatus {
    Starting,
    Ready,
    Failed(String),
    Crashed(String),
}

struct Running {
    installed: Installed,
    status: ExtStatus,
    /// Dropping it kills the process.
    _handle: ExtHandle,
    _pump: Task<()>,
}

pub struct ExtHost {
    /// Sent in `initialize`.
    project: PathBuf,
    running: HashMap<ExtId, Running>,
}

impl ExtHost {
    pub fn new(project: PathBuf) -> Self {
        Self {
            project,
            running: HashMap::new(),
        }
    }

    /// Runs exactly the enabled extensions of `list` that have a process: starts the missing
    /// ones, stops the rest.
    pub fn sync(&mut self, list: &[Installed], cx: &mut Context<Self>) {
        let wanted = |i: &&Installed| i.enabled && i.manifest.process.is_some();
        let wanted: Vec<_> = list.iter().filter(wanted).collect();
        self.running
            .retain(|id, _| wanted.iter().any(|i| i.manifest.id == *id));
        for installed in wanted {
            if !self.running.contains_key(&installed.manifest.id) {
                self.start(installed.clone(), cx);
            }
        }
        cx.notify();
    }

    /// Kills its process now, on this thread.
    pub fn stop(&mut self, id: &ExtId) {
        self.running.remove(id);
    }

    pub fn stop_all(&mut self) {
        self.running.clear();
    }

    /// `initialize` told them the project: they start again in the new one.
    pub fn set_project(&mut self, project: PathBuf, cx: &mut Context<Self>) {
        if project == self.project {
            return;
        }
        self.project = project;
        let running: Vec<_> = self.running.drain().map(|(_, r)| r.installed).collect();
        for installed in running {
            self.start(installed, cx);
        }
        cx.notify();
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
            _handle: handle,
            _pump: pump,
        };
        self.running.insert(id, running);
    }

    fn apply(&mut self, id: &ExtId, event: ExtEvent, cx: &mut Context<Self>) {
        let status = match event {
            ExtEvent::Ready { .. } => ExtStatus::Ready,
            ExtEvent::Failed { error, .. } => ExtStatus::Failed(error),
            ExtEvent::Crashed { stderr_tail, .. } => ExtStatus::Crashed(stderr_tail),
            // ponytail: panels (3.4) and notifications and sessions (3.5) are routed later; a
            // dropped request is answered "wai did not answer".
            _ => return,
        };
        if let Some(running) = self.running.get_mut(id) {
            running.status = status;
            cx.notify();
        }
    }
}
