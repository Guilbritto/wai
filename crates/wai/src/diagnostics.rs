//! A plain-text report the user can paste into an issue: no environment, no extension values,
//! no custom commands, and token-looking words masked.

use gpui_kit::component::WindowExt as _;
use gpui_kit::component::notification::Notification;
use gpui_kit::{AppContext as _, ClipboardItem, Context, Window};
use std::fmt::Write as _;
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use wai_settings::{Settings, builtin};

use crate::workspace::Workspace;

pub const LOG_BYTES: u64 = 32 * 1024;
pub const LOG_LINES: usize = 200;

pub struct AgentLine {
    pub id: String,
    pub status: String,
    pub adapter: Option<String>,
    pub command: String,
}

pub struct Inputs {
    pub wai: &'static str,
    pub os: String,
    pub settings: Settings,
    pub home: Option<PathBuf>,
    pub agents: Vec<AgentLine>,
    /// `(id, version, enabled)`
    pub extensions: Vec<(String, String, bool)>,
    pub log_tail: String,
}

pub fn os_line() -> String {
    let mut line = format!("{} {}", std::env::consts::OS, std::env::consts::ARCH);
    #[cfg(target_os = "macos")]
    if let Ok(out) = std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
    {
        let v = String::from_utf8_lossy(&out.stdout);
        if !v.trim().is_empty() {
            let _ = write!(line, " {}", v.trim());
        }
    }
    line
}

fn basename(program: &str) -> &str {
    program.rsplit(['/', '\\']).next().unwrap_or(program)
}

/// The built-in command as is; anything else only as its program name and argument count.
fn command_shown(id: &str, command: &str) -> String {
    if builtin(id).is_some_and(|b| b.command == command) {
        return command.to_string();
    }
    let mut words = command.split_whitespace();
    let program = basename(words.next().unwrap_or(""));
    format!("<custom: {program}, {} args>", words.count())
}

pub fn report(inputs: &Inputs) -> String {
    let s = &inputs.settings;
    let mut out = String::new();
    let _ = writeln!(out, "wai\n  version: {}\n", inputs.wai);
    let _ = writeln!(out, "System\n  {}\n", inputs.os);
    out.push_str("Agents\n");
    for a in &inputs.agents {
        let adapter = a.adapter.as_deref().unwrap_or("-");
        let _ = writeln!(
            out,
            "  {}: {} (adapter {}) {}",
            a.id,
            a.status,
            adapter,
            command_shown(&a.id, &a.command)
        );
    }
    out.push_str("\nSettings\n");
    let _ = writeln!(
        out,
        "  default_agent: {}",
        s.default_agent.as_deref().unwrap_or("-")
    );
    let _ = writeln!(out, "  theme: {:?}", s.theme);
    let shell = s.terminal.shell.as_deref().unwrap_or("");
    let _ = writeln!(
        out,
        "  terminal.shell: {}",
        basename(shell.split_whitespace().next().unwrap_or("-"))
    );
    let _ = writeln!(out, "  updates.check: {}", s.updates.check);
    let _ = writeln!(
        out,
        "  updates.repo: {}",
        s.updates.repo.as_deref().unwrap_or("-")
    );
    let _ = writeln!(
        out,
        "  conversation.processing: {:?}",
        s.conversation.processing
    );
    for (id, cfg) in &s.agents {
        let _ = writeln!(out, "  agents.{id}: {}", command_shown(id, &cfg.command));
    }
    for (id, model) in &s.models {
        let _ = writeln!(out, "  models.{id}: {model}");
    }
    out.push_str("\nExtensions\n");
    for (id, version, enabled) in &inputs.extensions {
        let state = if *enabled { "enabled" } else { "disabled" };
        let _ = writeln!(out, "  {id} {version} ({state})");
    }
    let _ = write!(out, "\nLog (last {LOG_LINES} lines)\n{}\n", inputs.log_tail);
    scrub(&out, inputs.home.as_deref())
}

const SECRET_KEYS: [&str; 5] = ["key", "token", "secret", "password", "authorization"];

fn is_secret_key(key: &str) -> bool {
    let key = key.trim_matches(|c: char| c == '"' || c == '\'');
    SECRET_KEYS.iter().any(|k| key.eq_ignore_ascii_case(k))
}

fn is_long_secret(word: &str) -> bool {
    word.chars().count() >= 32
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_+/=.-".contains(c))
        && word.split('/').filter(|s| !s.is_empty()).count() < 3
}

