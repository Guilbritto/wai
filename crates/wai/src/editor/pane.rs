//! An editor pane: open files in tabs, each a code editor kept in step with the disk.

use std::fs;
use std::path::{Path, PathBuf};

use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::dialog::DialogFooter;
use gpui_kit::component::input::{Editor, EditorState, InputEvent, Position};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{
    ActiveTheme as _, IconName, Sizable as _, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::FluentBuilder as _;
use gpui_kit::*;

use super::file::{self, Loaded, OpenError, OpenMode, Stamp};

/// One open file.
pub struct FileEditor {
    pub path: PathBuf,
    /// Edited since it was read or saved.
    pub dirty: bool,
    /// Over 20 MB, or `scratch`: shown, never edited or saved.
    pub read_only: bool,
    /// A tool call's output (R21).
    pub scratch: bool,
    /// The file changed on disk while this buffer could not take the change (edited, or
    /// read-only); the banner asks what to do.
    pub disk_changed: bool,
    pub editor: Entity<EditorState>,
    /// The file as last read, saved or seen changed.
    stamp: Option<Stamp>,
    /// The resolved real path at load.
    real: Option<PathBuf>,
    /// The file is gone from disk (a clean tab can then only close).
    deleted: bool,
    _edits: Subscription,
}

impl FileEditor {
    pub fn new(
        path: PathBuf,
        loaded: Loaded,
        scratch: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let read_only = scratch || loaded.mode == OpenMode::ReadOnlyLarge;
        let language = language(&path, read_only);
        let editor = cx.new(|cx| {
            let mut state = EditorState::new(window, cx)
                .soft_wrap(false)
                .language(language);
            state.set_value(loaded.text, window, cx);
            state
        });
        let edits = cx.subscribe(&editor, |this, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) && !this.dirty {
                this.dirty = true;
                cx.notify();
            }
        });
        Self {
            path,
            dirty: false,
            read_only,
            scratch,
            disk_changed: false,
            editor,
            stamp: loaded.stamp,
            real: loaded.real,
            deleted: false,
            _edits: edits,
        }
    }

    pub fn name(&self) -> String {
        let name = self.path.file_name().map(|n| n.to_string_lossy());
        name.unwrap_or_default().into_owned()
    }

    /// Puts the caret at the start of `line` (1-based) and focuses the editor.
    pub fn go_to(&self, line: u32, window: &mut Window, cx: &mut App) {
        let position = Position::new(line.saturating_sub(1), 0);
        self.editor
            .update(cx, |s, cx| s.set_cursor_position(position, window, cx));
    }

    /// Writes the buffer (⌘S, Save all, the close dialog). `false` when nothing was written:
    /// read-only, the file changed on disk since it was read (R39: the banner asks first), or
    /// the write failed (a notice says why).
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.read_only {
            return false;
        }
        if let Ok(now) = fs::canonicalize(&self.path)
            && Some(&now) != self.real.as_ref()
        {
            let notice = "File was replaced by a link — reopen it to edit";
            window.push_notification(Notification::error(notice), cx);
            return false;
        }
        // The stamp catches what no watcher reports (a file outside the project).
        if self.disk_changed || file::stamp(&self.path) != self.stamp {
            self.disk_changed = true;
            let notice = format!(
                "Changed on disk: {} — Reload or Keep mine before saving",
                self.name()
            );
            window.push_notification(Notification::warning(notice), cx);
            cx.notify();
            return false;
        }
        // ponytail: written on the main thread; at most 20 MB, so tens of ms at worst. Move it to
        // the background executor (with an edit counter for `dirty`) if it ever shows.
        let text = self.editor.read(cx).text().to_string();
        match file::save(&self.path, &text) {
            Ok(stamp) => {
                self.stamp = stamp;
                (self.dirty, self.disk_changed) = (false, false);
                cx.notify();
                true
            }
            Err(e) => {
                let notice = format!("Could not save {}: {e}", self.name());
                window.push_notification(Notification::error(notice).autohide(false), cx);
                false
            }
        }
    }

    /// The watcher saw the file change. A clean buffer reloads; an edited or read-only one keeps
    /// what it shows, and the banner asks. A change that leaves the stamp as it was (wai's own
    /// save) is no change.
    pub fn disk_event(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        // ponytail: one stat per touched open file, on the main thread.
        let now = file::stamp(&self.path);
        if now == self.stamp {
            return;
        }
        // The stamp stays: until Reload's text lands (or Keep mine), a Save must not pass.
        self.deleted = now.is_none();
        // R30: a large file, read-only already or grown past 20 MB now, is not reloaded on each
        // change (a growing log would block the main thread each time).
        let large = |(_, len): Stamp| len > file::READ_ONLY_ABOVE;
        if self.dirty || self.read_only || now.is_none_or(large) {
            self.disk_changed = true;
            cx.notify();
        } else {
            self.reload(false, window, cx);
        }
    }

    /// Reads the file again on the background executor. `force` (Reload) drops the edits;
    /// otherwise an edit made while it reads wins, and the banner asks.
    fn reload(&mut self, force: bool, window: &mut Window, cx: &mut Context<Self>) {
        let path = self.path.clone();
        cx.spawn_in(window, async move |this, cx| {
            let loaded = cx.background_spawn(async move { file::load(&path) }).await;
            this.update_in(cx, |this, window, cx| {
                this.reloaded(loaded, force, window, cx)
            })
            .ok();
        })
        .detach();
    }

    fn reloaded(
        &mut self,
        loaded: Result<Loaded, OpenError>,
        force: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match loaded {
            Ok(_) if self.dirty && !force => self.disk_changed = true,
            Ok(loaded) => {
                let read_only = self.scratch || loaded.mode == OpenMode::ReadOnlyLarge;
                self.stamp = loaded.stamp;
                self.deleted = false;
                (self.dirty, self.disk_changed) = (false, false);
                self.editor.update(cx, |s, cx| {
                    // Crossing 20 MB either way switches the grammar too (before the text).
                    if read_only != self.read_only {
                        s.set_highlighter(language(&self.path, read_only), cx);
                    }
                    // The caret and the scroll stay where they were, as far as the text allows.
                    let (selection, scroll) = (s.selected_range(), s.scroll_offset());
                    s.set_value(loaded.text, window, cx);
                    s.set_selected_range(selection, cx);
                    s.set_scroll_offset(scroll, cx);
                });
                self.read_only = read_only;
            }
            Err(e) => {
                // R31: the buffer no longer shows the file; a Save must not pass for one.
                self.disk_changed = true;
                let notice = format!("Could not reload {}: {e}", self.name());
                window.push_notification(Notification::error(notice), cx);
            }
        }
        cx.notify();
    }
}

