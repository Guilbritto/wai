use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use gpui_kit::component::attachment::{Attachment as Chip, AttachmentContent, AttachmentTitle};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::{
    ActiveTheme as _, Icon, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_settings::{SeenModel, SeenModes, agent_label, builtin, canonical, shown};

use super::composer;
use super::input::{Attachment, attach_button, paste_into, pick_paths, send_button};
use super::view::ChatSession;
use crate::aurora::{self, DISPLAY_FONT};
use crate::git::{self, Repo};
use crate::workspace::{Dragged, DraggedSession, START_KEY};

pub enum DraftEvent {
    Send,
    Changed,
}

/// What the workspace tells a draft for its menus; `Workspace::draft_data` builds it.
#[derive(Clone, Default)]
pub struct DraftData {
    /// The project, the open sessions' folders and History's, without duplicates.
    pub known: Vec<PathBuf>,
    /// Agent ids in the menu's order.
    pub agents: Vec<String>,
    /// Agent id -> its command, for the installed check.
    pub commands: BTreeMap<String, String>,
    /// Agent id -> the models its last session offered.
    pub models: BTreeMap<String, Vec<SeenModel>>,
    /// Agent id -> the model Settings starts it with.
    pub default_models: BTreeMap<String, String>,
    /// Agent id -> the modes its last session reported, and the one it was in.
    pub modes: BTreeMap<String, SeenModes>,
    /// Agent id -> the model its last draft sent with.
    pub last_models: BTreeMap<String, String>,
    /// Repository root -> the base its last worktree draft sent from.
    pub last_bases: BTreeMap<PathBuf, String>,
}

impl DraftData {
    fn start_model(&self, agent: &str) -> Option<String> {
        let last = self.last_models.get(agent);
        last.or_else(|| self.default_models.get(agent)).cloned()
    }

    fn start_mode(&self, agent: &str) -> Option<String> {
        self.modes.get(agent)?.current.clone()
    }

    fn base(&self, repo: &Repo) -> String {
        let last = self.last_bases.get(&repo.root);
        last.unwrap_or(&repo.default).clone()
    }
}

const OPEN_FOLDER: &str = "\0open-folder";
const CLONE: &str = "\0clone";
const FOOTER: &str = "\0footer";
const CURRENT: &str = "\0current";
const LOCAL: &str = "local";
const WORKTREE: &str = "worktree";

#[derive(Clone)]
enum Lead {
    None,
    Folder,
    Glyph(&'static str, Hsla),
    Icon(gpui_kit::assets::IconName),
}

/// A row of any of the three menus.
#[derive(Clone)]
struct Pick {
    value: String,
    title: SharedString,
    detail: Option<SharedString>,
    lead: Lead,
    tag: Option<&'static str>,
    /// What the closed chip reads, when it differs from the row's title.
    chip: Option<SharedString>,
    /// An `ambar` pill beside the chip's text.
    count: Option<usize>,
    disabled: bool,
}

impl Pick {
    fn new(value: impl Into<String>, title: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            title: title.into(),
            detail: None,
            lead: Lead::None,
            tag: None,
            chip: None,
            count: None,
            disabled: false,
        }
    }

    fn section(text: &str) -> Self {
        Self {
            disabled: true,
            ..Self::new(format!("\0{text}"), text.to_string())
        }
    }

    fn footer(text: String) -> Self {
        Self {
            disabled: true,
            ..Self::new(FOOTER, text)
        }
    }

    fn folder(path: &Path) -> Self {
        let name = path
            .file_name()
            .map_or_else(|| shown(path), |name| name.to_string_lossy().into_owned());
        Self {
            detail: path.parent().map(|parent| shown(parent).into()),
            lead: Lead::Folder,
            ..Self::new(path.to_string_lossy(), name)
        }
    }

    fn row(&self, full: bool) -> Div {
        let row = h_flex().gap_2().items_center().min_w_0();
        let row = match self.lead {
            Lead::Folder => row.child(Icon::new(IconName::Folder).xsmall()),
            Lead::Glyph(glyph, color) => row.child(div().text_color(color).child(glyph)),
            Lead::Icon(icon) => row.child(Icon::new(icon).xsmall()),
            Lead::None => row,
        };
        let title = match (full, &self.chip) {
            (false, Some(chip)) => chip.clone(),
            _ => self.title.clone(),
        };
        let row = row.child(div().truncate().child(title));
        if !full {
            return row.when_some(self.count, |el, count| {
                let pill = div()
                    .px(px(6.))
                    .rounded_full()
                    .text_size(px(11.5))
                    .text_color(aurora::ambar())
                    .bg(aurora::ambar().opacity(0.12))
                    .child(count.to_string());
                el.child(pill)
            });
        }
        row.when(self.disabled, |el| el.text_xs().text_color(aurora::nevoa()))
            .when_some(self.detail.clone(), |el, detail| {
                let text = div().truncate().text_xs().text_color(aurora::nevoa());
                el.child(text.child(detail))
            })
            .when_some(self.tag, |el, tag| {
                el.child(div().text_xs().text_color(aurora::nevoa()).child(tag))
            })
    }
}

impl SelectItem for Pick {
    type Value = String;

    fn title(&self) -> SharedString {
        self.title.clone()
    }

    fn display_title(&self) -> Option<AnyElement> {
        Some(self.row(false).into_any_element())
    }

    fn render(&self, _: &mut Window, _: &mut App) -> impl IntoElement {
        self.row(true)
    }

    fn value(&self) -> &String {
        &self.value
    }

    fn matches(&self, query: &str) -> bool {
        let query = query.to_lowercase();
        let detail = self.detail.as_deref().unwrap_or_default();
        self.title.to_lowercase().contains(&query) || detail.to_lowercase().contains(&query)
    }

    fn disabled(&self) -> bool {
        self.disabled
    }
}

type Menu = Entity<SelectState<Vec<Pick>>>;

/// The agent's mark, as the mockup draws it; an agent without one shows none.
fn glyph(agent: &str) -> Lead {
    match agent {
        "claude" => Lead::Glyph("✳", aurora::brasa()),
        "codex" => Lead::Glyph("◎", aurora::tinta_2()),
        "copilot" => Lead::Glyph("◉", rgb(0x5bb8ff).into()),
        "antigravity" => Lead::Glyph("▲", aurora::magenta()),
        _ => Lead::None,
    }
}

/// A session that has not started: where, with whom, and what to ask. Nothing runs until Send.
pub struct DraftView {
    pub agent: String,
    pub folder: PathBuf,
    pub model: Option<String>,
    pub mode: Option<String>,
    /// The folder's repository; `None` while it is read, or when the folder is not one.
    pub(crate) repo: Option<Repo>,
    /// A `read_repo` is running.
    pub(crate) reading: bool,
    /// Work in a new worktree instead of the folder itself.
    pub(crate) worktree: bool,
    /// Local: the branch to check out (`None` stays on the current one). Worktree: the base.
    pub(crate) branch: Option<String>,
    pub notice: Option<String>,
    /// The URL being cloned.
    pub(crate) cloning: Option<String>,
    /// The chat of a send in flight: the draft shows it until the session starts.
    pub starting: Option<Entity<ChatSession>>,
    pub(super) input: Entity<TextareaState>,
    pub(crate) attachments: Vec<Attachment>,
    /// A worktree is being created for this draft: the next send waits for it to be recorded.
    pub(crate) creating: bool,
    pub(crate) data: DraftData,
    /// Agent id -> its program is on PATH; empty until the check ends.
    installed: HashMap<String, bool>,
    show_install: bool,
    folders: Menu,
    agents: Menu,
    models: Menu,
    modes: Menu,
    places: Menu,
    branches: Menu,
    _read: Option<Task<()>>,
    /// The menus' items and selections are rebuilt at the next frame (it has the window).
    menus_stale: bool,
    _check: Option<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<DraftEvent> for DraftView {}

impl DraftView {
    pub fn new(
        agent: String,
        folder: PathBuf,
        data: DraftData,
        prompt: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| {
            let mut input = TextareaState::new(window, cx)
                .auto_grow(3, 12)
                .placeholder("Pitch your idea");
            input.set_submit_on_enter(true, cx);
            if let Some(prompt) = prompt {
                input.set_value(prompt, window, cx);
            }
            input
        });
        let menu = |searchable, window: &mut Window, cx: &mut Context<Self>| {
            cx.new(|cx| {
                SelectState::new(Vec::<Pick>::new(), None, window, cx).searchable(searchable)
            })
        };
        let (folders, agents, models) = (
            menu(true, window, cx),
            menu(false, window, cx),
            menu(false, window, cx),
        );
        let (places, branches) = (menu(false, window, cx), menu(true, window, cx));
        let modes = menu(false, window, cx);
        let subscriptions = vec![
            cx.subscribe_in(&input, window, |_, _, event: &InputEvent, _, cx| {
                match event {
                    // Shift+Enter inserted a newline already.
                    InputEvent::PressEnter { shift: false, .. } => cx.emit(DraftEvent::Send),
                    InputEvent::Change => cx.emit(DraftEvent::Changed),
                    _ => {}
                }
            }),
            cx.subscribe_in(
                &folders,
                window,
                |this, _, event: &SelectEvent<Vec<Pick>>, window, cx| {
                    let SelectEvent::Confirm(Some(value)) = event else {
                        return;
                    };
                    match value.as_str() {
                        OPEN_FOLDER => this.open_folder(cx),
                        CLONE => this.clone_dialog(window, cx),
                        _ => this.set_folder(PathBuf::from(value), cx),
                    }
                },
            ),
            cx.subscribe(&agents, |this, _, event: &SelectEvent<Vec<Pick>>, cx| {
                if let SelectEvent::Confirm(Some(agent)) = event {
                    this.set_agent(agent.clone(), cx);
                }
            }),
            cx.subscribe(&places, |this, _, event: &SelectEvent<Vec<Pick>>, cx| {
                if let SelectEvent::Confirm(Some(place)) = event {
                    this.set_worktree(place == WORKTREE, cx);
                }
            }),
            cx.subscribe(&branches, |this, _, event: &SelectEvent<Vec<Pick>>, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                let current = this.repo.as_ref().and_then(|r| r.current.as_ref());
                let stay = value == CURRENT || (!this.worktree && Some(value) == current);
                this.set_branch((!stay).then(|| value.clone()), cx);
            }),
            cx.subscribe(&models, |this, _, event: &SelectEvent<Vec<Pick>>, cx| {
                if let SelectEvent::Confirm(Some(model)) = event {
                    this.set_model(Some(model.clone()), cx);
                }
            }),
            cx.subscribe(&modes, |this, _, event: &SelectEvent<Vec<Pick>>, cx| {
                if let SelectEvent::Confirm(Some(mode)) = event {
                    this.set_mode(Some(mode.clone()), cx);
                }
            }),
        ];
        let mut this = Self {
            model: data.start_model(&agent),
            mode: data.start_mode(&agent),
            agent,
            folder,
            repo: None,
            reading: false,
            worktree: false,
            branch: None,
            notice: None,
            cloning: None,
            starting: None,
            input,
            attachments: Vec::new(),
            creating: false,
            data,
            installed: HashMap::new(),
            show_install: false,
            folders,
            agents,
            models,
            modes,
            places,
            branches,
            _read: None,
            menus_stale: true,
            _check: None,
            _subscriptions: subscriptions,
        };
        this.check_installed(cx);
        this.read_repo(cx);
        this
    }

    /// New menu data; the installed check runs again.
    pub fn set_data(&mut self, data: DraftData, cx: &mut Context<Self>) {
        self.data = data;
        if self.mode.is_none() {
            self.mode = self.data.start_mode(&self.agent);
        }
        self.menus_stale = true;
        self.check_installed(cx);
        cx.notify();
    }

    /// Where the session will work. The workspace's project does not follow.
    pub fn set_folder(&mut self, folder: PathBuf, cx: &mut Context<Self>) {
        let folder = canonical(&folder);
        self.menus_stale = true;
        if folder != self.folder {
            self.folder = folder;
            self.read_repo(cx);
            cx.emit(DraftEvent::Changed);
        }
        cx.notify();
    }

    /// Reads the folder's repository off the UI thread; the chips wait for it.
    pub fn read_repo(&mut self, cx: &mut Context<Self>) {
        self.repo = None;
        self.reading = true;
        self.menus_stale = true;
        let folder = self.folder.clone();
        let read = cx.background_spawn(async move { git::repo(&folder) });
        // A newer read replaces this one: dropping the task cancels it.
        self._read = Some(cx.spawn(async move |this, cx| {
            let repo = read.await;
            this.update(cx, |this, cx| {
                this.reading = false;
                this.menus_stale = true;
                let blocked = repo
                    .as_ref()
                    .is_none_or(|r| git::worktree_blocked(r).is_some());
                if blocked {
                    this.worktree = false;
                }
                this.branch = match &repo {
                    Some(repo) if this.worktree => Some(this.data.base(repo)),
                    _ => None,
                };
                this.repo = repo;
                cx.emit(DraftEvent::Changed);
                cx.notify();
            })
            .ok();
        }));
        cx.notify();
    }

    /// Why New worktree is off in this folder.
    pub fn worktree_blocked(&self) -> Option<&'static str> {
        self.repo.as_ref().and_then(git::worktree_blocked)
    }

    /// Refused while the repository is unknown or blocks worktrees.
    pub fn set_worktree(&mut self, on: bool, cx: &mut Context<Self>) {
        if on && (self.repo.is_none() || self.worktree_blocked().is_some()) {
            return;
        }
        self.worktree = on;
        self.branch = (self.repo.as_ref().filter(|_| on)).map(|r| self.data.base(r));
        self.menus_stale = true;
        cx.emit(DraftEvent::Changed);
        cx.notify();
    }

    pub fn set_branch(&mut self, branch: Option<String>, cx: &mut Context<Self>) {
        self.branch = branch;
        self.menus_stale = true;
        cx.emit(DraftEvent::Changed);
        cx.notify();
    }

    /// A send created the worktree at `cwd` for the draft's `from` folder: the draft moves
    /// there, so a retry reuses it.
    pub(crate) fn use_worktree(&mut self, from: &Path, cwd: PathBuf, cx: &mut Context<Self>) {
        if self.folder != from || !self.worktree {
            return;
        }
        self.folder = cwd;
        self.worktree = false;
        self.read_repo(cx);
        cx.emit(DraftEvent::Changed);
    }

    /// The model goes back to the last one used with the agent, else what Settings starts it with.
    pub fn set_agent(&mut self, agent: String, cx: &mut Context<Self>) {
        self.model = self.data.start_model(&agent);
        self.mode = self.data.start_mode(&agent);
        self.agent = agent;
        self.show_install = false;
        self.menus_stale = true;
        self.check_installed(cx);
        cx.emit(DraftEvent::Changed);
        cx.notify();
    }

    pub fn set_model(&mut self, model: Option<String>, cx: &mut Context<Self>) {
        self.model = model;
        self.menus_stale = true;
        cx.emit(DraftEvent::Changed);
        cx.notify();
    }

    pub fn set_mode(&mut self, mode: Option<String>, cx: &mut Context<Self>) {
        self.mode = mode;
        self.menus_stale = true;
        cx.emit(DraftEvent::Changed);
        cx.notify();
    }

    /// Whether the chosen agent's program is on PATH; `None` until the check ends.
    pub fn installed(&self) -> Option<bool> {
        self.installed.get(&self.agent).copied()
    }

    fn check_installed(&mut self, cx: &mut Context<Self>) {
        let commands = self.data.commands.clone();
        let found = cx.background_spawn(async move {
            let found = commands.into_iter();
            let found = found.map(|(id, command)| (id, wai_agent::program(&command).is_some()));
            found.collect::<HashMap<_, _>>()
        });
        self._check = Some(cx.spawn(async move |this, cx| {
            let found = found.await;
            this.update(cx, |this, cx| {
                this.installed = found;
                this.menus_stale = true;
                cx.emit(DraftEvent::Changed);
                cx.notify();
            })
            .ok();
        }));
    }

    fn open_folder(&mut self, cx: &mut Context<Self>) {
        // The menu shows "Open folder…" as picked until it is rebuilt.
        self.menus_stale = true;
        cx.notify();
        let picked = pick_folder(cx);
        cx.spawn(async move |this, cx| {
            if let Some(folder) = picked.await {
                this.update(cx, |this, cx| this.set_folder(folder, cx)).ok();
            }
        })
        .detach();
    }

    /// Asks for the URL, then for the folder to clone into.
    fn clone_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.menus_stale = true;
        cx.notify();
        let url = cx
            .new(|cx| InputState::new(window, cx).placeholder("https://github.com/owner/repo.git"));
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let (field, this) = (url.clone(), this.clone());
            let choose = move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                let text = field.read(cx).value().trim().to_string();
                if text.is_empty() {
                    return;
                }
                window.close_dialog(cx);
                let picked = pick_folder(cx);
                let this = this.clone();
                cx.spawn(async move |cx| {
                    if let Some(parent) = picked.await {
                        this.update(cx, |this, cx| this.clone_into(text, parent, cx))
                            .ok();
                    }
                })
                .detach();
            };
            dialog
                .title("Clone repository")
                .child(div().id("clone-url").child(Input::new(&url)).test_support())
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("clone-cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("clone-choose")
                                .primary()
                                .label("Choose folder…")
                                .on_click(choose),
                        ),
                )
        });
    }

    /// Clones `url` into `parent` off the UI thread; the draft then works in the clone.
    pub fn clone_into(&mut self, url: String, parent: PathBuf, cx: &mut Context<Self>) {
        self.cloning = Some(url.clone());
        self.notice = None;
        cx.emit(DraftEvent::Changed);
        cx.notify();
        let job = {
            let url = url.clone();
            cx.background_spawn(async move { git::clone(&url, &parent) })
        };
        cx.spawn(async move |this, cx| {
            let done = job.await;
            this.update(cx, |this, cx| {
                this.cloning = None;
                match done {
                    Ok(dest) => this.set_folder(dest, cx),
                    Err(why) => this.notice = Some(format!("Could not clone {url}: {why}")),
                }
                cx.emit(DraftEvent::Changed);
                cx.notify();
            })
            .ok();
        })
        .detach();
    }

    fn sync_menus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.menus_stale) {
            return;
        }
        let mut known = self.data.known.clone();
        if !known.contains(&self.folder) {
            known.insert(0, self.folder.clone());
        }
        let mut items: Vec<Pick> = known.iter().map(|path| Pick::folder(path)).collect();
        items.push(Pick::new(OPEN_FOLDER, "Open folder…"));
        items.push(Pick::new(CLONE, "Clone repository…"));
        let current = self.folder.to_string_lossy().into_owned();
        self.folders.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            select.set_selected_value(&current, window, cx);
        });

        let mut items: Vec<Pick> = (self.data.agents.iter())
            .map(|id| Pick {
                lead: glyph(id),
                tag: (self.installed.get(id) == Some(&false)).then_some("Not installed"),
                ..Pick::new(id.clone(), agent_label(id))
            })
            .collect();
        items.push(Pick::footer(
            "Default agent comes from Settings → Agents.".into(),
        ));
        let current = self.agent.clone();
        self.agents.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            select.set_selected_value(&current, window, cx);
        });

        self.sync_git_menus(window, cx);

        let seen = self.data.models.get(&self.agent).into_iter().flatten();
        let mut items: Vec<Pick> = seen
            .map(|m| Pick::new(m.id.clone(), m.name.clone()))
            .collect();
        items.push(Pick::footer(format!(
            "Last used for {}. The list refreshes when the session starts.",
            agent_label(&self.agent)
        )));
        let current = self.model.clone();
        self.models.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            match &current {
                Some(model) => select.set_selected_value(model, window, cx),
                None => select.set_selected_index(None, window, cx),
            }
        });

        let seen = self.data.modes.get(&self.agent).into_iter();
        let items: Vec<Pick> = (seen.flat_map(|s| &s.modes))
            .map(|m| Pick {
                lead: Lead::Icon(gpui_kit::assets::IconName::Shield),
                ..Pick::new(m.id.clone(), m.name.clone())
            })
            .collect();
        let current = self.mode.clone();
        self.modes.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            match &current {
                Some(mode) => select.set_selected_value(mode, window, cx),
                None => select.set_selected_index(None, window, cx),
            }
        });
    }

    /// The branch menu's rows for `repo`.
    fn branch_picks(&self, repo: &Repo) -> Vec<Pick> {
        let worktree = self.worktree;
        let pick = |branch: &str| Pick {
            chip: worktree.then(|| format!("from {branch}").into()),
            ..Pick::new(branch, branch.to_string())
        };
        let mut items = Vec::new();
        if !worktree {
            let title = repo
                .current
                .clone()
                .unwrap_or_else(|| "Detached HEAD".into());
            let detail = match repo.dirty {
                0 => "Current".to_string(),
                1 => "Current · 1 uncommitted file".to_string(),
                n => format!("Current · {n} uncommitted files"),
            };
            items.push(Pick {
                detail: Some(detail.into()),
                count: (repo.dirty > 0).then_some(repo.dirty),
                ..Pick::new(CURRENT, title)
            });
        }
        if worktree || repo.current.as_ref() != Some(&repo.default) {
            items.push(Pick::section("Default branch"));
            items.push(pick(&repo.default));
        }
        let rest: Vec<_> = (repo.branches.iter())
            .filter(|b| **b != repo.default && (worktree || Some(*b) != repo.current.as_ref()))
            .collect();
        if !rest.is_empty() {
            items.push(Pick::section("Branches"));
            items.extend(rest.into_iter().map(|b| pick(b)));
        }
        items.push(Pick::footer(
            match worktree {
                true => "The new worktree starts from this branch.",
                false => "Picking another branch checks it out when the session starts.",
            }
            .into(),
        ));
        items
    }

    fn sync_git_menus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(repo) = self.repo.clone() else {
            return;
        };
        let blocked = self.worktree_blocked();
        let place = |value: &str, icon, title: &str, detail: &str| Pick {
            lead: Lead::Icon(icon),
            detail: Some(detail.to_string().into()),
            ..Pick::new(value, title.to_string())
        };
        let local = place(
            LOCAL,
            gpui_kit::assets::IconName::Monitor,
            "Local",
            "Works in the folder already on this computer",
        );
        let new = Pick {
            disabled: blocked.is_some(),
            ..place(
                WORKTREE,
                gpui_kit::assets::IconName::GitFork,
                "New worktree",
                blocked.unwrap_or("Creates a separate copy for this session, on send"),
            )
        };
        let current = if self.worktree { WORKTREE } else { LOCAL };
        self.places.update(cx, |select, cx| {
            select.set_items(vec![local, new], window, cx);
            select.set_selected_value(&current.to_string(), window, cx);
        });

        let items = self.branch_picks(&repo);
        let worktree = self.worktree;
        let selected = match (&self.branch, worktree) {
            (Some(branch), _) => branch.clone(),
            (None, true) => repo.default.clone(),
            (None, false) => CURRENT.to_string(),
        };
        self.branches.update(cx, |select, cx| {
            select.set_items(items, window, cx);
            select.set_selected_value(&selected, window, cx);
        });
    }

    /// Puts the caret in the prompt box.
    pub fn focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(chat) = &self.starting {
            return chat.update(cx, |chat, cx| chat.focus(window, cx));
        }
        self.input.update(cx, |input, cx| input.focus(window, cx));
    }

    pub fn text(&self, cx: &App) -> String {
        self.input.read(cx).value().trim().to_string()
    }

    pub fn can_send(&self, cx: &App) -> bool {
        self.starting.is_none()
            && !self.creating
            && !(self.worktree && self.repo.is_none())
            && self.cloning.is_none()
            && self.installed() != Some(false)
            && !self.text(cx).is_empty()
    }

    /// What goes out: the draft keeps both, in case the start fails.
    pub(crate) fn message(&self, cx: &App) -> (String, Vec<Attachment>) {
        (self.text(cx), self.attachments.clone())
    }

    /// Back to editing, with `error` (if any) shown above the prompt and what was typed in the
    /// chat meanwhile (`leftover`) after the draft's own text.
    pub fn fail(
        &mut self,
        error: Option<String>,
        leftover: (String, Vec<Attachment>),
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (leftover, files) = leftover;
        self.starting = None;
        self.attachments.extend(files);
        if !leftover.is_empty() {
            let text = [self.text(cx), leftover];
            let text = text.iter().filter(|t| !t.is_empty());
            let text = text.cloned().collect::<Vec<_>>().join("\n\n");
            self.input
                .update(cx, |input, cx| input.set_value(text, window, cx));
        }
        self.notice = error;
        cx.emit(DraftEvent::Changed);
        cx.notify();
    }

    fn attach(&mut self, paths: &ExternalPaths, _: &mut Window, cx: &mut Context<Self>) {
        self.attach_paths(paths.paths(), cx);
    }

    fn attach_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        self.attachments
            .extend(paths.iter().map(|path| Attachment::new(path)));
        cx.notify();
    }

    fn pick_files(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let picked = pick_paths(cx);
        cx.spawn(async move |this, cx| {
            if let Some(paths) = picked.await {
                this.update(cx, |this, cx| this.attach_paths(&paths, cx))
                    .ok();
            }
        })
        .detach();
    }
}

