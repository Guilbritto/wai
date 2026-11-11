//! The Settings modal: a section nav, a search over the page and the rows of the sections. It
//! shows what the workspace gives it (`set`) and says what the user changed (`SettingsEvent`);
//! the workspace writes the user's file.

use std::collections::BTreeMap;

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogEntrance;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::*;
use wai_settings::{Change, Processing, SeenModel, Settings, ThemeChoice, agent_label, builtin};

use crate::aurora::{self, DISPLAY_FONT};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Ai,
    Appearance,
    Conversation,
    Terminal,
    Updates,
}

impl Section {
    fn title(self) -> &'static str {
        match self {
            Section::Ai => "AI",
            Section::Appearance => "Appearance",
            Section::Conversation => "Conversation",
            Section::Terminal => "Terminal",
            Section::Updates => "Updates",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Section::Ai => "ai",
            Section::Appearance => "appearance",
            Section::Conversation => "conversation",
            Section::Terminal => "terminal",
            Section::Updates => "updates",
        }
    }

    fn description(self) -> &'static str {
        match self {
            Section::Ai => "Choose the agent that answers in new sessions.",
            Section::Appearance => "How wai looks.",
            Section::Conversation => "How a conversation shows what the agent does.",
            Section::Terminal => "The shell the terminal runs.",
            Section::Updates => "Whether wai looks for new versions.",
        }
    }
}

/// The nav's groups, each with its label and sections.
pub(crate) const NAV: &[(&str, &[Section])] = &[(
    "wai",
    &[
        Section::Ai,
        Section::Appearance,
        Section::Conversation,
        Section::Terminal,
        Section::Updates,
    ],
)];

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
            .child(view.clone())
    });
}

/// Every row with its section and label: what the search matches.
const ROWS: [(Section, &str); 5] = [
    (Section::Appearance, "Theme"),
    (Section::Conversation, "Processing"),
    (Section::Terminal, "Shell"),
    (Section::Updates, "Check for updates"),
    (Section::Updates, "Repository"),
];

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
}

pub enum SettingsEvent {
    Change(Change),
    OpenFile,
    Test(String),
}

#[derive(Clone)]
struct Theme(ThemeChoice);

impl SelectItem for Theme {
    type Value = ThemeChoice;

    fn title(&self) -> SharedString {
        match self.0 {
            ThemeChoice::Dark => "Dark",
            ThemeChoice::Light => "Light",
        }
        .into()
    }

