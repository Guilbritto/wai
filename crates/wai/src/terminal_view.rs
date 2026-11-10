use std::path::PathBuf;

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;
use wai_terminal::{
    DEFAULT_BG, DEFAULT_FG, GridSize, GridSnapshot, Rgb, TerminalEvent, TerminalModel,
    key_to_bytes, paste_bytes,
};

use crate::app::{CopySelection, Paste};

const EXITED: &str = "[process exited] Click to restart.";

/// The bottom panel: a shell in the project folder, painted from the model's grid snapshots.
/// It redraws only when the shell writes (`Wakeup`), the panel changes size, or the user scrolls
/// (10,000 lines of history) or selects with the mouse.
// ponytail: the wheel scrolls wai's history only; in the alternate screen (less, vim) it does
// nothing, where alacritty sends arrow keys. A drag that leaves the panel stops extending.
pub struct TerminalView {
    shell: Option<String>,
    /// Where the next shell starts: the project folder.
    pub cwd: PathBuf,
    /// `None` until the first `activate` and after the shell exits.
    model: Option<TerminalModel>,
    pub snapshot: GridSnapshot,
    size: GridSize,
    /// Why no shell runs: it exited or could not start.
    notice: Option<String>,
    focus: FocusHandle,
    /// Where the grid was painted and one cell's size: they map the mouse to cells.
    grid: Bounds<Pixels>,
    cell: Size<Pixels>,
    /// Wheel movement not yet a whole line (trackpads scroll in pixels).
    wheel: Pixels,
    _events: Option<Task<()>>,
}

impl TerminalView {
    pub fn new(shell: Option<String>, cwd: PathBuf, cx: &mut Context<Self>) -> Self {
        Self {
            shell,
            cwd,
            model: None,
            snapshot: GridSnapshot::default(),
            size: GridSize { cols: 80, rows: 24 },
            notice: None,
            focus: cx.focus_handle(),
            grid: Bounds::default(),
            cell: Size::default(),
            wheel: px(0.),
            _events: None,
        }
    }

    /// Starts the shell unless it runs (first show, or after it exited) and focuses the panel.
    pub fn activate(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.model.is_none() {
            self.start(cx);
        }
        window.focus(&self.focus, cx);
    }

    /// Kills the shell (dropping the model does).
    pub fn stop(&mut self) {
        self.model = None;
    }

    fn start(&mut self, cx: &mut Context<Self>) {
        let spawned = TerminalModel::spawn(self.shell.as_deref(), &self.cwd, self.size);
        let (model, events) = match spawned {
            Ok(spawned) => spawned,
            Err(e) => {
                self.notice = Some(format!("Could not start the shell: {e}"));
                return cx.notify();
            }
        };
        self.model = Some(model);
        self.notice = None;
        // Drained for as long as it is open: the reader thread blocks on a full channel.
        self._events = Some(cx.spawn(async move |this, cx| {
            while let Ok(event) = events.recv().await {
                // One snapshot and one redraw per burst. Titles are not shown.
                let mut exited = event == TerminalEvent::Exited;
                while let Ok(event) = events.try_recv() {
                    exited |= event == TerminalEvent::Exited;
                }
                if this.update(cx, |t, cx| t.refresh(exited, cx)).is_err() {
                    break;
                }
            }
        }));
        cx.notify();
    }

    fn refresh(&mut self, exited: bool, cx: &mut Context<Self>) {
        if let Some(model) = &self.model {
            self.snapshot = model.snapshot();
        }
        if exited {
            self.model = None;
            self.notice = Some(EXITED.into());
        }
        cx.notify();
    }

