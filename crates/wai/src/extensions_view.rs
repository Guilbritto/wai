//! The Extensions page, in the settings modal: a list of the installed and the listed
//! extensions, and the selected one's page (`page.rs`). Every install goes through the consent
//! dialog, which lists each permission the extension asks for; nothing installs without it.

mod page;

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme as _, Disableable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::*;
// A manifest's text goes through `clamp`: a hostile one cannot push the permissions out of the
// consent dialog.
use wai_ext::index::{Listing, index_url};
use wai_ext::panel::clamp;
use wai_ext::{ExtId, ExtStore, Installed, Permission, RequiredPlugin, Source, Staged};
use wai_settings::{Change, Settings, write_change};

use crate::claude_plugins::{self, INSTALL_TIMEOUT};
use crate::ext_host::ExtHost;
use crate::marketplace::{self, Entry, Listings};

/// v1 has no sandbox, and the consent dialog says so in these words.
pub const LIMITATION: &str = "wai cannot restrict files or network access in this version; only install extensions you trust.";

/// A permission in plain words.
pub fn permission_text(permission: &Permission) -> String {
    match permission {
        Permission::Process => "Run its own programs".into(),
        Permission::SessionsRead => "See your sessions and their progress".into(),
        Permission::SessionsControl => "Start sessions and send prompts".into(),
        Permission::SessionsContent => "Read your sessions' conversations".into(),
        Permission::Notify => "Show notifications".into(),
        Permission::Panels => "Show panels in the sidebar".into(),
        Permission::Bar => "Show items in the top and bottom bars".into(),
        Permission::Network => "Use the network (not enforced)".into(),
        Permission::Listen => "Listen for connections on a network port".into(),
        Permission::Fs(path) => format!("Access {} (not enforced)", path.display()),
        Permission::Events(id) => format!("Receive events from the {} extension", id.as_str()),
    }
}

/// What "Install in terminal" types for a Claude Code plugin an extension suggests: `None` for
/// a name that could do more than name a plugin (`[A-Za-z0-9._@/-]+`, no leading `-`, at most
/// 214 characters like an npm package: the PTY write is on the main thread).
pub fn plugin_install(name: &str) -> Option<String> {
    let ok = |c: char| c.is_ascii_alphanumeric() || "._@/-".contains(c);
    let valid = (1..=214).contains(&name.len()) && !name.starts_with('-') && name.chars().all(ok);
    valid.then(|| format!("claude plugin install {name}"))
}

/// Type this into the terminal, without Enter.
pub struct InstallPlugin(pub String);

impl EventEmitter<InstallPlugin> for ExtensionsView {}

/// Suggested plugins listed per extension; a manifest may list more.
const PLUGINS_SHOWN: usize = 20;

fn permissions_text(permissions: &[Permission]) -> Vec<String> {
    if permissions.is_empty() {
        return vec!["No permissions".into()];
    }
    permissions.iter().map(permission_text).collect()
}

pub(crate) const NAME_CHARS: usize = 80;
const DESCRIPTION_CHARS: usize = 500;

/// Deletes a stage's files off the main thread.
fn discard(staged: Staged, cx: &App) {
    cx.background_spawn(async move { drop(staged) }).detach();
}

/// The stage the consent dialog holds. However the dialog goes (Cancel, its close button,
/// Escape), its files are deleted off the main thread.
struct Pending(Option<Staged>, BackgroundExecutor);

impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(staged) = self.0.take() {
            self.1.spawn(async move { drop(staged) }).detach();
        }
    }
}

/// The AVAILABLE list: fetched the first time the page shows, and on Refresh.
enum Available {
    NotFetched,
    Loading { _fetch: Task<()> },
    Loaded(Listings),
}

pub struct ExtensionsView {
    config_dir: PathBuf,
    store: ExtStore,
    /// Runs what the store has enabled; told after each change.
    host: Entity<ExtHost>,
    url: Entity<InputState>,
    /// The Add index dialog's input.
    index: Entity<InputState>,
    /// The stage in progress: a copy or a clone, off the main thread. A new one replaces it.
    staging: Option<Task<()>>,
    available: Available,
    search: Entity<InputState>,
    /// The extension whose page shows; `current` falls back when it is gone.
    selected: Option<ExtId>,
    tab: DetailTab,
    /// INSTALLED, AVAILABLE.
    closed: [bool; 2],
    _host_changed: Subscription,
    _search_changed: Subscription,
}

/// The tabs of an extension's page; a listing has only Details.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DetailTab {
    Details,
    Features,
    Permissions,
}

impl ExtensionsView {
    pub fn new(
        config_dir: &Path,
        host: Entity<ExtHost>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://… or git@…"));
        let index = cx.new(|cx| InputState::new(window, cx).placeholder("owner/repo or https://…"));
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search extensions"));
        Self {
            config_dir: config_dir.to_path_buf(),
            store: ExtStore::open(config_dir),
            _host_changed: cx.observe(&host, |_, _, cx| cx.notify()),
            host,
            url,
            index,
            staging: None,
            available: Available::NotFetched,
            _search_changed: cx.subscribe(&search, |_, _, _: &InputEvent, cx| cx.notify()),
            search,
            selected: None,
            tab: DetailTab::Details,
            closed: [false; 2],
        }
    }

    fn entries(&self) -> &[Entry] {
        match &self.available {
            Available::Loaded(listings) => &listings.entries,
            _ => &[],
        }
    }

    /// The extension whose page shows: the selected one while it is installed or listed, else
    /// the first installed, else the first listing not installed.
    fn current(&self, installed: &[Installed], entries: &[Entry]) -> Option<ExtId> {
        let is_installed = |id: &ExtId| installed.iter().any(|i| i.manifest.id == *id);
        let known = |id: &ExtId| is_installed(id) || page::listing_for(id, entries).is_some();
        let mut listed = (entries.iter()).filter(|e| !is_installed(&e.listing.id));
        (self.selected.clone().filter(known))
            .or_else(|| installed.first().map(|i| i.manifest.id.clone()))
            .or_else(|| listed.next().map(|e| e.listing.id.clone()))
    }

    /// Installed extensions an index lists another version of: the nav's dot.
    pub(crate) fn updates(&self) -> usize {
        let entries = self.entries();
        (self.store.list().iter())
            .filter(|ext| page::listed_update(ext, entries).is_some())
            .count()
    }

    fn sync_host(&self, cx: &mut Context<Self>) {
        let list = self.store.list();
        self.host.update(cx, |host, cx| host.sync(&list, cx));
    }

    pub fn store(&self) -> &ExtStore {
        &self.store
    }

    /// Stages `source` off the main thread, then asks for consent. `expect`: the id a project
    /// suggested; an extension with another id is refused. One already installed is an update.
    pub fn review(
        &mut self,
        source: Source,
        expect: Option<ExtId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        self.review_with(move || store.stage(&source), expect, window, cx)
    }

    /// Downloads the listing's package for this platform and reviews it; its manifest must have
    /// the listing's id.
    fn install_listing(&mut self, listing: &Listing, window: &mut Window, cx: &mut Context<Self>) {
        let Some(package) = listing.package().cloned() else {
            return;
        };
        let store = self.store.clone();
        let stage = move || {
            let download = |to: &Path| marketplace::get_package(&package.url, to);
            store.stage_package(&package.url, &package.sha256, download)
        };
        self.review_with(stage, Some(listing.id.clone()), window, cx)
    }