    fn value(&self) -> &ThemeChoice {
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
        let choices = vec![Theme(ThemeChoice::Dark), Theme(ThemeChoice::Light)];
        let theme = cx.new(|cx| SelectState::new(choices, None, window, cx));
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
                        && *choice != this.data.settings.theme
                    {
                        this.change(Change::Theme(*choice), cx);
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
            processing,
            models: BTreeMap::new(),
            shell,
            repo,
            command,
            advanced: None,
            editing: [false; 3],
            stale: true,
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
    pub(crate) fn nav_count(&self, section: Section, _cx: &App) -> Option<usize> {
        match section {
            Section::Ai => Some(self.data.agents.len()),
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

    fn commit(&mut self, ix: usize, cx: &mut Context<Self>) {
        if !std::mem::take(&mut self.editing[ix]) {
            return;
        }
        let input = if ix == 0 { &self.shell } else { &self.repo };
        let text = input.read(cx).value().trim().to_string();
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
        let theme = settings.theme;
        self.theme
            .update(cx, |s, cx| s.set_selected_value(&theme, window, cx));
        let processing = settings.conversation.processing;
        self.processing
            .update(cx, |s, cx| s.set_selected_value(&processing, window, cx));
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
                input.update(cx, |i, cx| i.set_value(text, window, cx));
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
        selector: &'static str,
        control: impl IntoElement,
        cx: &App,
    ) -> Div {
        h_flex()
            .debug_selector(move || selector.into())
            .justify_between()
            .items_center()
            .gap_4()
            .py_2()
            .child(div().text_color(cx.theme().foreground).child(label))
            .child(div().w(px(260.)).flex_none().child(control))
    }

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
        v_flex()
            .py_2()
            .child(
                h_flex()
                    .justify_between()
                    .items_center()
                    .gap_4()
                    .child(
                        div().debug_selector(move || selector).child(
                            Radio::new(SharedString::from(format!("agent-{}", row.id)))
                                .label(title)
                                .checked(row.is_default)
                                .disabled(off)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.change(Change::DefaultAgent(id.clone()), cx)
                                })),
                        ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().text_color(muted).child(row.status.text(login)))
                            .child(
                                div().debug_selector(move || test_selector).child(
                                    Button::new(SharedString::from(format!("test-{}", row.id)))
                                        .small()
                                        .label("Test")
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(SettingsEvent::Test(test.clone()))
                                        })),
                                ),
                            ),
                    ),
            )
            .children(install)
            .children(warn)
            .child(self.default_model(row, cx))
            .children(row.extension.is_none().then(|| self.advanced(row, cx)))
    }

    fn default_model(&self, row: &AgentRow, cx: &mut Context<Self>) -> Div {
        let selector = format!("settings-model-{}", row.id);
        let control = match (row.models.is_empty(), self.models.get(&row.id)) {
            (false, Some(select)) => div().w(px(260.)).child(
                Select::new(select)
                    .small()
                    .disabled(self.data.error.is_some()),
            ),
            _ => div()
                .text_color(cx.theme().muted_foreground)
                .child("Start a session or press Test to see the models."),
        };
        h_flex()
            .debug_selector(move || selector)
            .gap_4()
            .items_center()
            .child(
                div()
                    .text_color(cx.theme().muted_foreground)
                    .child("Default model"),
            )
            .child(control)
    }

    /// Folded: the command, which Enter asks to run.
    fn advanced(&self, row: &AgentRow, cx: &mut Context<Self>) -> Div {
        let open = self.advanced.as_deref() == Some(row.id.as_str());
        let id = row.id.clone();
        let selector = format!("settings-command-{}", row.id);
        let command = open.then(|| {
            h_flex()
                .debug_selector(move || selector)
                .gap_4()
                .items_center()
                .child(
                    div()
                        .text_color(cx.theme().muted_foreground)
                        .child("Command"),
                )
                .child(
                    div().flex_1().child(
                        Input::new(&self.command)
                            .small()
                            .disabled(self.data.error.is_some()),
                    ),
                )
        });
        v_flex()
            .child(
                h_flex().child(
                    Button::new(SharedString::from(format!("advanced-{}", row.id)))
                        .ghost()
                        .small()
                        .icon(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .label("Advanced")
                        .on_click(cx.listener(move |this, _, _, cx| this.toggle_advanced(&id, cx))),
                ),
            )
            .children(command)
    }

    fn rows(&self, label: &str, cx: &mut Context<Self>) -> Div {
        let off = self.data.error.is_some();
        match label {
            "Theme" => self.row(
                "Theme",
                "settings-theme",
                Select::new(&self.theme).small().disabled(off),
                cx,
            ),
            "Processing" => self.row(
                "Processing",
                "settings-processing",
                Select::new(&self.processing).small().disabled(off),
                cx,
            ),
            "Shell" => self.row(
                "Shell",
                "settings-shell",
                Input::new(&self.shell).small().disabled(off),
                cx,
            ),
            "Check for updates" => self.row(
                "Check for updates",
                "settings-updates-check",
                h_flex().justify_end().child(
                    Switch::new("settings-updates-switch")
                        .checked(self.data.settings.updates.check)
                        .disabled(off)
                        .on_click(cx.listener(|this, on: &bool, _, cx| {
                            this.change(Change::UpdatesCheck(*on), cx)
                        })),
                ),
                cx,
            ),
            _ => self.row(
                "Repository",
                "settings-updates-repo",
                Input::new(&self.repo).small().disabled(off),
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
        let (muted, faint) = (cx.theme().muted_foreground, aurora::nevoa_2());
        let section = self.section;
        let rows: Vec<_> = (ROWS.iter())
            .filter(|(s, label)| *s == section && label.to_lowercase().contains(&query))
            .map(|(_, label)| *label)
            .collect();
        let ai = section == Section::Ai;
        let agents: Vec<_> = if ai {
            (self.data.agents.iter())
                .filter(|a| a.label.to_lowercase().contains(&query))
                .cloned()
                .collect()
        } else {
            Vec::new()
        };
        let project = ai
            .then(|| self.data.project_default.as_deref().map(agent_label))
            .flatten()
            .map(|a| div().child(format!("This project uses {a}.")));
        let notice = ai
            .then(|| self.notice())
            .flatten()
            .map(|n| div().text_color(warning).child(n));
        let advanced =
            (ai && "settings.toml".contains(&query)).then(|| {
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .font_family(DISPLAY_FONT)
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .child("Advanced"),
                    )
                    .child(
                        h_flex()
                            .justify_between()
                            .items_center()
                            .gap_4()
                            .py_2()
                            .child(v_flex().child(div().child("settings.toml")).child(
                                div().text_color(muted).child("Everything above, as a file"),
                            ))
                            .child(
                                Button::new("settings-open-file")
                                    .small()
                                    .label("Open")
                                    .on_click(
                                        cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile)),
                                    ),
                            ),
                    )
            });
        let nav = NAV.iter().map(|(label, sections)| {
            v_flex()
                .gap_1()
                .child(
                    div()
                        .px_2()
                        .pt_3()
                        .pb_1()
                        .text_size(px(11.))
                        .text_color(faint)
                        .child(label.to_uppercase()),
                )
                .children(sections.iter().map(|&s| {
                    let count = self.nav_count(s, cx);
                    aurora::tab(s == section, cx)
                        .id(ElementId::Name(format!("settings-nav-{}", s.key()).into()))
                        .px_2()
                        .py_1()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .justify_between()
                        .child(s.title())
                        .children(count.map(|n| div().text_color(faint).child(n.to_string())))
                        .on_click(cx.listener(move |this, _, window, cx| this.show(s, window, cx)))
                        .test_support()
                }))
        });
        let banner = self.banner().map(|text| {
            h_flex()
                .justify_between()
                .items_center()
                .px_3()
                .py_2()
                .text_color(danger)
                .child(text)
                .child(
                    Button::new("settings-error-open")
                        .small()
                        .label("Open")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
                )
        });
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
                            .text_size(px(14.))
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
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .child(
                        v_flex()
                            .id("settings-nav")
                            .w(px(230.))
                            .flex_none()
                            .overflow_y_scroll()
                            .p_2()
                            .gap_1()
                            .children(nav),
                    )
                    .child(
                        v_flex()
                            .id("settings-content")
                            .flex_1()
                            .min_w_0()
                            .overflow_y_scroll()
                            .p(px(30.))
                            .gap_4()
                            .child(
                                v_flex()
                                    .gap_1()
                                    .child(
                                        div()
                                            .font_family(DISPLAY_FONT)
                                            .text_size(px(26.))
                                            .font_weight(FontWeight::BOLD)
                                            .child(section.title()),
                                    )
                                    .child(div().text_color(muted).child(section.description())),
                            )
                            .children(banner)
                            .child(Input::new(&self.search).small())
                            .children(notice)
                            .children(project)
                            .children(agents.iter().map(|a| self.agent_row(a, cx)))
                            .children(rows.into_iter().map(|label| self.rows(label, cx)))
                            .children(advanced),
                    ),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::component::WindowExt as _;
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
            window.render_frame(cx);
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
            w.change_setting(Change::Theme(ThemeChoice::Light), window, cx)
        });
        let text = std::fs::read_to_string(dir.path().join("settings.toml")).unwrap();
        assert!(text.contains("theme = \"light\""), "{text}");
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
        view.update(cx, |v, cx| v.change(Change::Theme(ThemeChoice::Light), cx));
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
}
