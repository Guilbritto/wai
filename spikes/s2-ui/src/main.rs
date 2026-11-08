//! S2 spike: one GPUI window with a code editor (big file), a streamed markdown
//! view and a PTY terminal. Throwaway; the outputs are the numbers and NOTES.
//! Env knobs are listed in docs/spikes/S2-ui.md ("Como reproduzir").

use std::cell::RefCell;
use std::io::{Read, Write};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event as TermEvent, EventListener};
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::{Config as TermConfig, Term, TermMode, cell::Flags, test::TermSize};
use alacritty_terminal::vte::ansi::Processor;
use gpui_kit::component::input::{CursorBlink, Editor, EditorState, InputEvent};
use gpui_kit::component::text::{TextView, TextViewState};
use gpui_kit::component::{
    ActiveTheme as _, Theme, ThemeMode, h_flex, h_resizable, resizable_panel, v_flex, v_resizable,
};
use gpui_kit::*;
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

actions!(s2, [Quit, ClearMarkdown]);

static START: OnceLock<Instant> = OnceLock::new();

fn since_start() -> Duration {
    START.get().unwrap().elapsed()
}

// ---------------------------------------------------------------- terminal

#[derive(Clone)]
struct PtyWriter(Arc<Mutex<Box<dyn Write + Send>>>);

impl PtyWriter {
    fn send(&self, bytes: &[u8]) {
        let mut w = self.0.lock().unwrap();
        let _ = w.write_all(bytes).and_then(|_| w.flush());
    }
}

/// The terminal answers some queries (cursor position, DA) by asking us to
/// write back to the PTY.
impl EventListener for PtyWriter {
    fn send_event(&self, event: TermEvent) {
        if let TermEvent::PtyWrite(text) = event {
            self.send(text.as_bytes());
        }
    }
}

const TERM_ROWS: usize = 14;

struct Terminal {
    term: Term<PtyWriter>,
    parser: Processor,
    writer: PtyWriter,
    master: Box<dyn MasterPty + Send>,
    cols: usize,
    focus: FocusHandle,
    _child: Box<dyn Child + Send + Sync>,
    _reader: Task<()>,
}

impl Terminal {
    fn new(cx: &mut Context<Self>) -> anyhow::Result<Self> {
        let cols = 100;
        let pair = native_pty_system().openpty(PtySize {
            rows: TERM_ROWS as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut cmd = CommandBuilder::new(std::env::var("SHELL").unwrap_or("/bin/sh".into()));
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer = PtyWriter(Arc::new(Mutex::new(pair.master.take_writer()?)));

        // Blocking reads live on their own thread; the UI only wakes up when
        // bytes arrive.
        let (tx, rx) = smol::channel::unbounded::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 64 * 1024];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 || tx.send_blocking(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });
        let reader_task = cx.spawn(async move |this, cx| {
            while let Ok(mut bytes) = rx.recv().await {
                while let Ok(more) = rx.try_recv() {
                    bytes.extend(more);
                }
                let alive = this.update(cx, |t, cx| {
                    t.parser.advance(&mut t.term, &bytes);
                    cx.notify();
                });
                if alive.is_err() {
                    break;
                }
            }
        });

        Ok(Self {
            term: Term::new(
                TermConfig::default(),
                &TermSize::new(cols, TERM_ROWS),
                writer.clone(),
            ),
            parser: Processor::new(),
            writer,
            master: pair.master,
            cols,
            focus: cx.focus_handle(),
            _child: child,
            _reader: reader_task,
        })
    }