    fn review_with(
        &mut self,
        stage: impl FnOnce() -> Result<Staged, wai_ext::InstallError> + Send + 'static,
        expect: Option<ExtId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let staging = cx.background_spawn(async move { stage() });
        self.staging = Some(cx.spawn_in(window, async move |this, cx| {
            let staged = staging.await;
            this.update_in(cx, |this, window, cx| {
                this.staging = None;
                cx.notify();
                let refused = |e: String, window: &mut Window, cx: &mut App| {
                    let notice = Notification::error(format!("Could not install: {e}"));
                    window.push_notification(notice.autohide(false), cx)
                };
                let staged = match staged {
                    Ok(staged) => staged,
                    Err(e) => return refused(e.to_string(), window, cx),
                };
                let id = staged.manifest().id.clone();
                let old = (this.store.list().into_iter())
                    .find(|i| i.manifest.id == id)
                    .map(|i| i.manifest.version);
                match &expect {
                    Some(want) if *want != id => {
                        discard(staged, cx);
                        let e = format!(
                            "the extension there is {}, not {}",
                            id.as_str(),
                            want.as_str()
                        );
                        refused(e, window, cx)
                    }
                    _ => this.consent(staged, old, window, cx),
                }
            })
            .ok();
        }));
        cx.notify();
    }

