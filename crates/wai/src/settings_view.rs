//! The Settings modal: a section nav, a search over the page and the rows of the sections. It
//! shows what the workspace gives it (`set`) and says what the user changed (`SettingsEvent`);
//! the workspace writes the user's file.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogEntrance;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, ThemeMode, h_flex, v_flex,
};
use gpui_kit::*;
use wai_ext::panel::Pick;
use wai_settings::claude::{ClaudeData, Stamp};
use wai_settings::{
    Change, FontKey, Processing, SeenModel, Settings, ThemeChoice, agent_label, builtin,
};

use crate::appearance::catalog::ThemeCatalog;
use crate::appearance::{Fonts, fonts};
use crate::aurora::{self, DISPLAY_FONT};
use crate::campo;
use crate::extensions_view::ExtensionsView;
use crate::path_field::{self, PathField};
use crate::update::UpdateStatus;

pub(crate) mod claude;

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Section {
    Ai,
    Appearance,
    Conversation,
    Terminal,
    Updates,
    Extensions,
    Plugins,
    Mcp,
    Skills,
    Instructions,
    Agents,
    Hooks,
    Codex,
    Copilot,
    Antigravity,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Ai => "AI",
            Section::Appearance => "Appearance",
            Section::Conversation => "Conversation",
            Section::Terminal => "Terminal",
            Section::Updates => "Updates",
            Section::Extensions => "Extensions",
            Section::Plugins => "Plugins",
            Section::Mcp => "MCP Servers",
            Section::Skills => "Skills",
            Section::Instructions => "Instructions",
            Section::Agents => "Agents",
            Section::Hooks => "Hooks",
            Section::Codex => "Codex",
            Section::Copilot => "Copilot",
            Section::Antigravity => "Antigravity",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Section::Ai => "ai",
            Section::Appearance => "appearance",
            Section::Conversation => "conversation",
            Section::Terminal => "terminal",
            Section::Updates => "updates",
            Section::Extensions => "extensions",
            Section::Plugins => "plugins",
            Section::Mcp => "mcp",
            Section::Skills => "skills",
            Section::Instructions => "instructions",
            Section::Agents => "agents",
            Section::Hooks => "hooks",
            Section::Codex => "codex",
            Section::Copilot => "copilot",
            Section::Antigravity => "antigravity",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Section::Ai => "Which AI starts new sessions, and how wai runs each one.",
            Section::Appearance => "How wai looks.",
            Section::Conversation => {
                "What a conversation shows while the agent works. Permission cards, questions and file edits always show."
            }
            Section::Terminal => "The terminal under the panes.",
            Section::Updates => {
                "wai checks for a new release each time it opens, downloads it in the background and installs it when you click Restart now."
            }
            Section::Extensions => {
                "Extensions add panels, commands, agents and MCP servers to wai. They run with your permissions."
            }
            Section::Plugins => {
                "What Claude loads from ~/.claude/plugins: commands, skills, agents, hooks and MCP servers. A change applies to new Claude sessions."
            }
            Section::Skills => {
                "Instructions Claude loads on its own when a task fits. They come with plugins; turn the plugin off to drop its skills."
            }
            Section::Instructions => {
                "Files Claude reads at the start of every session (CLAUDE.md and what it includes)."
            }
            Section::Agents => "Subagents Claude can hand work to.",
            Section::Hooks => {
                "Commands Claude Code runs on its own at set moments. They run with your permissions."
            }
            Section::Codex => {
                "Plugins, MCP servers, skills, instructions, agents and hooks of Codex, like the Claude ones."
            }
            Section::Copilot => {
                "Plugins, MCP servers, skills, instructions, agents and hooks of Copilot, like the Claude ones."
            }
            Section::Antigravity => {
                "Plugins, MCP servers, skills, instructions, agents and hooks of Antigravity, like the Claude ones."
            }
            Section::Mcp => {
                "Servers that give Claude more tools. Turning one off applies to this project, in new Claude sessions, and keeps its configuration. Servers from a .mcp.json file and your claude.ai connectors cannot be turned off here yet."
            }
        }
    }
}

impl Section {
    fn is_soon(self) -> bool {
        matches!(
            self,
            Section::Codex | Section::Copilot | Section::Antigravity
        )
    }
}

/// What a Coming soon page says under "Not in wai yet".
pub(crate) fn soon_text(section: Section) -> String {
    let x = section.title();
    format!(
        "Today wai shows what Claude uses. {x} comes next; its own settings stay in its CLI meanwhile."
    )
}

/// The nav's groups, each with its label and sections.
pub(crate) const NAV: &[(&str, &[Section])] = &[
    (
        "wai",
        &[
            Section::Ai,
            Section::Appearance,
            Section::Conversation,
            Section::Terminal,
            Section::Updates,
            Section::Extensions,
        ],
    ),
    (
        "Claude",
        &[
            Section::Plugins,
            Section::Mcp,
            Section::Skills,
            Section::Instructions,
            Section::Agents,
            Section::Hooks,
        ],
    ),
    (
        "Other agents",
        &[Section::Codex, Section::Copilot, Section::Antigravity],
    ),
];

/// 88% x 86% of the viewport; maximized, the viewport less 24 px on each axis.
pub(crate) fn modal_size(viewport: Size<Pixels>, maximized: bool) -> Size<Pixels> {
    let (w, h) = (f32::from(viewport.width), f32::from(viewport.height));
    if maximized {
        size(px(w - 24.), px(h - 24.))
    } else {
        size(px((w * 0.88).round()), px((h * 0.86).round()))
    }
}

/// Opens the modal. The dialog builder holds the only strong handle: closing it drops the view.
pub(crate) fn open_modal(view: Entity<SettingsView>, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, window, cx| {
        let size = modal_size(window.viewport_size(), view.read(cx).maximized());
        dialog
            .w(size.width)
            .h(size.height)
            .centered()
            .close_button(false)
            .entrance(DialogEntrance::None)
            .p_0()
            // Not `child`: the dialog puts children in a scroll area as tall as they are, so the
            // whole modal would scroll. The content slot has the dialog's height; the nav and the
            // page scroll on their own inside it.
            .content({
                let view = view.clone();
                move |content, _, _| content.overflow_hidden().child(view.clone())
            })
    });
}

/// Every row with its section, the card it sits in (titled unless empty) and its label: the
/// label is what the search matches.
const ROWS: [(Section, &str, &str); 16] = [
    (Section::Appearance, "", "Mode"),
    (Section::Appearance, "", "Light theme"),
    (Section::Appearance, "", "Dark theme"),
    (Section::Appearance, "", "Themes folder"),
    (Section::Appearance, "Colors", "Colors"),
    (Section::Appearance, "Fonts", "Interface"),
    (Section::Appearance, "Fonts", "Editor"),
    (Section::Appearance, "Fonts", "Terminal"),
    (Section::Appearance, "Fonts", "Chat"),
    (Section::Appearance, "Fonts", "Chat code"),
    (Section::Conversation, "", "Processing"),
    (Section::Terminal, "", "Shell"),
    (Section::Updates, "", "Automatic updates"),
    (Section::Updates, "", "Version"),
    (Section::Updates, "Troubleshooting", "Diagnostics"),
    (Section::Updates, "Troubleshooting", "Repository"),
];

/// The font areas of Settings > Appearance: label, key and the id its row carries.
const FONT_AREAS: [(&str, FontKey, &str); 5] = [
    ("Interface", FontKey::Interface, "interface"),
    ("Editor", FontKey::Editor, "editor"),
    ("Terminal", FontKey::Terminal, "terminal"),
    ("Chat", FontKey::Chat, "chat"),
    ("Chat code", FontKey::ChatCode, "chat-code"),
];

/// The page's column (padding included) and the width of a select or text field in it.
const COLUMN: f32 = 760.;
const FIELD: f32 = 240.;

/// `inner` as the page's column, centered.
fn column(inner: Div) -> Div {
    h_flex()
        .w_full()
        .flex_none()
        .justify_center()
        .child(inner.w_full().max_w(px(COLUMN)).px(px(32.)))
}

/// Hairlines between rows and around the header.
fn divider(cx: &App) -> Hsla {
    if cx.theme().is_dark() {
        aurora::borda(cx)
    } else {
        cx.theme().border
    }
}

/// Rows in a card, a hairline between them: glass in dark mode, the theme's tokens in light.
pub(crate) fn card(rows: impl IntoIterator<Item = Div>, cx: &App) -> Div {
    let theme = cx.theme();
    let fill = if theme.is_dark() {
        aurora::vidro(cx)
    } else {
        theme.background
    };
    let line = divider(cx).opacity(0.6);
    v_flex()
        .rounded(px(12.))
        .border_1()
        .border_color(divider(cx))
        .bg(fill)
        .overflow_hidden()
        .children(rows.into_iter().enumerate().map(|(ix, row)| match ix {
            0 => row,
            _ => row.border_t_1().border_color(line),
        }))
}

/// `body` under its small-capitals title; no title when `title` is empty.
fn titled(title: &str, body: Div, cx: &App) -> Div {
    v_flex()
        .gap_2()
        .children((!title.is_empty()).then(|| campo::label(title.to_string(), cx).pl(px(2.))))
        .child(body)
}

/// What a row is on the left, its control on the right: every control ends on the card's edge.
fn grid(left: impl IntoElement, control: impl IntoElement) -> Div {
    h_flex()
        .items_center()
        .gap(px(24.))
        .child(div().flex_1().min_w_0().child(left))
        .child(
            h_flex()
                .flex_none()
                .justify_end()
                .items_center()
                .gap_2()
                .child(control),
        )
}

/// A row of a card.
pub(crate) fn setting(left: impl IntoElement, control: impl IntoElement) -> Div {
    grid(left, control).px_4().py(px(12.)).min_h(px(56.))
}