    fn resize(&mut self, cols: usize) {
        self.cols = cols;
        self.term.resize(TermSize::new(cols, TERM_ROWS));
        let _ = self.master.resize(PtySize {
            rows: TERM_ROWS as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        let ks = &ev.keystroke;
        let m = ks.modifiers;
        if m.platform {
            return; // cmd-* stays with the app keybindings
        }
        let app_cursor = self.term.mode().contains(TermMode::APP_CURSOR);
        let arrow = |c: char| {
            if app_cursor {
                format!("\x1bO{c}")
            } else {
                format!("\x1b[{c}")
            }
            .into_bytes()
        };
        let bytes = match ks.key.as_str() {
            "enter" => b"\r".to_vec(),
            "backspace" => b"\x7f".to_vec(),
            "tab" => b"\t".to_vec(),
            "escape" => b"\x1b".to_vec(),
            "up" => arrow('A'),
            "down" => arrow('B'),
            "right" => arrow('C'),
            "left" => arrow('D'),
            k if m.control && k.len() == 1 => vec![k.as_bytes()[0] & 0x1f],
            _ => match &ks.key_char {
                Some(s) => s.as_bytes().to_vec(),
                None => return,
            },
        };
        self.writer.send(&bytes);
        cx.stop_propagation();
    }
}

impl Render for Terminal {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (fg, bg) = (theme.foreground, theme.background);
        let family = theme.mono_font_family.clone();
        let font_size = theme.mono_font_size;

        // Fit the grid to the window width (the terminal spans the full width).
        let ts = window.text_system();
        let cell_w = ts
            .advance(ts.resolve_font(&font(family.clone())), font_size, 'm')
            .map(|s| s.width)
            .unwrap_or(px(8.));
        let cols = ((window.viewport_size().width - px(32.)) / cell_w).max(20.) as usize;
        if cols != self.cols {
            self.resize(cols);
        }

        let grid = self.term.grid();
        let cursor = grid.cursor.point;
        let show_cursor =
            self.term.mode().contains(TermMode::SHOW_CURSOR) && self.focus.is_focused(window);
        let rows = (0..TERM_ROWS).map(|l| {
            let row = &grid[Line(l as i32)];
            let mut text: Vec<char> = Vec::with_capacity(self.cols);
            for c in 0..self.cols {
                let cell = &row[Column(c)];
                if !cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
                    text.push(cell.c);
                }
            }
            if show_cursor && cursor.line.0 == l as i32 && cursor.column.0 < text.len() {
                let (before, rest) = text.split_at(cursor.column.0);
                h_flex()
                    .child(before.iter().collect::<String>())
                    .child(div().bg(fg).text_color(bg).child(rest[0].to_string()))
                    .child(rest[1..].iter().collect::<String>())
                    .into_any_element()
            } else {
                div()
                    .child(text.into_iter().collect::<String>())
                    .into_any_element()
            }
        });

        v_flex()
            .id("terminal")
            .size_full()
            .p_2()
            .bg(bg)
            .font_family(family)
            .text_size(font_size)
            .whitespace_nowrap()
            .overflow_hidden()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|t, _, window, cx| window.focus(&t.focus, cx)),
            )
            .children(rows)
    }
}

// ---------------------------------------------------------------- workspace

/// Frame probe: render() of the workspace marks the start, a canvas painted
/// last marks the end, so `end - start` is the CPU time of our subtree's
/// layout + prepaint + paint for that frame.
#[derive(Default)]
struct Probe {
    frame_start: Option<Instant>,
    frames: usize,
}

struct Workspace {
    editor: Entity<EditorState>,
    markdown: Entity<TextViewState>,
    terminal: Entity<Terminal>,
    probe: Rc<RefCell<Probe>>,
    _tasks: Vec<Task<()>>,
    _subscriptions: Vec<Subscription>,
}

const SAMPLE_MD: &str = "## Step {n}: reading the module\n\nThe agent opens **src/main.rs** and finds `three` panes. \
It proposes a plan:\n\n- open the editor with a *large* file\n- stream markdown at about forty tokens per second\n\
- run a real shell inside a PTY\n\n```rust\nfn main() {\n    let token = {n};\n    println!(\"hello from step {}\", token);\n}\n```\n\n\
> Nothing polls once the stream ends.\n\n";

impl Workspace {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let wrap = std::env::var("S2_WRAP").is_ok();
        let editor = cx.new(|cx| EditorState::new(window, cx).soft_wrap(wrap));
        let markdown = cx.new(|cx| TextViewState::markdown("# Agent\n\n", cx));
        markdown
            .read(cx)
            .list_state()
            .set_follow_mode(FollowMode::Tail);
        let terminal = cx.new(|cx| Terminal::new(cx).expect("spawn pty"));
        if std::env::var("S2_FOCUS").as_deref() == Ok("editor") {
            editor.update(cx, |s, cx| s.focus(window, cx)); // its caret blinks
        } else {
            window.focus(&terminal.read(cx).focus.clone(), cx);
        }