    /// Install or Cancel. The permissions and the v1 limitation come right after the name and
    /// version, before anything long the manifest says (its description goes last, clamped).
    /// `old`: the installed version when this replaces it.
    fn consent(
        &mut self,
        staged: Staged,
        old: Option<String>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let updating = old.is_some();
        let m = staged.manifest().clone();
        let (name, version) = (clamp(&m.name, NAME_CHARS), clamp(&m.version, NAME_CHARS));
        let version = match old {
            Some(old) => format!("{} → {version}", clamp(&old, NAME_CHARS)),
            None => version,
        };
        let verb = if updating { "Update" } else { "Install" };
        let description = (m.description.as_deref()).map(|d| clamp(d, DESCRIPTION_CHARS));
        let source = format!("From {}", clamp(&staged.source().to_string(), 300));
        let mut lines = permissions_text(&m.permissions);
        if !m.contributes.items.is_empty() {
            let titles = (m.contributes.items.iter())
                .map(|i| clamp(&i.title, NAME_CHARS))
                .collect::<Vec<_>>();
            lines.push(format!("Bar items: {}", titles.join(", ")));
        }
        for p in &m.requires.claude_plugins {
            let (name, marketplace) = (clamp(&p.name, NAME_CHARS), clamp(&p.marketplace, 300));
            lines.push(format!(
                "Install Claude Code plugin {name} from {marketplace}"
            ));
        }
        let pending = Pending(Some(staged), cx.background_executor().clone());
        let pending = Rc::new(RefCell::new(pending));
        let this = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, cx| {
            let muted = cx.theme().muted_foreground;
            // Each line is findable by its text in tests (`consent: <text>`).
            let line = |text: String| {
                let id = format!("consent: {text}");
                div().debug_selector(move || id).child(text)
            };
            let body = v_flex()
                .gap_2()
                .text_sm()
                .child(
                    h_flex()
                        .gap_2()
                        .child(line(name.clone()).font_weight(FontWeight::SEMIBOLD))
                        .child(line(version.clone()).text_color(muted)),
                )
                .child(div().font_weight(FontWeight::SEMIBOLD).child("It asks to:"))
                .children(lines.iter().map(|text| line(text.clone()).pl_2()))
                .child(line(LIMITATION.into()).text_color(cx.theme().warning))
                .child(line(source.clone()).text_xs().text_color(muted))
                .children(description.clone().map(|d| {
                    div()
                        .debug_selector(|| "consent-description".into())
                        .pt_1()
                        .text_color(muted)
                        .child(d)
                }));
            let (install, this) = (pending.clone(), this.clone());
            dialog.title(format!("{verb} {name}?")).child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("consent-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("consent-install")
                            .primary()
                            .label(verb)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let Some(staged) = install.borrow_mut().0.take() else {
                                    return;
                                };
                                this.update(cx, |this, cx| {
                                    this.install(staged, updating, window, cx)
                                })
                                .ok();
                            }),
                    ),
            )
        });
    }

    /// v1 consent is all or nothing: everything the manifest asks for is granted.
    /// An update stops the running process first; the host starts it again from the new folder.
    fn install(
        &mut self,
        staged: Staged,
        updating: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let name = clamp(&staged.manifest().name, NAME_CHARS);
        let granted = staged.manifest().permissions.clone();
        let required = staged.manifest().requires.claude_plugins.clone();
        let result = if updating {
            let id = staged.manifest().id.clone();
            self.host.update(cx, |host, cx| host.stop(&id, cx));
            self.store.update(staged, granted).map(|(_, trash)| {
                cx.background_spawn(async move { std::fs::remove_dir_all(trash).ok() })
                    .detach()
            })
        } else {
            self.store.commit(staged, granted).map(|_| ())
        };
        // A failed update has put the old folder back; restart what `stop` dropped.
        self.sync_host(cx);
        let notice = match result {
            Ok(()) => {
                self.install_required(required, window, cx);
                let did = if updating { "Updated" } else { "Installed" };
                Notification::success(format!("{did} {name}"))
            }
            Err(e) => {
                let did = if updating { "update" } else { "install" };
                Notification::error(format!("Could not {did} {name}: {e}")).autohide(false)
            }
        };
        window.push_notification(notice, cx);
        cx.notify();
    }

    /// After the extension is committed: installs the Claude Code plugins it requires, off the
    /// main thread. The extension stays installed whatever happens here.
    fn install_required(
        &mut self,
        required: Vec<RequiredPlugin>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if required.is_empty() {
            return;
        }
        let dir = self.config_dir.clone();
        cx.spawn_in(window, async move |_, cx| {
            for p in required {
                let (dir, plugin) = (dir.clone(), p.clone());
                let done = cx
                    .background_spawn(async move {
                        let claude = wai_settings::find_program(Path::new("claude"))
                            .ok_or_else(|| "claude is not on PATH".to_string())?;
                        claude_plugins::install(&claude, &plugin, &dir, INSTALL_TIMEOUT)
                    })
                    .await;
                let name = clamp(&p.name, NAME_CHARS);
                cx.update(|window, cx| {
                    let notice = match done {
                        Ok(()) => {
                            Notification::success(format!("Installed Claude Code plugin {name}"))
                        }
                        Err(e) => Notification::error(format!(
                            "Could not install Claude Code plugin {name}: {e}"
                        ))
                        .autohide(false),
                    };
                    window.push_notification(notice, cx);
                })
                .ok();
            }
        })
        .detach();
    }

    /// Reads the indexes again: the official one, then the user's settings' (never a project's).
    fn refresh(&mut self, cx: &mut Context<Self>) {
        let dir = self.config_dir.clone();
        let listings = cx.background_spawn(async move {
            let settings = Settings::load(&dir, None).map(|(s, _)| s);
            marketplace::fetch_all(&settings.unwrap_or_default().extensions.indexes)
        });
        let fetch = cx.spawn(async move |this, cx| {
            let listings = listings.await;
            this.update(cx, |this, cx| {
                this.available = Available::Loaded(listings);
                cx.notify();
            })
            .ok();
        });
        self.available = Available::Loading { _fetch: fetch };
    }

    fn add_index_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (index, this) = (self.index.clone(), cx.entity().downgrade());
        window.open_dialog(cx, move |dialog, _, cx| {
            let input = index.read(cx).value().to_string();
            let body = v_flex().gap_2().child(Input::new(&index)).child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("A GitHub repo (owner/repo) or an https URL."),
            );
            let this = this.clone();
            dialog.title("Add an extension index").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("index-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("index-add")
                            .primary()
                            .label("Add")
                            .disabled(index_url(&input).is_none())
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                this.update(cx, |this, cx| this.add_index(&input, window, cx))
                                    .ok();
                            }),
                    ),
            )
        });
    }

    fn install_url_dialog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (url, this) = (self.url.clone(), cx.entity().downgrade());
        window.open_dialog(cx, move |dialog, _, cx| {
            let empty = url.read(cx).value().trim().is_empty();
            let body = v_flex().gap_2().child(Input::new(&url)).child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child("wai clones it, then shows what it asks for before it installs."),
            );
            let this = this.clone();
            dialog.title("Install from a git URL").child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("git-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("git-install")
                            .primary()
                            .label("Install")
                            .disabled(empty)
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                this.update(cx, |this, cx| this.install_url(window, cx))
                                    .ok();
                            }),
                    ),
            )
        });
    }

    /// Saves `input`'s index to the user's settings and reads the indexes again.
    fn add_index(&mut self, input: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(url) = index_url(input) else {
            return;
        };
        match write_change(&self.config_dir, &Change::AddIndex(url)) {
            Err(e) => {
                let notice = Notification::warning(format!("Could not save settings: {e}"));
                window.push_notification(notice, cx);
            }
            Ok(()) => {
                self.index.update(cx, |s, cx| s.set_value("", window, cx));
                self.refresh(cx);
                cx.notify();
            }
        }
    }

    fn pick_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: None,
        });
        cx.spawn_in(window, async move |this, cx| {
            let Ok(Ok(Some(mut paths))) = picked.await else {
                return;
            };
            let Some(dir) = paths.pop() else {
                return;
            };
            this.update_in(cx, |this, window, cx| {
                this.review(Source::Folder(dir), None, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn install_url(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let url = self.url.read(cx).value().trim().to_string();
        if !url.is_empty() {
            self.review(Source::Git { url, rev: None }, None, window, cx);
        }
    }

    fn set_enabled(&mut self, id: &ExtId, on: bool, window: &mut Window, cx: &mut Context<Self>) {
        if let Err(e) = self.store.set_enabled(id, on) {
            let notice = format!("Could not change {}: {e}", id.as_str());
            window.push_notification(Notification::error(notice), cx);
        }
        self.sync_host(cx);
        cx.notify();
    }

    /// Its process is killed first (Review Focus 3). The folder leaves `extensions/` at once;
    /// its files are deleted off the main thread.
    fn uninstall(&mut self, id: &ExtId, window: &mut Window, cx: &mut Context<Self>) {
        self.host.update(cx, |host, cx| host.stop(id, cx));
        match self.store.uninstall(id) {
            Ok(trash) => cx
                .background_spawn(async move { std::fs::remove_dir_all(trash).ok() })
                .detach(),
            Err(e) => {
                let notice = format!("Could not uninstall {}: {e}", id.as_str());
                window.push_notification(Notification::error(notice).autohide(false), cx);
            }
        }
        cx.notify();
    }
}

impl Render for ExtensionsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        // No request until the user looks at the page.
        if matches!(self.available, Available::NotFetched) {
            self.refresh(cx);
        }
        self.page(cx)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use std::cell::RefCell;
    use std::path::{Path, PathBuf};
    use std::rc::Rc;
    use std::time::Duration;

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, Entity, TestAppContext, VisualTestContext};
    #[cfg(unix)]
    use wai_ext::ExtId;
    use wai_ext::ExtStore;
    use wai_ext::{Installed, MANIFEST_FILE, Permission, Source, staging_dir};
    use wai_settings::Settings;

    use wai_ext::panel::clamp;

    use super::{ExtensionsView, InstallPlugin, LIMITATION, plugin_install};
    #[cfg(unix)]
    use crate::ext_host::ExtStatus;
    use crate::settings_view::Section;
    use crate::workspace::Workspace;
    use crate::workspace::tests::open;
    #[cfg(unix)]
    use crate::workspace::tests::wait_until;

    const MANIFEST: &str = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.2.0\"\n\
        description = \"A sample\"\n\
        permissions = [\"process\", \"sessions:control\", \"network\", \"fs:data\"]\n\
        [process]\ncommand = \"bin/run\"\n";

    fn sample(dir: &Path) -> PathBuf {
        sample_with(dir, MANIFEST)
    }

    pub(crate) fn sample_with(dir: &Path, manifest: &str) -> PathBuf {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join(MANIFEST_FILE), manifest).unwrap();
        dir.to_path_buf()
    }

    const PERMISSION_LINES: [&str; 4] = [
        "Run its own programs",
        "Start sessions and send prompts",
        "Use the network (not enforced)",
        "Access data (not enforced)",
    ];

    /// A workspace whose config dir is `config`, never the developer's.
    pub(crate) fn workspace<'a>(
        config: &Path,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let (workspace, cx) = open(cx, Settings::default());
        workspace.update(cx, |w, _| w.user_dir = config.to_path_buf());
        (workspace, cx)
    }

    pub(crate) fn click(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    /// The consent dialog; the settings modal is a dialog too.
    fn consent(cx: &mut VisualTestContext) -> bool {
        cx.update(|w, cx| {
            w.render_frame(cx);
            w.try_find("consent-install").is_some()
        })
    }

    fn notices(cx: &mut VisualTestContext) -> usize {
        cx.update(|window, cx| window.notifications(cx).len())
    }

    fn installed(view: &Entity<ExtensionsView>, cx: &mut VisualTestContext) -> Vec<Installed> {
        view.read_with(cx, |v, _| v.store().list())
    }

    fn review(view: &Entity<ExtensionsView>, src: &Path, cx: &mut VisualTestContext) {
        let source = Source::Folder(src.to_path_buf());
        cx.update(|window, cx| view.update(cx, |v, cx| v.review(source, None, window, cx)));
        cx.run_until_parked();
    }

    fn staged(config: &Path) -> usize {
        std::fs::read_dir(staging_dir(config)).map_or(0, |d| d.count())
    }

    /// The dialog lists the name, version, each permission in plain words, the v1 limitation,
    /// the source and the description. Cancel installs nothing; Install records the consent,
    /// enabled.
    #[gpui_kit::test]
    fn consent_dialog_lists_permissions(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (config, src) = (tmp.path().join("config"), sample(&tmp.path().join("src")));
        let (workspace, cx) = workspace(&config, cx);
        click("sidebar-extensions", cx);
        let view = workspace.read_with(cx, |w, _| w.extensions.clone());
        let view = view.expect("the Extensions view");

        review(&view, &src, cx);
        assert!(consent(cx), "no consent dialog");
        cx.update(|window, cx| window.render_frame(cx));
        let from = format!("From {}", src.display());
        let lines = ["Sample", "1.2.0", LIMITATION, from.as_str()];
        for line in PERMISSION_LINES.iter().chain(&lines) {
            // `debug_bounds` takes a `&'static str`.
            let says = format!("consent: {line}").leak();
            assert!(
                cx.debug_bounds(says).is_some(),
                "the dialog does not say {line:?}"
            );
        }
        assert!(cx.debug_bounds("consent-description").is_some());
        assert!(installed(&view, cx).is_empty(), "nothing before consent");
        click("consent-cancel", cx);
        assert!(!consent(cx));
        assert!(installed(&view, cx).is_empty());
        assert_eq!(staged(&config), 0, "Cancel drops the stage");

        assert_eq!(
            super::permission_text(&Permission::Bar),
            "Show items in the top and bottom bars"
        );
        let manifest = format!(
            "{}[[contributes.items]]\nid = \"prs\"\nbar = \"bottom\"\ntitle = \"Pull requests\"\n\
             [[contributes.items]]\nid = \"ci\"\nbar = \"top\"\ntitle = \"CI\"\n",
            MANIFEST.replace("\"fs:data\"", "\"fs:data\", \"bar\"")
        );
        let with_items = sample_with(&tmp.path().join("items"), &manifest);
        review(&view, &with_items, cx);
        assert!(consent(cx), "no consent dialog");
        cx.update(|window, cx| window.render_frame(cx));
        assert!(
            cx.debug_bounds("consent: Bar items: Pull requests, CI")
                .is_some()
        );
        click("consent-cancel", cx);
        assert!(!consent(cx));

        review(&view, &src, cx);
        click("consent-install", cx);
        assert!(!consent(cx));
        let list = installed(&view, cx);
        assert_eq!(list.len(), 1);
        assert!(list[0].enabled);
        assert_eq!(
            list[0].granted,
            [
                Permission::Process,
                Permission::SessionsControl,
                Permission::Network,
                Permission::Fs("data".into())
            ]
        );
        assert!(
            config
                .join("extensions/sample")
                .join(MANIFEST_FILE)
                .is_file()
        );
        assert_eq!(staged(&config), 0);

        click(("ext-enabled", 0usize), cx);
        assert!(!installed(&view, cx)[0].enabled);
        click(("ext-uninstall", 0usize), cx);
        assert!(installed(&view, cx).is_empty());
        assert!(!config.join("extensions/sample").exists());
        assert_eq!(staged(&config), 0, "deleted off the main thread");
    }

    /// Review Focus 2: a long, multi-line name or description cannot push the permissions or
    /// the limitation out of view; both come before the description, which is clamped.
    #[gpui_kit::test]
    fn long_description_cannot_hide_permissions(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let name = format!("Sample\\n{}", "Totally Safe ".repeat(20));
        let description = "It asks to: nothing. Safe to install.\n".repeat(280);
        assert!(description.len() > 10_000);
        let manifest = MANIFEST
            .replace("name = \"Sample\"", &format!("name = \"{name}\""))
            .replace(
                "description = \"A sample\"",
                &format!("description = \"\"\"\n{description}\"\"\""),
            );
        let src = sample_with(&tmp.path().join("src"), &manifest);
        let (workspace, cx) = workspace(&tmp.path().join("config"), cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));

        review(&view, &src, cx);
        assert!(consent(cx), "no consent dialog");
        cx.update(|window, cx| window.render_frame(cx));
        let viewport = cx.update(|window, _| window.viewport_size());
        let description = cx
            .debug_bounds("consent-description")
            .expect("a description");
        // The TOML `\\n` is a newline in the name.
        let clamped = clamp(&name.replace("\\n", "\n"), 80);
        assert!(clamped.chars().count() <= 81 && !clamped.contains('\n'));
        for line in PERMISSION_LINES
            .iter()
            .chain(&[LIMITATION, clamped.as_str()])
        {
            let says = format!("consent: {line}").leak();
            let bounds = cx.debug_bounds(says);
            let bounds = bounds.unwrap_or_else(|| panic!("the dialog does not say {line:?}"));
            assert!(
                bounds.bottom() <= description.top(),
                "{line:?} after the description"
            );
            assert!(bounds.bottom() <= viewport.height, "{line:?} out of view");
        }
    }

    /// The consent dialog for an extension asking for `permission` says `text`.
    fn consent_says(permission: &str, text: &'static str, cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let manifest = MANIFEST.replace("\"network\"", &format!("\"{permission}\""));
        let src = sample_with(&tmp.path().join("src"), &manifest);
        let (workspace, cx) = workspace(&tmp.path().join("config"), cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));

        review(&view, &src, cx);
        assert!(consent(cx), "no consent dialog");
        cx.update(|window, cx| window.render_frame(cx));
        let says = format!("consent: {text}").leak();
        assert!(
            cx.debug_bounds(says).is_some(),
            "the dialog does not say {text:?}"
        );
    }

    /// PAIR-01: `listen` is "Listen for connections on a network port".
    #[gpui_kit::test]
    fn consent_lists_listen(cx: &mut TestAppContext) {
        consent_says("listen", "Listen for connections on a network port", cx);
    }

    /// HOST-11: `sessions:content` is "Read your sessions' conversations".
    #[gpui_kit::test]
    fn consent_lists_sessions_content(cx: &mut TestAppContext) {
        consent_says("sessions:content", "Read your sessions' conversations", cx);
    }

    /// `wai-fake-ext`, next to the test binary's `deps/` (`cargo test --workspace` builds it).
    #[cfg(unix)]
    fn fake_ext() -> PathBuf {
        let name = format!("wai-fake-ext{}", std::env::consts::EXE_SUFFIX);
        let found = std::env::current_exe()
            .unwrap()
            .ancestors()
            .map(|d| d.join(&name))
            .find(|p| p.is_file());
        found.expect("run `cargo build -p wai-ext --bin wai-fake-ext` first")
    }

    /// An extension `sample` whose process is `wai-fake-ext --pid-file <pid_file> <flags>`.
    #[cfg(unix)]
    fn running_sample(dir: &Path, pid_file: &Path, flags: &str) -> PathBuf {
        let manifest = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.0.0\"\n\
            permissions = [\"process\"]\n[process]\ncommand = \"bin/run\"\n";
        running_sample_with(dir, manifest, pid_file, flags)
    }

    /// `running_sample` with `manifest`, whose process must be `bin/run`.
    #[cfg(unix)]
    pub(crate) fn running_sample_with(
        dir: &Path,
        manifest: &str,
        pid_file: &Path,
        flags: &str,
    ) -> PathBuf {
        use std::os::unix::fs::PermissionsExt as _;
        let src = sample_with(dir, manifest);
        let run = src.join("bin/run");
        std::fs::create_dir_all(src.join("bin")).unwrap();
        let script = format!(
            "#!/bin/sh\nexec '{}' --pid-file '{}' {flags}\n",
            fake_ext().display(),
            pid_file.display()
        );
        std::fs::write(&run, script).unwrap();
        std::fs::set_permissions(&run, std::fs::Permissions::from_mode(0o755)).unwrap();
        src
    }

    /// The pid a newly started extension process wrote.
    #[cfg(unix)]
    fn started(pid_file: &Path, cx: &mut VisualTestContext) -> String {
        let written = || std::fs::read_to_string(pid_file).unwrap_or_default();
        wait_until(cx, "the extension's process", |_| !written().is_empty());
        written()
    }

    #[cfg(unix)]
    fn wait_gone(pid: &str) {
        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while alive() {
            assert!(
                std::time::Instant::now() < deadline,
                "extension {pid} still alive"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    /// R44, Review Focus 3: installing starts the extension's process, disabling stops it,
    /// enabling starts it again, and uninstalling kills it before its folder goes.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn uninstall_stops_processes(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let pid_file = tmp.path().join("ext.pid");
        let src = running_sample(&tmp.path().join("src"), &pid_file, "");
        let (workspace, cx) = workspace(&tmp.path().join("config"), cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let id = ExtId::new("sample").unwrap();

        review(&view, &src, cx);
        click("consent-install", cx);
        let first = started(&pid_file, cx);
        wait_until(cx, "Ready", |cx| {
            host.read_with(cx, |h, _| h.status(&id) == Some(&ExtStatus::Ready))
        });

        click(("ext-enabled", 0usize), cx);
        wait_gone(&first);
        std::fs::remove_file(&pid_file).unwrap();
        click(("ext-enabled", 0usize), cx);
        let second = started(&pid_file, cx);

        click(("ext-uninstall", 0usize), cx);
        wait_gone(&second);
        assert_eq!(host.read_with(cx, |h, _| h.status(&id).cloned()), None);
    }

    /// Installing the same id again stops the running process, swaps the folder and starts
    /// the new one; the version and `enabled` carry over.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn update_stops_then_restarts(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (config, pid_file) = (tmp.path().join("config"), tmp.path().join("ext.pid"));
        let src = running_sample(&tmp.path().join("src"), &pid_file, "");
        let manifest = "id = \"sample\"\nname = \"Sample\"\nversion = \"0.2.0\"\n\
            permissions = [\"process\"]\n[process]\ncommand = \"bin/run\"\n";
        let new = running_sample_with(&tmp.path().join("new"), manifest, &pid_file, "");
        let (workspace, cx) = workspace(&config, cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let id = ExtId::new("sample").unwrap();

        review(&view, &src, cx);
        click("consent-install", cx);
        let first = started(&pid_file, cx);
        std::fs::remove_file(&pid_file).unwrap();

        review(&view, &new, cx);
        click("consent-install", cx);
        wait_gone(&first);
        let second = started(&pid_file, cx);
        assert_ne!(first, second);
        wait_until(cx, "Ready", |cx| {
            host.read_with(cx, |h, _| h.status(&id) == Some(&ExtStatus::Ready))
        });
        let list = installed(&view, cx);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].manifest.version, "0.2.0");
        assert!(list[0].enabled);
        assert!(list[0].dir.starts_with(config.join("extensions")));
        assert_eq!(staged(&config), 0, "the old folder is deleted");
    }

    /// A failed update puts the old folder back, and the old extension runs again.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn failed_update_restarts_the_old_extension(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (config, pid_file) = (tmp.path().join("config"), tmp.path().join("ext.pid"));
        let src = running_sample(&tmp.path().join("src"), &pid_file, "");
        let manifest = "id = \"sample\"\nname = \"Sample\"\nversion = \"0.2.0\"\n\
            permissions = [\"process\"]\n[process]\ncommand = \"bin/run\"\n";
        let new = running_sample_with(&tmp.path().join("new"), manifest, &pid_file, "");
        let (workspace, cx) = workspace(&config, cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));

        review(&view, &src, cx);
        click("consent-install", cx);
        let first = started(&pid_file, cx);
        std::fs::remove_file(&pid_file).unwrap();

        // `save` cannot replace a non-empty folder at this name, so the update fails.
        std::fs::create_dir_all(config.join("extensions.json.tmp/x")).unwrap();
        review(&view, &new, cx);
        click("consent-install", cx);
        wait_gone(&first);
        let second = started(&pid_file, cx);
        assert_ne!(first, second);
        assert_eq!(installed(&view, cx)[0].manifest.version, "1.0.0");
    }

    /// The dialog for an installed id is an update: `old → new`, Cancel changes nothing.
    #[gpui_kit::test]
    fn update_shows_old_and_new_version(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let old = MANIFEST.replace("1.2.0", "0.1.0");
        let new = MANIFEST.replace("1.2.0", "0.2.0");
        let old = sample_with(&tmp.path().join("old"), &old);
        let new = sample_with(&tmp.path().join("new"), &new);
        let (workspace, cx) = workspace(&config, cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));

        review(&view, &old, cx);
        click("consent-install", cx);
        review(&view, &new, cx);
        assert!(consent(cx), "no update dialog");
        cx.update(|window, cx| window.render_frame(cx));
        assert!(cx.debug_bounds("consent: 0.1.0 → 0.2.0").is_some());
        click("consent-cancel", cx);
        assert_eq!(installed(&view, cx)[0].manifest.version, "0.1.0");
        assert_eq!(staged(&config), 0, "Cancel drops the stage");
    }

    /// A package of `manifest` (made with the system's `tar`), and the URL and SHA-256 an index
    /// lists it by in tests. Relative paths: GNU tar (Git Bash on Windows) reads `C:` as a host.
    fn package(dir: &Path, name: &str, manifest: &str) -> (String, String) {
        sample_with(&dir.join(name), manifest);
        let archive = dir.join(format!("{name}.tar.gz"));
        let packed = std::process::Command::new("tar")
            .current_dir(dir)
            .args(["-czf", &format!("{name}.tar.gz"), "-C", name, "."])
            .status();
        assert!(packed.unwrap().success());
        let url = crate::marketplace::tests::url(&archive);
        (url, wai_ext::sha256_file(&archive).unwrap())
    }

    /// AVAILABLE lists the user's indexes once the page shows (the official one is offline in
    /// tests). Install downloads the package and asks for consent; a listing whose package is
    /// another extension is refused; after Refresh, a new version offers Update.
    #[gpui_kit::test]
    fn installs_and_updates_from_an_index(cx: &mut TestAppContext) {
        use crate::marketplace::tests::{entry, index};
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.2.0\"\n";
        let (url, sha) = package(tmp.path(), "v1", manifest);
        let listed = entry("sample", "1.2.0", &url, &sha) + &entry("other", "1.0.0", &url, &sha);
        let listed = index(tmp.path(), "index.toml", &listed);
        std::fs::create_dir_all(&config).unwrap();
        let settings = format!("[extensions]\nindexes = ['{listed}']\n");
        std::fs::write(config.join("settings.toml"), settings).unwrap();
        let (workspace, cx) = workspace(&config, cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));
        cx.update(|window, cx| window.render_frame(cx));
        cx.run_until_parked();
        let listings = view.read_with(cx, |v, _| match &v.available {
            super::Available::Loaded(l) => (l.entries.len(), l.errors.len()),
            _ => (0, 0),
        });
        assert_eq!(
            listings,
            (2, 1),
            "the user's index, and the official one failing"
        );

        click(("ext-available", 0usize), cx);
        assert!(consent(cx), "no consent dialog");
        cx.update(|window, cx| window.render_frame(cx));
        let from = format!("consent: From {url}").leak();
        assert!(
            cx.debug_bounds(from).is_some(),
            "the dialog names the package"
        );
        click("consent-install", cx);
        assert_eq!(installed(&view, cx)[0].manifest.version, "1.2.0");
        assert_eq!(staged(&config), 0, "the download and the stage are gone");
        assert!(
            !has(("ext-available", 0usize), cx),
            "installed: no longer AVAILABLE"
        );
        assert!(
            !has(("ext-update", 0usize), cx),
            "the same version: no Update"
        );

        let before = notices(cx);
        click(("ext-available", 1usize), cx);
        assert!(!consent(cx), "another extension's package");
        assert_eq!(notices(cx), before + 1);
        assert_eq!(installed(&view, cx).len(), 1);

        let (url, sha) = package(tmp.path(), "v2", &manifest.replace("1.2.0", "1.3.0"));
        index(
            tmp.path(),
            "index.toml",
            &entry("sample", "1.3.0", &url, &sha),
        );
        click("ext-refresh", cx);
        click(("ext-update", 0usize), cx);
        assert!(consent(cx), "no update dialog");
        cx.update(|window, cx| window.render_frame(cx));
        assert!(cx.debug_bounds("consent: 1.2.0 → 1.3.0").is_some());
        click("consent-install", cx);
        assert_eq!(installed(&view, cx)[0].manifest.version, "1.3.0");
    }

    /// Add index writes the expanded shorthand to the user's settings once and reads the indexes
    /// again; an invalid input writes nothing.
    #[gpui_kit::test]
    fn adds_an_index(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        std::fs::create_dir_all(&config).unwrap();
        let (workspace, cx) = workspace(&config, cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));
        let type_in = |text: &str, cx: &mut VisualTestContext| {
            cx.update(|window, cx| {
                let input = view.read(cx).index.clone();
                input.update(cx, |s, cx| s.set_value(text, window, cx));
            });
        };
        cx.update(|window, cx| view.update(cx, |v, cx| v.add_index_dialog(window, cx)));
        type_in("not a repo", cx);
        click("index-add", cx);
        assert!(
            !config.join("settings.toml").exists(),
            "an invalid index is not written"
        );
        type_in("waycarbon/wai-waycarbon", cx);
        click("index-add", cx);
        let (s, _) = Settings::load(&config, None).unwrap();
        assert_eq!(
            s.extensions.indexes,
            [
                "https://github.com/waycarbon/wai-waycarbon/releases/latest/download/index-entry.toml"
            ]
        );
        let errors = view.read_with(cx, |v, _| match &v.available {
            super::Available::Loaded(l) => l.errors.clone(),
            _ => vec![],
        });
        assert!(
            errors
                .iter()
                .any(|e| e.starts_with("Could not load waycarbon/wai-waycarbon:")),
            "{errors:?}"
        );
    }

    /// Enabled before launch, it starts once the window is open; quitting kills it.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn quit_stops_extensions(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (config, pid_file) = (tmp.path().join("config"), tmp.path().join("ext.pid"));
        let src = running_sample(&tmp.path().join("src"), &pid_file, "");
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        store.commit(staged, vec![Permission::Process]).unwrap();
        let (workspace, cx) = workspace(&config, cx);

        workspace.update(cx, |w, cx| w.start_extensions(cx));
        let pid = started(&pid_file, cx);
        // What the platform's quit runs: the quit hooks, then the windows close.
        cx.cx.update(|app| app.shutdown());
        wait_gone(&pid);
    }

    /// The constraint's "at most 3 restarts per session of wai, then Crashed": a project switch
    /// restarts the running extensions but leaves a crashed one crashed, with its stderr tail.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn crashed_extension_stays_crashed_across_project_switch(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (config, pid_file) = (tmp.path().join("config"), tmp.path().join("ext.pid"));
        let src = running_sample(&tmp.path().join("src"), &pid_file, "--crash");
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        store.commit(staged, vec![Permission::Process]).unwrap();
        let (workspace, cx) = workspace(&config, cx);
        let host = workspace.read_with(cx, |w, _| w.ext_host.clone());
        let id = ExtId::new("sample").unwrap();
        let status = |cx: &mut VisualTestContext| host.read_with(cx, |h, _| h.status(&id).cloned());

        workspace.update(cx, |w, cx| w.start_extensions(cx));
        wait_until(
            cx,
            "Crashed",
            |cx| matches!(status(cx), Some(ExtStatus::Crashed(tail)) if tail.contains("fake ext crashed")),
        );
        std::fs::remove_file(&pid_file).unwrap();

        let elsewhere = tmp.path().to_path_buf();
        host.update(cx, |h, cx| h.set_project(elsewhere, cx));
        assert!(matches!(status(cx), Some(ExtStatus::Crashed(_))));
        std::thread::sleep(Duration::from_millis(300));
        cx.run_until_parked();
        assert!(matches!(status(cx), Some(ExtStatus::Crashed(_))));
        assert!(!pid_file.exists(), "a crashed extension started again");
    }

    /// R15: `.wai/extensions.toml` only suggests. The notice installs nothing; its Review opens
    /// the same stage and consent flow, and an installed suggestion is not suggested again.
    #[gpui_kit::test]
    fn project_suggestion_needs_review_and_consent(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // set_project starts the project watcher, a real thread
        let tmp = tempfile::tempdir().unwrap();
        let (config, project) = (tmp.path().join("config"), tmp.path().join("project"));
        sample(&project.join("tools/ext"));
        std::fs::create_dir_all(project.join(".wai")).unwrap();
        std::fs::write(
            project.join(".wai/extensions.toml"),
            "[[suggest]]\nid = \"sample\"\nsource = \"tools/ext\"\n",
        )
        .unwrap();
        let (workspace, cx) = workspace(&config, cx);
        let suggest = |cx: &mut VisualTestContext| {
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.suggest_extensions(window, cx)));
            cx.run_until_parked();
        };

        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(project.clone(), window, cx))
        });
        cx.run_until_parked();
        assert_eq!(notices(cx), 1, "one notice for the suggestion");
        assert!(!consent(cx));
        assert!(!config.join("extensions").exists() && !config.join("extensions.json").exists());

        // The notice slides in first (the test clock only moves when told).
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        click(("suggest-review", 0usize), cx);
        assert!(consent(cx), "Review asks for consent");
        click("consent-install", cx);
        let view = workspace
            .read_with(cx, |w, _| w.extensions.clone())
            .unwrap();
        assert_eq!(installed(&view, cx).len(), 1);

        let before = notices(cx);
        suggest(cx);
        assert_eq!(notices(cx), before, "installed: not suggested again");

        // The test window is never dropped: stop the project's watcher before its temp folder
        // goes.
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project("/nonexistent".into(), window, cx))
        });
    }

    /// The Extensions page lists what is installed and offers the installs.
    #[gpui_kit::test]
    fn extensions_page_shows_installed(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let (config, src) = (tmp.path().join("config"), sample(&tmp.path().join("src")));
        let mut store = wai_ext::ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        let granted = staged.manifest().permissions.clone();
        store.commit(staged, granted).unwrap();
        let (workspace, cx) = workspace(&config, cx);
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.open_settings(Section::Extensions, None, window, cx)
            })
        });
        let view = workspace.read_with(cx, |w, _| w.settings_view()).unwrap();
        assert_eq!(
            view.read_with(cx, |v, cx| v.nav_count(Section::Extensions, cx)),
            Some(1)
        );
        cx.update(|window, cx| window.render_frame(cx));
        assert!(cx.update(|w, _| w.try_find(("ext-enabled", 0usize)).is_some()));
        assert!(cx.update(|w, _| w.try_find("ext-more").is_some()));
    }

    /// The top bar's Extensions button opens the modal on the Extensions page.
    #[gpui_kit::test]
    fn topbar_extensions_opens_the_page(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let (workspace, cx) = workspace(&tmp.path().join("config"), cx);
        click("sidebar-extensions", cx);
        let view = workspace.read_with(cx, |w, _| w.settings_view()).unwrap();
        assert_eq!(view.read_with(cx, |v, _| v.section()), Section::Extensions);
    }

    const PLAIN: &str = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.2.0\"\n\
        description = \"A sample\"\n";
    const OTHER: &str = "id = \"other\"\nname = \"Other\"\nversion = \"0.1.0\"\n\
        description = \"Another one\"\n";

    /// Installs each manifest, in order, with what it asks for granted.
    fn install_all(config: &Path, tmp: &Path, manifests: &[&str]) {
        let mut store = ExtStore::open(config);
        for (n, manifest) in manifests.iter().enumerate() {
            let src = sample_with(&tmp.join(format!("src{n}")), manifest);
            let staged = store.stage(&Source::Folder(src)).unwrap();
            let granted = staged.manifest().permissions.clone();
            store.commit(staged, granted).unwrap();
        }
    }

    /// The settings modal on Extensions, after the indexes are read.
    fn shown<'a>(
        config: &Path,
        cx: &'a mut TestAppContext,
    ) -> (
        Entity<Workspace>,
        Entity<ExtensionsView>,
        &'a mut VisualTestContext,
    ) {
        let (workspace, cx) = workspace(config, cx);
        let view =
            cx.update(|window, cx| workspace.update(cx, |w, cx| w.show_extensions(window, cx)));
        cx.update(|window, cx| window.render_frame(cx));
        cx.run_until_parked();
        (workspace, view, cx)
    }

    fn has(id: impl Into<ElementId>, cx: &mut VisualTestContext) -> bool {
        let id = id.into();
        cx.update(|w, cx| {
            w.render_frame(cx);
            w.try_find(id).is_some()
        })
    }

    fn shows(selector: &str, cx: &mut VisualTestContext) -> bool {
        cx.update(|w, cx| w.render_frame(cx));
        cx.debug_bounds(selector.to_string().leak()).is_some()
    }

    fn search(view: &Entity<ExtensionsView>, text: &str, cx: &mut VisualTestContext) {
        cx.update(|window, cx| {
            let input = view.read(cx).search.clone();
            input.update(cx, |s, cx| s.set_value(text, window, cx));
        });
    }

    /// The first installed one shows by default; a row click shows another, whose detail
    /// enables and disables it.
    #[gpui_kit::test]
    fn selecting_a_row_shows_its_detail(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install_all(&config, tmp.path(), &[PLAIN, OTHER]);
        let (_, view, cx) = shown(&config, cx);
        assert!(shows("ext-detail: sample", cx), "the first one by default");
        click(("ext-row", 1usize), cx);
        assert!(shows("ext-detail: other", cx));
        assert!(!shows("ext-detail: sample", cx));
        click(("ext-enabled", 1usize), cx);
        let list = installed(&view, cx);
        assert!(list[0].enabled && !list[1].enabled);
    }

    /// Review Focus 4: the search keeps the matching rows, each with its store index.
    #[gpui_kit::test]
    fn search_filters_rows(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install_all(&config, tmp.path(), &[PLAIN, OTHER]);
        let (_, view, cx) = shown(&config, cx);
        assert!(!shows("ext-no-match: installed", cx));
        search(&view, "oth", cx);
        assert!(!has(("ext-row", 0usize), cx));
        assert!(has(("ext-row", 1usize), cx));
        search(&view, "ANOTHER", cx);
        assert!(has(("ext-row", 1usize), cx), "matches the description");
        search(&view, "zzz", cx);
        assert!(!has(("ext-row", 0usize), cx) && !has(("ext-row", 1usize), cx));
        assert!(shows("ext-no-match: installed", cx));
    }

    /// Review Focus 4: a search that empties one section says so under that section only.
    #[gpui_kit::test]
    fn no_match_shows_under_each_section(cx: &mut TestAppContext) {
        use crate::marketplace::tests::{entry, index};
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install_all(&config, tmp.path(), &[PLAIN]);
        let (url, sha) = package(tmp.path(), "other", OTHER);
        let listed = index(
            tmp.path(),
            "index.toml",
            &entry("other", "0.1.0", &url, &sha),
        );
        let settings = format!("[extensions]\nindexes = ['{listed}']\n");
        std::fs::write(config.join("settings.toml"), settings).unwrap();
        let (_, view, cx) = shown(&config, cx);
        search(&view, "sample", cx);
        assert!(!shows("ext-no-match: installed", cx));
        assert!(shows("ext-no-match: available", cx));
        search(&view, "other", cx);
        assert!(shows("ext-no-match: installed", cx));
        assert!(!shows("ext-no-match: available", cx));
    }

    #[gpui_kit::test]
    fn sections_collapse(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install_all(&config, tmp.path(), &[PLAIN]);
        let (_, _, cx) = shown(&config, cx);
        assert!(has(("ext-row", 0usize), cx));
        click("ext-section-installed", cx);
        assert!(!has(("ext-row", 0usize), cx));
        click("ext-section-installed", cx);
        assert!(has(("ext-row", 0usize), cx));
    }

    /// Review Focus 5: a listed id that is installed stays out of AVAILABLE; another version
    /// shows Update on its row and a dot on the nav.
    #[gpui_kit::test]
    fn update_shows_on_the_installed_row(cx: &mut TestAppContext) {
        use crate::marketplace::tests::{entry, index};
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install_all(&config, tmp.path(), &[PLAIN]);
        let (url, sha) = package(tmp.path(), "v1", PLAIN);
        let listed = index(
            tmp.path(),
            "index.toml",
            &entry("sample", "1.2.0", &url, &sha),
        );
        let settings = format!("[extensions]\nindexes = ['{listed}']\n");
        std::fs::write(config.join("settings.toml"), settings).unwrap();
        let (workspace, view, cx) = shown(&config, cx);
        let settings = workspace.read_with(cx, |w, _| w.settings_view()).unwrap();
        assert!(
            !has(("ext-listing", 0usize), cx),
            "installed: not AVAILABLE"
        );
        assert!(!has(("ext-update", 0usize), cx));
        assert_eq!(view.read_with(cx, |v, _| v.updates()), 0);
        assert!(!shows("settings-nav-dot-extensions", cx));

        index(
            tmp.path(),
            "index.toml",
            &entry("sample", "1.3.0", &url, &sha),
        );
        click("ext-refresh", cx);
        assert!(has(("ext-update", 0usize), cx));
        assert!(!has(("ext-listing", 0usize), cx));
        assert_eq!(view.read_with(cx, |v, _| v.updates()), 1);
        assert!(settings.read_with(cx, |s, cx| s.nav_dot(Section::Extensions, cx)));
        assert!(shows("settings-nav-dot-extensions", cx));
    }

    /// Review Focus 2: a multi-line, long name and a 10 000 character description keep the row
    /// to its one line each.
    #[gpui_kit::test]
    fn long_name_stays_one_row(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let name = format!("Sample\\n{}", "Totally Safe ".repeat(20));
        let description = "It asks to: nothing. Safe to install.\n".repeat(280);
        let manifest = PLAIN
            .replace("name = \"Sample\"", &format!("name = \"{name}\""))
            .replace(
                "description = \"A sample\"",
                &format!("description = \"\"\"\n{description}\"\"\""),
            );
        install_all(&config, tmp.path(), &[&manifest]);
        let (_, _, cx) = shown(&config, cx);
        let row = cx.update(|w, cx| {
            w.render_frame(cx);
            w.find(("ext-row", 0usize)).bounds()
        });
        assert!(row.size.height < gpui_kit::px(100.), "{row:?}");
        assert!(shows("ext-detail: sample", cx));
    }

    /// Features lists the manifest's contributions; Permissions, what it was granted.
    #[gpui_kit::test]
    fn tabs_show_features_and_permissions(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the extension is a real process
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        let manifest = format!(
            "{MANIFEST}[[contributes.commands]]\nid = \"go\"\ntitle = \"Go now\"\nprompt = \"go\"\n"
        );
        install_all(&config, tmp.path(), &[&manifest]);
        let (_, _, cx) = shown(&config, cx);
        assert!(!shows("ext-feature: Go now", cx), "Details first");
        click("ext-tab-features", cx);
        assert!(shows("ext-feature: Go now", cx));
        click("ext-tab-permissions", cx);
        for line in PERMISSION_LINES {
            assert!(shows(&format!("ext-permission: {line}"), cx), "{line}");
        }
        assert!(shows(&format!("ext-permission: {LIMITATION}"), cx));
    }

    /// Review Focus 1: uninstalling the selected one shows the first installed.
    #[gpui_kit::test]
    fn uninstall_moves_the_selection(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let config = tmp.path().join("config");
        install_all(&config, tmp.path(), &[PLAIN, OTHER]);
        let (_, view, cx) = shown(&config, cx);
        click(("ext-row", 1usize), cx);
        assert!(shows("ext-detail: other", cx));
        click(("ext-uninstall", 1usize), cx);
        assert_eq!(installed(&view, cx).len(), 1);
        assert!(shows("ext-detail: sample", cx));
    }

    /// Review Focus 3: nothing installed and nothing listed.
    #[gpui_kit::test]
    fn empty_page_has_an_empty_detail(cx: &mut TestAppContext) {
        let tmp = tempfile::tempdir().unwrap();
        let (_, _, cx) = shown(&tmp.path().join("config"), cx);
        assert!(shows("ext-empty-detail", cx));
    }

    /// The typed command names a plugin and nothing else: no spaces, control characters, shell
    /// syntax or option.
    #[test]
    fn plugin_names_are_validated() {
        for name in [
            "beehive-waycarbon",
            "@scope/plugin",
            "plugin@market",
            "a.b_c-1",
        ] {
            let typed = plugin_install(name);
            assert_eq!(
                typed.as_deref(),
                Some(format!("claude plugin install {name}").as_str())
            );
        }
        let bad = [
            "",
            "-rf",
            "--help",
            "a b",
            "a;b",
            "a\nb",
            "a\rb",
            "a\tb",
            "a$(id)",
            "a`id`",
            "a|b",
            "a&b",
            "a>b",
            "a'b",
            "a\"b",
            "a\\b",
            "plügin",
            "a\u{7f}b",
            "a\u{1b}[2J",
        ];
        for name in bad {
            assert_eq!(plugin_install(name), None, "{name:?}");
        }
        assert!(plugin_install(&"a".repeat(214)).is_some());
        assert_eq!(plugin_install(&"a".repeat(215)), None);
    }

    /// A required plugin is named in the consent dialog and listed, with its terminal command,
    /// under the installed extension.
    #[gpui_kit::test]
    fn consent_lists_required_plugins(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let tmp = tempfile::tempdir().unwrap();
        let manifest = format!(
            "api = \"1.5\"\n{MANIFEST}[[requires.claude_plugins]]\nname = \"waycarbon\"\n\
             marketplace = \"waycarbon/claude-code\"\n"
        );
        let src = sample_with(&tmp.path().join("src"), &manifest);
        let config = tmp.path().join("config");
        let (workspace, cx) = workspace(&config, cx);
        click("sidebar-extensions", cx);
        let view = workspace.read_with(cx, |w, _| w.extensions.clone());
        let view = view.expect("the Extensions view");
        review(&view, &src, cx);
        assert!(consent(cx), "no consent dialog");
        cx.update(|window, cx| window.render_frame(cx));
        let says = "consent: Install Claude Code plugin waycarbon from waycarbon/claude-code";
        assert!(
            cx.debug_bounds(says).is_some(),
            "the dialog does not name the plugin"
        );
        // Not Install: that would run the real `claude`.
        click("consent-cancel", cx);
    }

    /// The installed extension lists its required plugin first, typed without Enter.
    #[gpui_kit::test]
    fn required_plugin_row_emits_the_terminal_line(cx: &mut TestAppContext) {
        cx.executor().allow_parking();
        let tmp = tempfile::tempdir().unwrap();
        let manifest = format!(
            "api = \"1.5\"\n{MANIFEST}[[requires.claude_plugins]]\nname = \"waycarbon\"\n\
             marketplace = \"waycarbon/claude-code\"\n"
        );
        let src = sample_with(&tmp.path().join("src"), &manifest);
        let config = tmp.path().join("config");
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        let granted = staged.manifest().permissions.clone();
        store.commit(staged, granted).unwrap();
        let (workspace, cx) = workspace(&config, cx);
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.show_extensions(window, cx);
            })
        });
        let view = workspace
            .read_with(cx, |w, _| w.extensions.clone())
            .unwrap();

        let events = Rc::new(RefCell::new(Vec::new()));
        let seen = events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&view, move |_, e: &InstallPlugin, _| {
                seen.borrow_mut().push(e.0.clone())
            })
            .detach()
        });
        cx.update(|window, cx| window.render_frame(cx));
        click(ElementId::Name("ext-plugin:sample:0".into()), cx);
        assert_eq!(
            *events.borrow(),
            ["claude plugin install waycarbon --marketplace waycarbon/claude-code"]
        );
    }

    /// Never installed silently: "Install in terminal" shows the terminal and types the command
    /// without Enter, so it waits at the prompt for the user.
    #[cfg(unix)]
    #[gpui_kit::test]
    fn plugin_install_types_without_enter(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let tmp = tempfile::tempdir().unwrap();
        let manifest = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.0.0\"\n\
            [contributes]\nclaude_plugins = [\"sample-plugin\", \"bad name\"]\n";
        let src = sample_with(&tmp.path().join("src"), manifest);
        let config = tmp.path().join("config");
        let mut store = ExtStore::open(&config);
        let staged = store.stage(&Source::Folder(src)).unwrap();
        store.commit(staged, vec![]).unwrap();
        let (workspace, cx) = workspace(&config, cx);
        // Even if Enter came, this shell finds no `claude` to run.
        cx.simulate_keystrokes("ctrl-`");
        cx.simulate_input("PATH=/nonexistent");
        cx.simulate_keystrokes("enter ctrl-`");
        assert!(!workspace.read_with(cx, |w, _| w.terminal_visible));
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| {
                w.show_extensions(window, cx);
            })
        });
        assert!(
            cx.update(|window, cx| {
                window.render_frame(cx);
                window
                    .try_find(ElementId::Name("ext-plugin:sample:1".into()))
                    .is_none()
            }),
            "no button for an invalid name"
        );

        click(ElementId::Name("ext-plugin:sample:0".into()), cx);
        assert!(workspace.read_with(cx, |w, _| w.settings_view().is_none()));
        assert!(workspace.read_with(cx, |w, _| w.terminal_visible));
        let terminal = workspace.read_with(cx, |w, _| w.terminal().cloned().unwrap());
        let typed = "claude plugin install sample-plugin";
        let at_prompt = |cx: &mut VisualTestContext| {
            terminal.read_with(cx, |t, _| {
                let Some((col, row)) = t.snapshot.cursor else {
                    return false;
                };
                let line: String = (t.snapshot.rows.get(row as usize).into_iter().flatten())
                    .map(|run| run.text.as_str())
                    .collect();
                // The cursor sits right after the command, on its line: no Enter went.
                line.trim_end().ends_with(typed) && line.trim_end().len() == col as usize
            })
        };
        wait_until(cx, "the typed command", at_prompt);
        std::thread::sleep(Duration::from_millis(300));
        cx.run_until_parked();
        assert!(at_prompt(cx), "still waiting at the prompt");
    }
}
