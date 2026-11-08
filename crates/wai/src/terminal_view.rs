use std::path::PathBuf;

use gpui_kit::component::{ActiveTheme as _, v_flex};
use gpui_kit::*;
use wai_terminal::{
    DEFAULT_BG, DEFAULT_FG, GridSize, GridSnapshot, Rgb, TerminalEvent, TerminalModel, key_to_bytes,
};

const EXITED: &str = "[process exited] Click to restart.";

/// The bottom panel: a shell in the project folder, painted from the model's grid snapshots.
/// It redraws only when the shell writes (`Wakeup`) or the panel changes size.
pub struct TerminalView {
    shell: Option<String>,
    cwd: PathBuf,
    /// `None` until the first `activate` and after the shell exits.
    model: Option<TerminalModel>,
    pub snapshot: GridSnapshot,
    size: GridSize,
    /// Why no shell runs: it exited or could not start.
    notice: Option<String>,
    focus: FocusHandle,
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
    fn fit(&mut self, size: GridSize, cx: &mut Context<Self>) {
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
            model.write(&bytes);
            cx.stop_propagation();
        }
    }
}

fn color(Rgb(r, g, b): Rgb) -> Hsla {
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
        let fit = canvas(
            move |bounds, _, cx| {
                let cols = (bounds.size.width / cell_width).floor().max(2.) as u16;
                let rows = (bounds.size.height / line_height).floor().max(1.) as u16;
                view.update(cx, |t, cx| t.fit(GridSize { cols, rows }, cx));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full();

        div()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, _, window, cx| t.activate(window, cx)),
            )
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
    use gpui_kit::TestAppContext;
    use wai_settings::Settings;

    use super::EXITED;
    use crate::workspace::tests::{open, wait_until};

    fn sh() -> Settings {
        let mut settings = Settings::default();
        settings.terminal.shell = Some("/bin/sh".into());
        settings
    }

    #[gpui_kit::test]
    fn terminal_view_shows_shell_output(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, sh());

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
    fn exited_shell_restarts_on_toggle(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // the shell is a real process
        let (workspace, cx) = open(cx, sh());
        cx.simulate_keystrokes("ctrl-`");
        let terminal = workspace.read_with(cx, |w, _| w.terminal.clone().expect("first toggle"));

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
    }
}
