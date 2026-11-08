//! An editor pane: open files in tabs, each a code editor kept in step with the disk.

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
    /// Over 20 MB: shown, never edited or saved.
    pub read_only: bool,
    /// The file changed on disk while this buffer could not take the change (edited, or
    /// read-only); the banner asks what to do.
    pub disk_changed: bool,
    pub editor: Entity<EditorState>,
    /// The file as last read, saved or seen changed.
    stamp: Option<Stamp>,
    _edits: Subscription,
}

impl FileEditor {
    pub fn new(path: PathBuf, loaded: Loaded, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let read_only = loaded.mode == OpenMode::ReadOnlyLarge;
        // The extension names the grammar ("rs", "ts", "py"…); a large file shows plain, since
        // its parse would hold a tree as big as the file.
        let language = match path.extension() {
            Some(ext) if !read_only => ext.to_string_lossy().into_owned(),
            _ => "text".into(),
        };
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
            disk_changed: false,
            editor,
            stamp: loaded.stamp,
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

    /// Writes the buffer (⌘S). `false` when nothing was written: read-only, or the write failed
    /// (a notice says why).
    pub fn save(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if self.read_only {
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
        self.stamp = now;
        // A read-only file is large: reloading it on each change (a growing log) would block
        // the main thread each time.
        if self.dirty || self.read_only || now.is_none() {
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
                self.read_only = loaded.mode == OpenMode::ReadOnlyLarge;
                self.stamp = loaded.stamp;
                (self.dirty, self.disk_changed) = (false, false);
                self.editor.update(cx, |s, cx| {
                    // The caret and the scroll stay where they were, as far as the text allows.
                    let (selection, scroll) = (s.selected_range(), s.scroll_offset());
                    s.set_value(loaded.text, window, cx);
                    s.set_selected_range(selection, cx);
                    s.set_scroll_offset(scroll, cx);
                });
            }
            Err(e) => {
                let notice = format!("Could not reload {}: {e}", self.name());
                window.push_notification(Notification::error(notice), cx);
            }
        }
        cx.notify();
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
        let keep = if self.dirty { "Keep mine" } else { "Dismiss" };
        v_flex()
            .size_full()
            .when(self.read_only, |el| {
                el.child(
                    banner(cx)
                        .debug_selector(|| "read-only-banner".into())
                        .child("Read-only: files over 20 MB open read-only in wai."),
                )
            })
            .when(self.disk_changed, |el| {
                el.child(
                    banner(cx)
                        .child(div().flex_1().child("Changed on disk"))
                        .child(Button::new("reload").xsmall().label("Reload").on_click(
                            cx.listener(|this, _, window, cx| this.reload(true, window, cx)),
                        ))
                        .child(
                            Button::new("keep-mine")
                                .xsmall()
                                .ghost()
                                .label(keep)
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.disk_changed = false;
                                    cx.notify();
                                })),
                        ),
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
}

/// The last tab closed: the workspace drops the pane.
pub struct Emptied;

impl EventEmitter<Emptied> for EditorPane {}

impl EditorPane {
    pub fn new() -> Self {
        Self {
            tabs: Vec::new(),
            active: 0,
        }
    }

    pub fn find(&self, path: &Path, cx: &App) -> Option<usize> {
        self.tabs.iter().position(|tab| tab.read(cx).path == path)
    }

    pub fn add(&mut self, tab: Entity<FileEditor>, window: &mut Window, cx: &mut Context<Self>) {
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
