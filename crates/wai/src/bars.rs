//! What the bars show: the keys the user hid or ordered, and the menu that toggles them.

use std::collections::BTreeSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_ext::panel::{LINE_CHARS, Tone, clamp};

use crate::ext_panel::tone_color;
use crate::settings_view::Section;
use crate::sidebar::{SidebarTab, TABS};
use crate::update::UpdateStatus;
use crate::workspace::Workspace;

/// The running build: the version, then the commit it was built from (`build.rs`), when known.
pub(crate) fn version() -> String {
    match env!("WAI_BUILD") {
        "" => env!("CARGO_PKG_VERSION").to_string(),
        build => format!("{} · {build}", env!("CARGO_PKG_VERSION")),
    }
}

/// What a bottom-bar item says: its text, the tone that colours it, and its tooltip.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Status {
    pub text: String,
    pub tone: Tone,
    pub tooltip: String,
}

/// `tag` without its `v`, clamped.
fn shown_version(tag: &str) -> String {
    clamp(tag.strip_prefix('v').unwrap_or(tag), 32)
}

/// The version item: a ready or available update, else the running version.
pub(crate) fn version_status(update: &UpdateStatus) -> Status {
    match update {
        UpdateStatus::Ready(staged) => Status {
            text: format!("{} ready · Restart", shown_version(&staged.tag)),
            tone: Tone::Info,
            tooltip: staged.text(),
        },
        UpdateStatus::Available { release, .. } => Status {
            text: format!("{} available", shown_version(&release.tag)),
            tone: Tone::Info,
            tooltip: release.text(),
        },
        _ => Status {
            text: format!("wai {}", env!("CARGO_PKG_VERSION")),
            tone: Tone::Neutral,
            tooltip: version(),
        },
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum VersionClick {
    Restart,
    Open(String),
    Settings,
}

pub(crate) fn version_click(update: &UpdateStatus) -> VersionClick {
    match update {
        UpdateStatus::Ready(_) => VersionClick::Restart,
        UpdateStatus::Available { release, .. } => VersionClick::Open(release.page.clone()),
        _ => VersionClick::Settings,
    }
}

/// The stopped-extensions item, from each one's name and error; none when all run.
pub(crate) fn stopped_status(stopped: &[(String, String)]) -> Option<Status> {
    let text = match stopped {
        [] => return None,
        [(name, _)] => format!("{} stopped", clamp(name, 24)),
        many => format!("{} extensions stopped", many.len()),
    };
    let lines: Vec<String> = (stopped.iter())
        .map(|(name, error)| {
            let first = error.lines().next().unwrap_or_default();
            format!("{}: {}", clamp(name, 24), clamp(first, LINE_CHARS))
        })
        .collect();
    Some(Status {
        text,
        tone: Tone::Danger,
        tooltip: lines.join("\n"),
    })
}

/// One bottom-bar item, coloured by its tone; the caller adds the click and `test_support`.
pub(crate) fn bottom_item(
    id: impl Into<ElementId>,
    status: &Status,
    icon: Option<Lucide>,
    cx: &App,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (ring, hover) = (theme.ring, theme.list_hover);
    let (text, tooltip) = (status.text.clone(), status.tooltip.clone());
    h_flex()
        .id(id)
        .flex_none()
        .h_full()
        .px_2()
        .gap_1()
        .rounded(px(5.))
        .cursor_pointer()
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .text_color(tone_color(status.tone, cx))
        .role(Role::Button)
        .aria_label(text.clone())
        .focus_visible(move |el| el.border_color(ring))
        .hover(move |el| el.bg(hover))
        // Focusable, and Enter or Space clicks it.
        .tab_index(0)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .children(icon.map(|name| Icon::new(name).size(px(12.))))
        .child(text)
}

/// `default`'s keys: those in `order` first, in its order, then the rest as `default` has them.
pub(crate) fn ordered(default: &[String], order: &[String]) -> Vec<String> {
    let first = order.iter().filter(|key| default.contains(key));
    let rest = default.iter().filter(|key| !order.contains(key));
    first.chain(rest).cloned().collect()
}

/// One hideable item in a bar's menu; entries with a group list under its heading.
pub(crate) struct HideEntry {
    pub key: String,
    pub label: SharedString,
    pub group: Option<SharedString>,
}

/// `menu` with a checked entry per item; a click shows or hides it.
pub(crate) fn hide_menu(
    mut menu: PopupMenu,
    entries: &[HideEntry],
    hidden: &BTreeSet<String>,
    workspace: WeakEntity<Workspace>,
) -> PopupMenu {
    let mut groups: Vec<&SharedString> = Vec::new();
    for group in entries.iter().filter_map(|e| e.group.as_ref()) {
        if !groups.contains(&group) {
            groups.push(group);
        }
    }
    let section = |menu: PopupMenu, group: Option<&SharedString>| {
        entries
            .iter()
            .filter(|e| e.group.as_ref() == group)
            .fold(menu, |menu, e| {
                let (key, workspace) = (e.key.clone(), workspace.clone());
                menu.item(
                    PopupMenuItem::new(e.label.clone())
                        .checked(!hidden.contains(&e.key))
                        .on_click(move |_, _, cx| {
                            workspace.update(cx, |w, cx| w.toggle_hidden(&key, cx)).ok();
                        }),
                )
            })
    };
    menu = section(menu, None);
    for group in groups {
        menu = menu.item(PopupMenuItem::label(group.clone()));
        menu = section(menu, Some(group));
    }
    menu
}

impl Workspace {
    /// The status bar: the stopped extensions on the left, the version on the right.
    pub(crate) fn render_bottom_bar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let stopped = (self.shown("wai.extensions"))
            .then(|| stopped_status(&self.ext_host.read(cx).stopped()))
            .flatten()
            .map(|status| {
                bottom_item(
                    "bottombar-extensions",
                    &status,
                    Some(Lucide::TriangleAlert),
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.show_extensions(window, cx);
                }))
                .test_support()
            });
        let version = self.shown("wai.version").then(|| {
            let status = version_status(&self.update);
            bottom_item("bottombar-version", &status, None, cx)
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    match version_click(&this.update) {
                        VersionClick::Restart => this.install_update(window, cx),
                        VersionClick::Open(url) => cx.open_url(&url),
                        VersionClick::Settings => {
                            this.open_settings(Section::Updates, None, window, cx)
                        }
                    }
                }))
                .test_support()
        });
        let entries: Vec<HideEntry> = [
            ("wai.mcp", "MCP servers"),
            ("wai.extensions", "Stopped extensions"),
            ("wai.usage", "Usage limit"),
            ("wai.version", "Version"),
        ]
        .into_iter()
        .map(|(key, label)| HideEntry {
            key: key.into(),
            label: label.into(),
            group: None,
        })
        .collect();
        let (hidden, workspace) = (self.hidden.clone(), cx.entity().downgrade());
        h_flex()
            .id("bottombar")
            .debug_selector(|| "bottombar".into())
            .flex_none()
            .w_full()
            .h(px(24.))
            .px_2()
            .gap_1()
            .text_xs()
            .border_t_1()
            .border_color(theme.title_bar_border)
            // Transparent over the aurora in dark mode, like the top bar.
            .when(!theme.is_dark(), |el| el.bg(theme.title_bar))
            .children(stopped)
            .child(div().flex_1())
            .children(version)
            .test_support()
            .context_menu(move |menu, _, _| hide_menu(menu, &entries, &hidden, workspace.clone()))
    }

    pub(crate) fn shown(&self, key: &str) -> bool {
        !self.hidden.contains(key)
    }

    /// Hiding the selected sidebar tab shows the sessions; a main panel's pane stays open.
    pub(crate) fn toggle_hidden(&mut self, key: &str, cx: &mut Context<Self>) {
        if !self.hidden.remove(key) {
            self.hidden.insert(key.to_string());
            if self.tab_key(self.sidebar_tab, cx).as_deref() == Some(key) {
                self.sidebar_tab = SidebarTab::Sessions;
            }
        }
        self.save_state(cx);
        cx.notify();
    }

    /// The rail's keys in the user's order, hidden ones included.
    pub(crate) fn rail_keys(&self, cx: &App) -> Vec<String> {
        let panels = self.ext_host.read(cx).panels();
        let default: Vec<String> = (TABS.iter().map(|(_, key, ..)| key.to_string()))
            .chain(panels.iter().map(|p| p.key()))
            .collect();
        ordered(&default, &self.rail_order)
    }

    /// Puts rail icon `from` where `to` is.
    pub(crate) fn move_rail(&mut self, from: &str, to: &str, cx: &mut Context<Self>) {
        if from == to {
            return;
        }
        let mut keys = self.rail_keys(cx);
        let (Some(at), Some(was)) = (
            keys.iter().position(|k| k == to),
            keys.iter().position(|k| k == from),
        ) else {
            return;
        };
        let key = keys.remove(was);
        keys.insert(at, key);
        self.rail_order = keys;
        self.save_state(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use gpui_kit::{TestAppContext, VisualTestContext};
    use wai_ext::panel::Tone;
    use wai_settings::AppState;

    use super::{
        Status, VersionClick, ordered, stopped_status, version, version_click, version_status,
    };
    use crate::settings_view::Section;
    use crate::sidebar::tests::label;
    use crate::update::{Release, Staged, UpdateStatus};
    use crate::workspace::tests::{
        click, fake_settings, launch, menu_item, open, open_at, question,
    };

    fn release() -> Release {
        Release {
            assets: Vec::new(),
            tag: "v0.10.0".into(),
            page: "https://github.com/o/r/releases/tag/v0.10.0".into(),
        }
    }

    fn staged() -> Staged {
        Staged {
            tag: "v0.10.0".into(),
            file: "wai.zip".into(),
            sha256: "00".into(),
        }
    }

    fn available() -> UpdateStatus {
        UpdateStatus::Available {
            release: release(),
            why: None,
        }
    }

    /// The version, then the commit this test binary was built from (build.rs); only the
    /// version outside a git checkout.
    #[test]
    fn version_names_the_build() {
        let head = std::process::Command::new("git")
            .args(["rev-parse", "--short", "HEAD"])
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| String::from_utf8_lossy(&out.stdout).trim().to_owned());
        let expected = match head {
            Some(head) => format!("{} · {head}", env!("CARGO_PKG_VERSION")),
            None => env!("CARGO_PKG_VERSION").to_string(),
        };
        assert_eq!(version(), expected);
    }

    #[test]
    fn version_status_follows_the_update() {
        let idle = version_status(&UpdateStatus::Idle);
        assert_eq!(
            idle,
            Status {
                text: format!("wai {}", env!("CARGO_PKG_VERSION")),
                tone: Tone::Neutral,
                tooltip: version(),
            }
        );
        let ready = version_status(&UpdateStatus::Ready(staged()));
        assert_eq!(
            ready,
            Status {
                text: "0.10.0 ready · Restart".into(),
                tone: Tone::Info,
                tooltip: staged().text(),
            }
        );
        let found = version_status(&available());
        assert_eq!(
            found,
            Status {
                text: "0.10.0 available".into(),
                tone: Tone::Info,
                tooltip: release().text(),
            }
        );
    }

    #[test]
    fn version_click_targets() {
        assert_eq!(
            version_click(&UpdateStatus::Ready(staged())),
            VersionClick::Restart
        );
        assert_eq!(
            version_click(&available()),
            VersionClick::Open("https://github.com/o/r/releases/tag/v0.10.0".into())
        );
        for status in [
            UpdateStatus::Idle,
            UpdateStatus::Checking,
            UpdateStatus::UpToDate,
            UpdateStatus::Failed("no".into()),
        ] {
            assert_eq!(version_click(&status), VersionClick::Settings);
        }
    }

    #[test]
    fn stopped_status_names_one_and_counts_more() {
        let pair = |name: &str, error: &str| (name.to_string(), error.to_string());
        assert_eq!(stopped_status(&[]), None);
        assert_eq!(
            stopped_status(&[pair("wai-rotinas", "exit code 1\nstack")]),
            Some(Status {
                text: "wai-rotinas stopped".into(),
                tone: Tone::Danger,
                tooltip: "wai-rotinas: exit code 1".into(),
            })
        );
        assert_eq!(
            stopped_status(&[pair("a", "x"), pair("b", "y")]),
            Some(Status {
                text: "2 extensions stopped".into(),
                tone: Tone::Danger,
                tooltip: "a: x\nb: y".into(),
            })
        );
    }

    #[gpui_kit::test]
    fn version_click_opens_updates(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        click("bottombar-version", cx);
        let view = workspace.read_with(cx, |w, _| w.settings_view().expect("settings open"));
        assert_eq!(view.read_with(cx, |v, _| v.section()), Section::Updates);
    }

    /// Hiding Version in the bottom bar's menu removes it, and a relaunch keeps it hidden.
    #[gpui_kit::test]
    fn bottom_items_hide_and_survive_a_relaunch(
        cx: &mut TestAppContext,
        relaunch: &mut TestAppContext,
    ) {
        let drawn = |cx: &mut VisualTestContext| label("bottombar-version", cx).is_some();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state.json");
        cx.executor().allow_parking();
        let (_workspace, cx) = open_at(cx, fake_settings(""), Some(path.clone()));
        assert!(drawn(cx));
        menu_item("bottombar", "Version", cx);
        assert!(!drawn(cx));
        assert!(AppState::load(&path).hidden.contains("wai.version"));

        relaunch.executor().allow_parking();
        let (workspace, cx) = open_at(relaunch, fake_settings(""), Some(path.clone()));
        launch(&workspace, cx);
        while question(&workspace, cx).is_some() {
            click("approval-allow", cx);
        }
        assert!(!drawn(cx));
        menu_item("bottombar", "Version", cx);
        assert!(drawn(cx));
    }

    #[test]
    fn ordered_skips_gone_keys_and_appends_new_ones() {
        let keys = |keys: &[&str]| keys.iter().map(|k| k.to_string()).collect::<Vec<_>>();
        assert_eq!(
            ordered(&keys(&["a", "b", "c", "d"]), &keys(&["c", "x", "a"])),
            keys(&["c", "a", "b", "d"])
        );
    }
}