fn pick_folder(cx: &mut App) -> impl Future<Output = Option<PathBuf>> + use<> {
    let picked = cx.prompt_for_paths(PathPromptOptions {
        files: false,
        directories: true,
        multiple: false,
        prompt: None,
    });
    async move { picked.await.ok()?.ok()??.into_iter().next() }
}

impl Render for DraftView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_menus(window, cx);
        if let Some(chat) = &self.starting {
            return chat.clone().into_any_element();
        }
        let theme = cx.theme();
        let dark = theme.is_dark();
        let muted = match dark {
            true => aurora::nevoa_2(),
            false => theme.muted_foreground,
        };
        let dragged = DraggedSession {
            title: "New session".into(),
            source: Dragged::Draft(cx.entity()),
        };
        let header = aurora::card_bar(cx)
            .gap_3()
            .px_3()
            .py_2()
            .child(
                div()
                    .id("draft-title")
                    .flex_shrink(1.)
                    .min_w_0()
                    .truncate()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(14.))
                    .font_weight(FontWeight::BOLD)
                    .cursor_grab()
                    .child("New session")
                    .on_drag(dragged, DraggedSession::preview)
                    // Registers the id so tests can find and drag it.
                    .test_support(),
            )
            .child(
                aurora::pill(cx)
                    .flex_none()
                    .border_dashed()
                    .text_color(match dark {
                        true => aurora::nevoa(),
                        false => theme.muted_foreground,
                    })
                    .child("Draft · starts when you send"),
            );
        let chips = self.attachments.iter().enumerate().map(|(ix, attachment)| {
            Chip::new()
                .id(("attachment", ix))
                .content(
                    AttachmentContent::new().title(AttachmentTitle::new(attachment.name.clone())),
                )
                .on_remove(cx.listener(move |this, _, _, cx| {
                    this.attachments.remove(ix);
                    cx.notify();
                }))
        });
        let missing = (self.installed() == Some(false)).then(|| {
            let label = agent_label(&self.agent);
            let command = self.data.commands.get(&self.agent);
            let program = command.and_then(|command| wai_agent::program_name(command));
            let program = program.map(|p| p.display().to_string()).unwrap_or_default();
            let install = builtin(&self.agent).map(|agent| agent.install);
            v_flex()
                .id("draft-notice")
                .gap_1()
                .text_size(px(13.))
                .child(
                    div()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.danger)
                        .child(format!("{label} is not installed")),
                )
                .child(div().text_color(muted).child(format!(
                    "wai runs {label} through its CLI and could not find {program} on your PATH."
                )))
                .when_some(install, |el, text| {
                    let button = Button::new("draft-install")
                        .ghost()
                        .small()
                        .label("How to install")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if text.starts_with("https://") {
                                cx.open_url(text);
                            } else {
                                this.show_install = true;
                                cx.notify();
                            }
                        }));
                    el.child(h_flex().child(button))
                })
                .when_some(install.filter(|_| self.show_install), |el, text| {
                    el.child(div().text_color(muted).child(text))
                })
                // Registers the id so tests can find it.
                .test_support()
                .into_any_element()
        });
        let cloning = self.cloning.as_ref().map(|url| format!("Cloning {url}…"));
        let notice = missing.or_else(|| {
            cloning.or_else(|| self.notice.clone()).map(|notice| {
                div()
                    .id("draft-notice")
                    .text_size(px(13.))
                    .text_color(theme.danger)
                    .child(notice)
                    // Registers the id so tests can find it.
                    .test_support()
                    .into_any_element()
            })
        });
        let send = send_button(!self.can_send(cx), cx)
            .on_click(cx.listener(|_, _, _, cx| cx.emit(DraftEvent::Send)));
        let attach = attach_button()
            .on_click(cx.listener(|this, _, window, cx| this.pick_files(window, cx)));
        let max_h = window.viewport_size().height * 0.5;
        let chip = |id: &'static str,
                    menu: &Menu,
                    width: f32,
                    search: Option<&'static str>,
                    bar: bool,
                    shrink: f32| {
            let select = Select::new(menu)
                .appearance(false)
                .small()
                .menu_width(px(width + 80.))
                .menu_max_h(max_h)
                .when_some(search, |select, text| select.search_placeholder(text));
            composer::chip(id, select, bar, shrink, cx)
                // Registers the id so tests can find it.
                .test_support()
        };
        let strip = composer::strip("draft-strip", cx)
            .child(chip(
                "draft-folder",
                &self.folders,
                220.,
                Some("Search projects"),
                false,
                2.,
            ))
            .when(self.repo.is_some(), |el| {
                let places = chip("draft-workspace", &self.places, 150., None, false, 0.);
                let branches = chip(
                    "draft-branch",
                    &self.branches,
                    260.,
                    Some("Filter branches"),
                    false,
                    4.,
                );
                el.child(places).child(branches)
            })
            .child(chip("draft-agent", &self.agents, 130., None, false, 0.));
        let strip = composer::strip_row(strip);
        let model = (!self
            .data
            .models
            .get(&self.agent)
            .is_none_or(|m| m.is_empty()))
        .then(|| {
            chip("draft-model", &self.models, 150., None, true, 0.)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        });
        let mode = self.data.modes.contains_key(&self.agent).then(|| {
            chip("draft-mode", &self.modes, 150., None, true, 0.)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        });
        let this = cx.entity().downgrade();
        let textarea =
            Textarea::new(&self.input)
                .appearance(false)
                .on_paste(move |item, window, cx| {
                    let paste = |s: &mut Self, cx: &mut Context<Self>| {
                        let pasted = paste_into(&mut s.attachments, item, window, cx);
                        if pasted {
                            cx.notify();
                        }
                        pasted
                    };
                    this.update(cx, paste).unwrap_or(false)
                });
        let focused = self.input.read(cx).focus_handle(cx).is_focused(window);
        let prompt = v_flex()
            .id("draft-composer")
            .gap_2()
            .p_3()
            .rounded(px(20.))
            .border_1()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus(window, cx)),
            );
        let prompt = match (dark, focused) {
            (true, true) => prompt
                .bg(aurora::noite_2().alpha(0.78))
                .border_color(aurora::boreal()),
            (true, false) => prompt
                .bg(aurora::noite_2().alpha(0.78))
                .border_color(aurora::borda_forte()),
            (false, true) => prompt.bg(theme.background).border_color(theme.ring),
            (false, false) => prompt.bg(theme.background).border_color(theme.input),
        }
        .children(notice)
        .when(!self.attachments.is_empty(), |el| {
            el.child(h_flex().gap_2().flex_wrap().children(chips))
        })
        .child(textarea)
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(attach)
                .children(mode)
                .child(div().flex_1())
                .children(model)
                .child(send),
        )
        // Registers the id so tests can find it.
        .test_support();
        // `.hint`: the keys as small chips, the draft shortcut at the right.
        let key = |keys: &str, what: String| {
            h_flex()
                .gap(px(6.))
                .child(
                    div()
                        .px(px(5.))
                        .rounded(px(5.))
                        .border_1()
                        .text_size(px(11.))
                        .font_family(theme.mono_font_family.clone())
                        .bg(aurora::vidro())
                        .border_color(aurora::borda())
                        .text_color(aurora::nevoa())
                        .child(keys.to_string()),
                )
                .child(what)
        };
        let hints = h_flex()
            .justify_between()
            .px(px(6.))
            .pt(px(8.))
            .text_size(px(12.))
            .text_color(muted)
            .child(
                h_flex()
                    .gap_3()
                    .child(key("⏎", "start session".into()))
                    .child(key("⇧⏎", "new line".into())),
            )
            .child(key(START_KEY, "another draft".into()));
        let body = v_flex()
            .w_full()
            .max_w(px(820.))
            .child(
                v_flex()
                    .items_center()
                    .mb(px(34.))
                    .child(
                        div()
                            .font_family(DISPLAY_FONT)
                            .text_size(px(42.))
                            .font_weight(FontWeight::EXTRA_BOLD)
                            .mb(px(6.))
                            .child("What are we building?"),
                    )
                    .child(
                        div()
                            .text_size(px(14.))
                            .text_color(muted)
                            .child("Pick where and with whom. Nothing starts until you send."),
                    ),
            )
            .child(v_flex().child(strip).child(prompt))
            .child(hints);
        v_flex()
            .key_context("DraftView")
            .on_drop(cx.listener(Self::attach))
            .size_full()
            .child(header)
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .items_start()
                    .justify_center()
                    .px_4()
                    .pb_4()
                    .pt(window.viewport_size().height * 0.09)
                    .child(body),
            )
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{TestAppContext, VisualTestContext, px, size};

    use super::DraftView;
    use crate::app::NewSession;
    use crate::git::Repo;
    use crate::workspace::tests::{fake_settings, open};

    /// In a narrow pane a long branch name shrinks its chip: no chip leaves the composer.
    #[gpui_kit::test]
    fn chips_stay_inside_the_composer_in_a_narrow_pane(cx: &mut TestAppContext) {
        narrow_pane(cx);
    }

    #[gpui_kit::test]
    fn current_default_branch_is_listed_once(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.new_session(&NewSession, window, cx))
        });
        let draft = workspace.read_with(cx, |w, _| w.panes[0].draft().cloned());
        let draft = draft.expect("the draft");
        let repo = Repo {
            root: draft.read_with(cx, |d, _| d.folder.clone()),
            current: Some("main".into()),
            default: "main".into(),
            branches: vec!["main".into(), "dev".into()],
            dirty: 0,
            submodules: false,
            origin: false,
        };
        let mains = |d: &DraftView| {
            d.branch_picks(&repo)
                .iter()
                .filter(|p| p.value == "main")
                .count()
        };
        assert_eq!(draft.read_with(cx, |d, _| mains(d)), 0);
        draft.update(cx, |d, _| d.worktree = true);
        assert_eq!(draft.read_with(cx, |d, _| mains(d)), 1);
    }

    fn narrow_pane(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.simulate_resize(size(px(860.), px(700.)));
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.new_session(&NewSession, window, cx))
        });
        let draft = workspace.read_with(cx, |w, _| w.panes[0].draft().cloned());
        let draft = draft.expect("the draft");
        draft.update(cx, |d, cx| {
            d.repo = Some(Repo {
                root: d.folder.clone(),
                current: Some("chore/ec-9688-energy-dashboard-with-a-very-long-name".into()),
                default: "main".into(),
                branches: vec!["main".into()],
                dirty: 3,
                submodules: false,
                origin: false,
            });
            d.menus_stale = true;
            cx.notify();
        });
        let right = |id: &'static str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                window.render_frame(cx);
                window.find(id).bounds().right()
            })
        };
        let composer = right("draft-composer", cx);
        for id in [
            "draft-folder",
            "draft-workspace",
            "draft-branch",
            "draft-agent",
        ] {
            let edge = right(id, cx);
            assert!(
                edge <= composer + px(0.5),
                "{id} ends at {edge:?} > {composer:?}"
            );
        }
    }
}
