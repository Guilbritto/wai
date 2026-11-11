//! The Claude pages of the Settings modal: what Claude Code keeps on disk, in groups of rows.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::*;
use wai_settings::claude::{ClaudeData, Entry, McpSource, McpStatus, short};

use super::{Section, SettingsEvent, SettingsView};
use crate::aurora;

#[derive(Debug, PartialEq)]
pub(crate) struct Group {
    pub title: String,
    pub rows: Vec<Row>,
    pub add_soon: bool,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Row {
    pub name: String,
    pub badge: Option<String>,
    pub path: Option<String>,
    pub summary: Option<String>,
    pub status: Option<(Tone, String)>,
    pub control: Control,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Tone {
    Muted,
    Ok,
    Warn,
    Err,
}

/// The last `claude mcp list` run in this modal.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum McpCheck {
    #[default]
    NotRun,
    Running,
    Done(BTreeMap<String, McpStatus>),
    Failed(String),
}

/// The status line of an MCP server row.
pub(crate) fn status_line(check: &McpCheck, name: &str, enabled: bool) -> (Tone, String) {
    let muted = |t: &str| (Tone::Muted, t.to_string());
    if !enabled {
        return muted("Off for this project");
    }
    match check {
        McpCheck::NotRun => muted("Not checked"),
        McpCheck::Running => muted("Checking…"),
        McpCheck::Failed(t) => (Tone::Warn, t.clone()),
        McpCheck::Done(map) => match map.get(name) {
            Some(McpStatus::Connected) => (Tone::Ok, "Connected".into()),
            Some(McpStatus::NeedsAuth) => (Tone::Warn, "Needs sign-in · run /mcp in Claude".into()),
            Some(McpStatus::Failed(r)) if r.is_empty() => (Tone::Err, "Failed to connect".into()),
            Some(McpStatus::Failed(r)) => {
                (Tone::Err, format!("Failed to connect · {}", clamp(r, 160)))
            }
            Some(McpStatus::Pending) => (
                Tone::Warn,
                "Pending approval · start Claude in this project to approve".into(),
            ),
            Some(McpStatus::Disabled) => muted("Off for this project"),
            Some(McpStatus::Other(t)) => (Tone::Muted, clamp(t, 160)),
            None => muted("Not listed by claude mcp list"),
        },
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum Control {
    Plugin {
        ix: usize,
        on: bool,
        locked: bool,
    },
    Mcp {
        ix: usize,
        on: bool,
    },
    Open {
        kind: &'static str,
        ix: usize,
        path: PathBuf,
        read_only: bool,
    },
    Soon,
}

/// The plugins, one group per marketplace.
pub(crate) fn plugin_groups(data: &ClaudeData, home: &Path) -> Vec<Group> {
    let mut groups: BTreeMap<&str, Vec<Row>> = BTreeMap::new();
    for (ix, p) in data.plugins.iter().enumerate() {
        let badge = match p.version.chars().next() {
            None => None,
            Some(c) if c.is_ascii_digit() => Some(format!("v{}", p.version)),
            Some(_) => Some(p.version.clone()),
        };
        groups.entry(&p.marketplace).or_default().push(Row {
            name: p.name.clone(),
            badge,
            path: Some(short(&p.root, home)),
            summary: Some(p.summary.clone()).filter(|s| !s.is_empty()),
            status: p.locked.clone().map(|text| (Tone::Muted, text)),
            control: Control::Plugin {
                ix,
                on: p.enabled,
                locked: p.locked.is_some(),
            },
        });
    }
    (groups.into_iter())
        .map(|(title, rows)| Group {
            title: title.to_string(),
            rows,
            add_soon: true,
        })
        .collect()
}

/// The MCP servers by source; the claude.ai connectors group is always there, as a Coming soon row.
pub(crate) fn mcp_groups(data: &ClaudeData, check: &McpCheck) -> Vec<Group> {
    let mut groups: [(&str, Vec<Row>); 3] = [
        ("From plugins", vec![]),
        ("User", vec![]),
        ("This project", vec![]),
    ];
    for (ix, s) in data.mcp.iter().enumerate() {
        let (g, badge) = match &s.source {
            McpSource::Plugin(name) => (0, format!("plugin: {name}")),
            McpSource::User => (1, "user".to_string()),
            McpSource::Local => (2, "local".to_string()),
            McpSource::Project => (2, ".mcp.json".to_string()),
        };
        groups[g].1.push(Row {
            name: s.label.clone(),
            badge: Some(badge),
            path: None,
            summary: Some(s.transport.to_string()),
            status: Some(status_line(check, &s.name, s.enabled)),
            control: Control::Mcp { ix, on: s.enabled },
        });
    }
    let connectors = Group {
        title: "claude.ai connectors".into(),
        rows: vec![Row {
            name: "Connectors from your claude.ai account".into(),
            badge: None,
            path: None,
            summary: Some("They live in your Claude account, not in a local file".into()),
            status: None,
            control: Control::Soon,
        }],
        add_soon: false,
    };
    (groups.into_iter())
        .filter(|(_, rows)| !rows.is_empty())
        .map(|(title, rows)| Group {
            title: title.into(),
            rows,
            add_soon: false,
        })
        .chain([connectors])
        .collect()
}

/// Entries (skills, agents, ...) in groups, in the entries' order; each row opens its file.
pub(crate) fn entry_groups(entries: &[Entry], kind: &'static str, home: &Path) -> Vec<Group> {
    let mut groups: Vec<Group> = Vec::new();
    for (ix, e) in entries.iter().enumerate() {
        if groups.last().is_none_or(|g| g.title != e.group) {
            groups.push(Group {
                title: e.group.clone(),
                rows: Vec::new(),
                add_soon: matches!(e.group.as_str(), "User" | "Project"),
            });
        }
        // Hooks from plugins share one group; the plugin is the row's name.
        let plugin = if e.group == "From plugins" {
            &e.name
        } else {
            &e.group
        };
        let path = if e.read_only {
            format!("plugin: {plugin}")
        } else {
            short(&e.path, home)
        };
        groups.last_mut().expect("pushed above").rows.push(Row {
            name: e.name.clone(),
            badge: None,
            path: Some(path),
            summary: Some(e.summary.clone()).filter(|s| !s.is_empty()),
            status: None,
            control: Control::Open {
                kind,
                ix,
                path: e.path.clone(),
                read_only: e.read_only,
            },
        });
    }
    groups
}

/// Keeps the rows whose name, badge, path or summary hold `query`, and the groups left with any.
pub(crate) fn filter(groups: Vec<Group>, query: &str) -> Vec<Group> {
    let query = query.trim().to_lowercase();
    if query.is_empty() {
        return groups;
    }
    let has = |s: &str| s.to_lowercase().contains(&query);
    let keep = |r: &Row| {
        has(&r.name)
            || r.badge.as_deref().is_some_and(has)
            || r.path.as_deref().is_some_and(has)
            || r.summary.as_deref().is_some_and(has)
    };
    (groups.into_iter())
        .map(|mut g| {
            g.rows.retain(|r| keep(r));
            g
        })
        .filter(|g| !g.rows.is_empty())
        .collect()
}

/// Big pages open only their first group.
pub(crate) fn default_open(total_rows: usize, ix: usize) -> bool {
    total_rows <= 20 || ix == 0
}

fn clamp(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", text[..end].trim_end()),
        None => text.to_string(),
    }
}

impl SettingsView {
    pub fn set_claude(&mut self, data: ClaudeData, cx: &mut Context<Self>) {
        self.claude = Some(data);
        self.busy = false;
        cx.notify();
    }

    pub fn set_mcp_check(&mut self, check: McpCheck, cx: &mut Context<Self>) {
        self.mcp_check = check;
        cx.notify();
    }

    #[cfg(test)]
    pub fn mcp_check(&self) -> &McpCheck {
        &self.mcp_check
    }

    #[cfg(test)]
    pub fn claude(&self) -> Option<&ClaudeData> {
        self.claude.as_ref()
    }

    /// Says the user flipped a plugin; the switches wait for the write.
    fn toggle_plugin(&mut self, ix: usize, on: bool, cx: &mut Context<Self>) {
        let Some(data) = self.claude.as_ref().filter(|_| !self.busy) else {
            return;
        };
        let Some(plugin) = data.plugins.get(ix) else {
            return;
        };
        let event = SettingsEvent::TogglePlugin {
            id: plugin.id.clone(),
            on,
            seen: data.settings_stamp,
        };
        self.busy = true;
        cx.emit(event);
        cx.notify();
    }

    /// The buttons beside a Claude page's title.
    pub(super) fn claude_actions(&self, section: Section, cx: &mut Context<Self>) -> Option<Div> {
        let buttons = match section {
            Section::Plugins => [
                Button::new("soon-browse").small().label("Browse"),
                Button::new("soon-plugins-menu")
                    .ghost()
                    .small()
                    .icon(IconName::Ellipsis),
            ]
            .into_iter()
            .collect::<Vec<_>>(),
            Section::Mcp => {
                let running = self.mcp_check == McpCheck::Running;
                let check = Button::new("mcp-check")
                    .small()
                    .label(if running {
                        "Checking…"
                    } else {
                        "Check status"
                    })
                    .disabled(running)
                    .on_click(cx.listener(|_, _, _, cx| cx.emit(SettingsEvent::CheckMcp)));
                return Some(
                    h_flex().gap_2().items_center().child(check).child(
                        Button::new("soon-add-server")
                            .small()
                            .label("Add server…")
                            .disabled(true),
                    ),
                );
            }
            _ => return None,
        };
        let mut row = h_flex().gap_2().items_center();
        for (ix, button) in buttons.into_iter().enumerate() {
            row = row.child(button.disabled(true));
            if ix == 0 {
                row = row.child(aurora::soon(cx));
            }
        }
        Some(row)
    }

    pub(super) fn claude_page(&self, section: Section, query: &str, cx: &mut Context<Self>) -> Div {
        let (muted, danger) = (cx.theme().muted_foreground, cx.theme().danger);
        let Some(data) = &self.claude else {
            return v_flex().child(div().text_color(muted).child("Reading ~/.claude…"));
        };
        let learn_more = (section == Section::Plugins).then(|| {
            div()
                .id("plugins-learn-more")
                .text_color(cx.theme().link)
                .cursor_pointer()
                .child("Learn more about plugins")
                .on_click(|_, _, cx| cx.open_url("https://code.claude.com/docs/en/plugins"))
                .test_support()
        });
        let problems = data
            .problems
            .iter()
            .map(|p| div().px_3().py_2().text_color(danger).child(p.clone()));
        let all = match section {
            Section::Mcp => mcp_groups(data, &self.mcp_check),
            Section::Skills => entry_groups(&data.skills, "skill", &self.data.home),
            Section::Instructions => {
                entry_groups(&data.instructions, "instruction", &self.data.home)
            }
            Section::Agents => entry_groups(&data.agents, "agent", &self.data.home),
            _ => plugin_groups(data, &self.data.home),
        };
        let total: usize = all.iter().map(|g| g.rows.len()).sum();
        let searching = !query.is_empty();
        let groups = filter(all, query);
        v_flex()
            .gap_3()
            .children(learn_more)
            .children(problems)
            .children(groups.into_iter().enumerate().map(|(ix, group)| {
                let open =
                    searching || default_open(total, ix) != self.toggled.contains(&(section, ix));
                self.group(section, ix, group, open, cx)
            }))
    }

    fn group(
        &self,
        section: Section,
        ix: usize,
        group: Group,
        open: bool,
        cx: &mut Context<Self>,
    ) -> Div {
        let (muted, faint) = (cx.theme().muted_foreground, aurora::nevoa_2());
        let header = h_flex()
            .gap_2()
            .items_center()
            .child(
                h_flex()
                    .id(("settings-group", ix))
                    .flex_1()
                    .gap_2()
                    .items_center()
                    .cursor_pointer()
                    .child(
                        gpui_kit::component::Icon::new(if open {
                            IconName::ChevronDown
                        } else {
                            IconName::ChevronRight
                        })
                        .small()
                        .text_color(faint),
                    )
                    .child(
                        div()
                            .font_weight(FontWeight::BOLD)
                            .child(group.title.clone()),
                    )
                    .child(
                        aurora::pill(cx)
                            .h(px(20.))
                            .child(group.rows.len().to_string()),
                    )
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.toggled.remove(&(section, ix)) {
                            this.toggled.insert((section, ix));
                        }
                        cx.notify();
                    }))
                    .test_support(),
            )
            .children(group.add_soon.then(|| {
                Button::new(("soon-add", ix))
                    .ghost()
                    .small()
                    .icon(IconName::Plus)
                    .tooltip("Coming soon")
                    .disabled(true)
            }));
        let rows = open.then(|| {
            v_flex().children(group.rows.iter().map(|row| self.claude_row(row, muted, cx)))
        });
        v_flex().gap_1().child(header).children(rows)
    }

    fn claude_row(&self, row: &Row, muted: Hsla, cx: &mut Context<Self>) -> Div {
        let status = row.status.as_ref().map(|(tone, text)| {
            let color = match tone {
                Tone::Muted => muted,
                Tone::Ok => cx.theme().success,
                Tone::Warn => cx.theme().warning,
                Tone::Err => cx.theme().danger,
            };
            h_flex()
                .gap_1()
                .items_center()
                .child(div().size(px(7.)).rounded_full().bg(color))
                .child(div().text_color(color).child(text.clone()))
        });
        let control = match &row.control {
            Control::Plugin { ix, on, locked } => {
                let ix = *ix;
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(
                        Switch::new(("plugin-toggle", ix))
                            .checked(*on)
                            .disabled(*locked || self.busy || self.claude.is_none())
                            .on_click(cx.listener(move |this, on: &bool, _, cx| {
                                this.toggle_plugin(ix, *on, cx)
                            })),
                    )
                    .child(
                        Button::new(("plugin-menu", ix))
                            .ghost()
                            .small()
                            .icon(IconName::Ellipsis)
                            .tooltip("Update, uninstall: coming soon")
                            .disabled(true),
                    )
            }
            Control::Mcp { ix, on } => h_flex()
                .gap_2()
                .items_center()
                .child(Switch::new(("mcp-toggle", *ix)).checked(*on).disabled(true))
                .child(aurora::soon(cx)),
            Control::Open {
                kind,
                ix,
                path,
                read_only,
            } => {
                let (path, read_only) = (path.clone(), *read_only);
                h_flex().child(
                    Button::new(ElementId::NamedInteger(
                        format!("open-{kind}").into(),
                        *ix as u64,
                    ))
                    .ghost()
                    .small()
                    .icon(IconName::ExternalLink)
                    .label("Open")
                    .on_click(cx.listener(move |_, _, _, cx| {
                        cx.emit(SettingsEvent::Open {
                            path: path.clone(),
                            read_only,
                        })
                    })),
                )
            }
            Control::Soon => h_flex().child(aurora::soon(cx)),
        };
        h_flex()
            .justify_between()
            .items_center()
            .gap_4()
            .py_2()
            .child(
                v_flex()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .child(clamp(&row.name, 80)),
                            )
                            .children(row.badge.clone().map(|b| aurora::badge(b, cx))),
                    )
                    .children(row.path.clone().map(|p| {
                        div()
                            .font_family(cx.theme().mono_font_family.clone())
                            .text_xs()
                            .text_color(muted)
                            .child(p)
                    }))
                    .children(
                        (row.summary.as_deref())
                            .map(|s| div().text_color(muted).child(clamp(s, 160))),
                    )
                    .children(status),
            )
            .child(control)
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use wai_settings::claude::{ClaudeData, McpServer, McpSource, Plugin};

    use super::{
        Control, McpCheck, Tone, default_open, filter, mcp_groups, plugin_groups, status_line,
    };
    use wai_settings::claude::McpStatus;

    fn plugin(id: &str, version: &str, enabled: bool, locked: Option<&str>) -> Plugin {
        let (name, marketplace) = id.split_once('@').unwrap();
        Plugin {
            id: id.into(),
            name: name.into(),
            marketplace: marketplace.into(),
            version: version.into(),
            root: PathBuf::from(format!(
                "/h/.claude/plugins/cache/{marketplace}/{name}/{version}"
            )),
            enabled,
            locked: locked.map(Into::into),
            summary: format!("{name} tools"),
        }
    }

    fn data() -> ClaudeData {
        ClaudeData {
            plugins: vec![
                plugin("alpha@m", "1.2.0", true, None),
                plugin(
                    "beta@m",
                    "abc1234",
                    false,
                    Some("Set in this project's .claude/settings.json"),
                ),
                plugin("delta@n", "2.0.0", true, None),
            ],
            ..Default::default()
        }
    }

    #[test]
    fn plugin_rows_group_by_marketplace() {
        let groups = plugin_groups(&data(), Path::new("/h"));
        let titles: Vec<_> = groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["m", "n"]);
        let badges: Vec<_> = groups[0].rows.iter().map(|r| r.badge.as_deref()).collect();
        assert_eq!(badges, [Some("v1.2.0"), Some("abc1234")]);
        let (alpha, beta) = (&groups[0].rows[0], &groups[0].rows[1]);
        assert_eq!(
            alpha.path.as_deref(),
            Some("~/.claude/plugins/cache/m/alpha/1.2.0")
        );
        assert_eq!(
            alpha.control,
            Control::Plugin {
                ix: 0,
                on: true,
                locked: false
            }
        );
        assert_eq!(
            beta.control,
            Control::Plugin {
                ix: 1,
                on: false,
                locked: true
            }
        );
        assert_eq!(
            beta.status,
            Some((
                Tone::Muted,
                "Set in this project's .claude/settings.json".into()
            ))
        );
        assert!(groups.iter().all(|g| g.add_soon));
    }

    #[test]
    fn filter_keeps_matching_rows_and_drops_empty_groups() {
        let home = Path::new("/h");
        let groups = filter(plugin_groups(&data(), home), "ALP");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].rows.len(), 1);
        assert_eq!(
            filter(plugin_groups(&data(), home), ""),
            plugin_groups(&data(), home)
        );
    }

    #[test]
    fn mcp_rows_show_source_and_transport() {
        let server = |name: &str, source, transport, enabled| McpServer {
            name: name.into(),
            label: name.into(),
            source,
            transport,
            enabled,
        };
        let data = ClaudeData {
            mcp: vec![
                server("db", McpSource::Plugin("alpha".into()), "stdio", false),
                server("web", McpSource::Plugin("beta".into()), "HTTP", true),
                server("u", McpSource::User, "stdio", true),
                server("l", McpSource::Local, "SSE", true),
                server("p", McpSource::Project, "WebSocket", true),
            ],
            ..Default::default()
        };
        let groups = mcp_groups(&data, &McpCheck::NotRun);
        let titles: Vec<_> = groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "From plugins",
                "User",
                "This project",
                "claude.ai connectors"
            ]
        );
        let rows: Vec<_> = groups[..3].iter().flat_map(|g| &g.rows).collect();
        let badges: Vec<_> = rows.iter().map(|r| r.badge.as_deref().unwrap()).collect();
        assert_eq!(
            badges,
            [
                "plugin: alpha",
                "plugin: beta",
                "user",
                "local",
                ".mcp.json"
            ]
        );
        let summaries: Vec<_> = rows.iter().map(|r| r.summary.as_deref().unwrap()).collect();
        assert_eq!(summaries, ["stdio", "HTTP", "stdio", "SSE", "WebSocket"]);
        assert_eq!(
            rows[0].status,
            Some((Tone::Muted, "Off for this project".into()))
        );
        assert_eq!(rows[1].status, Some((Tone::Muted, "Not checked".into())));
        let controls: Vec<_> = rows.iter().map(|r| &r.control).collect();
        assert_eq!(controls[0], &Control::Mcp { ix: 0, on: false });
        assert_eq!(controls[4], &Control::Mcp { ix: 4, on: true });
        let connectors = &groups[3];
        assert!(!connectors.add_soon);
        assert_eq!(connectors.rows.len(), 1);
        assert_eq!(
            connectors.rows[0].name,
            "Connectors from your claude.ai account"
        );
        assert_eq!(
            connectors.rows[0].summary.as_deref(),
            Some("They live in your Claude account, not in a local file")
        );
        assert_eq!(connectors.rows[0].control, Control::Soon);

        let empty = mcp_groups(&ClaudeData::default(), &McpCheck::NotRun);
        assert_eq!(empty.len(), 1);
        assert_eq!(empty[0].title, "claude.ai connectors");
    }

    #[test]
    fn status_lines_read_plainly() {
        use std::collections::BTreeMap;
        let line = |check: &McpCheck, name: &str, on: bool| status_line(check, name, on);
        let t = |tone, text: &str| (tone, text.to_string());
        let done = |s| McpCheck::Done(BTreeMap::from([("a".to_string(), s)]));
        assert_eq!(
            line(&McpCheck::Running, "a", false),
            t(Tone::Muted, "Off for this project")
        );
        assert_eq!(
            line(&McpCheck::NotRun, "a", true),
            t(Tone::Muted, "Not checked")
        );
        assert_eq!(
            line(&McpCheck::Running, "a", true),
            t(Tone::Muted, "Checking…")
        );
        assert_eq!(
            line(&McpCheck::Failed("Needs the claude CLI".into()), "a", true),
            t(Tone::Warn, "Needs the claude CLI")
        );
        assert_eq!(
            line(&done(McpStatus::Connected), "a", true),
            t(Tone::Ok, "Connected")
        );
        assert_eq!(
            line(&done(McpStatus::NeedsAuth), "a", true),
            t(Tone::Warn, "Needs sign-in · run /mcp in Claude")
        );
        assert_eq!(
            line(&done(McpStatus::Failed("HTTP 400".into())), "a", true),
            t(Tone::Err, "Failed to connect · HTTP 400")
        );
        assert_eq!(
            line(&done(McpStatus::Failed(String::new())), "a", true),
            t(Tone::Err, "Failed to connect")
        );
        let long = line(&done(McpStatus::Failed("x".repeat(200))), "a", true).1;
        assert_eq!(long, format!("Failed to connect · {}…", "x".repeat(160)));
        assert_eq!(
            line(&done(McpStatus::Pending), "a", true),
            t(
                Tone::Warn,
                "Pending approval · start Claude in this project to approve"
            )
        );
        assert_eq!(
            line(&done(McpStatus::Disabled), "a", true),
            t(Tone::Muted, "Off for this project")
        );
        assert_eq!(
            line(&done(McpStatus::Other("odd".into())), "a", true),
            t(Tone::Muted, "odd")
        );
        assert_eq!(
            line(&done(McpStatus::Connected), "b", true),
            t(Tone::Muted, "Not listed by claude mcp list")
        );
    }

    #[test]
    fn entry_rows_group_and_open() {
        use wai_settings::claude::Entry;
        let entry = |group: &str, name: &str, read_only| Entry {
            group: group.into(),
            name: name.into(),
            path: PathBuf::from(format!("/h/.claude/skills/{name}/SKILL.md")),
            summary: String::new(),
            read_only,
        };
        let entries = [
            entry("User", "mine", false),
            entry("Project", "local", false),
            entry("alpha", "one", true),
        ];
        let groups = super::entry_groups(&entries, "skill", Path::new("/h"));
        let titles: Vec<_> = groups.iter().map(|g| g.title.as_str()).collect();
        assert_eq!(titles, ["User", "Project", "alpha"]);
        let soon: Vec<_> = groups.iter().map(|g| g.add_soon).collect();
        assert_eq!(soon, [true, true, false]);
        assert_eq!(groups[2].rows[0].path.as_deref(), Some("plugin: alpha"));
        assert_eq!(
            groups[0].rows[0].path.as_deref(),
            Some("~/.claude/skills/mine/SKILL.md")
        );
        assert_eq!(groups[0].rows[0].summary, None);
        for (ix, g) in groups.iter().enumerate() {
            assert!(matches!(
                g.rows[0].control,
                Control::Open { kind: "skill", ix: i, .. } if i == ix
            ));
        }
    }

    #[test]
    fn big_pages_open_only_the_first_group() {
        assert!(default_open(10, 1));
        assert!(default_open(21, 0));
        assert!(!default_open(21, 1));
    }
}