fn mask_home(text: &str, home: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(i) = rest.find(home) {
        let after = &rest[i + home.len()..];
        out.push_str(&rest[..i]);
        // only a whole path component: `/Users/x`, not `/Users/xavier`
        let whole = after
            .chars()
            .next()
            .is_none_or(|c| !(c.is_alphanumeric() || c == '_' || c == '-' || c == '.'));
        out.push_str(if whole { "~" } else { home });
        rest = after;
    }
    out.push_str(rest);
    out
}

pub fn scrub(text: &str, home: Option<&Path>) -> String {
    let text = match home.and_then(Path::to_str).filter(|h| h.len() > 1) {
        Some(h) => mask_home(text, h),
        None => text.to_string(),
    };
    let mut out = Vec::new();
    for line in text.split('\n') {
        let mut mask_next = false;
        let words: Vec<String> = line
            .split(' ')
            .map(|word| {
                if std::mem::take(&mut mask_next) && !word.is_empty() {
                    return "***".to_string();
                }
                if word.eq_ignore_ascii_case("bearer") {
                    mask_next = true;
                    return word.to_string();
                }
                if let Some(i) = word.find(['=', ':']) {
                    let (key, rest) = word.split_at(i);
                    let key = key.trim_start_matches('-');
                    if is_secret_key(key) {
                        if rest.len() == 1 {
                            mask_next = true; // `token: abc`
                            return word.to_string();
                        }
                        return format!("{}{}***", &word[..i], &rest[..1]);
                    }
                }
                if is_long_secret(word) {
                    return "***".to_string();
                }
                word.to_string()
            })
            .collect();
        out.push(words.join(" "));
    }
    out.join("\n")
}

/// The last `max_lines` lines of the last `max_bytes` of the file; `"(no log)"` when unreadable.
pub fn tail(path: &Path, max_bytes: u64, max_lines: usize) -> String {
    let read = || -> std::io::Result<Vec<u8>> {
        let mut file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        let start = len.saturating_sub(max_bytes);
        file.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        file.take(max_bytes).read_to_end(&mut buf)?;
        if start > 0 {
            // the first line is cut: drop it (this also drops a half char)
            let skip = buf
                .iter()
                .position(|&b| b == b'\n')
                .map_or(buf.len(), |i| i + 1);
            buf.drain(..skip);
        }
        Ok(buf)
    };
    let Ok(buf) = read() else {
        return "(no log)".into();
    };
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    lines[lines.len().saturating_sub(max_lines)..].join("\n")
}