/// The grammar the extension names ("rs", "ts", "py"…). A large file shows plain: its parse
/// would hold a tree as big as the file.
fn language(path: &Path, read_only: bool) -> String {
    match path.extension() {
        Some(ext) if !read_only => ext.to_string_lossy().into_owned(),
        _ => "text".into(),
    }
}

fn banner(cx: &App) -> Div {
    let theme = cx.theme();
    h_flex()
        .w_full()
        .px_3()
        .py_1()
        .gap_2()
        .text_sm()
        .bg(theme.warning.opacity(0.15))
        .border_b_1()
        .border_color(theme.border)
}

impl Render for FileEditor {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let why = match self.scratch {
            true => "Read-only: a tool call's full output.",
            false => "Read-only: files over 20 MB open read-only in wai.",
        };
        v_flex()
            .size_full()
            .when(self.read_only, |el| {
                el.child(
                    banner(cx)
                        .debug_selector(|| "read-only-banner".into())
                        .child(why),
                )
            })
            .when(self.disk_changed, |el| {
                // A clean tab has no edits to keep: it reloads, or closes when the file is gone.
                let gone = !self.dirty && self.deleted;
                el.child(
                    banner(cx)
                        .child(div().flex_1().child(if gone {
                            "Deleted on disk"
                        } else {
                            "Changed on disk"
                        }))
                        .when(!gone, |el| {
                            el.child(div().debug_selector(|| "reload".into()).child(
                                Button::new("reload").xsmall().label("Reload").on_click(
                                    cx.listener(|this, _, window, cx| {
                                        this.reload(true, window, cx)
                                    }),
                                ),
                            ))
                        })
                        .when(gone, |el| {
                            el.child(
                                Button::new("close")
                                    .xsmall()
                                    .label("Close")
                                    .on_click(cx.listener(|_, _, _, cx| cx.emit(CloseRequest))),
                            )
                        })
                        .when(self.dirty, |el| {
                            el.child(
                                div().debug_selector(|| "keep-mine".into()).child(
                                    Button::new("keep-mine")
                                        .xsmall()
                                        .ghost()
                                        .label("Keep mine")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            // What is on disk now is what the next Save replaces.
                                            this.stamp = file::stamp(&this.path);
                                            this.disk_changed = false;
                                            cx.notify();
                                        })),
                                ),
                            )
                        }),
                )
            })
            .child(
                Editor::new(&self.editor)
                    .readonly(self.read_only)
                    .bordered(false)
                    .size_full(),
            )
    }
}

/// Open files in tabs; the workspace has one editor pane at most.
pub struct EditorPane {
    pub tabs: Vec<Entity<FileEditor>>,
    pub active: usize,
    _subs: Vec<Subscription>,
}

