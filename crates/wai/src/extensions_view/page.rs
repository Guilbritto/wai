//! The Extensions page: the list on the left, the selected extension's page on the right.

use gpui_kit::assets::IconName as Lucide;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::{DropdownMenu as _, PopupMenuItem};
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_ext::index::Listing;
use wai_ext::panel::clamp;
use wai_ext::{ExtId, Installed, Manifest};

use super::{
    Available, DESCRIPTION_CHARS, DetailTab, ExtensionsView, InstallPlugin, LIMITATION, NAME_CHARS,
    PLUGINS_SHOWN, permissions_text, plugin_install,
};
use crate::appearance::fonts;
use crate::aurora::{self, DISPLAY_FONT};
use crate::campo;
use crate::claude_plugins;
use crate::ext_host::ExtStatus;
use crate::marketplace::Entry;

/// What a row says under an installed extension's name.
#[derive(Debug, Clone, PartialEq)]
pub(super) enum RowStatus {
    Running,
    Starting,
    Enabled,
    Crashed(String),
    Failed(String),
    Disabled,
}

impl RowStatus {
    pub(super) fn label(&self) -> &'static str {
        match self {
            Self::Running => "Running",
            Self::Starting => "Starting",
            Self::Enabled => "Enabled",
            Self::Crashed(_) => "Crashed",
            Self::Failed(_) => "Failed",
            Self::Disabled => "Disabled",
        }
    }
}

pub(super) fn row_status(enabled: bool, status: Option<&ExtStatus>) -> RowStatus {
    match status {
        _ if !enabled => RowStatus::Disabled,
        None => RowStatus::Enabled,
        Some(ExtStatus::Ready) => RowStatus::Running,
        Some(ExtStatus::Starting) => RowStatus::Starting,
        Some(ExtStatus::Crashed(tail)) => RowStatus::Crashed(tail.clone()),
        Some(ExtStatus::Failed(error)) => RowStatus::Failed(error.clone()),
    }
}

pub(super) fn listing_for<'a>(id: &ExtId, entries: &'a [Entry]) -> Option<&'a Entry> {
    entries.iter().find(|e| e.listing.id == *id)
}

/// The listing to update `ext` to: same id, a package for this platform, another version.
pub(super) fn listed_update<'a>(ext: &Installed, entries: &'a [Entry]) -> Option<&'a Listing> {
    let listing = &listing_for(&ext.manifest.id, entries)?.listing;
    (listing.package().is_some() && listing.version != ext.manifest.version).then_some(listing)
}

/// The manifest's contributions by kind, only the kinds it has.
pub(super) fn features(m: &Manifest) -> Vec<(&'static str, Vec<String>)> {
    fn names<T>(list: &[T], name: impl Fn(&T) -> &str) -> Vec<String> {
        list.iter().map(|t| clamp(name(t), NAME_CHARS)).collect()
    }
    let c = &m.contributes;
    [
        ("Panels", names(&c.panels, |p| &p.title)),
        ("Bar items", names(&c.items, |i| &i.title)),
        ("Commands", names(&c.commands, |c| &c.title)),
        ("Agents", names(&c.agents, |a| &a.name)),
        ("MCP servers", names(&c.mcp_servers, |s| &s.name)),
        ("Templates", names(&c.templates, |t| &t.name)),
        ("Themes", names(&c.themes, |t| t)),
    ]
    .into_iter()
    .filter(|(_, names)| !names.is_empty())
    .collect()
}

/// A GitHub repository's releases page.
pub(super) fn releases_url(repository: &str) -> Option<String> {
    let repo = repository.trim_end_matches('/');
    let path = repo.strip_prefix("https://github.com/")?;
    let (owner, name) = path.split_once('/')?;
    (!owner.is_empty() && !name.is_empty() && !name.contains('/'))
        .then(|| format!("{repo}/releases"))
}

/// The monogram's color, picked from the id: one of six.
pub(super) fn tint(id: &str) -> usize {
    id.bytes().map(usize::from).sum::<usize>() % 6
}

/// The search: any field contains the query, ignoring case.
pub(super) fn matches(query: &str, fields: &[&str]) -> bool {
    let query = query.trim().to_lowercase();
    query.is_empty() || fields.iter().any(|f| f.to_lowercase().contains(&query))
}

pub(super) fn platform_label(key: &str) -> String {
    let (os, arch) = key.split_once('-').unwrap_or((key, ""));
    let os = match os {
        "darwin" => "macOS",
        "linux" => "Linux",
        "windows" => "Windows",
        "any" => "Any",
        other => other,
    };
    format!("{os} {arch}").trim_end().to_string()
}

