mod keys;
pub use keys::{key_to_bytes, paste_bytes};

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode, viewport_to_point};
use alacritty_terminal::vte::ansi::{Color, NamedColor, Processor};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub cols: u16,
    pub rows: u16,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Rgb(pub u8, pub u8, pub u8);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CellRun {
    pub text: String,
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

#[derive(Clone, Debug, Default)]
pub struct GridSnapshot {
    pub rows: Vec<Vec<CellRun>>,
    /// `(col, row)` of the visible cursor.
    pub cursor: Option<(u16, u16)>,
    /// How many lines the view is scrolled up into the history.
    pub display_offset: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    Wakeup,
    Title(String),
    Exited,
}

type SharedWriter = Arc<Mutex<Box<dyn Write + Send>>>;

fn send_bytes(w: &SharedWriter, bytes: &[u8]) {
    if let Ok(mut w) = w.lock() {
        let _ = w.write_all(bytes).and_then(|_| w.flush());
    }
}

/// Runs under the term lock, so it must never block: replies go to a writer
/// thread through an unbounded channel and events are queued for the reader
/// thread to forward after the lock is released.
#[derive(Clone)]
struct Listener {
    replies: std::sync::mpsc::Sender<String>,
    events: Arc<Mutex<Vec<TerminalEvent>>>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let ev = match event {
            // Replies to cursor-position / device-attribute queries.
            Event::PtyWrite(text) => {
                let _ = self.replies.send(text);
                return;
            }
            Event::Title(t) => TerminalEvent::Title(t),
            Event::ResetTitle => TerminalEvent::Title(String::new()),
            Event::ChildExit(_) | Event::Exit => TerminalEvent::Exited,
            _ => return,
        };
        if let Ok(mut q) = self.events.lock() {
            q.push(ev);
        }
    }
}

pub struct TerminalModel {
    term: Arc<FairMutex<Term<Listener>>>,
    writer: SharedWriter,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

/// Default text and background colours; a run with `DEFAULT_BG` has no background of its own.
pub const DEFAULT_FG: Rgb = Rgb(0xd9, 0xde, 0xf5);
pub const DEFAULT_BG: Rgb = Rgb(0x0a, 0x0e, 0x24);
/// Lines kept above the screen.
const SCROLLBACK: usize = 10_000;

const ANSI16: [Rgb; 16] = [
    Rgb(0x1b, 0x21, 0x47),
    Rgb(0xff, 0x6b, 0x8b),
    Rgb(0x46, 0xe3, 0x9a),
    Rgb(0xff, 0xc1, 0x5a),
    Rgb(0x5b, 0xb8, 0xff),
    Rgb(0xff, 0x6e, 0xc7),
    Rgb(0x36, 0xe2, 0xc0),
    Rgb(0xc4, 0xcb, 0xeb),
    Rgb(0x62, 0x6c, 0x98),
    Rgb(0xff, 0x8f, 0xa6),
    Rgb(0x7a, 0xf0, 0xb8),
    Rgb(0xff, 0xd3, 0x8a),
    Rgb(0x8f, 0xd0, 0xff),
    Rgb(0xff, 0x9a, 0xd9),
    Rgb(0x7f, 0xf0, 0xd8),
    Rgb(0xec, 0xef, 0xfe),
];

fn indexed(i: u8) -> Rgb {
    match i {
        0..=15 => ANSI16[i as usize],
        16..=231 => {
            let n = i - 16;
            let level = |v: u8| if v == 0 { 0 } else { 55 + 40 * v };
            Rgb(level(n / 36), level(n / 6 % 6), level(n % 6))
        }
        232..=255 => {
            let v = 8 + 10 * (i - 232);
            Rgb(v, v, v)
        }
    }
}

fn resolve(color: Color, default: Rgb) -> Rgb {
    match color {
        Color::Spec(c) => Rgb(c.r, c.g, c.b),
        Color::Indexed(i) => indexed(i),
        Color::Named(n) => match n as usize {
            i @ 0..=15 => ANSI16[i],
            // Dim variants (261..) map to their normal color.
            i @ 259..=266 => ANSI16[i - 259],
            _ => match n {
                NamedColor::Background => DEFAULT_BG,
                _ => default,
            },
        },
    }
}

impl TerminalModel {
    pub fn spawn(
        shell: Option<&str>,
        cwd: &Path,
        size: GridSize,
    ) -> anyhow::Result<(TerminalModel, async_channel::Receiver<TerminalEvent>)> {
        let pair = native_pty_system().openpty(pty_size(size))?;
        let shell = shell
            .map(str::to_owned)
            .or_else(|| std::env::var("SHELL").ok())
            .unwrap_or_else(default_shell);
        let mut cmd = CommandBuilder::new(shell);
        // cmd.exe refuses a `\\?\` cwd ("UNC paths are not supported").
        cmd.cwd(dunce::simplified(cwd));
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer()?));
        let (tx, rx) = async_channel::bounded(1);
        let (replies_tx, replies_rx) = std::sync::mpsc::channel::<String>();
        let events = Arc::new(Mutex::new(Vec::new()));
        let config = Config {
            scrolling_history: SCROLLBACK,
            ..Config::default()
        };
        let term = Arc::new(FairMutex::new(Term::new(
            config,
            &TermSize::new(size.cols as usize, size.rows as usize),
            Listener {
                replies: replies_tx,
                events: events.clone(),
            },
        )));
        let reply_writer = writer.clone();
        std::thread::spawn(move || {
            while let Ok(text) = replies_rx.recv() {
                send_bytes(&reply_writer, text.as_bytes());
            }
        });

