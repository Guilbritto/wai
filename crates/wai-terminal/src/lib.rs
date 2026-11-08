mod keys;
pub use keys::key_to_bytes;

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::sync::FairMutex;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{Config, Term, TermMode};
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

#[derive(Clone)]
struct Listener {
    writer: SharedWriter,
    tx: async_channel::Sender<TerminalEvent>,
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            // Replies to cursor-position / device-attribute queries.
            Event::PtyWrite(text) => send_bytes(&self.writer, text.as_bytes()),
            Event::Title(t) => {
                let _ = self.tx.send_blocking(TerminalEvent::Title(t));
            }
            _ => {}
        }
    }
}

pub struct TerminalModel {
    term: Arc<FairMutex<Term<Listener>>>,
    writer: SharedWriter,
    master: Mutex<Box<dyn MasterPty + Send>>,
    child: Mutex<Box<dyn Child + Send + Sync>>,
}

const DEFAULT_FG: Rgb = Rgb(0xcc, 0xcc, 0xcc);
const DEFAULT_BG: Rgb = Rgb(0x1e, 0x1e, 0x1e);

const ANSI16: [Rgb; 16] = [
    Rgb(0x00, 0x00, 0x00),
    Rgb(0xcd, 0x31, 0x31),
    Rgb(0x0d, 0xbc, 0x79),
    Rgb(0xe5, 0xe5, 0x10),
    Rgb(0x24, 0x72, 0xc8),
    Rgb(0xbc, 0x3f, 0xbc),
    Rgb(0x11, 0xa8, 0xcd),
    Rgb(0xe5, 0xe5, 0xe5),
    Rgb(0x66, 0x66, 0x66),
    Rgb(0xf1, 0x4c, 0x4c),
    Rgb(0x23, 0xd1, 0x8b),
    Rgb(0xf5, 0xf5, 0x43),
    Rgb(0x3b, 0x8e, 0xea),
    Rgb(0xd6, 0x70, 0xd6),
    Rgb(0x29, 0xb8, 0xdb),
    Rgb(0xff, 0xff, 0xff),
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
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader()?;
        let writer: SharedWriter = Arc::new(Mutex::new(pair.master.take_writer()?));
        let (tx, rx) = async_channel::bounded(1);
        let term = Arc::new(FairMutex::new(Term::new(
            Config::default(),
            &TermSize::new(size.cols as usize, size.rows as usize),
            Listener {
                writer: writer.clone(),
                tx: tx.clone(),
            },
        )));

        let thread_term = term.clone();
        std::thread::spawn(move || {
            let mut parser: Processor = Processor::new();
            let mut buf = [0u8; 16 * 1024];
            // Blocks on read; no polling.
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                parser.advance(&mut *thread_term.lock(), &buf[..n]);
                // A full channel already holds a pending Wakeup: coalesce.
                let _ = tx.try_send(TerminalEvent::Wakeup);
            }
            let _ = tx.send_blocking(TerminalEvent::Exited);
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

    pub fn app_cursor(&self) -> bool {
        self.term.lock().mode().contains(TermMode::APP_CURSOR)
    }

    pub fn snapshot(&self) -> GridSnapshot {
        let term = self.term.lock();
        let grid = term.grid();
        let mut rows = Vec::with_capacity(grid.screen_lines());
        for r in 0..grid.screen_lines() {
            let mut runs: Vec<CellRun> = Vec::new();
            for c in 0..grid.columns() {
                let cell = &grid[Line(r as i32)][Column(c)];
                if cell
                    .flags
                    .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
                {
                    continue;
                }
                let (mut fg, mut bg) = (resolve(cell.fg, DEFAULT_FG), resolve(cell.bg, DEFAULT_BG));
                if cell.flags.contains(Flags::INVERSE) {
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
        let p = grid.cursor.point;
        let cursor = (term.mode().contains(TermMode::SHOW_CURSOR) && p.line.0 >= 0)
            .then_some((p.column.0 as u16, p.line.0 as u16));
        GridSnapshot { rows, cursor }
    }
}

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
    fn echo_reaches_grid() {
        let (m, rx, _d) = spawn();
        m.write(b"printf 'wai-%s' ok\r");
        wait_for(&m, &rx, |s| {
            s.rows.iter().any(|r| text(r).contains("wai-ok"))
        });
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
        assert!(row[i].fg.0 > 150 && row[i].fg.1 < 100);
        assert!(row[i + 1].text.starts_with("xx"));
    }
}