        let mut tasks = Vec::new();

        // Markdown stream: a background producer at ~40 tokens/s for 20 s,
        // a channel, and a foreground consumer that appends to the view.
        let (tx, rx) = smol::channel::unbounded::<String>();
        let exec = cx.background_executor().clone();
        tasks.push(cx.background_spawn(async move {
            let start = Instant::now();
            let mut sent = 0u32;
            for n in 1.. {
                let chunk = SAMPLE_MD.replace("{n}", &n.to_string());
                for token in chunk.split_inclusive(' ') {
                    // 40 tokens/s on a fixed schedule, so timer slack does not add up.
                    let due = start + Duration::from_millis(25) * sent;
                    if due >= start + Duration::from_secs(20) {
                        return;
                    }
                    exec.timer(due.saturating_duration_since(Instant::now()))
                        .await;
                    sent += 1;
                    if tx.send(token.to_string()).await.is_err() {
                        return;
                    }
                }
            }
        }));
        let force_draw = std::env::var("S2_FORCE_DRAW").is_ok();
        tasks.push(cx.spawn_in(window, async move |this, cx| {
            let mut tokens = 0usize;
            let mut draws = Vec::new();
            while let Ok(mut text) = rx.recv().await {
                tokens += 1;
                while let Ok(more) = rx.try_recv() {
                    tokens += 1;
                    text.push_str(&more);
                }
                let _ = this.update(cx, |w, cx| {
                    w.markdown.update(cx, |md, cx| md.push_str(&text, cx));
                });
                if force_draw {
                    draws.extend(cx.update(draw_timed).ok());
                }
            }
            if !draws.is_empty() {
                eprintln!("[s2] draws while streaming: {}", stats(&mut draws));
            }
            let frames = this.update(cx, |w, _| w.probe.borrow().frames).unwrap_or(0);
            eprintln!(
                "[s2] markdown stream ended at {:?}: {tokens} tokens, {frames} frames painted so far",
                since_start()
            );
        }));

        // Big file: read on the background executor, then hand it to the editor.
        let path = std::env::var("S2_FILE").unwrap_or_else(|_| {
            concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/s2-ui.big.txt").into()
        });
        let scroll_test = std::env::var("S2_SCROLL_TEST").is_ok();
        tasks.push(cx.spawn_in(window, async move |this, cx| {
            let t0 = Instant::now();
            let read = cx
                .background_executor()
                .spawn(async move { std::fs::read_to_string(&path) })
                .await;
            let read_time = t0.elapsed();
            let text = match read {
                Ok(text) => text,
                Err(e) => return eprintln!("[s2] could not read file: {e} (run gen-big.sh)"),
            };
            let mb = text.len() as f64 / 1e6;
            let _ = this.update_in(cx, |w, window, cx| {
                // S2_LOAD_LOOP=n repeats the load so a profiler can catch it.
                let repeat = std::env::var("S2_LOAD_LOOP").map_or(0, |n| n.parse().unwrap_or(0));
                for _ in 0..repeat {
                    w.editor
                        .update(cx, |s, cx| s.set_value(text.clone(), window, cx));
                }
                let t1 = Instant::now();
                w.editor.update(cx, |s, cx| s.set_value(text, window, cx));
                eprintln!(
                    "[s2] file {mb:.1} MB: read {read_time:?}, set_value (main thread) {:?}",
                    t1.elapsed()
                );
            });
            if let Ok(dt) = cx.update(draw_timed) {
                eprintln!(
                    "[s2] first draw with the file: {dt:?}, open -> drawn {:?}",
                    t0.elapsed()
                );
            }
            if scroll_test {
                Self::scroll_test(this, cx).await;
            }
        }));

        // Offscreen screenshot: works with a locked screen, needs test-support.
        #[cfg(feature = "test-support")]
        if let Ok(path) = std::env::var("S2_SHOT") {
            tasks.push(cx.spawn_in(window, async move |_, cx| {
                cx.background_executor()
                    .timer(Duration::from_secs(12))
                    .await;
                let shot = cx.update(|window, cx| {
                    window.draw(cx).clear(cx);
                    window.render_to_image()
                });
                match shot.map(|img| img.map(|img| img.save(&path))) {
                    Ok(Ok(Ok(()))) => eprintln!("[s2] screenshot saved to {path}"),
                    other => {
                        eprintln!("[s2] screenshot failed: {:?}", other.map(|r| r.map(|_| ())))
                    }
                }
            }));
        }