/// The last tab closed: the workspace drops the pane.
pub struct Emptied;

impl EventEmitter<Emptied> for EditorPane {}

/// A clean tab whose file is gone asks to be closed.
pub struct CloseRequest;

impl EventEmitter<CloseRequest> for FileEditor {}

impl EditorPane {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
            _subs: Vec::new(),
        }
    }

    pub fn find(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.read(cx).path == path)
    }

    pub fn add(&mut self, tab: Entity<FileEditor>, window: &mut Window, cx: &mut Context<Self>) {
        let closing = cx.subscribe_in(&tab, window, |this, tab, _: &CloseRequest, window, cx| {
            this.remove(tab, window, cx)
        });
        self._subs.push(closing);
        self.tabs.push(tab);
        self.activate(self.tabs.len() - 1, window, cx);
    }

    pub fn activate(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.active = ix;
        self.focus(window, cx);
        cx.notify();
    }

    pub fn focus(&self, window: &mut Window, cx: &mut App) {
        if let Some(tab) = self.tabs.get(self.active) {
            let editor = tab.read(cx).editor.clone();
            editor.update(cx, |s, cx| s.focus(window, cx));
        }
    }

    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(tab) = self.tabs.get(self.active) {
            tab.update(cx, |tab, cx| tab.save(window, cx));
        }
    }

    /// Closes tab `ix`. With unsaved edits it asks first: Save, Don't save, or Cancel.
    pub fn close(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get(ix).cloned() else {
            return;
        };
        if !tab.read(cx).dirty {
            return self.remove(&tab, window, cx);
        }
        let title = format!("Save changes to {}?", tab.read(cx).name());
        let pane = cx.entity().downgrade();
        window.open_dialog(cx, move |dialog, _, _| {
            let answer = |save: bool| {
                let (pane, tab) = (pane.clone(), tab.clone());
                move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
                    window.close_dialog(cx);
                    pane.update(cx, |pane, cx| {
                        if !save || tab.update(cx, |tab, cx| tab.save(window, cx)) {
                            pane.remove(&tab, window, cx);
                        }
                    })
                    .ok();
                }
            };
            dialog
                .title(title.clone())
                .child("Your changes are lost if you don't save them.")
                .footer(
                    DialogFooter::new()
                        .child(
                            Button::new("cancel")
                                .label("Cancel")
                                .on_click(|_, window, cx| window.close_dialog(cx)),
                        )
                        .child(
                            Button::new("dont-save")
                                .label("Don't save")
                                .on_click(answer(false)),
                        )
                        .child(
                            Button::new("save")
                                .primary()
                                .label("Save")
                                .on_click(answer(true)),
                        ),
                )
        });
    }

    fn remove(&mut self, tab: &Entity<FileEditor>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(ix) = self.tabs.iter().position(|t| t == tab) else {
            return;
        };
        self.tabs.remove(ix);
        if self.tabs.is_empty() {
            return cx.emit(Emptied);
        }
        if ix < self.active || self.active == self.tabs.len() {
            self.active -= 1;
        }
        self.activate(self.active, window, cx);
    }
}

impl Render for EditorPane {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        // ponytail: tabs show the file name only; two `mod.rs` look alike until a tooltip or the
        // parent folder tells them apart.
        let tabs = self.tabs.iter().enumerate().map(|(ix, tab)| {
            let tab = tab.read(cx);
            let label = if tab.dirty {
                format!("● {}", tab.name())
            } else {
                tab.name()
            };
            let active = ix == self.active;
            h_flex()
                .id(("editor-tab", ix))
                .h(px(32.))
                .pl_3()
                .pr_1()
                .gap_1()
                .text_sm()
                .cursor_pointer()
                .border_r_1()
                .border_color(theme.border)
                .map(|el| {
                    if active {
                        el.bg(theme.tab_active)
                            .text_color(theme.tab_active_foreground)
                    } else {
                        el.text_color(theme.muted_foreground)
                    }
                })
                .child(label)
                .child(
                    Button::new(("editor-tab-close", ix))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .on_click(cx.listener(move |this, _, window, cx| {
                            cx.stop_propagation();
                            this.close(ix, window, cx);
                        })),
                )
                .on_click(cx.listener(move |this, _, window, cx| this.activate(ix, window, cx)))
        });
        v_flex()
            .size_full()
            .child(
                h_flex()
                    .w_full()
                    .flex_none()
                    .overflow_x_hidden()
                    .bg(theme.tab_bar)
                    .border_b_1()
                    .border_color(theme.border)
                    .children(tabs),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .children(self.tabs.get(self.active).cloned()),
            )
    }
}
