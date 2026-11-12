/// Maps a GPUI-style key name to the bytes a terminal expects.
pub fn key_to_bytes(
    key: &str,
    key_char: Option<&str>,
    ctrl: bool,
    alt: bool,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    let arrow = |c: char| {
        let intro = if app_cursor { "\x1bO" } else { "\x1b[" };
        Some(format!("{intro}{c}").into_bytes())
    };
    match key {
        "enter" => return Some(b"\r".to_vec()),
        "backspace" => return Some(vec![0x7f]),
        "tab" => return Some(b"\t".to_vec()),
        "escape" => return Some(vec![0x1b]),
        "up" => return arrow('A'),
        "down" => return arrow('B'),
        "right" => return arrow('C'),
        "left" => return arrow('D'),
        "home" => return Some(b"\x1b[H".to_vec()),
        "end" => return Some(b"\x1b[F".to_vec()),
        "delete" => return Some(b"\x1b[3~".to_vec()),
        "pageup" => return Some(b"\x1b[5~".to_vec()),
        "pagedown" => return Some(b"\x1b[6~".to_vec()),
        _ => {}
    }
    if ctrl && key.len() == 1 && key.as_bytes()[0].is_ascii_alphabetic() {
        let b = key.as_bytes()[0].to_ascii_lowercase() & 0x1f;
        return Some(if alt { vec![0x1b, b] } else { vec![b] });
    }
    let mut out = Vec::new();
    if alt {
        out.push(0x1b);
    }
    out.extend_from_slice(key_char?.as_bytes());
    Some(out)
}

/// The bytes a paste of `text` writes to the shell: with CRLF line ends as one CR (one Enter), or
/// between bracketed-paste markers when the shell turned that mode on, without ESC (the text
/// cannot close the bracket itself) nor ^C (it cannot interrupt).
pub fn paste_bytes(text: &str, bracketed: bool) -> Vec<u8> {
    if !bracketed {
        return text.replace("\r\n", "\r").into_bytes();
    }
    let mut out = b"\x1b[200~".to_vec();
    out.extend(text.bytes().filter(|&b| b != 0x1b && b != 0x03));
    out.extend_from_slice(b"\x1b[201~");
    out
}

/// What the wheel does for the screen the program is on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelMode {
    /// wai's own history.
    History,
    /// Arrow keys (the alternate screen, as `less` and `vim` read them).
    Arrows { app_cursor: bool },
    /// Wheel button reports (a program that asked for the mouse).
    Mouse { sgr: bool },
}

/// The bytes the wheel writes to the program: `lines > 0` is the wheel moving up, `col`/`row`
/// the 0-based visible cell under the pointer.
pub fn wheel_bytes(mode: WheelMode, lines: i32, col: u16, row: u16) -> Vec<u8> {
    let up = lines > 0;
    let mut out = Vec::new();
    for _ in 0..lines.unsigned_abs() {
        match mode {
            WheelMode::History => {}
            WheelMode::Arrows { app_cursor } => {
                out.extend_from_slice(if app_cursor { b"\x1bO" } else { b"\x1b[" });
                out.push(if up { b'A' } else { b'B' });
            }
            WheelMode::Mouse { sgr } => {
                let button = if up { 64 } else { 65 };
                let (c, r) = (col as u32 + 1, row as u32 + 1);
                if sgr {
                    out.extend(format!("\x1b[<{button};{c};{r}M").bytes());
                } else if c <= 223 && r <= 223 {
                    out.extend_from_slice(&[
                        0x1b,
                        b'[',
                        b'M',
                        32 + button,
                        32 + c as u8,
                        32 + r as u8,
                    ]);
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{WheelMode, key_to_bytes, paste_bytes, wheel_bytes};

    #[test]
    fn paste_brackets_only_when_the_shell_asks() {
        assert_eq!(paste_bytes("ls\necho", false), b"ls\necho");
        assert_eq!(paste_bytes("ls\necho", true), b"\x1b[200~ls\necho\x1b[201~");
        // Pasted text cannot close the bracket early and run the rest as typed.
        assert_eq!(
            paste_bytes("a\x1b[201~rm", true),
            b"\x1b[200~a[201~rm\x1b[201~"
        );
    }

    /// A pasted ^C cannot interrupt what runs; unbracketed, a CRLF line end is one Enter.
    #[test]
    fn paste_strips_ctrl_c_when_bracketed_and_crlf_otherwise() {
        assert_eq!(paste_bytes("a\x03b", true), b"\x1b[200~ab\x1b[201~");
        assert_eq!(paste_bytes("ls\r\necho\r\n", false), b"ls\recho\r");
    }

    #[test]
    fn key_mapping_table() {
        let k = |key, ch, ctrl, alt, app| key_to_bytes(key, ch, ctrl, alt, app);
        assert_eq!(k("enter", None, false, false, false), Some(b"\r".to_vec()));
        assert_eq!(k("backspace", None, false, false, false), Some(vec![0x7f]));
        assert_eq!(k("tab", None, false, false, false), Some(b"\t".to_vec()));
        assert_eq!(k("escape", None, false, false, false), Some(vec![0x1b]));
        assert_eq!(k("up", None, false, false, false), Some(b"\x1b[A".to_vec()));
        assert_eq!(k("up", None, false, false, true), Some(b"\x1bOA".to_vec()));
        assert_eq!(k("c", None, true, false, false), Some(vec![0x03]));
        assert_eq!(k("a", Some("a"), false, false, false), Some(b"a".to_vec()));
        assert_eq!(
            k("f", Some("f"), false, true, false),
            Some(b"\x1bf".to_vec())
        );
        assert_eq!(k("unknown", None, false, false, false), None);
    }

    #[test]
    fn arrows_follow_app_cursor() {
        let m = |app_cursor| WheelMode::Arrows { app_cursor };
        assert_eq!(wheel_bytes(m(false), 2, 0, 0), b"\x1b[A\x1b[A");
        assert_eq!(wheel_bytes(m(true), -1, 0, 0), b"\x1bOB");
    }

    #[test]
    fn sgr_wheel_reports() {
        let m = WheelMode::Mouse { sgr: true };
        assert_eq!(wheel_bytes(m, 1, 4, 9), b"\x1b[<64;5;10M");
        assert_eq!(wheel_bytes(m, -1, 4, 9), b"\x1b[<65;5;10M");
    }

    #[test]
    fn x10_wheel_reports() {
        let m = WheelMode::Mouse { sgr: false };
        assert_eq!(
            wheel_bytes(m, 1, 4, 9),
            [&b"\x1b[M"[..], &[96, 37, 42]].concat()
        );
    }

    #[test]
    fn x10_reports_past_222_are_dropped() {
        let m = WheelMode::Mouse { sgr: false };
        assert!(wheel_bytes(m, 1, 223, 0).is_empty());
        assert!(wheel_bytes(m, 1, 0, 223).is_empty());
    }

    #[test]
    fn history_sends_nothing() {
        assert!(wheel_bytes(WheelMode::History, 3, 1, 1).is_empty());
    }
}