        let subscriptions = vec![cx.subscribe(&editor, |_, _, event: &InputEvent, _| {
            if let InputEvent::Change = event {
                eprintln!("[s2] editor changed");
            }
        })];

        Self {
            editor,
            markdown,
            terminal,
            probe: Rc::default(),
            _tasks: tasks,
            _subscriptions: subscriptions,
        }
    }

    /// Scrolls the editor programmatically and times a forced `window.draw()`
    /// after each step: the CPU side of a frame (render, layout, prepaint,
    /// paint), independent of the display link (the screen may be locked).
    async fn scroll_test(this: WeakEntity<Self>, cx: &mut AsyncWindowContext) {
        for (name, step) in [("smooth 60px/step", 60.), ("jump 80k px/step", 80_000.)] {
            let mut s = Vec::new();
            for i in 0..300 {
                let y = px(-(i as f32) * step);
                let _ = this.update(cx, |w, cx| {
                    w.editor
                        .update(cx, |s, cx| s.set_scroll_offset(point(px(0.), y), cx))
                });
                s.extend(cx.update(draw_timed).ok());
            }
            let rows = this.update(cx, |w, cx| w.editor.read(cx).visible_row_range());
            eprintln!("[s2] scroll {name}: {}, last rows {rows:?}", stats(&mut s));
        }
    }
}

fn stats(s: &mut [Duration]) -> String {
    s.sort();
    let pct = |p: f64| s[((s.len() - 1) as f64 * p) as usize];
    format!(
        "{} draws, p50 {:?}, p95 {:?}, max {:?}",
        s.len(),
        pct(0.5),
        pct(0.95),
        s[s.len() - 1]
    )
}

fn draw_timed(window: &mut Window, cx: &mut App) -> Duration {
    let t = Instant::now();
    window.draw(cx).clear(cx);
    t.elapsed()
}

impl Workspace {
    fn clear_markdown(&mut self, _: &ClearMarkdown, _: &mut Window, cx: &mut Context<Self>) {
        self.markdown.update(cx, |md, cx| md.set_text("", cx));
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.probe.borrow_mut().frame_start = Some(Instant::now());
        let probe = self.probe.clone();
        let theme = cx.theme();

        v_flex()
            .key_context("Workspace")
            .on_action(cx.listener(Self::clear_markdown))
            .size_full()
            .bg(theme.background)
            .text_color(theme.foreground)
            .child(
                v_resizable("main")
                    .child(
                        resizable_panel().child(
                            h_resizable("top")
                                .child(
                                    resizable_panel()
                                        .size(px(820.))
                                        .child(Editor::new(&self.editor).size_full()),
                                )
                                .child(
                                    resizable_panel().child(
                                        TextView::new(&self.markdown)
                                            .scrollable(true)
                                            .size_full()
                                            .p_3(),
                                    ),
                                ),
                        ),
                    )
                    .child(
                        resizable_panel()
                            .size(px(280.))
                            .child(self.terminal.clone()),
                    ),
            )
            .child(
                canvas(
                    |_, _, _| (),
                    move |_, _, _, _| {
                        let mut p = probe.borrow_mut();
                        p.frames += 1;
                        let dt = p.frame_start.take().map(|t| t.elapsed());
                        if p.frames == 1 {
                            eprintln!("[s2] first frame painted at {:?}", since_start());
                        }
                        if let Some(dt) = dt.filter(|d| *d > Duration::from_millis(50)) {
                            eprintln!("[s2] slow frame {dt:?} at {:?}", since_start());
                        }
                    },
                )
                .absolute()
                .size(px(1.)),
            )
    }
}