/// A line under an agent's header, indented past its radio.
fn sub_setting(left: impl IntoElement, control: impl IntoElement, cx: &App) -> Div {
    sub_fill(grid(left, control), cx)
}

fn sub_fill(el: Div, cx: &App) -> Div {
    let fill = if cx.theme().is_dark() {
        hsla(0., 0., 0., 0.12)
    } else {
        cx.theme().muted.opacity(0.4)
    };
    el.pl(px(44.)).pr_4().py(px(10.)).bg(fill)
}

/// A row's name and, under it, what it does.
fn named(label: impl Into<SharedString>, hint: Option<SharedString>, cx: &App) -> Div {
    v_flex()
        .gap(px(2.))
        .child(
            div()
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(cx.theme().foreground)
                .child(label.into()),
        )
        .children(hint.map(|h| {
            div()
                .text_size(fonts::ui(cx, 12.5))
                .text_color(cx.theme().muted_foreground)
                .child(h)
        }))
}

/// A select or text field, as wide as every other one.
fn field(control: impl IntoElement) -> Div {
    div().w(px(FIELD)).child(control)
}

#[derive(Clone, Debug, PartialEq)]
pub enum RowStatus {
    CliNotFound,
    NotChecked,
    Starting,
    Ready,
    SignedOut,
    Failed(String),
}

impl RowStatus {
    pub fn text(&self, login: Option<&str>) -> String {
        match self {
            RowStatus::CliNotFound => "CLI not found".into(),
            RowStatus::NotChecked => "Not checked".into(),
            RowStatus::Starting => "Starting…".into(),
            RowStatus::Ready => "Ready".into(),
            RowStatus::SignedOut => match login {
                Some(login) => format!("Not signed in · run {login}"),
                None => "Not signed in".into(),
            },
            RowStatus::Failed(error) => {
                format!("Failed: {}", error.lines().last().unwrap_or_default())
            }
        }
    }
}

#[derive(Clone, Debug)]
pub struct AgentRow {
    pub id: String,
    pub label: String,
    pub extension: Option<String>,
    pub status: RowStatus,
    /// `"<name> <version>"` of the running adapter; `None` until it is `Ready`.
    pub adapter: Option<String>,
    /// `"<version> is out (wai runs <pinned>)"` when npm has a newer adapter.
    pub adapter_note: Option<String>,
    /// The command in force (approved).
    pub command: String,
    pub models: Vec<SeenModel>,
    pub default_model: Option<String>,
    pub is_default: bool,
}

fn warning(row: &AgentRow) -> Option<&'static str> {
    (row.id == "copilot").then_some("Copilot may edit files and run commands without asking.")
}

pub struct SettingsData {
    pub settings: Settings,
    pub agents: Vec<AgentRow>,
    /// The project's own default agent, when it is the one in force.
    pub project_default: Option<String>,
    /// Why `settings.toml` cannot be read: the screen is read-only then.
    pub error: Option<String>,
    pub notice: Option<SharedString>,
    /// The title's suffix: the default agent and the project.
    pub context: String,
    pub extensions: Option<Entity<ExtensionsView>>,
    /// Where `~/.claude` lives: paths on the Claude pages are shown relative to it.
    pub home: PathBuf,
    pub update: UpdateStatus,
    /// Why this copy cannot install an update; shown whatever the status says.
    pub update_blocker: Option<String>,
}

pub enum SettingsEvent {
    Change(Change),
    OpenFile,
    CheckUpdates,
    RestartToUpdate,
    CopyDiagnostics,
    /// Creates the themes folder when missing and shows it in the file manager.
    OpenThemes,
    Test(String),
    CheckMcp,
    Open {
        path: PathBuf,
        read_only: bool,
    },
    /// `seen` is the settings.json stamp the switch was shown with.
    TogglePlugin {
        id: String,
        on: bool,
        seen: Stamp,
    },
    ToggleMcp {
        name: String,
        on: bool,
    },
}

#[derive(Clone)]
struct Theme(ThemeChoice);

impl SelectItem for Theme {
    type Value = ThemeChoice;

    fn title(&self) -> SharedString {
        match self.0 {
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
            ThemeChoice::System => "System",
        }
        .into()
    }

    fn value(&self) -> &ThemeChoice {
        &self.0
    }
}

/// A theme of the Light theme or Dark theme select.
#[derive(Clone)]
struct ThemeName(String);

impl SelectItem for ThemeName {
    type Value = String;

    fn title(&self) -> SharedString {
        self.0.clone().into()
    }

    fn value(&self) -> &String {
        &self.0
    }
}

#[derive(Clone)]
struct Steps(Processing);

impl SelectItem for Steps {
    type Value = Processing;

    fn title(&self) -> SharedString {
        match self.0 {
            Processing::Steps => "Step by step",
            Processing::Grouped => "Grouped",
            Processing::Hidden => "Hidden",
        }
        .into()
    }

    fn value(&self) -> &Processing {
        &self.0
    }
}

/// A model of the Default model select; the empty id is the agent's own choice.
#[derive(Clone)]
struct ModelChoice {
    id: String,
    name: String,
}

impl SelectItem for ModelChoice {
    type Value = String;

    fn title(&self) -> SharedString {
        self.name.clone().into()
    }

    fn value(&self) -> &String {
        &self.id
    }
}

fn model_choices(row: &AgentRow) -> Vec<ModelChoice> {
    let first = ModelChoice {
        id: String::new(),
        name: "Agent's choice".into(),
    };
    (std::iter::once(first))
        .chain(row.models.iter().map(|m| ModelChoice {
            id: m.id.clone(),
            name: m.name.clone(),
        }))
        .collect()
}

pub struct SettingsView {
    section: Section,
    maximized: bool,
    data: SettingsData,
    pub(crate) search: Entity<InputState>,
    theme: Entity<SelectState<Vec<Theme>>>,
    /// The Light theme and Dark theme selects.
    themes: [Entity<SelectState<Vec<ThemeName>>>; 2],
    /// Family and size of each of `FONT_AREAS`, in that order.
    font_inputs: Vec<Entity<InputState>>,
    /// Per font input: it holds edits not committed yet.
    font_editing: [bool; 10],
    /// The placeholder each font input shows (the value in effect).
    font_hints: Vec<String>,
    processing: Entity<SelectState<Vec<Steps>>>,
    /// Per agent id.
    models: BTreeMap<String, Entity<SelectState<Vec<ModelChoice>>>>,
    pub(crate) shell: Entity<InputState>,
    repo: Entity<InputState>,
    /// The unfolded Advanced row's command; Enter asks to run it.
    pub(crate) command: Entity<InputState>,
    /// The agent whose Advanced is unfolded.
    advanced: Option<String>,
    /// A text field holds edits not committed yet (Enter, or blur but for the command): a reload
    /// leaves it alone.
    editing: [bool; 3],
    /// `data` changed: the fields show it at the next frame.
    stale: bool,
    /// Read off the UI thread: `None` until it lands.
    claude: Option<ClaudeData>,
    /// A write is out: the switches wait for it.
    busy: bool,
    mcp_check: claude::McpCheck,
    /// Groups the user flipped from their default (open or closed), per page.
    toggled: HashSet<(Section, String)>,
    _subscriptions: Vec<Subscription>,
}

impl EventEmitter<SettingsEvent> for SettingsView {}

