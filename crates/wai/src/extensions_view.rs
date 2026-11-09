//! The Extensions view, in the sidebar: the installed extensions with enable/disable and
//! Uninstall, and installing from a folder or a git URL. Every install goes through the consent
//! dialog, which lists each permission the extension asks for; nothing installs without it.

use std::cell::RefCell;
use std::path::Path;
use std::rc::Rc;

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Input, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::{ActiveTheme as _, Sizable as _, WindowExt as _, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;
use wai_ext::{ExtId, ExtStore, Installed, Permission, Source, Staged};

/// v1 has no sandbox, and the consent dialog says so in these words.
pub const LIMITATION: &str = "wai cannot restrict files or network access in this version; only install extensions you trust.";

/// A permission in plain words.
pub fn permission_text(permission: &Permission) -> String {
    match permission {
        Permission::Process => "Run its own programs".into(),
        Permission::SessionsRead => "See your sessions and their progress".into(),
        Permission::SessionsControl => "Start sessions and send prompts".into(),
        Permission::Notify => "Show notifications".into(),
        Permission::Panels => "Show panels in the sidebar".into(),
        Permission::Network => "Use the network (not enforced)".into(),
        Permission::Fs(path) => format!("Access {} (not enforced)", path.display()),
    }
}

fn permissions_text(permissions: &[Permission]) -> Vec<String> {
    if permissions.is_empty() {
        return vec!["No permissions".into()];
    }
    permissions.iter().map(permission_text).collect()
}

/// A manifest's text on one line (whitespace and control characters collapsed), at most `max`
/// characters: a hostile manifest cannot push the permissions out of the consent dialog.
pub fn clamp(text: &str, max: usize) -> String {
    let words = text.split(|c: char| c.is_whitespace() || c.is_control());
    let line = words
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    match line.char_indices().nth(max) {
        Some((end, _)) => format!("{}…", &line[..end]),
        None => line,
    }
}

const NAME_CHARS: usize = 80;
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

pub struct ExtensionsView {
    store: ExtStore,
    url: Entity<InputState>,
    /// The stage in progress: a copy or a clone, off the main thread. A new one replaces it.
    staging: Option<Task<()>>,
}

impl ExtensionsView {
    pub fn new(config_dir: &Path, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let url = cx.new(|cx| InputState::new(window, cx).placeholder("https://… or git@…"));
        Self {
            store: ExtStore::open(config_dir),
            url,
            staging: None,
        }
    }

    pub fn store(&self) -> &ExtStore {
        &self.store
    }

    /// Stages `source` off the main thread, then asks for consent. `expect`: the id a project
    /// suggested; an extension with another id is refused, and so is one already installed.
    pub fn review(
        &mut self,
        source: Source,
        expect: Option<ExtId>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let store = self.store.clone();
        let staging = cx.background_spawn(async move { store.stage(&source) });
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
                let installed = this.store.list().iter().any(|i| i.manifest.id == id);
                let refusal = match &expect {
                    Some(want) if *want != id => Some(format!(
                        "the extension there is {}, not {}",
                        id.as_str(),
                        want.as_str()
                    )),
                    _ if installed => Some(format!(
                        "{} is already installed; uninstall it first",
                        id.as_str()
                    )),
                    _ => None,
                };
                match refusal {
                    None => this.consent(staged, window, cx),
                    Some(e) => {
                        discard(staged, cx);
                        refused(e, window, cx)
                    }
                }
            })
            .ok();
        }));
        cx.notify();
    }

    /// Install or Cancel. The permissions and the v1 limitation come right after the name and
    /// version, before anything long the manifest says (its description goes last, clamped).
    fn consent(&mut self, staged: Staged, window: &mut Window, cx: &mut Context<Self>) {
        let m = staged.manifest().clone();
        let (name, version) = (clamp(&m.name, NAME_CHARS), clamp(&m.version, NAME_CHARS));
        let description = (m.description.as_deref()).map(|d| clamp(d, DESCRIPTION_CHARS));
        let source = format!("From {}", clamp(&staged.source().to_string(), 300));
        let lines = permissions_text(&m.permissions);
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
            dialog.title(format!("Install {name}?")).child(body).footer(
                DialogFooter::new()
                    .child(
                        Button::new("consent-cancel")
                            .label("Cancel")
                            .on_click(|_, window, cx| window.close_dialog(cx)),
                    )
                    .child(
                        Button::new("consent-install")
                            .primary()
                            .label("Install")
                            .on_click(move |_, window, cx| {
                                window.close_dialog(cx);
                                let Some(staged) = install.borrow_mut().0.take() else {
                                    return;
                                };
                                this.update(cx, |this, cx| this.install(staged, window, cx))
                                    .ok();
                            }),
                    ),
            )
        });
    }

    /// v1 consent is all or nothing: everything the manifest asks for is granted.
    fn install(&mut self, staged: Staged, window: &mut Window, cx: &mut Context<Self>) {
        let name = clamp(&staged.manifest().name, NAME_CHARS);
        let granted = staged.manifest().permissions.clone();
        let notice = match self.store.commit(staged, granted) {
            Ok(_) => Notification::success(format!("Installed {name}")),
            Err(e) => Notification::error(format!("Could not install {name}: {e}")).autohide(false),
        };
        window.push_notification(notice, cx);
        cx.notify();
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
        cx.notify();
    }

    /// The folder leaves `extensions/` at once; its files are deleted off the main thread.
    /// Once extensions run, their processes stop before this.
    fn uninstall(&mut self, id: &ExtId, window: &mut Window, cx: &mut Context<Self>) {
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

    fn card(&self, ix: usize, ext: &Installed, cx: &Context<Self>) -> impl IntoElement + use<> {
        let theme = cx.theme();
        let (toggle, remove) = (ext.manifest.id.clone(), ext.manifest.id.clone());
        v_flex()
            .gap_1()
            .p_2()
            .border_1()
            .border_color(theme.border)
            .rounded(theme.radius)
            .child(
                h_flex()
                    .gap_2()
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(clamp(&ext.manifest.name, NAME_CHARS)),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(clamp(&ext.manifest.version, NAME_CHARS)),
                    ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(permissions_text(&ext.granted).join(", ")),
            )
            .child(
                h_flex()
                    .justify_between()
                    .child(
                        Switch::new(("ext-enabled", ix))
                            .xsmall()
                            .label("Enabled")
                            .checked(ext.enabled)
                            .on_click(cx.listener(move |this, on: &bool, window, cx| {
                                this.set_enabled(&toggle, *on, window, cx)
                            })),
                    )
                    .child(
                        Button::new(("ext-uninstall", ix))
                            .ghost()
                            .xsmall()
                            .label("Uninstall")
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.uninstall(&remove, window, cx)
                            })),
                    ),
            )
    }
}

