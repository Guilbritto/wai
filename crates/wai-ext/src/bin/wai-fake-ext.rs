//! A scripted wai extension for wai-ext's tests: line-delimited JSON-RPC 2.0 on stdio.
//!
//! It answers `initialize` with `{"panels": []}`, then reports it as a `notify` notification
//! titled `initialize` whose body is `{params, env_ext_dir, cwd}`. Every later message from wai
//! is reported the same way (title: the method, body: its params); requests are answered with
//! `null`. A response from wai (to a call, or an error with a null id) is reported titled `reply`,
//! with the whole response as the body. Flags:
//!
//! - `--pid-file <path>`: write the process id there at start.
//! - `--spawn-sleep <path>`: start `sleep 600` (unix) and write its process id there.
//! - `--no-init`: never answer `initialize`.
//! - `--crash`: right after answering `initialize`, print `fake ext crashed` to stderr, exit 3.
//! - `--flood <n>`: after `initialize`, send n `panel/render` notifications for panel `main`,
//!   with text trees `0` to `n - 1`.
//! - `--panel`: after `initialize`, render the sample tree (`sample` below) on panel `main`; each
//!   `panel/action` renders it again, its last text saying `last action: <id> <values as JSON>`.
//! - `--oversized`: after `initialize`, send a 5 MB `notify` request titled `big`, then a `notify`
//!   notification titled `after`.
//! - `--call <method>` (repeatable): after `initialize`, call each method with params `{}`, with
//!   ids from 1.
//! - `--exit`: after all of the above, exit 0.

use std::io::{BufRead as _, Write as _};

use serde_json::{Value, json};

fn send(msg: Value) {
    let mut out = std::io::stdout().lock();
    if writeln!(out, "{msg}").and_then(|()| out.flush()).is_err() {
        std::process::exit(0);
    }
}

fn notify(title: &str, body: String) -> Value {
    json!({"jsonrpc": "2.0", "method": "notify", "params": {"title": title, "body": body}})
}

/// One node of each type, an unknown one included.
fn sample(last: &str) -> Value {
    json!({"type": "column", "children": [
        {"type": "row", "children": [
            {"type": "text", "text": "My cards"},
            {"type": "badge", "text": "3 open"},
        ]},
        {"type": "markdown", "text": "Cards assigned to **you**, newest first."},
        {"type": "input", "id": "query", "placeholder": "Filter cards"},
        {"type": "row", "children": [
            {"type": "select", "id": "scope", "options": ["Mine", "Team"]},
            {"type": "checkbox", "id": "done", "label": "Include done"},
        ]},
        {"type": "list", "items": [
            {"id": "card-1", "title": "Fix the login timeout", "subtitle": "In progress", "badge": "P1"},
            {"id": "card-2", "title": "Add the export button", "subtitle": "To do"},
            {"id": "card-3", "title": "Update the docs", "subtitle": "In review", "badge": "P3"},
        ]},
        {"type": "progress", "value": 0.4},
        {"type": "row", "children": [
            {"type": "button", "id": "refresh", "label": "Refresh", "style": "primary"},
            {"type": "button", "id": "clear", "label": "Clear", "style": "danger"},
        ]},
        {"type": "sparkline", "points": [1, 2, 3]},
        {"type": "text", "text": last, "muted": true},
    ]})
}

fn render(tree: Value) -> Value {
    json!({"jsonrpc": "2.0", "method": "panel/render", "params": {"panel": "main", "tree": tree}})
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let flag = |name: &str| args.iter().any(|a| a == name);
    let values = |name: &str| -> Vec<String> {
        (args.windows(2))
            .filter(|w| w[0] == name)
            .map(|w| w[1].clone())
            .collect()
    };
    let value = |name: &str| values(name).pop();

    if let Some(path) = value("--pid-file") {
        std::fs::write(path, std::process::id().to_string()).unwrap();
    }
    if let Some(path) = value("--spawn-sleep") {
        let sleep = std::process::Command::new("sleep").arg("600").spawn();
        std::fs::write(path, sleep.unwrap().id().to_string()).unwrap();
    }

    for line in std::io::stdin().lock().lines() {
        let Ok(line) = line else { break };
        let Ok(msg) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match msg["method"].as_str() {
            Some("initialize") if flag("--no-init") => {}
            Some("initialize") => {
                send(json!({"jsonrpc": "2.0", "id": msg["id"], "result": {"panels": []}}));
                if flag("--crash") {
                    eprintln!("fake ext crashed");
                    std::process::exit(3);
                }
                let init = json!({
                    "params": msg["params"],
                    "env_ext_dir": std::env::var("WAI_EXT_DIR").ok(),
                    "cwd": std::env::current_dir().ok(),
                });
                send(notify("initialize", init.to_string()));
                if let Some(n) = value("--flood") {
                    for n in 0..n.parse::<u64>().unwrap() {
                        send(render(json!({"type": "text", "text": n.to_string()})));
                    }
                }
                if flag("--panel") {
                    send(render(sample("last action: none")));
                }
                if flag("--oversized") {
                    let big = "x".repeat(5 << 20);
                    send(json!({"jsonrpc": "2.0", "id": 99, "method": "notify",
                        "params": {"title": "big", "body": big}}));
                    send(notify("after", "{}".into()));
                }
                for (ix, method) in values("--call").iter().enumerate() {
                    send(json!({"jsonrpc": "2.0", "id": ix + 1, "method": method, "params": {}}));
                }
                if flag("--exit") {
                    std::process::exit(0);
                }
            }
            Some(method) => {
                send(notify(method, msg["params"].to_string()));
                if let Some(id) = msg.get("id") {
                    send(json!({"jsonrpc": "2.0", "id": id, "result": null}));
                }
                if method == "panel/action" && flag("--panel") {
                    let p = &msg["params"];
                    let action_id = p["action_id"].as_str().unwrap_or_default();
                    send(render(sample(&format!(
                        "last action: {action_id} {}",
                        p["values"]
                    ))));
                }
            }
            None => send(notify("reply", line)),
        }
    }
}