    /// Fits the grid to the panel. Runs on every layout, so it only works on a real change.
    fn fit(&mut self, grid: Bounds<Pixels>, cell: Size<Pixels>, cx: &mut Context<Self>) {
        (self.grid, self.cell) = (grid, cell);
        let cols = (grid.size.width / cell.width).floor().max(2.) as u16;
        let rows = (grid.size.height / cell.height).floor().max(1.) as u16;
        let size = GridSize { cols, rows };
        if size == self.size {
            return;
        }
        self.size = size;
        if let Some(model) = &self.model {
            model.resize(size);
            self.snapshot = model.snapshot();
            cx.notify();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let keystroke = &event.keystroke;
        let mods = keystroke.modifiers;
        // cmd-* stays with the keybindings.
        let Some(model) = self.model.as_ref().filter(|_| !mods.platform) else {
            return;
        };
        let key_char = keystroke.key_char.as_deref();
        let app_cursor = model.app_cursor();
        if let Some(bytes) =
            key_to_bytes(&keystroke.key, key_char, mods.control, mods.alt, app_cursor)
        {
            self.write(&bytes, cx);
            cx.stop_propagation();
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let text = cx.read_from_clipboard().and_then(|item| item.text());
        if let (Some(model), Some(text)) = (&self.model, text) {
            let bytes = paste_bytes(&text, model.bracketed_paste());
            self.write(&bytes, cx);
        }
    }

    /// Types `text` at the shell's prompt as the keyboard would, without adding Enter: the user
    /// reads it and confirms.
    pub fn type_text(&mut self, text: &str, cx: &mut Context<Self>) {
        self.write(text.as_bytes(), cx);
    }

    /// Sends input to the shell; the view goes back to the bottom, where it shows.
    fn write(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        let Some(model) = &self.model else {
            return;
        };
        model.write(bytes);
        // Not by the snapshot's offset: output since it was taken moved the view further up.
        if self.snapshot.display_offset > 0 {
            model.scroll_to_bottom();
            self.refresh(false, cx);
        }
    }

    /// The wheel scrolls through the history, a line per line height moved.
    fn on_scroll(&mut self, event: &ScrollEvent, _: &mut Window, cx: &mut Context<Self>) {
        let Some(model) = &self.model else {
            return;
        };
        self.wheel += event.delta.pixel_delta(self.cell.height).y;
        let lines = (self.wheel / self.cell.height).trunc();
        if lines != 0. {
            self.wheel -= self.cell.height * lines;
            model.scroll(lines as i32);
            self.refresh(false, cx);
        }
    }

    /// Starts (`start`) or extends the selection to the cell under `at`.
    fn select(&mut self, at: Point<Pixels>, start: bool, cx: &mut Context<Self>) {
        let Some(model) = &self.model else {
            return;
        };
        let at = at - self.grid.origin;
        let x = (at.x / self.cell.width).max(0.);
        let col = (x as u16).min(self.size.cols - 1);
        let row = ((at.y / self.cell.height).max(0.) as u16).min(self.size.rows - 1);
        let right = x - col as f32 >= 0.5;
        if start {
            model.select(col, row, right);
        } else {
            model.select_to(col, row, right);
        }
        self.refresh(false, cx);
    }

    /// Scrolls `lines` up into the history and selects visible row `row` (screenshots).
    #[cfg(feature = "screenshot")]
    pub fn show_history(&mut self, lines: i32, row: u16, cx: &mut Context<Self>) {
        if let Some(model) = &self.model {
            model.scroll(lines);
            model.select(0, row, false);
            model.select_to(self.size.cols - 1, row, true);
        }
        self.refresh(false, cx);
    }

    /// ⌘C (ctrl-shift-c elsewhere): the selection to the clipboard.
    fn copy(&mut self, _: &CopySelection, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.model.as_ref().and_then(TerminalModel::selected_text) {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }
}

pub(crate) fn color(Rgb(r, g, b): Rgb) -> Hsla {
    rgb(u32::from(r) << 16 | u32::from(g) << 8 | u32::from(b)).into()
}

impl Render for TerminalView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let font = font(theme.mono_font_family.clone());
        let font_size = theme.mono_font_size;
        let line_height = (font_size * 1.4).round();
        let text_system = window.text_system();
        let cell_width = text_system
            .advance(text_system.resolve_font(&font), font_size, 'm')
            .map_or(px(8.), |advance| advance.width);
        let (fg, bg) = (color(DEFAULT_FG), color(DEFAULT_BG));

        let rows = self.snapshot.rows.iter().map(|row| {
            let mut text = String::new();
            let runs: Vec<TextRun> = row
                .iter()
                .map(|run| {
                    text.push_str(&run.text);
                    TextRun {
                        len: run.text.len(),
                        font: Font {
                            weight: if run.bold {
                                FontWeight::BOLD
                            } else {
                                FontWeight::NORMAL
                            },
                            style: if run.italic {
                                FontStyle::Italic
                            } else {
                                FontStyle::Normal
                            },
                            ..font.clone()
                        },
                        color: color(run.fg),
                        background_color: (run.bg != DEFAULT_BG).then(|| color(run.bg)),
                        underline: run.underline.then(|| UnderlineStyle {
                            thickness: px(1.),
                            ..Default::default()
                        }),
                        strikethrough: None,
                    }
                })
                .collect();
            div()
                .h(line_height)
                .child(StyledText::new(text).with_runs(runs))
        });
        // A block while focused, an outline otherwise; drawn first so the glyph stays on top.
        let focused = self.focus.is_focused(window);
        let cursor = self
            .model
            .as_ref()
            .and(self.snapshot.cursor)
            .map(|(col, row)| {
                let cell = div()
                    .absolute()
                    .left(cell_width * col as f32)
                    .top(line_height * row as f32)
                    .w(cell_width)
                    .h(line_height);
                if focused {
                    cell.bg(fg.opacity(0.5))
                } else {
                    cell.border_1().border_color(fg.opacity(0.5))
                }
            });
        let notice = self.notice.clone().map(|text| {
            div()
                .absolute()
                .bottom_0()
                .left_0()
                .right_0()
                .bg(bg)
                .text_color(theme.muted_foreground)
                .child(text)
        });
        let view = cx.entity();
        let cell = size(cell_width, line_height);
        let fit = canvas(
            move |bounds, _, cx| view.update(cx, |t, cx| t.fit(bounds, cell, cx)),
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        div()
            .key_context("Terminal")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::paste))
            .on_action(cx.listener(Self::copy))
            .on_scroll(cx.listener(Self::on_scroll))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, event: &MouseDownEvent, window, cx| {
                    t.activate(window, cx);
                    t.select(event.position, true, cx);
                }),
            )
            .on_mouse_move(cx.listener(|t, event: &MouseMoveEvent, _, cx| {
                if event.pressed_button == Some(MouseButton::Left) {
                    t.select(event.position, false, cx);
                }
            }))
            .size_full()
            .p_2()
            .bg(bg)
            .text_color(fg)
            .font_family(theme.mono_font_family.clone())
            .text_size(font_size)
            .line_height(line_height)
            .whitespace_nowrap()
            .overflow_hidden()
            .child(
                v_flex()
                    .relative()
                    .size_full()
                    .child(fit)
                    .children(cursor)
                    .children(rows)
                    .children(notice),
            )
    }
}