/// The detail's subject: an installed extension, with its store index, or a listing.
enum Subject<'a> {
    Installed(usize, &'a Installed),
    Listed(&'a Entry),
}

/// Lilas, boreal, brasa, ceu, magenta, ok: `tint` picks one.
fn monogram(id: &ExtId, name: &str, size: f32, cx: &App) -> Div {
    let tints = [
        aurora::lilas,
        aurora::boreal,
        aurora::brasa,
        aurora::ceu,
        aurora::magenta,
        aurora::ok,
    ];
    let letter = (name.chars().find(|c| c.is_alphanumeric()))
        .map_or("?".into(), |c| c.to_uppercase().to_string());
    div()
        .flex()
        .flex_none()
        .items_center()
        .justify_center()
        .size(px(size))
        .rounded(px(size * 0.26))
        .bg(tints[tint(id.as_str())](cx))
        .text_color(aurora::noite(cx))
        .font_family(DISPLAY_FONT)
        .text_size(fonts::ui(cx, size * 0.42))
        .font_weight(FontWeight::BOLD)
        .child(letter)
}

fn status_line(status: &RowStatus, cx: &App) -> Div {
    let theme = cx.theme();
    let color = match status {
        RowStatus::Running => theme.success,
        RowStatus::Starting => theme.warning,
        RowStatus::Crashed(_) | RowStatus::Failed(_) => theme.danger,
        RowStatus::Enabled | RowStatus::Disabled => theme.muted_foreground,
    };
    h_flex()
        .gap_1()
        .items_center()
        .text_xs()
        .text_color(theme.muted_foreground)
        .child(div().size(px(6.)).rounded_full().bg(color))
        .child(status.label())
}

/// Small caps over a group: ABOUT, INSTALLED.
fn heading(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .text_size(fonts::ui(cx, 11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// A card of rows, each named `<prefix>: <text>` for tests.
fn card(prefix: &'static str, lines: Vec<String>, cx: &App) -> Div {
    let border = aurora::borda(cx);
    v_flex()
        .rounded(px(12.))
        .border_1()
        .border_color(border)
        .bg(aurora::card_fill(cx))
        .text_sm()
        .children(lines.into_iter().enumerate().map(|(n, line)| {
            let selector = format!("{prefix}: {line}");
            div()
                .px_3()
                .py_2()
                .when(n > 0, |el| el.border_t_1().border_color(border))
                .debug_selector(move || selector)
                .child(line)
        }))
}

fn features_tab(ext: &Installed, cx: &App) -> Div {
    let kinds = features(&ext.manifest);
    if kinds.is_empty() {
        return (div().text_sm().text_color(cx.theme().muted_foreground))
            .child("This extension adds no panels, commands, agents, MCP servers or themes.");
    }
    v_flex()
        .gap_4()
        .children(kinds.into_iter().map(|(kind, names)| {
            (v_flex().gap_2())
                .child(heading(kind.to_uppercase(), cx))
                .child(card("ext-feature", names, cx))
        }))
}

fn permissions_tab(ext: &Installed, cx: &App) -> Div {
    let selector = format!("ext-permission: {LIMITATION}");
    v_flex()
        .gap_2()
        .child(heading("IT ASKS TO", cx))
        .child(card("ext-permission", permissions_text(&ext.granted), cx))
        .child(
            div()
                .text_xs()
                .text_color(aurora::ambar(cx))
                .debug_selector(move || selector)
                .child(LIMITATION),
        )
}

/// A label over its value, in the meta column.
fn meta_row(label: &'static str, value: impl IntoElement, cx: &App) -> Div {
    (v_flex().gap(px(2.)).min_w_0())
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .child(label),
        )
        .child(value)
}

fn mono(text: String, cx: &App) -> Div {
    (div().min_w_0().truncate().text_xs())
        .font_family(fonts::editor_family(cx))
        .child(text)
}

fn link(id: &'static str, label: &'static str, url: String, cx: &App) -> Stateful<Div> {
    (h_flex()
        .id(id)
        .gap_1()
        .items_center()
        .cursor_pointer()
        .text_sm())
    .text_color(aurora::boreal(cx))
    .child(label)
    .child(Icon::new(Lucide::ExternalLink).with_size(px(11.)))
    .on_click(move |_, _, cx| cx.open_url(&url))
}

fn muted(text: &'static str, cx: &App) -> Div {
    (div().px_3().py_2().text_sm())
        .text_color(cx.theme().muted_foreground)
        .child(text)
}

/// Where a listing came from: the user's index, or the official one.
fn index_label(entry: &Entry) -> String {
    (entry.from.as_deref()).map_or("Official index".into(), |from| clamp(from, NAME_CHARS))
}

impl ExtensionsView {
    pub(super) fn page(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let installed = self.store.list();
        let current = self.current(&installed, self.entries());
        h_flex()
            .id("extensions")
            .flex_1()
            .min_h_0()
            .w_full()
            .items_stretch()
            .child(self.list_pane(&installed, current.as_ref(), cx))
            .child(self.detail(&installed, current.as_ref(), cx))
    }

    fn select(&mut self, id: ExtId, cx: &mut Context<Self>) {
        self.selected = Some(id);
        self.tab = DetailTab::Details;
        cx.notify();
    }

    fn list_pane(
        &self,
        installed: &[Installed],
        current: Option<&ExtId>,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let entries = self.entries();
        let query = self.search.read(cx).value();
        let found = |m: &Manifest| {
            let description = m.description.as_deref().unwrap_or_default();
            matches(&query, &[&m.name, m.id.as_str(), description])
        };
        let rows: Vec<_> = (installed.iter().enumerate())
            .filter(|(_, ext)| found(&ext.manifest))
            .collect();
        let is_installed = |id: &ExtId| installed.iter().any(|i| i.manifest.id == *id);
        let listed: Vec<_> = (entries.iter().enumerate())
            .filter(|(_, e)| !is_installed(&e.listing.id))
            .filter(|(_, e)| {
                let l = &e.listing;
                let description = l.description.as_deref().unwrap_or_default();
                matches(&query, &[&l.name, l.id.as_str(), description])
            })
            .collect();
        let listings = match &self.available {
            Available::Loaded(listings) => Some(listings),
            _ => None,
        };
        let no_match = !query.trim().is_empty() && rows.is_empty() && listed.is_empty();
        let this = cx.weak_entity();
        let more = Button::new("ext-more")
            .ghost()
            .xsmall()
            .icon(Lucide::Ellipsis)
            .tooltip("More actions")
            .dropdown_menu(move |menu, _, _| {
                let (folder, git, index) = (this.clone(), this.clone(), this.clone());
                menu.item(PopupMenuItem::new("Install from folder…").on_click(
                    move |_, window, cx| {
                        folder
                            .update(cx, |this, cx| this.pick_folder(window, cx))
                            .ok();
                    },
                ))
                .item(
                    PopupMenuItem::new("Install from git URL…").on_click(move |_, window, cx| {
                        git.update(cx, |this, cx| this.install_url_dialog(window, cx))
                            .ok();
                    }),
                )
                .item(
                    PopupMenuItem::new("Add index…").on_click(move |_, window, cx| {
                        index
                            .update(cx, |this, cx| this.add_index_dialog(window, cx))
                            .ok();
                    }),
                )
            });
        let installed_open = !self.closed[0];
        let available_open = !self.closed[1];
        v_flex()
            .w(px(340.))
            .flex_none()
            .min_h_0()
            .border_r_1()
            .border_color(theme.border)
            .child(
                h_flex()
                    .px_4()
                    .pt_4()
                    .pb_2()
                    .gap_1()
                    .items_center()
                    .child(
                        div()
                            .flex_1()
                            .font_family(DISPLAY_FONT)
                            .text_size(fonts::ui(cx, 15.))
                            .font_weight(FontWeight::BOLD)
                            .child("Extensions"),
                    )
                    .child(
                        Button::new("ext-refresh")
                            .ghost()
                            .xsmall()
                            .icon(Lucide::RefreshCw)
                            .tooltip("Refresh")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.refresh(cx);
                                cx.notify();
                            })),
                    )
                    .child(more),
            )
            .child(div().px_4().pb_2().child(campo::field(
                Input::new(&self.search).small(),
                false,
                cx,
            )))
            .when(self.staging.is_some(), |el| {
                el.child(muted("Preparing…", cx))
            })
            .child(
                v_flex()
                    .id("ext-list")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_2()
                    .pb_3()
                    .child(self.section(0, "INSTALLED", Some(rows.len()), cx))
                    .when(installed_open && installed.is_empty(), |el| {
                        el.child(muted("No extensions installed", cx))
                    })
                    .when(installed_open, |el| {
                        el.children(rows.iter().map(|(ix, ext)| {
                            let selected = current == Some(&ext.manifest.id);
                            self.installed_row(*ix, ext, selected, cx)
                        }))
                    })
                    .child(self.section(1, "AVAILABLE", listings.map(|_| listed.len()), cx))
                    .when(available_open, |el| {
                        let errors = listings.map_or(&[][..], |l| &l.errors[..]);
                        el.when(!errors.is_empty(), |el| {
                            el.child(
                                v_flex()
                                    .mx_1()
                                    .mb_1()
                                    .px_3()
                                    .py_2()
                                    .gap_1()
                                    .rounded(px(8.))
                                    .border_1()
                                    .border_color(theme.warning.opacity(0.4))
                                    .bg(theme.warning.opacity(0.08))
                                    .text_xs()
                                    .text_color(theme.warning)
                                    .children(errors.iter().map(|e| {
                                        div().truncate().child(clamp(e, DESCRIPTION_CHARS))
                                    })),
                            )
                        })
                        .when(listings.is_none(), |el| {
                            el.child(muted("Loading extensions…", cx))
                        })
                        .when(listings.is_some_and(|l| l.entries.is_empty()), |el| {
                            el.child(muted("No extensions listed", cx))
                        })
                        .children(listed.iter().map(|(ix, entry)| {
                            let selected = current == Some(&entry.listing.id);
                            self.listing_row(*ix, entry, selected, cx)
                        }))
                    })
                    .when(no_match, |el| {
                        el.child(
                            muted("No extensions match", cx)
                                .debug_selector(|| "ext-no-match".into()),
                        )
                    }),
            )
    }

    /// INSTALLED, AVAILABLE: a click folds it.
    fn section(
        &self,
        ix: usize,
        title: &'static str,
        count: Option<usize>,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let chevron = match self.closed[ix] {
            true => Lucide::ChevronRight,
            false => Lucide::ChevronDown,
        };
        let id = ["ext-section-installed", "ext-section-available"][ix];
        h_flex()
            .id(id)
            .gap_1()
            .px_2()
            .pt_3()
            .pb_1()
            .items_center()
            .cursor_pointer()
            .text_size(fonts::ui(cx, 11.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(cx.theme().muted_foreground)
            .child(Icon::new(chevron).with_size(px(12.)))
            .child(div().flex_1().child(title))
            .children(count.map(|n| n.to_string()))
            .on_click(cx.listener(move |this, _, _, cx| {
                this.closed[ix] = !this.closed[ix];
                cx.notify();
            }))
            .test_support()
    }

    /// Monogram, name and version, one line of description, and a line under them.
    fn row(
        &self,
        id: ElementId,
        selected: bool,
        (ext, name, version, description): (&ExtId, &str, &str, Option<&str>),
        line: Div,
        action: AnyElement,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let select = ext.clone();
        aurora::tab(selected, cx)
            .id(id)
            .w_full()
            .gap_3()
            .px_2()
            .py_2()
            .rounded(px(8.))
            .items_center()
            .child(monogram(ext, name, 38., cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap(px(2.))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child(clamp(name, NAME_CHARS)),
                            )
                            .child(
                                div()
                                    .flex_none()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(clamp(version, NAME_CHARS)),
                            ),
                    )
                    .child(
                        div()
                            .truncate()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(clamp(description.unwrap_or_default(), DESCRIPTION_CHARS)),
                    )
                    .child(line),
            )
            .child(action)
            .on_click(cx.listener(move |this, _, _, cx| this.select(select.clone(), cx)))
            .test_support()
    }

    /// Update when an index lists another version, else the gear's Enable/Disable and Uninstall.
    fn installed_row(
        &self,
        ix: usize,
        ext: &Installed,
        selected: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let m = &ext.manifest;
        let status = row_status(ext.enabled, self.host.read(cx).status(&m.id));
        let action = match listed_update(ext, self.entries()) {
            Some(listing) => {
                let listing = listing.clone();
                Button::new(("ext-update", ix))
                    .primary()
                    .xsmall()
                    .label("Update")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.install_listing(&listing, window, cx)
                    }))
                    .into_any_element()
            }
            None => {
                let (this, id, enabled) = (cx.weak_entity(), m.id.clone(), ext.enabled);
                Button::new(("ext-manage", ix))
                    .ghost()
                    .xsmall()
                    .icon(Lucide::Settings)
                    .tooltip("Manage")
                    .dropdown_menu(move |menu, _, _| {
                        let (toggle, remove) = (this.clone(), this.clone());
                        let (on, off) = (id.clone(), id.clone());
                        let label = if enabled { "Disable" } else { "Enable" };
                        menu.item(PopupMenuItem::new(label).on_click(move |_, window, cx| {
                            (toggle.update(cx, |this, cx| {
                                this.set_enabled(&on, !enabled, window, cx)
                            }))
                            .ok();
                        }))
                        .item(
                            PopupMenuItem::new("Uninstall").on_click(move |_, window, cx| {
                                remove
                                    .update(cx, |this, cx| this.uninstall(&off, window, cx))
                                    .ok();
                            }),
                        )
                    })
                    .into_any_element()
            }
        };
        let fields = (&m.id, &*m.name, &*m.version, m.description.as_deref());
        let line = status_line(&status, cx);
        self.row(("ext-row", ix).into(), selected, fields, line, action, cx)
    }

    /// A listing that is not installed: the index it came from, and Install.
    fn listing_row(
        &self,
        ix: usize,
        entry: &Entry,
        selected: bool,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let l = &entry.listing;
        let muted = cx.theme().muted_foreground;
        let (line, action) = match l.package() {
            Some(_) => {
                let listing = l.clone();
                let install = Button::new(("ext-available", ix))
                    .primary()
                    .xsmall()
                    .label("Install")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.install_listing(&listing, window, cx)
                    }));
                (index_label(entry), install.into_any_element())
            }
            None => (
                "Not available for this platform".into(),
                div().into_any_element(),
            ),
        };
        let line = div().truncate().text_xs().text_color(muted).child(line);
        let fields = (&l.id, &*l.name, &*l.version, l.description.as_deref());
        self.row(
            ("ext-listing", ix).into(),
            selected,
            fields,
            line,
            action,
            cx,
        )
    }

    fn detail(
        &self,
        installed: &[Installed],
        current: Option<&ExtId>,
        cx: &Context<Self>,
    ) -> AnyElement {
        let subject = current.and_then(|id| {
            let mut installed = installed.iter().enumerate();
            match installed.find(|(_, i)| i.manifest.id == *id) {
                Some((ix, ext)) => Some(Subject::Installed(ix, ext)),
                None => listing_for(id, self.entries()).map(Subject::Listed),
            }
        });
        let Some(subject) = subject else {
            return (v_flex().flex_1().items_center().justify_center())
                .text_color(cx.theme().muted_foreground)
                .debug_selector(|| "ext-empty-detail".into())
                .child("Select an extension")
                .into_any_element();
        };
        let (id, tab) = match subject {
            Subject::Installed(_, ext) => (ext.manifest.id.clone(), self.tab),
            Subject::Listed(entry) => (entry.listing.id.clone(), DetailTab::Details),
        };
        let selector = format!("ext-detail: {}", id.as_str());
        v_flex()
            .id("ext-detail")
            .flex_1()
            .min_w_0()
            .min_h_0()
            .overflow_y_scroll()
            .debug_selector(move || selector)
            .child(self.hero(&subject, cx))
            .child(self.tabs(&subject, tab, cx))
            .child(
                h_flex()
                    .items_start()
                    .gap(px(28.))
                    .px(px(28.))
                    .py(px(20.))
                    .child(v_flex().flex_1().min_w_0().child(match (tab, &subject) {
                        (DetailTab::Features, Subject::Installed(_, ext)) => {
                            features_tab(ext, cx).into_any_element()
                        }
                        (DetailTab::Permissions, Subject::Installed(_, ext)) => {
                            permissions_tab(ext, cx).into_any_element()
                        }
                        _ => self.details(&subject, cx).into_any_element(),
                    }))
                    .child(self.meta(&subject, cx)),
            )
            .into_any_element()
    }

    /// INSTALLATION or LISTING, then RESOURCES when a repository is known.
    fn meta(&self, subject: &Subject, cx: &Context<Self>) -> impl IntoElement + use<> {
        let entries = self.entries();
        let column = v_flex().w(px(210.)).flex_none().gap_3();
        let (column, id) = match subject {
            Subject::Installed(_, ext) => {
                let m = &ext.manifest;
                let update = listed_update(ext, entries).map(|l| {
                    let to = format!(" → {}", clamp(&l.version, NAME_CHARS));
                    div().text_color(aurora::boreal(cx)).child(to)
                });
                let version = (h_flex().text_sm())
                    .child(clamp(&m.version, NAME_CHARS))
                    .children(update);
                let column = column
                    .child(heading("INSTALLATION", cx))
                    .child(meta_row(
                        "Identifier",
                        mono(clamp(m.id.as_str(), NAME_CHARS), cx),
                        cx,
                    ))
                    .child(meta_row("Version", version, cx))
                    .child(meta_row(
                        "Folder",
                        mono(ext.dir.display().to_string(), cx),
                        cx,
                    ));
                (column, &m.id)
            }
            Subject::Listed(entry) => {
                let l = &entry.listing;
                let chips = (h_flex().flex_wrap().gap_1()).children(
                    l.packages
                        .keys()
                        .map(|key| aurora::badge(platform_label(key), cx)),
                );
                let column = column
                    .child(heading("LISTING", cx))
                    .child(meta_row(
                        "Identifier",
                        mono(clamp(l.id.as_str(), NAME_CHARS), cx),
                        cx,
                    ))
                    .child(meta_row(
                        "Version",
                        div().text_sm().child(clamp(&l.version, NAME_CHARS)),
                        cx,
                    ))
                    .child(meta_row(
                        "Index",
                        div().text_sm().child(index_label(entry)),
                        cx,
                    ))
                    .child(heading("PLATFORMS", cx).pt_2())
                    .child(chips);
                (column, &l.id)
            }
        };
        let repository = (listing_for(id, entries).and_then(|e| e.listing.repository.clone()))
            .filter(|url| wai_ext::index::https_url(url));
        column.children(repository.map(|url| {
            let releases = releases_url(&url);
            v_flex()
                .gap_2()
                .pt_2()
                .child(heading("RESOURCES", cx))
                .child(link("ext-link-repository", "Repository", url, cx))
                .children(releases.map(|url| link("ext-link-releases", "Releases", url, cx)))
        }))
    }

    fn hero(&self, subject: &Subject, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let entries = self.entries();
        let (id, name, version, description) = match subject {
            Subject::Installed(_, ext) => {
                let m = &ext.manifest;
                (&m.id, &m.name, &m.version, m.description.as_deref())
            }
            Subject::Listed(entry) => {
                let l = &entry.listing;
                (&l.id, &l.name, &l.version, l.description.as_deref())
            }
        };
        let entry = listing_for(id, entries);
        let update = match subject {
            Subject::Installed(_, ext) => listed_update(ext, entries),
            Subject::Listed(..) => None,
        };
        let repository = (entry.and_then(|e| e.listing.repository.clone()))
            .filter(|url| wai_ext::index::https_url(url));
        let boreal = aurora::boreal(cx);
        let sub = h_flex()
            .gap_2()
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(div().flex_none().child(clamp(id.as_str(), NAME_CHARS)))
            .children(entry.map(|e| div().flex_none().child(format!("· {}", index_label(e)))))
            .children(repository.map(|url| {
                let label = clamp(url.trim_start_matches("https://"), NAME_CHARS);
                h_flex()
                    .id("ext-repository")
                    .min_w_0()
                    .gap_1()
                    .cursor_pointer()
                    .text_color(boreal)
                    .child("·")
                    .child(div().min_w_0().truncate().child(label))
                    .child(Icon::new(Lucide::ExternalLink).with_size(px(11.)))
                    .on_click(move |_, _, cx| cx.open_url(&url))
            }));
        let actions = match subject {
            Subject::Installed(ix, ext) => {
                let (ix, enabled) = (*ix, ext.enabled);
                let (toggle, remove) = (ext.manifest.id.clone(), ext.manifest.id.clone());
                let status = row_status(enabled, self.host.read(cx).status(&ext.manifest.id));
                h_flex()
                    .gap_2()
                    .items_center()
                    .children(update.map(|listing| {
                        let label = format!("Update to {}", clamp(&listing.version, NAME_CHARS));
                        let listing = listing.clone();
                        Button::new("ext-update-detail")
                            .primary()
                            .small()
                            .label(label)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.install_listing(&listing, window, cx)
                            }))
                    }))
                    .child(
                        Button::new(("ext-enabled", ix))
                            .small()
                            .label(if enabled { "Disable" } else { "Enable" })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.set_enabled(&toggle, !enabled, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("ext-uninstall", ix))
                            .ghost()
                            .small()
                            .label("Uninstall")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.uninstall(&remove, window, cx)
                            })),
                    )
                    .child(div().pl_2().child(status_line(&status, cx)))
            }
            Subject::Listed(entry) => {
                let action = match entry.listing.package() {
                    Some(_) => {
                        let listing = entry.listing.clone();
                        Button::new("ext-install")
                            .primary()
                            .small()
                            .label("Install")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.install_listing(&listing, window, cx)
                            }))
                            .into_any_element()
                    }
                    None => div()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child("Not available for this platform")
                        .into_any_element(),
                };
                h_flex().gap_2().items_center().child(action)
            }
        };
        h_flex()
            .gap_5()
            .px_6()
            .pt_6()
            .pb_4()
            .items_start()
            .child(monogram(id, name, 76., cx))
            .child(
                v_flex()
                    .flex_1()
                    .min_w_0()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .min_w_0()
                                    .truncate()
                                    .font_family(DISPLAY_FONT)
                                    .text_size(fonts::ui(cx, 24.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(clamp(name, NAME_CHARS)),
                            )
                            .child(aurora::badge(clamp(version, NAME_CHARS), cx).flex_none())
                            .children(update.map(|l| {
                                let text = format!("{} available", clamp(&l.version, NAME_CHARS));
                                aurora::badge(text, cx)
                                    .flex_none()
                                    .text_color(boreal)
                                    .border_color(boreal.opacity(0.4))
                            })),
                    )
                    .child(sub)
                    .children(description.map(|d| {
                        (div().truncate().text_color(theme.muted_foreground))
                            .child(clamp(d, DESCRIPTION_CHARS))
                    }))
                    .child(div().pt_2().child(actions)),
            )
    }

    /// Details, Features, Permissions; a listing has only Details.
    fn tabs(
        &self,
        subject: &Subject,
        tab: DetailTab,
        cx: &Context<Self>,
    ) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let all = [
            (DetailTab::Details, "ext-tab-details", "Details"),
            (DetailTab::Features, "ext-tab-features", "Features"),
            (DetailTab::Permissions, "ext-tab-permissions", "Permissions"),
        ];
        let shown = match subject {
            Subject::Installed(..) => &all[..],
            Subject::Listed(..) => &all[..1],
        };
        let boreal = aurora::boreal(cx);
        h_flex()
            .px_6()
            .gap_1()
            .border_b_1()
            .border_color(theme.border)
            .children(shown.iter().map(|&(t, id, label)| {
                let active = t == tab;
                div()
                    .id(id)
                    .px_3()
                    .py_2()
                    .cursor_pointer()
                    .text_sm()
                    .border_b_2()
                    .border_color(if active { boreal } else { transparent_black() })
                    .text_color(if active {
                        theme.foreground
                    } else {
                        theme.muted_foreground
                    })
                    .child(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.tab = t;
                        cx.notify();
                    }))
                    .test_support()
            }))
    }

    /// The crash or failure, About, and the Claude Code plugins to install in the terminal.
    fn details(&self, subject: &Subject, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (description, ext) = match subject {
            Subject::Installed(_, ext) => (ext.manifest.description.as_deref(), Some(*ext)),
            Subject::Listed(entry) => (entry.listing.description.as_deref(), None),
        };
        let problem = ext.and_then(|ext| {
            match row_status(ext.enabled, self.host.read(cx).status(&ext.manifest.id)) {
                RowStatus::Failed(error) => Some(("Failed", error)),
                RowStatus::Crashed(tail) => Some(("Crashed", tail)),
                _ => None,
            }
        });
        let plugins: Vec<_> = ext
            .filter(|ext| ext.enabled)
            .map(|ext| self.plugin_rows(ext, cx))
            .unwrap_or_default();
        v_flex()
            .gap_4()
            .children(problem.map(|(title, text)| {
                v_flex()
                    .gap_1()
                    .px_3()
                    .py_2()
                    .rounded(px(8.))
                    .border_1()
                    .border_color(theme.danger.opacity(0.4))
                    .bg(theme.danger.opacity(0.08))
                    .text_color(theme.danger)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(
                        div()
                            .text_xs()
                            .font_family(fonts::editor_family(cx))
                            .child(clamp(&text, DESCRIPTION_CHARS)),
                    )
            }))
            .child(v_flex().gap_1().child(heading("ABOUT", cx)).child(
                div().text_sm().text_color(theme.foreground).child(
                    description.map_or("No description.".into(), |d| clamp(d, DESCRIPTION_CHARS)),
                ),
            ))
            .when(!plugins.is_empty(), |el| {
                el.child(
                    v_flex()
                        .gap_1()
                        .child(heading("CLAUDE CODE PLUGINS", cx))
                        .children(plugins),
                )
            })
            .when(ext.is_none(), |el| {
                el.child(div().text_xs().text_color(theme.muted_foreground).child(
                    "Permissions and features are listed when you install: wai downloads the \
                     package and shows what it asks for before anything runs.",
                ))
            })
    }

    /// Required plugins, then suggested ones. Never installed silently: typed into the
    /// terminal, and the user presses Enter.
    fn plugin_rows(&self, ext: &Installed, cx: &Context<Self>) -> Vec<Div> {
        let theme = cx.theme();
        let m = &ext.manifest;
        let required = (m.requires.claude_plugins.iter()).map(|p| {
            let line = Some(claude_plugins::terminal_line(p));
            (clamp(&p.name, NAME_CHARS), "Required", line)
        });
        let suggested = (m.contributes.claude_plugins.iter())
            .map(|name| (clamp(name, NAME_CHARS), "Suggested", plugin_install(name)));
        (required.chain(suggested).take(PLUGINS_SHOWN).enumerate())
            .map(|(p, (name, kind, command))| {
                let id = ElementId::Name(format!("ext-plugin:{}:{p}", m.id.as_str()).into());
                let row = h_flex()
                    .gap_2()
                    .py_1()
                    .items_center()
                    .text_sm()
                    .child(div().flex_1().min_w_0().truncate().child(name))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(kind),
                    );
                match command {
                    Some(command) => row.child(
                        Button::new(id)
                            .xsmall()
                            .label("Install in terminal")
                            .on_click(cx.listener(move |_, _, _, cx| {
                                cx.emit(InstallPlugin(command.clone()))
                            })),
                    ),
                    None => row.child(
                        div()
                            .text_xs()
                            .text_color(theme.danger)
                            .child("Invalid name"),
                    ),
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    use wai_ext::index::Package;
    use wai_ext::parse_manifest;

    use std::prelude::v1::test; // not gpui_kit's, which `super::*` brings in

    use super::*;

    fn manifest(text: &str) -> Manifest {
        parse_manifest(text, Path::new("/x")).unwrap()
    }

    fn installed(version: &str) -> Installed {
        let text = format!("id = \"s\"\nname = \"S\"\nversion = \"{version}\"\n");
        Installed {
            manifest: manifest(&text),
            dir: "/x/s".into(),
            data: "/x/data/s".into(),
            enabled: true,
            granted: vec![],
        }
    }

    fn listed(id: &str, version: &str, platform: Option<&str>) -> Entry {
        let package = Package {
            url: "https://example.com/s.tar.gz".into(),
            sha256: "0".repeat(64),
        };
        let packages = platform.map(|p| (p.to_string(), package)).into_iter();
        Entry {
            listing: Listing {
                id: ExtId::new(id).unwrap(),
                name: id.into(),
                version: version.into(),
                description: None,
                repository: None,
                packages: packages.collect::<BTreeMap<_, _>>(),
            },
            from: None,
        }
    }

    #[test]
    fn status_labels() {
        use ExtStatus::*;
        assert_eq!(row_status(false, Some(&Ready)).label(), "Disabled");
        assert_eq!(row_status(true, Some(&Ready)).label(), "Running");
        assert_eq!(row_status(true, Some(&Starting)).label(), "Starting");
        assert_eq!(row_status(true, None).label(), "Enabled");
        let crashed = row_status(true, Some(&Crashed("boom".into())));
        assert_eq!(crashed, RowStatus::Crashed("boom".into()));
        assert_eq!(crashed.label(), "Crashed");
        let failed = row_status(true, Some(&Failed("nope".into())));
        assert_eq!(failed, RowStatus::Failed("nope".into()));
        assert_eq!(failed.label(), "Failed");
    }

    #[test]
    fn links_and_labels() {
        assert_eq!(
            releases_url("https://github.com/o/r/").as_deref(),
            Some("https://github.com/o/r/releases")
        );
        assert_eq!(releases_url("https://github.com/o"), None);
        assert_eq!(releases_url("https://gitlab.com/o/r"), None);
        assert_eq!(platform_label("darwin-arm64"), "macOS arm64");
        assert_eq!(platform_label("darwin-x64"), "macOS x64");
        assert_eq!(platform_label("linux-x64"), "Linux x64");
        assert_eq!(platform_label("windows-x64"), "Windows x64");
        assert_eq!(platform_label("any"), "Any");
        assert!(matches("", &["x"]));
        assert!(matches(" USa ", &["wai-usage", "Usage"]));
        assert!(!matches("zzz", &["Usage", "wai-usage"]));
        assert_eq!(tint("wai-usage"), tint("wai-usage"));
        assert!((0..50).all(|n| tint(&format!("ext-{n}")) < 6));
        assert_ne!(tint("a"), tint("b"));
    }

    #[test]
    fn features_group_by_kind() {
        let m = manifest(
            "id = \"s\"\nname = \"S\"\nversion = \"1\"\npermissions = [\"panels\", \"process\"]\n\
             [process]\ncommand = \"bin/run\"\n\
             [[contributes.commands]]\nid = \"go\"\ntitle = \"Go now\"\nprompt = \"go\"\n\
             [[contributes.panels]]\nid = \"p\"\ntitle = \"Pet\"\n\
             [[contributes.mcp_servers]]\nname = \"playwright\"\ncommand = \"npx\"\n",
        );
        let kinds = features(&m);
        assert_eq!(
            kinds,
            vec![
                ("Panels", vec!["Pet".to_string()]),
                ("Commands", vec!["Go now".to_string()]),
                ("MCP servers", vec!["playwright".to_string()]),
            ]
        );
        assert!(features(&installed("1").manifest).is_empty());
    }

    #[test]
    fn update_needs_another_listed_version() {
        let ext = installed("1.0.0");
        let same = [listed("s", "1.0.0", Some("any"))];
        assert!(listed_update(&ext, &same).is_none());
        let other_id = [listed("t", "2.0.0", Some("any"))];
        assert!(listed_update(&ext, &other_id).is_none());
        let no_package = [listed("s", "2.0.0", None)];
        assert!(listed_update(&ext, &no_package).is_none());
        let newer = [
            listed("t", "1.0.0", Some("any")),
            listed("s", "1.1.0", Some("any")),
        ];
        assert_eq!(
            listed_update(&ext, &newer).map(|l| l.version.as_str()),
            Some("1.1.0")
        );
        assert_eq!(
            listing_for(&ext.manifest.id, &newer).map(|e| e.listing.version.as_str()),
            Some("1.1.0")
        );
    }
}