impl Workspace {
    /// Copies the report to the clipboard; the log and the OS version are read off the main thread.
    pub(crate) fn copy_diagnostics(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let agents = (self.agent_rows(cx).into_iter())
            .map(|row| AgentLine {
                id: row.id,
                status: format!("{:?}", row.status),
                adapter: row.adapter,
                command: row.command,
            })
            .collect();
        let list = match &self.extensions {
            Some(view) => view.read(cx).store().list(),
            None => wai_ext::ExtStore::open(&self.user_dir).list(),
        };
        let extensions = (list.into_iter())
            .map(|e| {
                (
                    e.manifest.id.as_str().to_string(),
                    e.manifest.version,
                    e.enabled,
                )
            })
            .collect();
        let mut inputs = Inputs {
            wai: env!("CARGO_PKG_VERSION"),
            os: String::new(),
            settings: self.settings.clone(),
            home: dirs::home_dir(),
            agents,
            extensions,
            log_tail: String::new(),
        };
        let read = cx.background_spawn(async move {
            let tail = crate::log::path().map(|p| tail(p, LOG_BYTES, LOG_LINES));
            (os_line(), tail.unwrap_or_else(|| "(no log)".into()))
        });
        cx.spawn_in(window, async move |_, cx| {
            let (os, log_tail) = read.await;
            (inputs.os, inputs.log_tail) = (os, log_tail);
            let text = report(&inputs);
            cx.update(|window, cx| {
                cx.write_to_clipboard(ClipboardItem::new_string(text));
                window.push_notification(Notification::success("Diagnostics copied"), cx);
            })
            .ok();
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs(settings: Settings) -> Inputs {
        Inputs {
            wai: "1.2.3",
            os: "testos x86_64".into(),
            settings,
            home: None,
            agents: vec![],
            extensions: vec![],
            log_tail: String::new(),
        }
    }

    #[test]
    fn custom_command_is_collapsed() {
        let mut s = Settings::default();
        s.agents.get_mut("claude").unwrap().command =
            "/opt/bin/myagent --api-key=sk-abc --x".into();
        let r = report(&inputs(s));
        assert!(!r.contains("sk-abc"));
        assert!(r.contains("<custom: myagent, 2 args>"));
        assert!(r.contains("agents.codex: npx -y @agentclientprotocol/codex-acp"));
    }

    #[test]
    fn scrub_masks_tokens_and_home() {
        let home = Path::new("/Users/x");
        assert_eq!(scrub("/Users/x/p", Some(home)), "~/p");
        assert_eq!(scrub("/Users/xavier/p", Some(home)), "/Users/xavier/p");
        assert_eq!(scrub("Bearer abc123", None), "Bearer ***");
        assert_eq!(scrub("token=abcdef", None), "token=***");
        assert_eq!(scrub("Password: hunter2", None), "Password: ***");
        let long = "a".repeat(40);
        assert_eq!(scrub(&format!("k {long} z"), None), "k *** z");
        let plain = "ordinary words in /usr/local/lib/x/y";
        assert_eq!(scrub(plain, None), plain);
        let short = "/a/b/c/dddddddd";
        assert_eq!(scrub(short, None), short);
        let deep = format!("/{}/{}/{}", "a".repeat(12), "b".repeat(12), "c".repeat(12));
        assert_eq!(scrub(&deep, None), deep);
    }

    #[test]
    fn report_has_no_env_or_extension_values() {
        let mut s = Settings::default();
        s.terminal.shell = Some("/bin/zsh -c hunter2".into());
        let mut i = inputs(s);
        i.extensions = vec![("ext".into(), "0.1.0".into(), true)];
        let r = report(&i);
        assert!(!r.contains("hunter2"));
        assert!(r.contains("terminal.shell: zsh"));
        assert!(r.contains("ext 0.1.0 (enabled)"));
        for (name, value) in std::env::vars() {
            let up = name.to_uppercase();
            if ["TOKEN", "KEY", "SECRET"].iter().any(|k| up.contains(k)) && value.len() >= 8 {
                assert!(!r.contains(&value), "{name} leaked");
            }
        }
    }

    #[test]
    fn tail_survives_missing_big_and_binary_logs() {
        let dir = std::env::temp_dir().join(format!("wai-diag-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(tail(&dir.join("none.log"), 1024, 10), "(no log)");

        let big = dir.join("big.log");
        std::fs::write(&big, "line of the log\n".repeat(64 * 1024)).unwrap();
        let t = tail(&big, 32 * 1024, 50);
        assert!(t.lines().count() <= 50 && t.len() < 32 * 1024);

        let bin = dir.join("bin.log");
        let mut bytes = vec![0xff, 0xfe, b'\n', b'a', b'\n'];
        bytes.extend("€".bytes().take(2)); // a 3-byte char, cut at the end
        std::fs::write(&bin, &bytes).unwrap();
        assert!(tail(&bin, 1024, 10).contains('\u{fffd}'));
        // cut mid-char at the start: the broken first line is dropped
        let cut = dir.join("cut.log");
        std::fs::write(&cut, "€€€\nok\n").unwrap();
        assert_eq!(tail(&cut, 5, 10), "ok");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn report_lists_each_agent_with_its_version() {
        let mut i = inputs(Settings::default());
        i.agents = vec![
            AgentLine {
                id: "claude".into(),
                status: "ready".into(),
                adapter: Some("claude-agent-acp 0.103.0".into()),
                command: "agy acp".into(),
            },
            AgentLine {
                id: "codex".into(),
                status: "stopped".into(),
                adapter: None,
                command: "agy acp".into(),
            },
        ];
        let r = report(&i);
        assert!(r.contains("claude: ready (adapter claude-agent-acp 0.103.0)"));
        assert!(r.contains("codex: stopped (adapter -)"));
        assert!(r.contains("Log (last 200 lines)"));
    }

    #[gpui_kit::test]
    fn copy_diagnostics_fills_the_clipboard(cx: &mut gpui_kit::TestAppContext) {
        use crate::workspace::tests::{fake_settings, open, wait_until};
        let (workspace, cx) = open(cx, fake_settings(""));
        cx.update(|window, cx| workspace.update(cx, |w, cx| w.copy_diagnostics(window, cx)));
        wait_until(cx, "the clipboard", |cx| cx.read_from_clipboard().is_some());
        let text = cx.read_from_clipboard().and_then(|c| c.text()).unwrap();
        assert!(text.starts_with(&format!("wai\n  version: {}", env!("CARGO_PKG_VERSION"))));
        assert!(text.contains("\nAgents\n"));
        if let Some(home) = dirs::home_dir().and_then(|h| h.to_str().map(String::from)) {
            assert!(home.len() < 2 || !text.contains(&home), "home leaked");
        }
    }
}