#[cfg(all(test, unix))]
mod tests {
    use gpui_kit::test::TestEventExt as _;
    use gpui_kit::{
        ClipboardItem, Entity, MouseButton, ScrollDelta, ScrollEvent, TestAppContext,
        VisualTestContext, point,
    };
    use wai_settings::Settings;

    use super::{EXITED, TerminalView};
    use crate::workspace::tests::{open, wait_until};

    /// The visible row holding `text`, and where in it.
    fn find(
        terminal: &Entity<TerminalView>,
        text: &str,
        cx: &mut VisualTestContext,
    ) -> Option<(usize, usize)> {
        terminal.read_with(cx, |t, _| {
            t.snapshot.rows.iter().enumerate().find_map(|(row, runs)| {
                let line: String = runs.iter().map(|run| run.text.as_str()).collect();
                line.find(text).map(|col| (row, col))
            })
        })
    }

    #[gpui_kit::test]
    fn wheel_scrolls_back_through_history(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("first toggle"));
        cx.simulate_input("seq 1 200");
        cx.simulate_keystrokes("enter");
        // Not "200": the typed command shows it at once.
        wait_until(cx, "the output", |cx| find(&terminal, "199", cx).is_some());

        let bounds = cx.debug_bounds("terminal").expect("terminal drawn");
        cx.simulate_event(ScrollEvent {
            position: bounds.center(),
            delta: ScrollDelta::Lines(point(0., 3.)),
            ..Default::default()
        });
        assert_eq!(terminal.read_with(cx, |t, _| t.snapshot.display_offset), 3);