impl Render for ExtensionsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let installed = self.store.list();
        let muted = |text: &'static str| {
            div()
                .text_sm()
                .text_color(theme.muted_foreground)
                .child(text)
        };
        v_flex()
            .id("extensions")
            .size_full()
            .p_2()
            .gap_2()
            .overflow_y_scroll()
            .child(
                Button::new("ext-install-folder")
                    .small()
                    .w_full()
                    .label("Install from folder…")
                    .on_click(cx.listener(|this, _, window, cx| this.pick_folder(window, cx))),
            )
            .child(Input::new(&self.url).small())
            .child(
                Button::new("ext-install-git")
                    .small()
                    .w_full()
                    .label("Install from git URL…")
                    .on_click(cx.listener(|this, _, window, cx| this.install_url(window, cx))),
            )
            .when(self.staging.is_some(), |el| el.child(muted("Preparing…")))
            .when(installed.is_empty(), |el| {
                el.child(muted("No extensions installed"))
            })
            .children(
                installed
                    .iter()
                    .enumerate()
                    .map(|(ix, ext)| self.card(ix, ext, cx)),
            )
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::Duration;

    use gpui_kit::component::WindowExt as _;
    use gpui_kit::test::TestWindowExt as _;
    use gpui_kit::{ElementId, Entity, TestAppContext, VisualTestContext};
    use wai_ext::{Installed, MANIFEST_FILE, Permission, Source, staging_dir};
    use wai_settings::Settings;

    use super::{ExtensionsView, LIMITATION, clamp};
    use crate::workspace::Workspace;
    use crate::workspace::tests::open;

    const MANIFEST: &str = "id = \"sample\"\nname = \"Sample\"\nversion = \"1.2.0\"\n\
        description = \"A sample\"\n\
        permissions = [\"process\", \"sessions:control\", \"network\", \"fs:data\"]\n\
        [process]\ncommand = \"bin/run\"\n";

    fn sample(dir: &Path) -> PathBuf {
        sample_with(dir, MANIFEST)
    }

    fn sample_with(dir: &Path, manifest: &str) -> PathBuf {
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
    fn workspace<'a>(
        config: &Path,
        cx: &'a mut TestAppContext,
    ) -> (Entity<Workspace>, &'a mut VisualTestContext) {
        let (workspace, cx) = open(cx, Settings::default());
        workspace.update(cx, |w, _| w.user_dir = config.to_path_buf());
        (workspace, cx)
    }

    fn click(id: impl Into<ElementId>, cx: &mut VisualTestContext) {
        let id = id.into();
        cx.update(|window, cx| {
            window.render_frame(cx);
            window.click(id, cx);
        });
        cx.run_until_parked();
    }

    fn dialog(cx: &mut VisualTestContext) -> bool {
        cx.update(|window, cx| window.has_active_dialog(cx))
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
        let tmp = tempfile::tempdir().unwrap();
        let (config, src) = (tmp.path().join("config"), sample(&tmp.path().join("src")));
        let (workspace, cx) = workspace(&config, cx);
        click("sidebar-extensions", cx);
        let view = workspace.read_with(cx, |w, _| w.extensions.clone());
        let view = view.expect("the Extensions view");

        review(&view, &src, cx);
        assert!(dialog(cx), "no consent dialog");
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
        assert!(!dialog(cx));
        assert!(installed(&view, cx).is_empty());
        assert_eq!(staged(&config), 0, "Cancel drops the stage");

        review(&view, &src, cx);
        click("consent-install", cx);
        assert!(!dialog(cx));
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
        assert!(dialog(cx), "no consent dialog");
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

    #[test]
    fn clamp_collapses_lines_and_cuts() {
        assert_eq!(clamp("  a\n\n b\t\u{7}c  ", 80), "a b c");
        let long = clamp(&"é".repeat(100), 80);
        assert_eq!(long.chars().count(), 81);
        assert!(long.ends_with('…'));
        assert_eq!(clamp("exactly", 7), "exactly");
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
        assert!(!dialog(cx));
        assert!(!config.join("extensions").exists() && !config.join("extensions.json").exists());

        // The notice slides in first (the test clock only moves when told).
        cx.executor().advance_clock(Duration::from_secs(1));
        cx.run_until_parked();
        click(("suggest-review", 0usize), cx);
        assert!(dialog(cx), "Review asks for consent");
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
}