fn main() {
    START.set(Instant::now()).ok();
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        if std::env::var("S2_BLINK_IDLE").is_ok() {
            // Stop the caret blink 5 s after the last edit/move: no timer when idle.
            Theme::update(cx, |t| {
                t.cursor_blink = CursorBlink::default().with_idle_timeout(Duration::from_secs(5))
            });
        }
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-k", ClearMarkdown, Some("Workspace")),
        ]);
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.on_window_closed(|cx, _| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1400.), px(900.)),
                cx,
            ))),
            titlebar: Some(TitlebarOptions {
                title: Some("wai · S2".into()),
                ..Default::default()
            }),
            ..Default::default()
        };
        gpui_kit::open_window(options, cx, |window, cx| {
            cx.new(|cx| Workspace::new(window, cx))
        })
        .expect("open window");
        cx.activate(true);
        eprintln!("[s2] window opened at {:?}", since_start());
    });
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    // Not `use super::*`: that drags in GPUI's `test` macro from the
    // `gpui_kit::*` glob, which shadows Rust's `#[test]` and makes
    // `#[gpui_kit::test]` recurse until the recursion limit.
    use super::{Column, Duration, Line, TERM_ROWS, Terminal};
    use gpui_kit::TestAppContext;
    use gpui_kit::prelude::*; // traits only, no `test` macro

    fn screen(t: &Terminal) -> String {
        let grid = t.term.grid();
        (0..TERM_ROWS)
            .flat_map(|l| (0..t.cols).map(move |c| grid[Line(l as i32)][Column(c)].c))
            .collect()
    }

    fn wait_for(
        cx: &mut gpui_kit::VisualTestContext,
        term: &gpui_kit::Entity<Terminal>,
        done: impl Fn(&str) -> bool,
    ) {
        for _ in 0..100 {
            std::thread::sleep(Duration::from_millis(50));
            cx.run_until_parked();
            if term.read_with(cx, |t, _| done(&screen(t))) {
                return;
            }
        }
        panic!("timed out: {}", term.read_with(cx, |t, _| screen(t)));
    }

    /// Typed keys reach a real shell through the PTY and its output comes
    /// back into the grid: printable chars, backspace, enter, ctrl-c, up.
    #[gpui_kit::test]
    fn keys_reach_the_shell(cx: &mut TestAppContext) {
        cx.executor().allow_parking(); // real PTY I/O happens outside the test scheduler
        cx.update(gpui_kit::init);
        let (term, cx) = cx.add_window_view(|window, cx| {
            let t = Terminal::new(cx).unwrap();
            window.focus(&t.focus, cx);
            t
        });
        cx.simulate_input("echo s2okX");
        cx.simulate_keystrokes("backspace space $ ( ( 40 + 2 ) ) enter");
        wait_for(cx, &term, |s| s.contains("s2ok 42"));

        cx.simulate_input("sleep 30");
        cx.simulate_keystrokes("enter");
        std::thread::sleep(Duration::from_millis(300));
        cx.simulate_keystrokes("ctrl-c");
        cx.simulate_input("echo int$((6*7))");
        cx.simulate_keystrokes("enter");
        wait_for(cx, &term, |s| s.contains("int42"));

        cx.simulate_keystrokes("up enter");
        wait_for(cx, &term, |s| s.matches("int42").count() == 2);
    }

    gpui_kit::actions!(t, [Bump]);

    struct Counter {
        n: usize,
        focus: gpui_kit::FocusHandle,
    }

    impl Render for Counter {
        fn render(&mut self, _: &mut gpui_kit::Window, cx: &mut Context<Self>) -> impl IntoElement {
            gpui_kit::div()
                .key_context("Counter")
                .track_focus(&self.focus)
                .on_action(cx.listener(|this: &mut Self, _: &Bump, _, cx| {
                    this.n += 1;
                    cx.notify();
                }))
                .child(self.n.to_string())
        }
    }

    /// A binding scoped to a key context dispatches its action to the
    /// focused element inside that context.
    #[gpui_kit::test]
    fn scoped_binding_dispatches_action(cx: &mut TestAppContext) {
        cx.update(|cx| {
            gpui_kit::init(cx);
            cx.bind_keys([gpui_kit::KeyBinding::new("cmd-k", Bump, Some("Counter"))]);
        });
        let (counter, cx) = cx.add_window_view(|window, cx| {
            let focus = cx.focus_handle();
            window.focus(&focus, cx);
            Counter { n: 0, focus }
        });
        cx.simulate_keystrokes("cmd-k cmd-k");
        assert_eq!(counter.read_with(cx, |c, _| c.n), 2);
    }
}