        // Typing goes back to the bottom.
        cx.simulate_input("x");
        assert_eq!(terminal.read_with(cx, |t, _| t.snapshot.display_offset), 0);
    }

    /// R41: output that comes while scrolled up moves the view with it, ahead of the snapshot
    /// until the next redraw; typing still lands at the bottom.
    #[gpui_kit::test]
    fn typing_returns_to_the_bottom_past_new_output(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("first toggle"));
        cx.simulate_input("seq 1 200");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the output", |cx| find(&terminal, "199", cx).is_some());
        let bounds = cx.debug_bounds("terminal").expect("terminal drawn");
        cx.simulate_event(ScrollEvent {
            position: bounds.center(),
            delta: ScrollDelta::Lines(point(0., 3.)),
            ..Default::default()
        });
        assert_eq!(terminal.read_with(cx, |t, _| t.snapshot.display_offset), 3);

        // More output, straight to the shell; the view's tasks do not run meanwhile.
        let offset = |cx: &mut VisualTestContext| {
            terminal.read_with(cx, |t, _| {
                t.model.as_ref().unwrap().snapshot().display_offset
            })
        };
        terminal.read_with(cx, |t, _| t.model.as_ref().unwrap().write(b"seq 1 50\r"));
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        while offset(cx) < 50 {
            assert!(std::time::Instant::now() < deadline, "no new output");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        cx.simulate_input("x");
        assert_eq!(terminal.read_with(cx, |t, _| t.snapshot.display_offset), 0);
    }

    #[gpui_kit::test]
    fn drag_selects_and_copy_takes_the_selection(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("first toggle"));
        cx.simulate_input("printf wai-%s sel");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the output", |cx| {
            find(&terminal, "wai-sel", cx).is_some()
        });
        let (row, col) = find(&terminal, "wai-sel", cx).unwrap();

        // From the left half of `w` to the right half of `l`.
        let (grid, cell) = terminal.read_with(cx, |t, _| (t.grid, t.cell));
        let at = |col: f32| {
            point(
                grid.origin.x + cell.width * col,
                grid.origin.y + cell.height * (row as f32 + 0.5),
            )
        };
        let (from, to) = (at(col as f32 + 0.2), at(col as f32 + 6.8));
        cx.update(|window, cx| {
            window.pointer_down(from, MouseButton::Left, cx);
            window.pointer_move(to, Some(MouseButton::Left), cx);
            window.pointer_up(to, MouseButton::Left, cx);
        });
        let copy = if cfg!(target_os = "macos") {
            "cmd-c"
        } else {
            "ctrl-shift-c"
        };
        cx.simulate_keystrokes(copy);

        let copied = cx.read_from_clipboard().and_then(|item| item.text());
        assert_eq!(copied.as_deref(), Some("wai-sel"));
    }

    #[gpui_kit::test]
    fn terminal_view_shows_shell_output(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, Settings::default());

        cx.simulate_keystrokes("ctrl-`");
        cx.simulate_input("printf wai-%s ok");
        cx.simulate_keystrokes("enter");

        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("first toggle"));
        wait_until(cx, "the shell output", |cx| {
            terminal.read_with(cx, |t, _| {
                t.snapshot.rows.iter().any(|row| {
                    let text: String = row.iter().map(|run| run.text.as_str()).collect();
                    text.contains("wai-ok")
                })
            })
        });
    }

    #[gpui_kit::test]
    fn paste_writes_clipboard_to_shell(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("first toggle"));

        cx.write_to_clipboard(ClipboardItem::new_string("printf wai-%s pasted".into()));
        let paste = if cfg!(target_os = "macos") {
            "cmd-v"
        } else {
            "ctrl-shift-v"
        };
        cx.simulate_keystrokes(paste);
        cx.simulate_keystrokes("enter");

        wait_until(cx, "the pasted command's output", |cx| {
            terminal.read_with(cx, |t, _| {
                t.snapshot.rows.iter().any(|row| {
                    let text: String = row.iter().map(|run| run.text.as_str()).collect();
                    text.contains("wai-pasted")
                })
            })
        });
    }

    /// A shell that exits starts again on the next show, in the project open by then.
    #[gpui_kit::test]
    fn exited_shell_restarts_in_current_project(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, Settings::default());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("first toggle"));
        let project = std::env::temp_dir().join(format!("wai-cwd-{}", std::process::id()));
        std::fs::create_dir_all(&project).unwrap();
        cx.update(|window, cx| {
            workspace.update(cx, |w, cx| w.set_project(project.clone(), window, cx))
        });

        cx.simulate_input("exit");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "the exit notice", |cx| {
            terminal.read_with(cx, |t, _| t.notice.as_deref() == Some(EXITED))
        });
        assert!(terminal.read_with(cx, |t, _| t.model.is_none()));

        cx.simulate_keystrokes("ctrl-` ctrl-`");
        terminal.read_with(cx, |t, _| {
            assert!(t.model.is_some() && t.notice.is_none(), "a new shell runs");
        });
        cx.simulate_input("touch wai-cwd");
        cx.simulate_keystrokes("enter");
        wait_until(cx, "a file in the new project", |_| {
            project.join("wai-cwd").exists()
        });
        std::fs::remove_dir_all(&project).ok();
    }
}
