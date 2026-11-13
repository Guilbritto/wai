//! What the bars show: the keys the user hid or ordered, and the menu that toggles them.

use std::collections::BTreeSet;

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{ActiveTheme as _, Icon, h_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_ext::Bar;
use wai_ext::panel::{LINE_CHARS, Tone, clamp};
use wai_settings::claude::McpStatus;

use crate::aurora;
use crate::claude_config::NO_CLAUDE;
use crate::ext_host::BarItem;
use crate::ext_panel::{lucide, tone_color};
use crate::settings_view::Section;
use crate::settings_view::claude::McpCheck;
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

/// The MCP item, from the last `claude mcp list`; none until it ran, with no servers, or without
/// the CLI.
pub(crate) fn mcp_status(check: &McpCheck) -> Option<Status> {
    let status = |text: String, tone, tooltip: String| {
        Some(Status {
            text,
            tone,
            tooltip,
        })
    };
    let map = match check {
        McpCheck::NotRun => return None,
        McpCheck::Running => {
            return status("MCP …".into(), Tone::Neutral, "Checking MCP servers".into());
        }
        McpCheck::Failed(e) if e == NO_CLAUDE => return None,
        McpCheck::Failed(e) => return status("MCP".into(), Tone::Warning, clamp(e, LINE_CHARS)),
        McpCheck::Done(map) if map.is_empty() => return None,
        McpCheck::Done(map) => map,
    };
    let named = |want: fn(&McpStatus) -> bool| -> Vec<&str> {
        (map.iter())
            .filter(|(_, s)| want(s))
            .map(|(n, _)| n.as_str())
            .collect()
    };
    let connected = named(|s| matches!(s, McpStatus::Connected)).len();
    let failed = named(|s| matches!(s, McpStatus::Failed(_)));
    let auth = named(|s| matches!(s, McpStatus::NeedsAuth));
    let mut lines = Vec::new();
    if !failed.is_empty() {
        lines.push(format!("Failed: {}", failed.join(", ")));
    }
    if !auth.is_empty() {
        lines.push(format!("Needs a login: {}", auth.join(", ")));
    }
    if lines.is_empty() {
        lines.push(format!("{connected} MCP servers connected"));
    }
    let tooltip = (lines.iter())
        .map(|l| clamp(l, LINE_CHARS))
        .collect::<Vec<_>>()
        .join("\n");
    if !failed.is_empty() {
        status(
            format!("MCP {connected} · {} failed", failed.len()),
            Tone::Danger,
            tooltip,
        )
    } else if !auth.is_empty() {
        status(
            format!("MCP {connected} · {} needs auth", auth.len()),
            Tone::Warning,
            tooltip,
        )
    } else {
        status(format!("MCP {connected}"), Tone::Neutral, tooltip)
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

/// One bottom-bar item, coloured by its tone; the caller adds the click and `test_support`. A
/// `clickable` one is a focusable button; the other is plain text with its tooltip.
pub(crate) fn bottom_item(
    id: impl Into<ElementId>,
    status: &Status,
    icon: Option<Lucide>,
    clickable: bool,
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
        .border_1()
        .border_color(gpui_kit::transparent_black())
        .text_color(tone_color(status.tone, cx))
        .aria_label(text.clone())
        .when(clickable, |el| {
            el.cursor_pointer()
                .role(Role::Button)
                .focus_visible(move |el| el.border_color(ring))
                .hover(move |el| el.bg(hover))
                // Focusable, and Enter or Space clicks it.
                .tab_index(0)
        })
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .children(icon.map(|name| Icon::new(name).size(px(12.))))
        .child(text)
}

/// Extension items drawn in the top bar, over all extensions.
pub(crate) const TOP_ITEMS: usize = 4;
/// The top bar draws an item's text shorter than the bottom bar does.
const TOP_TEXT_CHARS: usize = 24;

/// `"{tooltip or title} — {ext_name}"`: the extension is always named.
pub(crate) fn item_tooltip(item: &BarItem) -> String {
    let said = item.state.as_ref().and_then(|s| s.tooltip.as_deref());
    format!("{} — {}", said.unwrap_or(&item.title), item.ext_name)
}

/// An extension's item, as a focusable button that sends its click to the extension. In the top
/// bar it is a pill that shrinks before wai's buttons do.
pub(crate) fn ext_item(
    item: &BarItem,
    top: bool,
    id: impl Into<ElementId>,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let state = item.state.clone().unwrap_or_default();
    let text = (state.text.as_deref()).map(|t| match top {
        true => clamp(t, TOP_TEXT_CHARS),
        false => t.to_string(),
    });
    let icon = state.icon.as_deref().and_then(lucide);
    let label = text.clone().unwrap_or_else(|| item.title.clone());
    let tooltip = item_tooltip(item);
    let (ext, item_id) = (item.ext.clone(), item.id.clone());
    let click = cx.listener(move |this, _: &ClickEvent, _, cx| {
        this.ext_host.read(cx).item_action(&ext, &item_id);
    });
    if !top {
        let status = Status {
            text: text.unwrap_or_default(),
            tone: state.tone,
            tooltip,
        };
        return bottom_item(id, &status, icon, true, cx)
            .aria_label(label)
            .on_click(click)
            .test_support()
            .into_any_element();
    }
    let theme = cx.theme();
    let (ring, color) = (theme.ring, tone_color(state.tone, cx));
    aurora::pill(cx)
        .id(id)
        .min_w_0()
        .overflow_hidden()
        .text_color(color)
        .border_color(gpui_kit::transparent_black())
        .cursor_pointer()
        .role(Role::Button)
        .aria_label(label)
        .focus_visible(move |el| el.border_color(ring))
        .tab_index(0)
        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
        .on_click(click)
        .children(icon.map(|name| Icon::new(name).size(px(12.)).flex_none()))
        .children(text.map(|t| div().min_w_0().truncate().child(t)))
        .test_support()
        .into_any_element()
}

/// What the agent reports of the account's usage limit; `resets_at` in Unix seconds.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct RateLimit {
    pub status: String,
    pub resets_at: Option<i64>,
}

/// The report in an agent's `rate_limit` metadata; none without a string `status`.
pub(crate) fn rate_limit(meta: &serde_json::Value) -> Option<RateLimit> {
    let status = meta.get("status")?.as_str()?.to_string();
    let resets_at = meta
        .get("resetsAt")
        .and_then(|v| v.as_i64().or_else(|| v.as_f64().map(|f| f as i64)));
    // Above 10^12 it is milliseconds.
    let resets_at = resets_at.map(|t| if t > 1_000_000_000_000 { t / 1000 } else { t });
    Some(RateLimit { status, resets_at })
}

/// Unix seconds as the local `%H:%M`.
pub(crate) fn clock(secs: i64) -> String {
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|t| t.with_timezone(&chrono::Local).format("%H:%M").to_string())
        .unwrap_or_default()
}

pub(crate) fn usage_status(r: &RateLimit) -> Status {
    let resets = r
        .resets_at
        .map(|t| format!(" · resets {}", clock(t)))
        .unwrap_or_default();
    let (text, tone) = match r.status.as_str() {
        "allowed" => ("Usage ok".to_string(), Tone::Neutral),
        "allowed_warning" => (format!("Near usage limit{resets}"), Tone::Warning),
        "rejected" => (format!("Usage limit reached{resets}"), Tone::Danger),
        other => (clamp(other, 32), Tone::Neutral),
    };
    Status {
        text,
        tone,
        tooltip: r.status.clone(),
    }
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

/// wai's `own` entries, then `items` grouped under their extension's name.
pub(crate) fn hide_entries(own: &[(&str, &str)], items: &[BarItem]) -> Vec<HideEntry> {
    let own = own.iter().map(|(key, label)| HideEntry {
        key: key.to_string(),
        label: label.to_string().into(),
        group: None,
    });
    let ext = items.iter().map(|i| HideEntry {
        key: i.key.clone(),
        label: i.title.clone().into(),
        group: Some(i.ext_name.clone()),
    });
    own.chain(ext).collect()
}

impl Workspace {
    /// The items of `bar`, extensions in the rail's order (those without a panel after, as
    /// enabled), hidden ones included: draw those with a state that are `shown`.
    pub(crate) fn bar_items(&self, bar: Bar, cx: &App) -> Vec<BarItem> {
        let keys = self.rail_keys(cx);
        let rank = |item: &BarItem| {
            let prefix = format!("{}/", item.ext.as_str());
            (keys.iter().position(|k| k.starts_with(&prefix))).unwrap_or(usize::MAX)
        };
        let mut items = self.ext_host.read(cx).items();
        items.retain(|i| i.bar == bar);
        items.sort_by_key(rank);
        items
    }

    /// The status bar: the stopped extensions on the left, the version on the right.
    pub(crate) fn render_bottom_bar(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<> {
        let mcp = (self.shown("wai.mcp"))
            .then(|| mcp_status(&self.mcp))
            .flatten()
            .map(|status| {
                bottom_item("bottombar-mcp", &status, Some(Lucide::Server), true, cx)
                    .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                        this.open_settings(Section::Mcp, None, window, cx);
                        this.check_mcp(cx);
                    }))
                    .test_support()
            });
        let stopped = (self.shown("wai.extensions"))
            .then(|| stopped_status(&self.ext_host.read(cx).stopped()))
            .flatten()
            .map(|status| {
                bottom_item(
                    "bottombar-extensions",
                    &status,
                    Some(Lucide::TriangleAlert),
                    true,
                    cx,
                )
                .on_click(cx.listener(|this, _: &ClickEvent, window, cx| {
                    this.show_extensions(window, cx);
                }))
                .test_support()
            });
        let drawn = |item: &BarItem| item.state.is_some() && self.shown(&item.key);
        let items = self.bar_items(Bar::Bottom, cx);
        let ext_items: Vec<_> = (items.iter().filter(|i| drawn(i)))
            .enumerate()
            .map(|(ix, item)| ext_item(item, false, ("bottombar-item", ix), cx))
            .collect();
        let usage = (self.shown("wai.usage"))
            .then(|| self.rate_limit.as_ref().map(usage_status))
            .flatten()
            .map(|status| bottom_item("bottombar-usage", &status, None, false, cx).test_support());
        let version = self.shown("wai.version").then(|| {
            let status = version_status(&self.update);
            bottom_item("bottombar-version", &status, None, true, cx)
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
        let entries = hide_entries(
            &[
                ("wai.mcp", "MCP servers"),
                ("wai.extensions", "Stopped extensions"),
                ("wai.usage", "Usage limit"),
                ("wai.version", "Version"),
            ],
            &items,
        );
        let (hidden, workspace) = (self.hidden.clone(), cx.entity().downgrade());
        let theme = cx.theme();
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
            .children(mcp)
            .children(stopped)
            .children(ext_items)
            .child(div().flex_1())
            .children(usage)
            .children(version)
            .test_support()
            .context_menu(move |menu, _, _| hide_menu(menu, &entries, &hidden, workspace.clone()))
    }

    /// Stores the report, and drops it at its reset: the only timer wai keeps, one shot. A report
    /// whose reset has passed is stale and shows nothing.
    pub(crate) fn set_rate_limit(&mut self, r: RateLimit, cx: &mut Context<Self>) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        // An older wake would clear this report at the old reset.
        self.rate_wake = None;
        match r.resets_at {
            Some(t) if t <= now => self.rate_limit = None,
            Some(t) => {
                let wait = std::time::Duration::from_secs((t - now) as u64);
                self.rate_limit = Some(r);
                self.rate_wake = Some(cx.spawn(async move |this, cx| {
                    cx.background_executor().timer(wait).await;
                    this.update(cx, |this, cx| {
                        this.rate_limit = None;
                        cx.notify();
                    })
                    .ok();
                }));
            }
            None => self.rate_limit = Some(r),
        }
        cx.notify();
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
    use wai_ext::Bar;
    use wai_ext::panel::Tone;
    use wai_settings::AppState;

    use super::{
        RateLimit, Status, VersionClick, clock, item_tooltip, mcp_status, ordered, rate_limit,
        stopped_status, usage_status, version, version_click, version_status,
    };
    use crate::claude_config::NO_CLAUDE;
    use crate::settings_view::Section;
    use crate::sidebar::tests::label;
    use crate::update::{Release, Staged, UpdateStatus};
    use crate::workspace::tests::{
        click, fake_settings, launch, menu_item, open, open_at, question, start_now,
    };
    use serde_json::json;
    use wai_agent::SessionEvent;

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

    #[test]
    fn mcp_status_by_state() {
        use crate::settings_view::claude::McpCheck;
        use wai_settings::claude::McpStatus::{Connected, Failed, NeedsAuth};
        let done = |servers: &[(&str, wai_settings::claude::McpStatus)]| {
            McpCheck::Done(
                servers
                    .iter()
                    .map(|(n, s)| (n.to_string(), s.clone()))
                    .collect(),
            )
        };
        let status = |text: &str, tone, tooltip: &str| {
            Some(Status {
                text: text.into(),
                tone,
                tooltip: tooltip.into(),
            })
        };
        assert_eq!(mcp_status(&McpCheck::NotRun), None);
        assert_eq!(
            mcp_status(&McpCheck::Running).map(|s| (s.text, s.tone)),
            Some(("MCP …".into(), Tone::Neutral))
        );
        assert_eq!(mcp_status(&McpCheck::Failed(NO_CLAUDE.into())), None);
        assert_eq!(
            mcp_status(&McpCheck::Failed("Check failed: x".into())),
            status("MCP", Tone::Warning, "Check failed: x")
        );
        assert_eq!(mcp_status(&done(&[])), None);
        assert_eq!(
            mcp_status(&done(&[("a", Connected), ("b", Connected)])),
            status("MCP 2", Tone::Neutral, "2 MCP servers connected")
        );
        assert_eq!(
            mcp_status(&done(&[
                ("a", Connected),
                ("b", NeedsAuth),
                ("c", NeedsAuth)
            ])),
            status("MCP 1 · 2 needs auth", Tone::Warning, "Needs a login: b, c")
        );
        assert_eq!(
            mcp_status(&done(&[
                ("a", Connected),
                ("b", Failed("down".into())),
                ("c", NeedsAuth)
            ])),
            status(
                "MCP 1 · 1 failed",
                Tone::Danger,
                "Failed: b\nNeeds a login: c"
            )
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
    fn rate_limit_reads_status_and_reset() {
        let read = |v: serde_json::Value| rate_limit(&v);
        let warn = |resets_at| {
            Some(RateLimit {
                status: "allowed_warning".into(),
                resets_at,
            })
        };
        assert_eq!(
            read(json!({"status":"allowed_warning","resetsAt":1800000000})),
            warn(Some(1800000000))
        );
        assert_eq!(
            read(json!({"status":"allowed_warning","resetsAt":1800000000000_i64})),
            warn(Some(1800000000))
        );
        assert_eq!(
            read(json!({"status":"allowed_warning","resetsAt":1800000000.0})),
            warn(Some(1800000000))
        );
        assert_eq!(read(json!({"status":"allowed_warning"})), warn(None));
        assert_eq!(read(json!({"resetsAt":1800000000})), None);
    }

    #[test]
    fn usage_status_by_state() {
        let usage = |status: &str, resets_at| {
            let s = usage_status(&RateLimit {
                status: status.into(),
                resets_at,
            });
            (s.text, s.tone, s.tooltip)
        };
        assert_eq!(
            usage("allowed", None),
            ("Usage ok".into(), Tone::Neutral, "allowed".into())
        );
        assert_eq!(
            usage("allowed_warning", Some(1800000000)),
            (
                format!("Near usage limit · resets {}", clock(1800000000)),
                Tone::Warning,
                "allowed_warning".into()
            )
        );
        assert_eq!(
            usage("rejected", None),
            (
                "Usage limit reached".into(),
                Tone::Danger,
                "rejected".into()
            )
        );
        assert_eq!(
            usage("weird", None),
            ("weird".into(), Tone::Neutral, "weird".into())
        );
    }

    fn unix_now() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    fn report(status: &str, resets_at: Option<i64>) -> RateLimit {
        RateLimit {
            status: status.into(),
            resets_at,
        }
    }

    #[gpui_kit::test]
    fn rejected_usage_hides_at_its_reset(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        let set = |r, cx: &mut VisualTestContext| {
            workspace.update(cx, |w, cx| w.set_rate_limit(r, cx));
        };
        set(report("rejected", Some(unix_now() + 60)), cx);
        let text = label("bottombar-usage", cx).expect("drawn");
        assert!(text.starts_with("Usage limit reached · resets "), "{text}");
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(61));
        cx.run_until_parked();
        assert_eq!(label("bottombar-usage", cx), None);

        // A newer report without a reset drops the older wake.
        set(report("rejected", Some(unix_now() + 60)), cx);
        set(report("allowed", None), cx);
        cx.executor()
            .advance_clock(std::time::Duration::from_secs(61));
        cx.run_until_parked();
        assert_eq!(label("bottombar-usage", cx).as_deref(), Some("Usage ok"));
    }

    /// Review focus 2: a reset at or before now is stale, and nothing shows.
    #[gpui_kit::test]
    fn stale_usage_is_not_shown(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        workspace.update(cx, |w, cx| {
            w.set_rate_limit(report("rejected", Some(unix_now() - 5)), cx)
        });
        assert_eq!(label("bottombar-usage", cx), None);
    }

    #[gpui_kit::test]
    fn rate_limit_event_reaches_the_workspace(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the fake agent is a real process
        let (workspace, cx) = open(cx, fake_settings(""));
        let session = start_now(&workspace, cx);
        let usage = SessionEvent::Usage {
            used: 0,
            size: 0,
            cost_usd: None,
            rate_limit: Some(json!({"status": "allowed"})),
        };
        session.update(cx, |s, cx| s.handle_event(&usage, cx));
        assert_eq!(label("bottombar-usage", cx).as_deref(), Some("Usage ok"));
    }

    #[test]
    fn item_tooltip_names_the_extension() {
        use crate::ext_host::BarItem;
        use wai_ext::panel::ItemState;
        let mut item = BarItem {
            key: "bars/prs".into(),
            ext: wai_ext::parse_manifest(
                "id = \"bars\"\nname = \"Bars\"\nversion = \"1.0.0\"\n",
                std::path::Path::new("."),
            )
            .unwrap()
            .id,
            ext_name: "Bars".into(),
            id: "prs".into(),
            bar: Bar::Bottom,
            title: "PRs".into(),
            state: Some(ItemState {
                tooltip: Some("2 green".into()),
                ..Default::default()
            }),
        };
        assert_eq!(item_tooltip(&item), "2 green — Bars");
        item.state = None;
        assert_eq!(item_tooltip(&item), "PRs — Bars");
    }

    /// Hiding a top-bar button removes it, not its shortcut.
    #[gpui_kit::test]
    fn top_bar_buttons_hide(cx: &mut TestAppContext) {
        let (workspace, cx) = open(cx, fake_settings(""));
        assert!(label("topbar-terminal", cx).is_some());
        menu_item("topbar", "Terminal", cx);
        assert!(label("topbar-terminal", cx).is_none());
        menu_item("topbar", "New session", cx);
        assert!(label("sidebar-new", cx).is_none());
        cx.simulate_keystrokes("secondary-n");
        workspace.read_with(cx, |w, _| assert_eq!(w.drafts.len(), 1));
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