        let thread_term = term.clone();
        std::thread::spawn(move || {
            let mut parser: Processor = Processor::new();
            let mut buf = [0u8; 16 * 1024];
            let mut exited = false;
            // Blocks on read; no polling.
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                parser.advance(&mut *thread_term.lock(), &buf[..n]);
                // Term lock is released here; blocking sends are safe now.
                let queued = std::mem::take(&mut *events.lock().unwrap());
                for ev in queued {
                    exited |= ev == TerminalEvent::Exited;
                    let _ = tx.send_blocking(ev);
                }
                // A full channel already holds a pending Wakeup: coalesce.
                let _ = tx.try_send(TerminalEvent::Wakeup);
            }
            if !exited {
                let _ = tx.send_blocking(TerminalEvent::Exited);
            }
        });

        Ok((
            TerminalModel {
                term,
                writer,
                master: Mutex::new(pair.master),
                child: Mutex::new(child),
            },
            rx,
        ))
    }

    pub fn write(&self, bytes: &[u8]) {
        send_bytes(&self.writer, bytes);
    }

    pub fn resize(&self, size: GridSize) {
        self.term
            .lock()
            .resize(TermSize::new(size.cols as usize, size.rows as usize));
        if let Ok(m) = self.master.lock() {
            let _ = m.resize(pty_size(size));
        }
    }

    /// A job other than the shell owns the pty's foreground. Always `false` off unix.
    pub fn busy(&self) -> bool {
        #[cfg(unix)]
        {
            let leader = self.master.lock().ok().and_then(|m| m.process_group_leader());
            let shell = self.child.lock().ok().and_then(|c| c.process_id());
            if let (Some(leader), Some(shell)) = (leader, shell) {
                return u32::try_from(leader).is_ok_and(|leader| leader != shell);
            }
        }
        false
    }

    /// Moves the view `lines` up into the history (down when negative), as far as it goes.
    pub fn scroll(&self, lines: i32) {
        self.term.lock().scroll_display(Scroll::Delta(lines));
    }

    /// Back to the bottom, where the shell writes.
    pub fn scroll_to_bottom(&self) {
        self.term.lock().scroll_display(Scroll::Bottom);
    }

    /// Starts a selection at visible cell `(col, row)`; `right`: the pointer is on the cell's
    /// right half. A selection that never grows selects nothing.
    pub fn select(&self, col: u16, row: u16, right: bool) {
        let mut term = self.term.lock();
        let point = viewport_to_point(term.grid().display_offset(), cell(col, row));
        term.selection = Some(Selection::new(SelectionType::Simple, point, side(right)));
    }

    /// Moves the selection's end to visible cell `(col, row)`.
    pub fn select_to(&self, col: u16, row: u16, right: bool) {
        let mut term = self.term.lock();
        let point = viewport_to_point(term.grid().display_offset(), cell(col, row));
        if let Some(selection) = &mut term.selection {
            selection.update(point, side(right));
        }
    }

    pub fn selected_text(&self) -> Option<String> {
        self.term.lock().selection_to_string()
    }

    pub fn app_cursor(&self) -> bool {
        self.term.lock().mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term.lock().mode().contains(TermMode::BRACKETED_PASTE)
    }

    pub fn snapshot(&self) -> GridSnapshot {
        let term = self.term.lock();
        let grid = term.grid();
        let offset = grid.display_offset();
        let selection = term.selection.as_ref().and_then(|s| s.to_range(&term));
        let mut rows = Vec::with_capacity(grid.screen_lines());
        for r in 0..grid.screen_lines() {
            let line = Line(r as i32 - offset as i32);
            let mut runs: Vec<CellRun> = Vec::new();
            for c in 0..grid.columns() {
                let cell = &grid[line][Column(c)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let (mut fg, mut bg) = (resolve(cell.fg, DEFAULT_FG), resolve(cell.bg, DEFAULT_BG));
                // Selected cells show inverted.
                let selected = selection.is_some_and(|s| s.contains(Point::new(line, Column(c))));
                if cell.flags.contains(Flags::INVERSE) != selected {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let bold = cell.flags.contains(Flags::BOLD);
                let italic = cell.flags.contains(Flags::ITALIC);
                let underline = cell.flags.intersects(Flags::ALL_UNDERLINES);
                match runs.last_mut() {
                    Some(l)
                        if l.fg == fg
                            && l.bg == bg
                            && l.bold == bold
                            && l.italic == italic
                            && l.underline == underline =>
                    {
                        push_cell(&mut l.text, cell);
                    }
                    _ => {
                        let mut text = String::new();
                        push_cell(&mut text, cell);
                        runs.push(CellRun {
                            text,
                            fg,
                            bg,
                            bold,
                            italic,
                            underline,
                        });
                    }
                }
            }
            rows.push(runs);
        }
        // Scrolled up, the cursor may be below the view.
        let p = grid.cursor.point;
        let row = p.line.0 + offset as i32;
        let shown = (0..grid.screen_lines() as i32).contains(&row);
        let cursor = (term.mode().contains(TermMode::SHOW_CURSOR) && shown)
            .then_some((p.column.0 as u16, row as u16));
        GridSnapshot {
            rows,
            cursor,
            display_offset: offset,
        }
    }
}

/// Kills the shell; closing the PTY (ConPTY on Windows) then ends the console programs it ran.
/// No job object on Windows, on purpose: an app the user started from the shell keeps running.
impl Drop for TerminalModel {
    fn drop(&mut self) {
        if let Ok(mut c) = self.child.lock() {
            let _ = c.kill();
        }
    }
}

fn push_cell(text: &mut String, cell: &alacritty_terminal::term::cell::Cell) {
    text.push(cell.c);
    if let Some(z) = cell.zerowidth() {
        text.extend(z);
    }
}

fn cell(col: u16, row: u16) -> Point<usize> {
    Point::new(row as usize, Column(col as usize))
}

fn side(right: bool) -> Side {
    if right { Side::Right } else { Side::Left }
}

fn pty_size(s: GridSize) -> PtySize {
    PtySize {
        rows: s.rows,
        cols: s.cols,
        pixel_width: 0,
        pixel_height: 0,
    }
}

fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
    } else {
        "/bin/sh".into()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    fn spawn() -> (
        TerminalModel,
        async_channel::Receiver<TerminalEvent>,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let (m, rx) =
            TerminalModel::spawn(Some("/bin/sh"), dir.path(), GridSize { cols: 80, rows: 24 })
                .unwrap();
        (m, rx, dir)
    }

    fn wait_for(
        m: &TerminalModel,
        rx: &async_channel::Receiver<TerminalEvent>,
        pred: impl Fn(&GridSnapshot) -> bool,
    ) -> GridSnapshot {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let snap = m.snapshot();
            if pred(&snap) {
                return snap;
            }
            assert!(Instant::now() < deadline, "timed out; grid: {snap:?}");
            let _ = rx.recv_blocking_timeout(deadline);
        }
    }

    #[test]
    fn busy_follows_the_foreground_job() {
        let (m, _rx, _dir) = spawn();
        assert!(!m.busy());
        m.write(b"sleep 5\r");
        let poll = |want: bool| {
            let deadline = Instant::now() + Duration::from_secs(5);
            while m.busy() != want {
                assert!(Instant::now() < deadline, "busy never became {want}");
                std::thread::sleep(Duration::from_millis(10));
            }
        };
        poll(true);
        m.write(b"\x03");
        poll(false);
    }

    trait RecvDeadline {
        fn recv_blocking_timeout(&self, deadline: Instant) -> Option<TerminalEvent>;
    }
    impl RecvDeadline for async_channel::Receiver<TerminalEvent> {
        fn recv_blocking_timeout(&self, deadline: Instant) -> Option<TerminalEvent> {
            // Tests only: short sleep-and-try keeps the deadline honest.
            while Instant::now() < deadline {
                if let Ok(e) = self.try_recv() {
                    return Some(e);
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            None
        }
    }

    fn text(row: &[CellRun]) -> String {
        row.iter().map(|r| r.text.as_str()).collect()
    }

    #[test]
    fn title_does_not_block_snapshot() {
        let (m, rx, _d) = spawn();
        m.write(b"printf 'a\\033]0;wai-title\\007b'; printf 'again'\r");
        // Events are deliberately not drained; snapshot must still return.
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        let m = std::sync::Arc::new(m);
        let m2 = m.clone();
        std::thread::spawn(move || {
            for _ in 0..50 {
                m2.snapshot();
                std::thread::sleep(Duration::from_millis(10));
            }
            let _ = done_tx.send(());
        });
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("snapshot blocked under term lock");
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match rx.recv_blocking_timeout(deadline) {
                Some(TerminalEvent::Title(t)) => {
                    assert_eq!(t, "wai-title");
                    break;
                }
                Some(_) => {}
                None => panic!("no Title event"),
            }
        }
    }

    #[test]
    fn echo_reaches_grid() {
        let (m, rx, _d) = spawn();
        m.write(b"printf 'wai-%s' ok\r");
        wait_for(&m, &rx, |s| {
            s.rows.iter().any(|r| text(r).contains("wai-ok"))
        });
    }

    /// 10,000 lines of history: scrolling to the top shows where it starts, not line 1.
    #[test]
    fn scrollback_keeps_10000_lines() {
        let (m, rx, _d) = spawn();
        m.write(b"seq 1 12000\r");
        wait_for(&m, &rx, |s| {
            s.rows.iter().any(|r| text(r).trim() == "12000")
        });

        m.scroll(20_000);

        let snap = m.snapshot();
        assert_eq!(snap.display_offset, 10_000);
        let top: usize = text(&snap.rows[0]).trim().parse().expect("a number");
        assert!(top > 1, "the oldest lines are gone, top is {top}");
    }

    #[test]
    fn runs_merge_equal_styles() {
        let (m, rx, _d) = spawn();
        m.write(b"printf '\\033[31mRR\\033[0mxx'\r");
        let snap = wait_for(&m, &rx, |s| {
            s.rows.iter().any(|r| r.iter().any(|c| c.text == "RR"))
        });
        let row = snap
            .rows
            .iter()
            .find(|r| r.iter().any(|c| c.text == "RR"))
            .unwrap();
        let i = row.iter().position(|c| c.text == "RR").unwrap();
        assert_ne!(row[i].fg, row[i + 1].fg);
        assert_eq!(row[i].fg, ANSI16[1], "ANSI red");
        assert!(row[i + 1].text.starts_with("xx"));
    }
}
