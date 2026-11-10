//! The Settings pane: a section list, a search over the rows and the rows of the sections. It
//! shows what the workspace gives it (`set`) and says what the user changed (`SettingsEvent`);
//! the workspace writes the user's file.

use std::collections::BTreeMap;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::radio::Radio;
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, IconName, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use wai_settings::{Change, SeenModel, Settings, ThemeChoice, agent_label, builtin};

use crate::aurora::{self, DISPLAY_FONT};
use crate::workspace::{Dragged, DraggedSession};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Section {
    Ai,
    Appearance,
    Terminal,
    Updates,
}

impl Section {
    const ALL: [Section; 4] = [
        Section::Ai,
        Section::Appearance,
        Section::Terminal,
        Section::Updates,
    ];

    fn title(self) -> &'static str {
        match self {
            Section::Ai => "AI",
            Section::Appearance => "Appearance",
            Section::Terminal => "Terminal",
            Section::Updates => "Updates",
        }
    }
}

/// Every row with its section and label: what the search matches.
const ROWS: [(Section, &str); 4] = [
    (Section::Appearance, "Theme"),
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
    data: SettingsData,
    pub(crate) search: Entity<InputState>,
    theme: Entity<SelectState<Vec<Theme>>>,
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
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search settings"));
        let choices = vec![Theme(ThemeChoice::Dark), Theme(ThemeChoice::Light)];
        let theme = cx.new(|cx| SelectState::new(choices, None, window, cx));
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
            data,
            search,
            theme,
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

    pub fn show(&mut self, section: Section, cx: &mut Context<Self>) {
        self.section = section;
        cx.notify();
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
        let searching = !query.is_empty();
        let (warning, danger) = (cx.theme().warning, cx.theme().danger);
        let sections = Section::ALL.into_iter().filter_map(|section| {
            let rows: Vec<_> = (ROWS.iter())
                .filter(|(s, label)| *s == section && label.to_lowercase().contains(&query))
                .map(|(_, label)| *label)
                .collect();
            let shown = if searching {
                !rows.is_empty()
            } else {
                section == self.section
            };
            let agents: Vec<_> = if section == Section::Ai {
                (self.data.agents.iter())
                    .filter(|a| a.label.to_lowercase().contains(&query))
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };
            (shown || (searching && !agents.is_empty())).then_some((section, rows, agents))
        });
        let sections: Vec<_> = sections
            .map(|(section, rows, agents)| {
                let project = (section == Section::Ai)
                    .then(|| self.data.project_default.as_deref().map(agent_label))
                    .flatten()
                    .map(|a| div().child(format!("This project uses {a}.")));
                let notice = (section == Section::Ai)
                    .then(|| self.notice())
                    .flatten()
                    .map(|n| div().text_color(warning).child(n));
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .font_family(DISPLAY_FONT)
                            .text_size(px(16.))
                            .font_weight(FontWeight::BOLD)
                            .child(section.title()),
                    )
                    .children(notice)
                    .children(project)
                    .children(agents.iter().map(|a| self.agent_row(a, cx)))
                    .children(rows.into_iter().map(|label| self.rows(label, cx)))
            })
            .collect();
        let list =
            v_flex()
                .w(px(140.))
                .flex_none()
                .p_2()
                .gap_1()
                .children(Section::ALL.into_iter().map(|section| {
                    aurora::tab(!searching && section == self.section, cx)
                        .id(section.title())
                        .px_2()
                        .py_1()
                        .rounded(px(8.))
                        .cursor_pointer()
                        .child(section.title())
                        .on_click(cx.listener(move |this, _, _, cx| this.show(section, cx)))
                }));
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
            .child(
                aurora::card_bar(cx)
                    .px_3()
                    .py_2()
                    .gap_3()
                    // The title drags the pane to another place, as a session's does.
                    .child(
                        div()
                            .id("settings-title")
                            .flex_none()
                            .cursor_grab()
                            .font_family(DISPLAY_FONT)
                            .text_size(px(14.))
                            .font_weight(FontWeight::BOLD)
                            .child("Settings")
                            .on_drag(
                                DraggedSession {
                                    title: "Settings".into(),
                                    source: Dragged::Settings,
                                },
                                DraggedSession::preview,
                            )
                            .test_support(),
                    )
                    .child(div().flex_1().child(Input::new(&self.search).small())),
            )
            .children(banner)
            .child(
                h_flex().flex_1().min_h_0().child(list).child(
                    v_flex()
                        .id("settings-rows")
                        .flex_1()
                        .min_w_0()
                        .overflow_y_scroll()
                        .p_3()
                        .gap_4()
                        .children(sections),
                ),
            )
            .child(
                h_flex().flex_none().justify_end().p_2().child(
                    Button::new("settings-open-file")
                        .ghost()
                        .small()
                        .label("Open settings.toml")
                        .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::OpenFile))),
                ),
            )
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::{Change, Settings, ThemeChoice};

    use super::{RowStatus, Section, SettingsData, SettingsView};
    use crate::workspace::tests::{fake_settings, open, wait_until};
    use crate::workspace::{Pane, Workspace};

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
            .read_with(cx, |w, _| {
                w.panes.iter().find_map(|p| match p {
                    Pane::Settings(v) => Some(v.clone()),
                    _ => None,
                })
            })
            .expect("settings pane")
    }

    fn data(error: Option<&str>) -> SettingsData {
        SettingsData {
            settings: Settings::default(),
            error: error.map(Into::into),
            notice: None,
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
    fn advanced_command_is_written_on_enter(cx: &mut TestAppContext) {
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
        let (settings, _) = Settings::load(dir.path(), None).unwrap();
        let typed = fake_settings("--reply other").agents["fake"]
            .command
            .clone();
        assert_eq!(settings.agents["fake"].command, typed);
    }

    #[gpui_kit::test]
    fn cmd_comma_opens_one_settings_pane(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        cx.simulate_keystrokes("secondary-,");
        cx.simulate_keystrokes("secondary-,");
        workspace.read_with(cx, |w, _| {
            let n = w
                .panes
                .iter()
                .filter(|p| matches!(p, Pane::Settings(_)))
                .count();
            assert_eq!(n, 1);
            assert!(matches!(w.panes[w.focused_pane], Pane::Settings(_)));
        });
        cx.simulate_keystrokes("secondary-w");
        workspace.read_with(cx, |w, _| {
            assert!(!w.panes.iter().any(|p| matches!(p, Pane::Settings(_))));
        });
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

    #[gpui_kit::test]
    fn search_filters_rows_across_sections(cx: &mut TestAppContext) {
        let (workspace, cx, _dir) = settings_ws(cx);
        workspace.update_in(cx, |w, window, cx| {
            w.open_settings(Section::Appearance, None, window, cx)
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-theme").is_some());
        assert!(cx.debug_bounds("settings-shell").is_none());
        let view = view(&workspace, cx);
        view.update_in(cx, |v, window, cx| {
            v.search
                .update(cx, |s, cx| s.set_value("SHELL", window, cx));
            cx.notify();
        });
        cx.run_until_parked();
        assert!(cx.debug_bounds("settings-shell").is_some());
        for hidden in [
            "settings-theme",
            "settings-updates-check",
            "settings-updates-repo",
        ] {
            assert!(cx.debug_bounds(hidden).is_none(), "{hidden}");
        }
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
