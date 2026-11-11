//! The Claude pages of the Settings modal: what Claude Code keeps on disk, in groups of rows.

use std::collections::BTreeMap;
use std::path::Path;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, IconName, Sizable as _, h_flex, v_flex,
};
use gpui_kit::*;
use wai_settings::claude::{ClaudeData, short};

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
}

#[derive(Debug, PartialEq)]
pub(crate) enum Control {
    Plugin { ix: usize, on: bool, locked: bool },
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
    pub(super) fn claude_actions(&self, section: Section, cx: &App) -> Option<Div> {
        (section == Section::Plugins).then(|| {
            h_flex()
                .gap_2()
                .items_center()
                .child(
                    Button::new("soon-browse")
                        .small()
                        .label("Browse")
                        .disabled(true),
                )
                .child(aurora::soon(cx))
                .child(
                    Button::new("soon-plugins-menu")
                        .ghost()
                        .small()
                        .icon(IconName::Ellipsis)
                        .disabled(true),
                )
        })
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
        let all = plugin_groups(data, &self.data.home);
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

    use wai_settings::claude::{ClaudeData, Plugin};

    use super::{Control, Tone, default_open, filter, plugin_groups};

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
    fn big_pages_open_only_the_first_group() {
        assert!(default_open(10, 1));
        assert!(default_open(21, 0));
        assert!(!default_open(21, 1));
    }
}
