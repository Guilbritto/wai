use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};

use gpui_kit::component::attachment::{Attachment as Chip, AttachmentContent, AttachmentTitle};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{InputEvent, Textarea, TextareaState};
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::{ActiveTheme as _, Icon, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_settings::{SeenModel, SeenModes, agent_label, builtin, canonical, shown};

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
const FOOTER: &str = "\0footer";
const CURRENT: &str = "\0current";
const LOCAL: &str = "local";
const WORKTREE: &str = "worktree";

#[derive(Clone)]
enum Lead {
    None,
    Folder,
    Glyph(&'static str),
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
            Lead::Glyph(glyph) => row.child(glyph),
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
                    .text_xs()
                    .text_color(aurora::ambar())
                    .bg(aurora::ambar().opacity(0.16))
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

fn glyph(agent: &str) -> Lead {
    Lead::Glyph(match agent {
        "claude" => "✳",
        "codex" => "◎",
        "copilot" => "◉",
        "antigravity" => "▲",
        _ => "◆",
    })
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
    /// The chat of a send in flight: the draft shows it until the session starts.
    pub starting: Option<Entity<ChatSession>>,
    pub(super) input: Entity<TextareaState>,
    pub(crate) attachments: Vec<Attachment>,
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
            cx.subscribe(&folders, |this, _, event: &SelectEvent<Vec<Pick>>, cx| {
                let SelectEvent::Confirm(Some(value)) = event else {
                    return;
                };
                match value.as_str() {
                    OPEN_FOLDER => this.open_folder(cx),
                    _ => this.set_folder(PathBuf::from(value), cx),
                }
            }),
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
            starting: None,
            input,
            attachments: Vec::new(),
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
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn(async move |this, cx| {
            let paths = picked.await.ok().and_then(|picked| picked.ok()).flatten();
            if let Some(folder) = paths.and_then(|paths| paths.into_iter().next()) {
                this.update(cx, |this, cx| this.set_folder(folder, cx)).ok();
            }
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
        items.push(Pick::section("Default branch"));
        items.push(pick(&repo.default));
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
        self.starting.is_none() && self.installed() != Some(false) && !self.text(cx).is_empty()
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
        leftover: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.starting = None;
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
                    .flex_1()
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
        let notice = missing.or_else(|| {
            self.notice.clone().map(|notice| {
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
        let chip = |id: &'static str, menu: &Menu, width: f32, search: Option<&'static str>| {
            let select = Select::new(menu)
                .appearance(false)
                .small()
                .menu_width(px(width + 80.))
                .menu_max_h(max_h)
                .when_some(search, |select, text| select.search_placeholder(text));
            aurora::pill(cx)
                .id(id)
                .w(px(width))
                .px(px(4.))
                .child(select)
                // Registers the id so tests can find it.
                .test_support()
        };
        let strip = h_flex()
            .gap_2()
            .flex_wrap()
            .child(chip(
                "draft-folder",
                &self.folders,
                220.,
                Some("Search projects"),
            ))
            .child(chip("draft-agent", &self.agents, 150., None))
            .when(self.repo.is_some(), |el| {
                let places = chip("draft-workspace", &self.places, 170., None);
                let branches = chip(
                    "draft-branch",
                    &self.branches,
                    200.,
                    Some("Filter branches"),
                );
                el.child(places).child(branches)
            });
        let model = (!self
            .data
            .models
            .get(&self.agent)
            .is_none_or(|m| m.is_empty()))
        .then(|| {
            chip("draft-model", &self.models, 170., None)
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        });
        let mode = self.data.modes.contains_key(&self.agent).then(|| {
            chip("draft-mode", &self.modes, 150., None)
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
            .gap_2()
            .p_3()
            .rounded(px(16.))
            .border_1()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _, window, cx| this.focus(window, cx)),
            );
        let prompt = match (dark, focused) {
            (true, true) => prompt.bg(aurora::vidro()).border_color(aurora::boreal()),
            (true, false) => prompt
                .bg(aurora::vidro())
                .border_color(aurora::borda_forte()),
            (false, true) => prompt.bg(theme.background).border_color(theme.ring),
            (false, false) => prompt.bg(theme.background).border_color(theme.input),
        }
        .children(notice)
        .child(textarea)
        .when(!self.attachments.is_empty(), |el| {
            el.child(h_flex().gap_2().flex_wrap().children(chips))
        })
        .child(
            h_flex()
                .gap_2()
                .items_center()
                .child(attach)
                .children(mode)
                .child(div().flex_1())
                .children(model)
                .child(send),
        );
        let hints = h_flex()
            .gap_4()
            .justify_center()
            .text_size(px(12.))
            .text_color(muted)
            .child("⏎ start session")
            .child("⇧⏎ new line")
            .child(format!("{START_KEY} another draft"));
        let body = v_flex()
            .w_full()
            .max_w(px(640.))
            .gap_3()
            .child(
                div()
                    .font_family(DISPLAY_FONT)
                    .text_size(px(26.))
                    .font_weight(FontWeight::BOLD)
                    .child("What are we building?"),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .text_color(muted)
                    .child("Pick where and with whom. Nothing starts until you send."),
            )
            .child(strip)
            .child(prompt)
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
                    .items_center()
                    .justify_center()
                    .p_4()
                    .child(body),
            )
            .into_any_element()
    }
}