impl SettingsView {
    pub fn new(
        section: Section,
        data: SettingsData,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder = format!("Search {}…", section.title().to_lowercase());
        let search = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let choices = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark];
        let choices: Vec<Theme> = choices.into_iter().map(Theme).collect();
        let theme = cx.new(|cx| SelectState::new(choices, None, window, cx));
        let themes = [false, true]
            .map(|_| cx.new(|cx| SelectState::new(Vec::<ThemeName>::new(), None, window, cx)));
        let font_inputs: Vec<_> = (0..10)
            .map(|_| cx.new(|cx| InputState::new(window, cx)))
            .collect();
        let choices = [Processing::Steps, Processing::Grouped, Processing::Hidden];
        let choices: Vec<Steps> = choices.into_iter().map(Steps).collect();
        let processing = cx.new(|cx| SelectState::new(choices, None, window, cx));
        let shell = cx.new(|cx| InputState::new(window, cx).placeholder("System default"));
        let repo = cx.new(|cx| InputState::new(window, cx).placeholder("owner/repo"));
        let command = cx.new(|cx| InputState::new(window, cx));
        let mut subscriptions = vec![
            cx.observe(&search, |_, _, cx| cx.notify()),
            cx.subscribe(
                &theme,
                |this, _, SelectEvent::Confirm(choice): &SelectEvent<Vec<Theme>>, cx| {
                    if let Some(choice) = choice
                        && *choice != this.data.settings.theme.mode
                    {
                        this.change(Change::ThemeMode(*choice), cx);
                    }
                },
            ),
            cx.subscribe(
                &processing,
                |this, _, SelectEvent::Confirm(choice): &SelectEvent<Vec<Steps>>, cx| {
                    if let Some(choice) = choice
                        && *choice != this.data.settings.conversation.processing
                    {
                        this.change(Change::Processing(*choice), cx);
                    }
                },
            ),
        ];
        for (dark, select) in themes.iter().enumerate() {
            subscriptions.push(cx.subscribe(
                select,
                move |this, _, SelectEvent::Confirm(choice): &SelectEvent<Vec<ThemeName>>, cx| {
                    let dark = dark == 1;
                    if let Some(name) = choice
                        && *name != this.picked_theme(dark)
                    {
                        let name = Some(name.clone());
                        this.change(Change::ThemeName { dark, name }, cx);
                    }
                },
            ));
        }
        for (ix, input) in font_inputs.iter().enumerate() {
            subscriptions.push(cx.subscribe(input, move |this, _, event: &InputEvent, cx| {
                match event {
                    InputEvent::Change => this.font_editing[ix] = true,
                    InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit_font(ix, cx),
                    _ => {}
                }
            }));
        }
        for (ix, input) in [&shell, &repo].into_iter().enumerate() {
            subscriptions.push(cx.subscribe(input, move |this, _, event: &InputEvent, cx| {
                match event {
                    InputEvent::Change => this.editing[ix] = true,
                    InputEvent::PressEnter { .. } | InputEvent::Blur => this.commit(ix, cx),
                    _ => {}
                }
            }));
        }
        subscriptions.push(
            cx.subscribe(&command, |this, _, event: &InputEvent, cx| match event {
                InputEvent::Change => this.editing[2] = true,
                InputEvent::PressEnter { .. } => this.commit_command(cx),
                _ => {}
            }),
        );
        let mut this = Self {
            section,
            maximized: false,
            data,
            search,
            theme,
            themes,
            font_inputs,
            font_editing: [false; 10],
            font_hints: vec![String::new(); 10],
            processing,
            models: BTreeMap::new(),
            shell,
            repo,
            command,
            advanced: None,
            editing: [false; 3],
            stale: true,
            claude: None,
            busy: false,
            mcp_check: Default::default(),
            toggled: HashSet::new(),
            _subscriptions: subscriptions,
        };
        this.sync(window, cx);
        this
    }

    /// Shows `data` (a reload of the file): a field being edited keeps its text.
    pub fn set(&mut self, data: SettingsData, cx: &mut Context<Self>) {
        self.data = data;
        self.stale = true;
        cx.notify();
    }

    /// Shows `section`'s page with an empty search.
    pub fn show(&mut self, section: Section, window: &mut Window, cx: &mut Context<Self>) {
        self.section = section;
        let placeholder = format!("Search {}…", section.title().to_lowercase());
        self.search.update(cx, |s, cx| {
            s.set_value("", window, cx);
            s.set_placeholder(placeholder, window, cx);
        });
        cx.notify();
    }

    pub(crate) fn maximized(&self) -> bool {
        self.maximized
    }

    pub(crate) fn context(&self) -> &str {
        &self.data.context
    }

    /// The count the nav shows beside `section`.
    pub(crate) fn nav_count(&self, section: Section, cx: &App) -> Option<usize> {
        match section {
            Section::Ai => Some(self.data.agents.len()),
            Section::Extensions => {
                (self.data.extensions.as_ref()).map(|ext| ext.read(cx).store().list().len())
            }
            Section::Plugins => self.claude.as_ref().map(|d| d.plugins.len()),
            Section::Mcp => self.claude.as_ref().map(|d| d.mcp.len()),
            Section::Skills => self.claude.as_ref().map(|d| d.skills.len()),
            Section::Instructions => self.claude.as_ref().map(|d| d.instructions.len()),
            Section::Agents => self.claude.as_ref().map(|d| d.agents.len()),
            Section::Hooks => self.claude.as_ref().map(|d| d.hooks.len()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub(crate) fn section(&self) -> Section {
        self.section
    }

    pub(crate) fn notice(&self) -> Option<SharedString> {
        self.data.notice.clone()
    }

    pub(crate) fn banner(&self) -> Option<String> {
        (self.data.error.as_ref()).map(|e| format!("settings.toml has an error: {e}"))
    }

    /// Says what the user changed, unless the file is broken: nothing is written over it.
    pub(crate) fn change(&mut self, change: Change, cx: &mut Context<Self>) {
        if self.data.error.is_none() {
            cx.emit(SettingsEvent::Change(change));
        }
    }

    /// The theme in effect for the mode: the one named in the settings, else the bundled one.
    fn picked_theme(&self, dark: bool) -> String {
        let theme = &self.data.settings.theme;
        match (dark, &theme.dark, &theme.light) {
            (true, Some(name), _) | (false, _, Some(name)) => name.clone(),
            (true, ..) => crate::appearance::BUNDLED_DARK.into(),
            _ => crate::appearance::BUNDLED_LIGHT.into(),
        }
    }

    /// Writes font input `ix` (2 per area: family, size); empty removes the key, a size that is
    /// not a number is dropped.
    fn commit_font(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.font_editing[ix]) {
            return;
        }
        let text = self.font_inputs[ix].read(cx).value().trim().to_string();
        let key = FONT_AREAS[ix / 2].1;
        let (saved_family, saved_size) = self.saved_font(key);
        let change = if ix.is_multiple_of(2) {
            let family = (!text.is_empty()).then_some(text);
            (family != saved_family).then_some(Change::FontFamily(key, family))
        } else {
            let size = match text.parse::<f32>() {
                Ok(size) if size.is_finite() => Some(size),
                _ if text.is_empty() => None,
                _ => {
                    // Not a number: show the saved size again.
                    self.stale = true;
                    cx.notify();
                    return;
                }
            };
            (size != saved_size).then_some(Change::FontSize(key, size))
        };
        if let Some(change) = change {
            self.change(change, cx);
        }
    }

    /// The family and size the settings set for `key`.
    fn saved_font(&self, key: FontKey) -> (Option<String>, Option<f32>) {
        let s = &self.data.settings;
        let (family, size) = match key {
            FontKey::Interface => (&s.interface.font_family, s.interface.font_size),
            FontKey::Editor => (&s.editor.font_family, s.editor.font_size),
            FontKey::Terminal => (&s.terminal.font_family, s.terminal.font_size),
            FontKey::Chat => (&s.conversation.font_family, s.conversation.font_size),
            FontKey::ChatCode => (
                &s.conversation.code_font_family,
                s.conversation.code_font_size,
            ),
        };
        (family.clone(), size)
    }

    fn commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.editing[ix]) {
            return;
        }
        let text = match ix {
            0 => path_field::value(&self.shell, cx),
            _ => self.repo.read(cx).value().trim().to_string(),
        };
        let value = (!text.is_empty()).then_some(text);
        self.change(
            if ix == 0 {
                Change::Shell(value)
            } else {
                Change::UpdatesRepo(value)
            },
            cx,
        );
    }

    /// The shell: typed or picked; a saved path that is gone shows in red.
    fn shell_field(&self, off: bool, cx: &mut Context<Self>) -> PathField {
        let saved = self
            .data
            .settings
            .terminal
            .shell
            .as_deref()
            .unwrap_or_default();
        let gone = saved.contains(['/', '\\']) && !Path::new(saved).exists();
        let this = cx.entity().downgrade();
        PathField::new("settings-shell-path", &self.shell, Pick::File)
            .disabled(off)
            .error(gone.then(|| "This file no longer exists.".into()))
            .on_change(move |_, cx| {
                this.update(cx, |this, cx| {
                    this.editing[0] = true;
                    this.commit(0, cx)
                })
                .ok();
            })
    }

    /// The run goes through the reload, which asks for approval.
    fn commit_command(&mut self, cx: &mut Context<Self>) {
        self.editing[2] = false;
        let command = self.command.read(cx).value().trim().to_string();
        if let Some(agent) = self.advanced.clone()
            && !command.is_empty()
        {
            self.change(Change::AgentCommand { agent, command }, cx);
        }
    }

    fn toggle_advanced(&mut self, id: &str, cx: &mut Context<Self>) {
        self.advanced = (self.advanced.as_deref() != Some(id)).then(|| id.to_string());
        self.editing[2] = false;
        self.stale = true;
        cx.notify();
    }

    /// The theme selects list what the catalog loaded now.
    fn sync_themes(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !cx.has_global::<ThemeCatalog>() {
            return;
        }
        for (dark, select) in self.themes.clone().into_iter().enumerate() {
            let mode = if dark == 1 {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            let names = ThemeCatalog::global(cx).names(mode);
            let items = names.iter().map(|n| ThemeName(n.to_string())).collect();
            let picked = self.picked_theme(dark == 1);
            select.update(cx, |s, cx| {
                s.set_items(items, window, cx);
                s.set_selected_value(&picked, window, cx);
            });
        }
    }

    /// Each font field shows what the settings set, and the value in effect as its placeholder.
    fn sync_fonts(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let faces = cx.try_global::<Fonts>().map(|f| f.0.clone());
        for (area, (_, key, _)) in FONT_AREAS.iter().enumerate() {
            let (family, size) = self.saved_font(*key);
            let effective = faces.as_ref().map(|f| {
                let face = [&f.ui, &f.editor, &f.terminal, &f.chat, &f.chat_code][area];
                (face.family.clone(), face.size.to_string())
            });
            let (family_hint, size_hint) = effective.unwrap_or_default();
            let shown = [
                family.unwrap_or_default(),
                size.map(|s| s.to_string()).unwrap_or_default(),
            ];
            for (ix, (text, hint)) in [(0, (&shown[0], family_hint)), (1, (&shown[1], size_hint))] {
                let ix = area * 2 + ix;
                let (input, text) = (self.font_inputs[ix].clone(), text.clone());
                let editing = self.font_editing[ix];
                self.font_hints[ix] = hint.clone();
                input.update(cx, |i, cx| {
                    i.set_placeholder(hint, window, cx);
                    if !editing {
                        i.set_value(text, window, cx);
                    }
                });
            }
        }
    }

    fn sync_models(&mut self, row: &AgentRow, window: &mut Window, cx: &mut Context<Self>) {
        let items = model_choices(row);
        let current = row.default_model.clone().unwrap_or_default();
        let select = match self.models.get(&row.id) {
            Some(select) => {
                select.update(cx, |s, cx| s.set_items(items, window, cx));
                select.clone()
            }
            None => {
                let select = cx.new(|cx| SelectState::new(items, None, window, cx));
                let agent = row.id.clone();
                self._subscriptions.push(cx.subscribe(
                    &select,
                    move |this, _, event: &SelectEvent<Vec<ModelChoice>>, cx| {
                        let SelectEvent::Confirm(Some(id)) = event else {
                            return;
                        };
                        let model = (!id.is_empty()).then(|| id.clone());
                        if model != this.data.settings.models.get(&agent).cloned() {
                            this.change(
                                Change::Model {
                                    agent: agent.clone(),
                                    model,
                                },
                                cx,
                            );
                        }
                    },
                ));
                self.models.insert(row.id.clone(), select.clone());
                select
            }
        };
        select.update(cx, |s, cx| s.set_selected_value(&current, window, cx));
    }

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stale = false;
        let settings = &self.data.settings;
        let theme = settings.theme.mode;
        self.theme
            .update(cx, |s, cx| s.set_selected_value(&theme, window, cx));
        let processing = settings.conversation.processing;
        self.processing
            .update(cx, |s, cx| s.set_selected_value(&processing, window, cx));
        self.sync_themes(window, cx);
        self.sync_fonts(window, cx);
        for row in self.data.agents.clone() {
            self.sync_models(&row, window, cx);
        }
        let settings = &self.data.settings;
        let texts = [
            settings.terminal.shell.clone(),
            settings.updates.repo.clone(),
        ];
        for (ix, input) in [self.shell.clone(), self.repo.clone()]
            .into_iter()
            .enumerate()
        {
            if !self.editing[ix] {
                let text = texts[ix].clone().unwrap_or_default();
                match ix {
                    0 => path_field::set(&input, &text, window, cx),
                    _ => input.update(cx, |i, cx| i.set_value(text, window, cx)),
                }
            }
        }
        let advanced =
            (self.advanced.as_ref()).and_then(|id| self.data.agents.iter().find(|a| &a.id == id));
        if let Some(row) = advanced
            && !self.editing[2]
        {
            let text = row.command.clone();
            (self.command).update(cx, |i, cx| i.set_value(text, window, cx));
        }
    }

    fn row(
        &self,
        label: &'static str,
        hint: &'static str,
        selector: &'static str,
        control: impl IntoElement,
        cx: &App,
    ) -> Div {
        setting(named(label, Some(hint.into()), cx), control)
            .debug_selector(move || selector.into())
    }

    /// A card: the radio, status and Test, then the agent's own lines indented under it.
    fn agent_row(&self, row: &AgentRow, cx: &mut Context<Self>) -> Div {
        let id = row.id.clone();
        let test = row.id.clone();
        let (off, muted) = (self.data.error.is_some(), cx.theme().muted_foreground);
        let login = builtin(&row.id).map(|a| a.login);
        let title = match &row.extension {
            Some(ext) => format!("{} · {ext}", row.label),
            None => row.label.clone(),
        };
        let install = builtin(&row.id)
            .filter(|_| row.status == RowStatus::CliNotFound)
            .map(|a| div().text_color(muted).child(a.install));
        let warn = warning(row).map(|w| div().text_color(cx.theme().warning).child(w));
        let selector = format!("settings-agent-{}", row.id);
        let test_selector = format!("settings-test-{}", row.id);
        // Under the name, past the radio.
        let indent = px(24.);
        let adapter = row.adapter.as_ref().map(|a| {
            let selector = format!("settings-adapter-{}", row.id);
            div()
                .debug_selector(move || selector)
                .pl(indent)
                .text_size(fonts::ui(cx, 12.))
                .text_color(muted)
                .child(format!("Adapter {a}"))
        });
        let adapter_note = row.adapter_note.as_ref().map(|note| {
            let selector = format!("settings-adapter-note-{}", row.id);
            div()
                .debug_selector(move || selector)
                .pl(indent)
                .text_size(fonts::ui(cx, 12.))
                .text_color(muted)
                .child(note.clone())
        });
        let tone = match row.status {
            RowStatus::Ready => cx.theme().success,
            RowStatus::SignedOut => cx.theme().warning,
            RowStatus::Failed(_) | RowStatus::CliNotFound => cx.theme().danger,
            _ => muted,
        };
        let name = v_flex()
            .gap(px(2.))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        div()
                            .debug_selector(move || selector)
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(
                                Radio::new(SharedString::from(format!("agent-{}", row.id)))
                                    .label(title)
                                    .checked(row.is_default)
                                    .disabled(off)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.change(Change::DefaultAgent(id.clone()), cx)
                                    })),
                            ),
                    )
                    .children(
                        row.is_default
                            .then(|| aurora::pill(cx).h(px(20.)).child("Default")),
                    ),
            )
            .children(adapter)
            .children(adapter_note);
        let status = h_flex()
            .gap_2()
            .items_center()
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(div().size(px(7.)).rounded_full().bg(tone))
                    .child(div().text_color(tone).child(row.status.text(login))),
            )
            .child(
                div().debug_selector(move || test_selector).child(
                    Button::new(SharedString::from(format!("test-{}", row.id)))
                        .small()
                        .label("Test")
                        .on_click(cx.listener(move |_, _, _, cx| {
                            cx.emit(SettingsEvent::Test(test.clone()))
                        })),
                ),
            );
        let mut lines = vec![setting(name, status)];
        lines.extend(warn.map(|w| sub_setting(w, div(), cx)));
        lines.extend(install.map(|i| sub_setting(i, div(), cx)));
        lines.push(self.default_model(row, cx));
        if row.extension.is_none() {
            lines.extend(self.advanced(row, cx));
        }
        card(lines, cx)
    }

    fn default_model(&self, row: &AgentRow, cx: &mut Context<Self>) -> Div {
        let selector = format!("settings-model-{}", row.id);
        let control = match (row.models.is_empty(), self.models.get(&row.id)) {
            (false, Some(select)) => field(campo::field(
                Select::new(select)
                    .small()
                    .disabled(self.data.error.is_some()),
                false,
                cx,
            )),
            _ => field(
                div()
                    .text_size(fonts::ui(cx, 12.))
                    .text_color(cx.theme().muted_foreground)
                    .child("Start a session or press Test to see the models."),
            ),
        };
        sub_setting(div().child("Default model"), control, cx).debug_selector(move || selector)
    }

    /// Folded: the command, which Enter asks to run.
    fn advanced(&self, row: &AgentRow, cx: &mut Context<Self>) -> Vec<Div> {
        let open = self.advanced.as_deref() == Some(row.id.as_str());
        let id = row.id.clone();
        let muted = cx.theme().muted_foreground;
        let toggle = h_flex()
            .id(ElementId::Name(format!("advanced-{}", row.id).into()))
            .gap_1()
            .items_center()
            .cursor_pointer()
            .text_size(fonts::ui(cx, 12.5))
            .text_color(muted)
            .hover(|el| el.text_color(cx.theme().foreground))
            .child(
                gpui_kit::component::Icon::new(if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                })
                .with_size(px(14.)),
            )
            .child("Advanced")
            .on_click(cx.listener(move |this, _, _, cx| this.toggle_advanced(&id, cx)))
            .test_support();
        let mut lines = vec![sub_setting(h_flex().child(toggle), div(), cx)];
        if open {
            let selector = format!("settings-command-{}", row.id);
            let command = h_flex()
                .debug_selector(move || selector)
                .gap(px(24.))
                .items_center()
                .child(div().w(px(110.)).flex_none().child("Command"))
                .child(
                    div().flex_1().child(campo::field(
                        Input::new(&self.command)
                            .small()
                            .disabled(self.data.error.is_some()),
                        false,
                        cx,
                    )),
                );
            lines.push(sub_fill(command, cx));
        }
        lines
    }

    /// The version and what the last check found on the left; the one button that fits it.
    fn version_row(&self, cx: &mut Context<Self>) -> Div {
        let status = &self.data.update;
        let (label, page) = match status {
            UpdateStatus::Ready(_) => ("Restart now", None),
            UpdateStatus::Available { release, .. } => {
                ("Open release page", Some(release.page.clone()))
            }
            _ => ("Check now", None),
        };
        let ready = matches!(status, UpdateStatus::Ready(_));
        let busy = matches!(status, UpdateStatus::Checking);
        let button = Button::new("settings-update-action")
            .label(label)
            .disabled(busy)
            .on_click(cx.listener(move |_, _, _, cx| match &page {
                Some(page) => cx.open_url(page),
                None if ready => cx.emit(SettingsEvent::RestartToUpdate),
                None => cx.emit(SettingsEvent::CheckUpdates),
            }));
        // A blocked copy opens the release page, but "turn on admin rights and install the next
        // update from here" needs a way to look again without reopening wai.
        let blocked_page = matches!(status, UpdateStatus::Available { why: Some(_), .. });
        let check = blocked_page.then(|| {
            Button::new("settings-update-check")
                .label("Check now")
                .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::CheckUpdates)))
        });
        let muted = cx.theme().muted_foreground;
        let color = |status: &UpdateStatus| match status {
            UpdateStatus::Failed(_) => cx.theme().danger,
            _ => muted,
        };
        let text = |id: &'static str, line: String, color| {
            div()
                .id(id)
                .aria_label(line.clone())
                .test_support()
                .text_size(fonts::ui(cx, 12.5))
                .text_color(color)
                .child(line)
        };
        let status_line = status
            .line()
            .map(|line| text("settings-update-status", line, color(status)));
        // `Available` already carries the sentence; otherwise it shows on its own.
        let shown = matches!(
            status,
            UpdateStatus::Available { why: Some(_), .. } | UpdateStatus::Ready(_)
        );
        let blocker = (self.data.update_blocker.clone())
            .filter(|_| !shown)
            .map(|why| text("settings-update-blocker", why, muted));
        let left = named(
            "Version",
            Some(format!("wai {}", crate::bars::version()).into()),
            cx,
        )
        .children(status_line)
        .children(blocker);
        setting(left, h_flex().gap_2().children(check).child(button))
            .debug_selector(|| "settings-update".into())
    }

    /// An area's family and size fields.
    fn font_row(&self, label: &str, off: bool, cx: &App) -> Div {
        let area = FONT_AREAS
            .iter()
            .position(|(l, ..)| *l == label)
            .unwrap_or(0);
        let id = FONT_AREAS[area].2;
        let (family, size) = (&self.font_inputs[area * 2], &self.font_inputs[area * 2 + 1]);
        let controls = h_flex()
            .gap_2()
            .child(field(campo::field(
                Input::new(family).small().disabled(off),
                false,
                cx,
            )))
            .child(div().w(px(80.)).child(campo::field(
                Input::new(size).small().disabled(off),
                false,
                cx,
            )));
        let hint = "Family, then size. Empty uses the default.";
        let selector = format!("settings-font-{id}");
        setting(named(label.to_string(), Some(hint.into()), cx), controls)
            .debug_selector(move || selector.clone())
    }

    fn rows(&self, label: &str, cx: &mut Context<Self>) -> Div {
        let off = self.data.error.is_some();
        match label {
            "Mode" => self.row(
                "Mode",
                "System follows macOS, switching with it.",
                "settings-theme",
                field(campo::field(
                    Select::new(&self.theme).small().disabled(off),
                    false,
                    cx,
                )),
                cx,
            ),
            "Light theme" => self.row(
                "Light theme",
                "Used in light mode.",
                "settings-theme-light",
                field(campo::field(
                    Select::new(&self.themes[0]).small().disabled(off),
                    false,
                    cx,
                )),
                cx,
            ),
            "Dark theme" => self.row(
                "Dark theme",
                "Used in dark mode.",
                "settings-theme-dark",
                field(campo::field(
                    Select::new(&self.themes[1]).small().disabled(off),
                    false,
                    cx,
                )),
                cx,
            ),
            "Themes folder" => self.row(
                "Themes folder",
                "Theme files saved there (*.json) join the lists above.",
                "settings-themes-folder",
                Button::new("settings-themes-open")
                    .label("Open themes folder")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenThemes))),
                cx,
            ),
            "Colors" => self.row(
                "Colors",
                "Override single colors in the [theme.overrides] table of settings.toml.",
                "settings-colors",
                Button::new("settings-colors-open")
                    .label("Open settings.toml")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
                cx,
            ),
            "Interface" | "Editor" | "Terminal" | "Chat" | "Chat code" => {
                self.font_row(label, off, cx)
            }
            "Processing" => self.row(
                "Processing",
                "How the agent's steps show while it works.",
                "settings-processing",
                field(campo::field(
                    Select::new(&self.processing).small().disabled(off),
                    false,
                    cx,
                )),
                cx,
            ),
            "Shell" => self.row(
                "Shell",
                "Leave it empty for the system default.",
                "settings-shell",
                field(self.shell_field(off, cx)),
                cx,
            ),
            "Automatic updates" => self.row(
                "Automatic updates",
                "Look for a new release each time wai opens.",
                "settings-updates-check",
                Switch::new("settings-updates-switch")
                    .checked(self.data.settings.updates.check)
                    .disabled(off)
                    .on_click(cx.listener(|this, on: &bool, _, cx| {
                        this.change(Change::UpdatesCheck(*on), cx)
                    })),
                cx,
            ),
            "Version" => self.version_row(cx),
            "Diagnostics" => self.row(
                "Diagnostics",
                "What wai and its agents report, ready to paste in an issue.",
                "settings-diagnostics",
                Button::new("settings-diagnostics-copy")
                    .label("Copy diagnostics")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::CopyDiagnostics))),
                cx,
            ),
            _ => self.row(
                "Repository",
                "The GitHub repository wai's releases come from.",
                "settings-updates-repo",
                field(campo::field(
                    Input::new(&self.repo).small().disabled(off),
                    false,
                    cx,
                )),
                cx,
            ),
        }
    }
}

