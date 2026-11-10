//! The Settings pane: a section list, a search over the rows and the rows of the sections. It
//! shows what the workspace gives it (`set`) and says what the user changed (`SettingsEvent`);
//! the workspace writes the user's file.

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::select::{Select, SelectEvent, SelectItem, SelectState};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, h_flex, v_flex};
use gpui_kit::*;
use wai_settings::{Change, Settings, ThemeChoice};

use crate::aurora::{self, DISPLAY_FONT};

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

pub struct SettingsData {
    pub settings: Settings,
    /// Why `settings.toml` cannot be read: the screen is read-only then.
    pub error: Option<String>,
    pub notice: Option<SharedString>,
}

#[allow(dead_code)] // `Test` is sent by the AI rows (Task 8)
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

pub struct SettingsView {
    section: Section,
    data: SettingsData,
    pub(crate) search: Entity<InputState>,
    theme: Entity<SelectState<Vec<Theme>>>,
    pub(crate) shell: Entity<InputState>,
    repo: Entity<InputState>,
    /// A text field holds edits not committed yet (Enter or blur): a reload leaves it alone.
    editing: [bool; 2],
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
        let mut this = Self {
            section,
            data,
            search,
            theme,
            shell,
            repo,
            editing: [false; 2],
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

    fn sync(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.stale = false;
        let settings = &self.data.settings;
        let theme = settings.theme;
        self.theme
            .update(cx, |s, cx| s.set_selected_value(&theme, window, cx));
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
            shown.then_some((section, rows))
        });
        let sections: Vec<_> = sections
            .map(|(section, rows)| {
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
                    .child(div().w_full().child(Input::new(&self.search).small())),
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
    use gpui_kit::{Entity, TestAppContext, VisualTestContext};
    use wai_settings::{Change, Settings, ThemeChoice};

    use super::{Section, SettingsData, SettingsView};
    use crate::workspace::tests::open;
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
        }
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
