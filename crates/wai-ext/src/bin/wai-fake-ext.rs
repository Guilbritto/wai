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
//!   with trees `{"n": 0}` to `{"n": n - 1}`.
//! - `--oversized`: after `initialize`, send a 5 MB `notify` request titled `big`, then a `notify`
//!   notification titled `after`.
//! - `--call <method>` (repeatable): after `initialize`, call each method with params `{}`, with
//!   ids from 1.

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
                        send(json!({"jsonrpc": "2.0", "method": "panel/render",
                            "params": {"panel": "main", "tree": {"n": n}}}));
                    }
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
            }
            Some(method) => {
                send(notify(method, msg["params"].to_string()));
                if let Some(id) = msg.get("id") {
                    send(json!({"jsonrpc": "2.0", "id": id, "result": null}));
                }
            }
            None => send(notify("reply", line)),
        }
    }
}