impl Render for SettingsView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.stale {
            self.sync(window, cx);
        }
        let query = self.search.read(cx).value().trim().to_lowercase();
        let (warning, danger) = (cx.theme().warning, cx.theme().danger);
        let muted = cx.theme().muted_foreground;
        let dark = cx.theme().is_dark();
        let faint = if dark { aurora::nevoa_2(cx) } else { muted };
        let section = self.section;
        // Rows with the same card title share a card.
        let mut cards: Vec<(&str, Vec<Div>)> = Vec::new();
        for (_, title, label) in ROWS
            .iter()
            .filter(|(s, _, label)| *s == section && label.to_lowercase().contains(&query))
        {
            let row = self.rows(label, cx);
            match cards.last_mut() {
                Some((t, rows)) if t == title => rows.push(row),
                _ => cards.push((title, vec![row])),
            }
        }
        let cards: Vec<_> = (cards.into_iter())
            .map(|(title, rows)| titled(title, card(rows, cx), cx))
            .collect();
        let ai = section == Section::Ai;
        let extensions = section == Section::Extensions;
        let soon_page = section.is_soon();
        let claude_page = matches!(
            section,
            Section::Plugins
                | Section::Mcp
                | Section::Skills
                | Section::Instructions
                | Section::Agents
                | Section::Hooks
        );
        let agents: Vec<_> = if ai {
            (self.data.agents.iter())
                .filter(|a| a.label.to_lowercase().contains(&query))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let agents = (!agents.is_empty()).then(|| {
            titled(
                "Agents",
                v_flex()
                    .gap(px(10.))
                    .children(agents.iter().map(|a| self.agent_row(a, cx))),
                cx,
            )
        });
        let project = ai
            .then(|| self.data.project_default.as_deref().map(agent_label))
            .flatten()
            .map(|a| {
                div()
                    .text_color(muted)
                    .child(format!("This project uses {a}."))
            });
        let notice = ai
            .then(|| self.notice())
            .flatten()
            .map(|n| div().text_color(warning).child(n));
        let advanced = (ai && "settings.toml".contains(&query)).then(|| {
            let file = setting(
                named(
                    "settings.toml",
                    Some("Everything above, as a file".into()),
                    cx,
                ),
                Button::new("settings-open-file")
                    .label("Open")
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
            );
            titled("Advanced", card([file], cx), cx)
        });
        let soon = if dark { aurora::ambar(cx) } else { warning };
        let nav_fill = if dark {
            white().opacity(0.018)
        } else {
            cx.theme().muted.opacity(0.3)
        };
        let nav: Vec<_> = NAV
            .iter()
            .map(|(label, sections)| {
                v_flex()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap_1()
                            .px(px(10.))
                            .pt(px(14.))
                            .pb(px(6.))
                            .text_size(fonts::ui(cx, 11.))
                            .text_color(faint)
                            .children(
                                (*label == "Claude")
                                    .then(|| div().text_color(aurora::brasa(cx)).child("✳")),
                            )
                            .child(label.to_uppercase()),
                    )
                    .children(sections.iter().map(|&s| {
                        let count = self.nav_count(s, cx);
                        aurora::tab(s == section, cx)
                            .id(ElementId::Name(format!("settings-nav-{}", s.key()).into()))
                            .h(px(30.))
                            .px(px(10.))
                            .gap_2()
                            .items_center()
                            .rounded(px(8.))
                            .cursor_pointer()
                            .child(div().flex_1().min_w_0().child(s.title()))
                            .children(s.is_soon().then(|| {
                                div()
                                    .text_size(fonts::ui(cx, 10.))
                                    .text_color(soon)
                                    .child("Soon")
                            }))
                            .children(count.map(|n| {
                                div()
                                    .min_w(px(22.))
                                    .text_right()
                                    .text_color(faint)
                                    .child(n.to_string())
                            }))
                            .on_click(
                                cx.listener(move |this, _, window, cx| this.show(s, window, cx)),
                            )
                            .test_support()
                    }))
            })
            .collect();
        let banner = self.banner().map(|text| {
            h_flex()
                .mt_3()
                .justify_between()
                .items_center()
                .gap_4()
                .px_3()
                .py_2()
                .rounded(px(8.))
                .border_1()
                .border_color(danger.opacity(0.4))
                .bg(danger.opacity(0.08))
                .text_color(danger)
                .child(text)
                .child(
                    Button::new("settings-error-open")
                        .small()
                        .label("Open")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
                )
        });
        // The page's header stays put; only what is under it scrolls.
        let header = column(
            v_flex()
                .pt(px(28.))
                .pb(px(16.))
                .child(
                    h_flex()
                        .items_start()
                        .gap_4()
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .gap_1()
                                .child(
                                    div()
                                        .font_family(DISPLAY_FONT)
                                        .text_size(fonts::ui(cx, 24.))
                                        .font_weight(FontWeight::BOLD)
                                        .child(section.title()),
                                )
                                .child(div().text_color(muted).child(section.description())),
                        )
                        .children(self.claude_actions(section, cx)),
                )
                .children(banner)
                .children((!extensions && !soon_page).then(|| {
                    div().mt(px(14.)).child(campo::field(
                        Input::new(&self.search).small(),
                        false,
                        cx,
                    ))
                })),
        );
        let body = column(
            v_flex()
                .pt(px(4.))
                .pb(px(40.))
                .gap(px(22.))
                .children(notice)
                .children(project)
                .children(agents)
                .children(cards)
                .children(soon_page.then(|| {
                    v_flex()
                        .debug_selector(|| "settings-soon-page".into())
                        .items_center()
                        .gap_2()
                        .py(px(40.))
                        .child(aurora::soon(cx))
                        .child(
                            div()
                                .font_family(DISPLAY_FONT)
                                .text_size(fonts::ui(cx, 20.))
                                .font_weight(FontWeight::BOLD)
                                .child("Not in wai yet"),
                        )
                        .child(div().text_color(muted).child(soon_text(section)))
                }))
                .children(claude_page.then(|| self.claude_page(section, &query, cx)))
                .children(advanced)
                .children(extensions.then(|| self.data.extensions.clone()).flatten()),
        );
        v_flex()
            .size_full()
            .debug_selector(|| "settings-modal".into())
            .child(
                aurora::card_bar(cx)
                    .h(px(48.))
                    .px_4()
                    .gap_3()
                    .items_center()
                    .child(
                        div()
                            .id("settings-title")
                            .flex_none()
                            .font_family(DISPLAY_FONT)
                            .text_size(fonts::ui(cx, 14.))
                            .font_weight(FontWeight::BOLD)
                            .child("Settings")
                            .test_support(),
                    )
                    .child(
                        div()
                            .flex_1()
                            .overflow_hidden()
                            .text_color(muted)
                            .child(self.context().to_string()),
                    )
                    .child(
                        Button::new("settings-maximize")
                            .ghost()
                            .small()
                            .icon(IconName::Maximize)
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.maximized = !this.maximized;
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("settings-close")
                            .ghost()
                            .small()
                            .icon(IconName::Close)
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    ),
            )
            .child(
                // Stretched, not centered: the nav and the page are as tall as the modal and
                // scroll on their own.
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .items_stretch()
                    .child(
                        v_flex()
                            .id("settings-nav")
                            .w(px(232.))
                            .flex_none()
                            .overflow_y_scroll()
                            .pt(px(10.))
                            .px(px(10.))
                            .pb(px(16.))
                            .border_r_1()
                            .border_color(divider(cx))
                            .bg(nav_fill)
                            .children(nav),
                    )
                    .child(
                        v_flex().flex_1().min_w_0().child(header).child(
                            v_flex()
                                .id("settings-content")
                                .flex_1()
                                .min_h_0()
                                .overflow_y_scroll()
                                .child(body)
                                .test_support(),
                        ),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestQueryExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::{Change, Processing, Settings, ThemeChoice};

    use super::{RowStatus, Section, SelectEvent, SelectItem, SettingsData, SettingsView};
    use crate::workspace::tests::{click, fake_settings, open, question, wait_until};
    use crate::workspace::{Pane, Workspace};
    use gpui_kit::{Modifiers, point, px};

    fn settings_ws(
        cx: &mut TestAppContext,
    ) -> (Entity<Workspace>, &mut VisualTestContext, tempfile::TempDir) {
        let (workspace, cx) = open(cx, Settings::default());
        let dir = tempfile::tempdir().unwrap();
        workspace.update(cx, |w, _| w.user_dir = dir.path().into());
        (workspace, cx, dir)
    }

    fn view(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> Entity<SettingsView> {
        workspace
            .read_with(cx, |w, _| w.settings_view())
            .expect("the settings modal")
    }

    fn data(error: Option<&str>) -> SettingsData {
        SettingsData {
            settings: Settings::default(),
            error: error.map(Into::into),
            notice: None,
            context: String::new(),
            agents: Vec::new(),
            project_default: None,
            extensions: None,
            home: Default::default(),
            update: crate::update::UpdateStatus::Idle,
            update_blocker: None,
        }
    }

    fn fake_ws<'a>(
        cx: &'a mut TestAppContext,
        flag: &str,
    ) -> (
        Entity<Workspace>,
        &'a mut VisualTestContext,
        tempfile::TempDir,
    ) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let mut settings = Settings::default();
        settings.agents.extend(fake_settings(flag).agents);
        settings.default_agent = Some("fake".into());
        let (workspace, cx) = open(cx, settings);
        let dir = tempfile::tempdir().unwrap();
        workspace.update(cx, |w, _| w.user_dir = dir.path().into());
        (workspace, cx, dir)
    }

    fn row(workspace: &Entity<Workspace>, id: &str, cx: &mut VisualTestContext) -> super::AgentRow {
        workspace.read_with(cx, |w, cx| {
            w.agent_rows(cx).into_iter().find(|r| r.id == id).unwrap()
        })
    }

    #[gpui_kit::test]
    fn rows_list_builtins_and_user_agents(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "");
        let rows = workspace.read_with(cx, |w, cx| w.agent_rows(cx));
        let ids: Vec<_> = rows.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, ["antigravity", "claude", "codex", "copilot", "fake"]);
        assert!(rows[4].is_default && !rows[0].is_default);
        assert_eq!(rows[4].status, RowStatus::NotChecked);
        if wai_agent::program("agy acp").is_none() {
            assert_eq!(rows[0].status, RowStatus::CliNotFound);
        }
    }

    /// The row the screen shows, not the workspace's.
    fn shown(view: &Entity<SettingsView>, id: &str, cx: &mut VisualTestContext) -> RowStatus {
        view.read_with(cx, |v, _| {
            let row = v.data.agents.iter().find(|r| r.id == id).unwrap();
            row.status.clone()
        })
    }

    /// Choosing a default starts it; the open screen's row follows it to Ready.
    #[gpui_kit::test]
    fn row_status_follows_the_agent(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "");
        let mut chosen = workspace.read_with(cx, |w, _| w.settings.clone());
        chosen.default_agent = Some("fake".into());
        workspace.update(cx, |w, _| w.settings.default_agent = None);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        let view = view(&workspace, cx);
        assert_eq!(shown(&view, "fake", cx), RowStatus::NotChecked);
        workspace.update_in(cx, |w, window, cx| {
            w.apply_settings(chosen, vec![], window, cx)
        });
        wait_until(cx, "Ready on the screen", |cx| {
            shown(&view, "fake", cx) == RowStatus::Ready
        });
    }

    /// A first run has no `settings.toml` yet: Open settings.toml creates it.
    #[gpui_kit::test]
    fn open_file_creates_a_missing_settings_toml(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        let view = view(&workspace, cx);
        view.update(cx, |_, cx| cx.emit(super::SettingsEvent::OpenFile));
        let path = crate::sidebar::canonical(&dir.path().join("settings.toml"));
        wait_until(cx, "the editor tab", |cx| {
            workspace.read_with(cx, |w, cx| {
                w.panes.iter().any(|p| match p {
                    Pane::Editor(pane) => {
                        (pane.read(cx).tabs.iter()).any(|tab| tab.read(cx).path == path)
                    }
                    _ => false,
                })
            })
        });
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
        assert!(workspace.read_with(cx, |w, _| w.settings_view().is_none()));
        assert!(cx.update(|window, cx| window.notifications(cx).is_empty()));
    }

    #[gpui_kit::test]
    fn settings_button_copies_diagnostics(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Updates, None, window, cx)
        });
        click("settings-diagnostics-copy", cx);
        wait_until(cx, "the clipboard", |cx| cx.read_from_clipboard().is_some());
        let text = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
        assert!(text.contains("\nAgents\n"));
    }

    #[gpui_kit::test]
    fn test_learns_ready_and_models(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "--models");
        workspace.update_in(cx, |w, window, cx| w.test_agent("fake", window, cx));
        wait_until(cx, "the models", |cx| {
            workspace.read_with(cx, |w, _| w.models_seen.contains_key("fake"))
        });
        assert_eq!(row(&workspace, "fake", cx).status, RowStatus::Ready);
        let seen = workspace.read_with(cx, |w, _| w.models_seen["fake"].clone());
        let ids: Vec<_> = seen.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["fake-small", "fake-large"]);
    }

    #[gpui_kit::test]
    fn settings_lists_seen_models(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "--models");
        workspace.update_in(cx, |w, window, cx| {
            w.start_session("fake".into(), None, false, None, window, cx)
        });
        wait_until(cx, "the session", |cx| {
            workspace.read_with(cx, |w, _| w.panes[0].session().is_some())
        });
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-model-fake").is_some());
        let names: Vec<_> = super::model_choices(&row(&workspace, "fake", cx))
            .iter()
            .map(|c| c.name.clone())
            .collect();
        assert_eq!(names, ["Agent's choice", "Fake Small", "Fake Large"]);
    }

    #[gpui_kit::test]
    fn test_reports_signed_out(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "--require-auth");
        workspace.update_in(cx, |w, window, cx| w.test_agent("fake", window, cx));
        wait_until(cx, "SignedOut", |cx| {
            row(&workspace, "fake", cx).status == RowStatus::SignedOut
        });
        assert_eq!(RowStatus::SignedOut.text(None), "Not signed in");
        assert_eq!(
            RowStatus::SignedOut.text(Some("agy login")),
            "Not signed in · run agy login"
        );
    }

    #[gpui_kit::test]
    fn copilot_row_warns(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "");
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-agent-copilot").is_some());
        assert!(cx.debug_bounds("settings-test-fake").is_some());
        let copilot = row(&workspace, "copilot", cx);
        assert_eq!(
            super::warning(&copilot),
            Some("Copilot may edit files and run commands without asking.")
        );
        assert_eq!(super::warning(&row(&workspace, "claude", cx)), None);
    }

    #[gpui_kit::test]
    fn advanced_command_is_written_on_allow(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = fake_ws(cx, "");
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        cx.update(|window, cx| {
            // The page scrolls under its header; `fake` is the last card.
            let delta = gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-2000.)));
            window.scroll("settings-content", delta, cx);
            window.click("advanced-fake", cx);
        });
        cx.run_until_parked();
        let view = view(&workspace, cx);
        let input = view.read_with(cx, |v, _| v.command.clone());
        let shown = input.read_with(cx, |i, _| i.value().to_string());
        assert_eq!(shown, fake_settings("").agents["fake"].command);
        input.update_in(cx, |i, window, cx| i.focus(window, cx));
        cx.simulate_input("--reply other");
        cx.simulate_keystrokes("enter");
        assert!(question(&workspace, cx).is_some());
        click("approval-allow", cx);
        assert!(workspace.read_with(cx, |w, _| w.settings_view().is_some()));
        let (settings, _) = Settings::load(dir.path(), None).unwrap();
        let typed = fake_settings("--reply other").agents["fake"]
            .command
            .clone();
        assert_eq!(settings.agents["fake"].command, typed);
    }

    #[test]
    fn modal_size_follows_the_viewport() {
        let vp = gpui_kit::size(gpui_kit::px(1400.), gpui_kit::px(900.));
        assert_eq!(
            super::modal_size(vp, false),
            gpui_kit::size(gpui_kit::px(1232.), gpui_kit::px(774.))
        );
        assert_eq!(
            super::modal_size(vp, true),
            gpui_kit::size(gpui_kit::px(1376.), gpui_kit::px(876.))
        );
    }

    /// A page taller than the modal scrolls inside it: the dialog's own scroll never moves the
    /// nav with the page.
    #[gpui_kit::test]
    fn tall_page_keeps_the_modal_height(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "");
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        cx.update(|window, cx| window.render_frame(cx));
        let viewport = cx.update(|window, _| window.viewport_size());
        let modal = cx.debug_bounds("settings-modal").unwrap();
        assert!(
            modal.size.height <= super::modal_size(viewport, false).height,
            "{modal:?} in {viewport:?}"
        );
        // The page scrolls inside the modal instead of spilling past it.
        let page = cx.update(|window, _| window.find("settings-content").bounds());
        assert!(page.bottom() <= modal.bottom(), "{page:?} in {modal:?}");
    }

    /// However wide the modal, the rows keep to one 760 px column and their controls end on one
    /// edge.
    #[gpui_kit::test]
    fn rows_share_one_column(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Updates, None, window, cx)
        });
        click("settings-maximize", cx);
        cx.update(|window, cx| window.render_frame(cx));
        let rows = [
            "settings-updates-check",
            "settings-update",
            "settings-diagnostics",
            "settings-updates-repo",
        ]
        .map(|s| cx.debug_bounds(s).unwrap_or_else(|| panic!("{s}")));
        for row in &rows[1..] {
            assert_eq!(row.right(), rows[0].right());
            assert_eq!(row.size.width, rows[0].size.width);
        }
        assert!(rows[0].size.width <= px(696.), "{:?}", rows[0].size.width);
    }

    #[gpui_kit::test]
    fn ai_row_shows_the_adapter_version(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "");
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-adapter-fake").is_none());
        workspace.update_in(cx, |w, window, cx| w.test_agent("fake", window, cx));
        wait_until(cx, "the adapter version", |cx| {
            row(&workspace, "fake", cx).adapter.is_some()
        });
        cx.update(|window, cx| window.render_frame(cx));
        let version = env!("CARGO_PKG_VERSION");
        assert_eq!(
            row(&workspace, "fake", cx).adapter.as_deref(),
            Some(format!("wai-fake-agent {version}").as_str())
        );
        assert!(cx.debug_bounds("settings-adapter-fake").is_some());
    }

    #[gpui_kit::test]
    fn settings_shows_update_status(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Updates, None, window, cx)
        });
        let release = crate::update::Release {
            tag: "v9.0.0".into(),
            page: "https://github.com/Guilbritto/wai/releases/tag/v9.0.0".into(),
            assets: vec![],
        };
        let found = crate::update::UpdateStatus::Available {
            release,
            why: Some("x".into()),
        };
        workspace.update(cx, |w, cx| w.set_update(found, cx));
        cx.update(|window, cx| window.render_frame(cx));
        assert!(
            cx.debug_bounds("settings-update").is_some(),
            "the Version row"
        );
        cx.update(|window, _| window.find_by_label("wai 9.0.0 is available. x"));
        cx.update(|window, _| window.find_by_label("Open release page"));
        // Blocked: the page is not the only way on; Check now stays beside it.
        cx.update(|window, _| window.find_by_label("Check now"));
        // Up to date, but this copy cannot install the next update: the sentence still shows.
        workspace.update(cx, |w, cx| {
            w.update_blocker = Some("cannot update itself here".into());
            w.set_update(crate::update::UpdateStatus::UpToDate, cx)
        });
        cx.update(|window, cx| window.render_frame(cx));
        cx.update(|window, _| window.find_by_label("cannot update itself here"));
        // A staged build turns the same button into Restart now, which goes through the quit flow.
        let staged = crate::update::Staged {
            tag: "v9.0.0".into(),
            file: "/nonexistent/wai.zip".into(),
            sha256: "00".into(),
        };
        workspace.update(cx, |w, cx| {
            w.set_update(crate::update::UpdateStatus::Ready(staged), cx)
        });
        cx.update(|window, cx| window.render_frame(cx));
        cx.update(|window, _| window.find_by_label("Restart now"));
        click("settings-update-action", cx);
        let status = workspace.read_with(cx, |w, _| w.update.clone());
        assert!(
            matches!(status, crate::update::UpdateStatus::Failed(_)),
            "{status:?}"
        );
    }

    #[gpui_kit::test]
    fn cmd_comma_opens_one_settings_modal(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        cx.simulate_keystrokes("secondary-,");
        cx.simulate_keystrokes("secondary-,");
        workspace.read_with(cx, |w, _| {
            assert!(w.settings_view().is_some());
            assert_eq!(w.panes.len(), 1, "no pane for settings");
        });
        assert!(cx.update(|window, cx| window.has_active_dialog(cx)));
        cx.simulate_keystrokes("escape");
        assert!(workspace.read_with(cx, |w, _| w.settings_view().is_none()));
        assert!(!cx.update(|window, cx| window.has_active_dialog(cx)));
    }

    #[gpui_kit::test]
    fn close_button_and_outside_click_close_the_modal(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        let open = |cx: &mut VisualTestContext| {
            workspace.update_in(cx, |w, window, cx| {
                w.open_settings(Section::Ai, None, window, cx)
            });
            cx.run_until_parked();
        };
        let closed = |cx: &mut VisualTestContext| {
            workspace.read_with(cx, |w, _| w.settings_view().is_none())
        };
        open(cx);
        assert!(!closed(cx));
        click("settings-close", cx);
        assert!(closed(cx));
        open(cx);
        let viewport = cx.update(|window, _| window.viewport_size());
        cx.simulate_click(point(px(8.), viewport.height - px(8.)), Modifiers::none());
        cx.run_until_parked();
        assert!(closed(cx));
    }

    #[gpui_kit::test]
    fn maximize_grows_the_modal(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        cx.run_until_parked();
        let before = cx.debug_bounds("settings-modal").unwrap().size.width;
        click("settings-maximize", cx);
        cx.update(|window, cx| window.render_frame(cx));
        let view = view(&workspace, cx);
        assert!(view.read_with(cx, |v, _| v.maximized()));
        let after = cx.debug_bounds("settings-modal").unwrap().size.width;
        assert!(after > before, "{after:?} > {before:?}");
    }

    #[gpui_kit::test]
    fn title_names_the_agent_and_the_project(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = fake_ws(cx, "");
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        let project = workspace.read_with(cx, |w, _| w.project.clone());
        let name = crate::workspace::folder_name(&project);
        let shown = view(&workspace, cx).read_with(cx, |v, _| v.context().to_string());
        assert_eq!(shown, format!("(fake · {name})"));
    }

    #[gpui_kit::test]
    fn title_without_an_agent_is_the_project(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        let project = workspace.read_with(cx, |w, _| w.project.clone());
        let name = crate::workspace::folder_name(&project);
        let shown = view(&workspace, cx).read_with(cx, |v, _| v.context().to_string());
        assert_eq!(shown, format!("({name})"));
    }

    #[test]
    fn nav_groups_match_the_spec() {
        use Section::*;
        let want: &[(&str, &[Section])] = &[
            (
                "wai",
                &[Ai, Appearance, Conversation, Terminal, Updates, Extensions],
            ),
            (
                "Claude",
                &[Plugins, Mcp, Skills, Instructions, Agents, Hooks],
            ),
            ("Other agents", &[Codex, Copilot, Antigravity]),
        ];
        assert_eq!(super::NAV, want);
    }

    #[gpui_kit::test]
    fn other_agents_are_coming_soon(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        click("settings-nav-codex", cx);
        assert!(cx.debug_bounds("settings-soon-page").is_some());
        let view = view(&workspace, cx);
        assert_eq!(
            view.read_with(cx, |v, cx| v.nav_count(Section::Codex, cx)),
            None
        );
        assert_eq!(
            super::soon_text(Section::Codex),
            "Today wai shows what Claude uses. Codex comes next; its own settings stay in its CLI meanwhile."
        );
    }

    #[gpui_kit::test]
    fn nav_switches_pages_and_clears_the_search(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        let view = view(&workspace, cx);
        view.update_in(cx, |v, window, cx| {
            v.search.update(cx, |s, cx| s.set_value("x", window, cx));
        });
        cx.run_until_parked();
        click("settings-nav-terminal", cx);
        view.read_with(cx, |v, cx| {
            assert_eq!(v.section(), Section::Terminal);
            assert_eq!(v.search.read(cx).value(), "");
        });
        assert!(cx.debug_bounds("settings-shell").is_some());
    }

    #[gpui_kit::test]
    fn theme_change_writes_the_user_file(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.change_setting(Change::ThemeMode(ThemeChoice::Light), window, cx)
        });
        let text = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert!(text.contains("[theme]\nmode = \"light\""), "{text}");
    }

    /// Conversation has the processing mode: its three choices, and a pick is written.
    #[gpui_kit::test]
    fn processing_pick_writes_the_user_file(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Conversation, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-processing").is_some());
        let view = view(&workspace, cx);
        let picked = view.read_with(cx, |v, cx| v.processing.read(cx).selected_value().copied());
        assert_eq!(picked, Some(Processing::Steps));
        let titles = [Processing::Steps, Processing::Grouped, Processing::Hidden]
            .map(|mode| SelectItem::title(&super::Steps(mode)).to_string());
        assert_eq!(titles, ["Step by step", "Grouped", "Hidden"]);

        view.update_in(cx, |v, window, cx| {
            v.processing.update(cx, |s, cx| {
                s.set_selected_value(&Processing::Grouped, window, cx);
                cx.emit(SelectEvent::Confirm(Some(Processing::Grouped)));
            })
        });
        cx.run_until_parked();
        let text = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert_eq!(text, "[conversation]\nprocessing = \"grouped\"\n");
    }

    #[gpui_kit::test]
    fn search_filters_the_page(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Updates, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-updates-check").is_some());
        let view = view(&workspace, cx);
        view.update_in(cx, |v, window, cx| {
            v.search.update(cx, |s, cx| s.set_value("repo", window, cx));
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-updates-repo").is_some());
        assert!(cx.debug_bounds("settings-updates-check").is_none());
    }

    #[gpui_kit::test]
    fn broken_file_makes_the_screen_read_only(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        let view = view(&workspace, cx);
        view.update(cx, |v, cx| v.set(data(Some("x")), cx));
        view.read_with(cx, |v, _| {
            assert_eq!(v.banner().unwrap(), "settings.toml has an error: x")
        });
        view.update(cx, |v, cx| {
            v.change(Change::ThemeMode(ThemeChoice::Light), cx)
        });
        cx.run_until_parked();
        assert!(!dir.path().join("settings.toml").exists());
    }

    #[gpui_kit::test]
    fn updates_switch_is_off_under_the_error(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Updates, None, window, cx)
        });
        let view = view(&workspace, cx);
        view.update(cx, |v, cx| v.set(data(Some("x")), cx));
        cx.update(|window, cx| {
            window.render_frame(cx);
            assert_eq!(
                window.find("settings-updates-switch").disabled(),
                Some(true)
            );
        });
    }

    #[gpui_kit::test]
    fn reload_keeps_a_field_being_edited(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Terminal, None, window, cx)
        });
        cx.run_until_parked();
        let view = view(&workspace, cx);
        let shell = view.read_with(cx, |v, _| v.shell.clone());
        shell.update_in(cx, |s, window, cx| s.focus(window, cx));
        cx.simulate_input("/bin/zsh");
        let mut reloaded = data(None);
        reloaded.settings.terminal.shell = Some("/bin/fish".into());
        view.update(cx, |v, cx| v.set(reloaded, cx));
        cx.run_until_parked();
        // Typed after the shell the harness configured.
        let text = shell.read_with(cx, |s, _| s.value());
        assert!(
            text.ends_with("/bin/zsh") && !text.contains("fish"),
            "{text}"
        );
    }

    #[gpui_kit::test]
    fn ai_row_shows_the_adapter_note(cx: &mut TestAppContext) {
        let mut settings = Settings::default();
        settings.agents.insert(
            "npx".into(),
            wai_settings::AgentConfig {
                command: "npx -y @a/b@1.0.0".into(),
            },
        );
        let (workspace, cx) = open(cx, settings);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Ai, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-adapter-note-npx").is_none());
        workspace.update(cx, |w, cx| {
            w.registry
                .update(cx, |r, _| r.set_latest("@a/b".into(), Some("1.1.0".into())));
            w.sync_settings_view(cx);
        });
        cx.update(|window, cx| window.render_frame(cx));
        assert_eq!(
            row(&workspace, "npx", cx).adapter_note.as_deref(),
            Some("1.1.0 is out (wai runs 1.0.0)")
        );
        assert!(cx.debug_bounds("settings-adapter-note-npx").is_some());
    }

    /// A user theme file in the config folder, loaded by an apply with `dark = "Mine"` chosen.
    fn with_user_theme(
        workspace: &Entity<Workspace>,
        dir: &tempfile::TempDir,
        cx: &mut VisualTestContext,
    ) {
        std::fs::create_dir(dir.path().join("themes")).unwrap();
        std::fs::write(
            dir.path().join("themes/mine.json"),
            r#"{"name":"set","themes":[{"name":"Mine","mode":"dark"},{"name":"Mine Light","mode":"light"}]}"#,
        )
        .unwrap();
        let mut settings = Settings::default();
        settings.theme.dark = Some("Mine".into());
        workspace.update_in(cx, |w, window, cx| {
            w.apply_settings(settings, vec![], window, cx)
        });
        cx.run_until_parked();
    }

    #[gpui_kit::test]
    fn appearance_lists_modes_and_themes(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        cx.run_until_parked();
        for id in [
            "settings-theme",
            "settings-theme-light",
            "settings-theme-dark",
            "settings-themes-folder",
        ] {
            assert!(cx.debug_bounds(id).is_some(), "{id}");
        }
        let titles = [ThemeChoice::System, ThemeChoice::Light, ThemeChoice::Dark]
            .map(|mode| SelectItem::title(&super::Theme(mode)).to_string());
        assert_eq!(titles, ["System", "Light", "Dark"]);
        let view = view(&workspace, cx);
        let picked = |view: &Entity<SettingsView>, cx: &mut VisualTestContext| {
            view.read_with(cx, |v, cx| {
                (
                    v.themes[0].read(cx).selected_value().cloned(),
                    v.themes[1].read(cx).selected_value().cloned(),
                )
            })
        };
        // Only the bundled themes load: each select shows its bundled one.
        assert_eq!(
            picked(&view, cx),
            (Some("wai Aurora Light".into()), Some("wai Aurora".into()))
        );
        // A theme from the folder joins the list: it can be the selected one.
        with_user_theme(&workspace, &dir, cx);
        assert_eq!(picked(&view, cx).1, Some("Mine".into()));
    }

    #[gpui_kit::test]
    fn picking_a_theme_writes_settings(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        let view = view(&workspace, cx);
        with_user_theme(&workspace, &dir, cx);
        view.update_in(cx, |v, window, cx| {
            v.theme.update(cx, |s, cx| {
                cx.emit(SelectEvent::Confirm(Some(ThemeChoice::Light)));
                s.set_selected_value(&ThemeChoice::Light, window, cx);
            });
            v.themes[0].update(cx, |_, cx| {
                cx.emit(SelectEvent::Confirm(Some("Mine Light".to_string())))
            });
        });
        cx.run_until_parked();
        let text = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert_eq!(text, "[theme]\nmode = \"light\"\nlight = \"Mine Light\"\n");
    }

    #[gpui_kit::test]
    fn open_themes_folder_creates_it(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        assert!(!dir.path().join("themes").exists());
        click("settings-themes-open", cx);
        assert!(dir.path().join("themes").is_dir());
    }

    #[gpui_kit::test]
    fn colors_row_opens_settings_toml(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-colors").is_some());
        click("settings-colors-open", cx);
        let path = crate::sidebar::canonical(&dir.path().join("settings.toml"));
        wait_until(cx, "the editor tab", |cx| {
            workspace.read_with(cx, |w, cx| {
                w.panes.iter().any(|p| match p {
                    Pane::Editor(pane) => {
                        (pane.read(cx).tabs.iter()).any(|tab| tab.read(cx).path == path)
                    }
                    _ => false,
                })
            })
        });
        assert!(path.exists());
    }

    #[gpui_kit::test]
    fn fonts_group_shows_areas_with_placeholders(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        cx.run_until_parked();
        for id in [
            "settings-font-interface",
            "settings-font-editor",
            "settings-font-terminal",
            "settings-font-chat",
            "settings-font-chat-code",
        ] {
            assert!(cx.debug_bounds(id).is_some(), "{id}");
        }
        let view = view(&workspace, cx);
        let shown = |cx: &mut VisualTestContext| {
            view.read_with(cx, |v, cx| {
                v.font_inputs
                    .iter()
                    .map(|i| i.read(cx).value().to_string())
                    .zip(v.font_hints.clone())
                    .collect::<Vec<_>>()
            })
        };
        let faces = cx.update(|_, cx| cx.global::<crate::appearance::Fonts>().0.clone());
        let shown_now = shown(cx);
        for (area, face) in [
            &faces.ui,
            &faces.editor,
            &faces.terminal,
            &faces.chat,
            &faces.chat_code,
        ]
        .into_iter()
        .enumerate()
        {
            // Unset: empty, with the family and size in effect as placeholders.
            assert_eq!(shown_now[area * 2], (String::new(), face.family.clone()));
            assert_eq!(
                shown_now[area * 2 + 1],
                (String::new(), face.size.to_string())
            );
        }
        // A set size shows as the value, and is the one in effect.
        let mut settings = Settings::default();
        settings.interface.font_size = Some(20.);
        workspace.update_in(cx, |w, window, cx| {
            w.apply_settings(settings, vec![], window, cx)
        });
        cx.update(|window, cx| window.render_frame(cx));
        assert_eq!(shown(cx)[1], ("20".to_string(), "20".to_string()));
    }

    #[gpui_kit::test]
    fn font_size_field_writes_settings(cx: &mut TestAppContext) {
        let (workspace, cx, dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        cx.run_until_parked();
        let view = view(&workspace, cx);
        let input = view.read_with(cx, |v, _| v.font_inputs[1].clone());
        input.update_in(cx, |i, window, cx| i.focus(window, cx));
        cx.simulate_input("18");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        let text = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert_eq!(text, "[interface]\nfont_size = 18.0\n");
    }
}
