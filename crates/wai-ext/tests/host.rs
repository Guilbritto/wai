//! The extension host against `wai-fake-ext` (see its flags in `src/bin/wai-fake-ext.rs`).

use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::time::{Duration, Instant};

use async_channel::Receiver;
use serde_json::{Value, json};
use wai_ext::*;

fn run(limit: Duration, test: impl Future<Output = ()>) {
    smol::block_on(smol::future::or(test, async move {
        smol::Timer::after(limit).await;
        panic!("test timed out after {limit:?}");
    }))
}

/// The fake extension `fake` in `dir`, with panel `main`, run with `args`.
fn installed(dir: &Path, args: &[&str], granted: Vec<Permission>) -> Installed {
    let manifest = Manifest {
        id: ExtId::new("fake").unwrap(),
        name: "Fake".into(),
        version: "1.0.0".into(),
        description: None,
        contributes: Contributes {
            panels: vec![PanelSpec {
                id: "main".into(),
                title: "Main".into(),
            }],
            ..Default::default()
        },
        process: Some(ProcessSpec {
            command: env!("CARGO_BIN_EXE_wai-fake-ext").into(),
            args: args.iter().map(|a| a.to_string()).collect(),
        }),
        permissions: granted.clone(),
    };
    Installed {
        manifest,
        dir: dir.to_path_buf(),
        data: dir.join("data"),
        enabled: true,
        granted,
    }
}

/// Collects events until `pred` matches; the matching event is the last one returned.
async fn until(rx: &Receiver<ExtEvent>, mut pred: impl FnMut(&ExtEvent) -> bool) -> Vec<ExtEvent> {
    let mut seen = Vec::new();
    loop {
        let event = rx.recv().await.expect("event channel closed");
        let done = pred(&event);
        seen.push(event);
        if done {
            return seen;
        }
    }
}

fn title(event: &ExtEvent) -> Option<&str> {
    match event {
        ExtEvent::Notify { title, .. } => Some(title),
        _ => None,
    }
}

/// The fake reports what wai sent as a notification: title, then the body as JSON.
async fn reported(rx: &Receiver<ExtEvent>, what: &str) -> Value {
    let events = until(rx, |e| title(e) == Some(what)).await;
    match events.last() {
        Some(ExtEvent::Notify { body, .. }) => serde_json::from_str(body).unwrap(),
        _ => unreachable!(),
    }
}

#[cfg(unix)]
fn wait_gone(pid: &str) {
    let alive = || {
        std::process::Command::new("kill")
            .args(["-0", pid.trim()])
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success()
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    while alive() {
        assert!(Instant::now() < deadline, "process {pid} still alive");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn initialize_roundtrip() {
    let tmp = tempfile::tempdir().unwrap();
    let dir = tmp.path().join("fake");
    std::fs::create_dir_all(&dir).unwrap();
    let granted = vec![Permission::Process, Permission::Notify, Permission::Panels];
    let mut ext = installed(&dir, &[], granted);
    let data = tmp.path().join("data");
    ext.data = data.clone();
    run(Duration::from_secs(10), async {
        let (handle, rx) = ExtHandle::spawn(&ext, Path::new("/some/project"));
        let first = rx.recv().await.unwrap();
        assert!(
            matches!(&first, ExtEvent::Ready { id } if id.as_str() == "fake"),
            "{first:?}"
        );

        let init = reported(&rx, "initialize").await;
        let dir = dir.to_str().unwrap();
        assert_eq!(
            init["params"],
            json!({
                "protocol": 1,
                "api": "1.1",
                "wai_version": env!("CARGO_PKG_VERSION"),
                "ext_id": "fake",
                "granted": ["process", "notify", "panels"],
                "project": "/some/project",
                "ext_dir": dir,
                "data_dir": data.to_str().unwrap(),
            })
        );
        assert_eq!(init["env_ext_dir"], dir);
        assert_eq!(init["env_ext_data"], data.to_str().unwrap());
        assert_eq!(init["params"]["data_dir"], data.to_str().unwrap());
        assert!(data.is_dir());
        // R18: never the extension's folder.
        assert_ne!(init["cwd"], dir);

        handle.run_command("hello");
        assert_eq!(reported(&rx, "command/run").await, json!({"id": "hello"}));
        handle.panel_action("main", "go", json!({"q": "x"}));
        assert_eq!(
            reported(&rx, "panel/action").await,
            json!({"panel": "main", "action_id": "go", "values": {"q": "x"}})
        );
        handle.notify_event(HostEvent::FileSaved {
            path: "/p/a.rs".into(),
        });
        assert_eq!(
            reported(&rx, "event/file_saved").await,
            json!({"path": "/p/a.rs"})
        );
    });
}

/// A granted ext→host request reaches wai with a reply, and wai's answer reaches the extension.
#[test]
fn granted_call_gets_reply() {
    let tmp = tempfile::tempdir().unwrap();
    let granted = vec![
        Permission::Process,
        Permission::Notify,
        Permission::SessionsRead,
    ];
    let ext = installed(tmp.path(), &["--call", "sessions/list"], granted);
    run(Duration::from_secs(10), async {
        let (_handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        let events = until(&rx, |e| matches!(e, ExtEvent::ListSessions { .. })).await;
        let Some(ExtEvent::ListSessions { reply }) = events.into_iter().last() else {
            unreachable!()
        };
        reply.send(Ok(json!([{"agent": "claude", "session": "s1"}])));
        let answer = reported(&rx, "reply").await;
        assert_eq!(answer["id"], 1);
        assert_eq!(
            answer["result"],
            json!([{"agent": "claude", "session": "s1"}])
        );
    });
}

/// Review Focus 1: every ext→host call and host→ext event is checked against the granted set.
#[test]
fn ungranted_call_denied() {
    let tmp = tempfile::tempdir().unwrap();
    let calls = [
        "sessions/list",
        "sessions/open",
        "sessions/prompt",
        "panel/render",
        "fs/read",
    ];
    let args: Vec<&str> = calls.iter().flat_map(|c| ["--call", *c]).collect();
    let ext = installed(
        tmp.path(),
        &args,
        vec![Permission::Process, Permission::Notify],
    );
    run(Duration::from_secs(10), async {
        let (handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        let mut errors = HashMap::new();
        let events = until(&rx, |e| {
            if let ExtEvent::Notify { title, body } = e
                && title == "reply"
            {
                let reply: Value = serde_json::from_str(body).unwrap();
                errors.insert(reply["id"].as_u64().unwrap(), reply["error"].clone());
            }
            errors.len() == calls.len()
        })
        .await;
        let denied = |permission: &str| json!({"code": -32001, "message": format!("Permission denied: {permission}")});
        assert_eq!(errors[&1], denied("sessions:read"));
        assert_eq!(errors[&2], denied("sessions:control"));
        assert_eq!(errors[&3], denied("sessions:control"));
        assert_eq!(errors[&4], denied("panels"));
        assert_eq!(errors[&5]["code"], -32601);
        assert!(
            events
                .iter()
                .all(|e| matches!(e, ExtEvent::Ready { .. } | ExtEvent::Notify { .. })),
            "a denied call reached wai: {events:?}"
        );

        // Host → ext: nothing the extension may not see is sent.
        let (agent, session) = ("claude".to_string(), "s1".to_string());
        handle.notify_event(HostEvent::SessionState {
            agent: agent.clone(),
            session: session.clone(),
            state: "Working".into(),
        });
        handle.notify_event(HostEvent::TurnEnded {
            agent,
            session,
            reason: "end_turn".into(),
        });
        handle.panel_action("main", "go", json!({}));
        handle.run_command("probe");
        let events = until(&rx, |e| title(e) == Some("command/run")).await;
        let leaked: Vec<_> = events.iter().filter_map(title).collect();
        assert_eq!(leaked, ["command/run"], "sent without permission");
    });
}

/// The fake's reports in its `--log` file, once `done` holds for them.
fn logged(log: &Path, mut done: impl FnMut(&[Value]) -> bool) -> Vec<Value> {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = std::fs::read_to_string(log).unwrap_or_default();
        // Whole lines only: the fake may be writing the last one.
        let lines: Vec<Value> = (text.split_inclusive('\n').filter(|l| l.ends_with('\n')))
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        if done(&lines) {
            return lines;
        }
        assert!(Instant::now() < deadline, "timed out; logged: {lines:?}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// `notify` needs its permission like the session methods (`ungranted_call_denied`): denied, it
/// never reaches wai. The fake reports to a file, since its `notify` reports are denied too.
#[test]
fn notify_denied_without_permission() {
    let tmp = tempfile::tempdir().unwrap();
    let log = tmp.path().join("log");
    let ext = installed(
        tmp.path(),
        &[
            "--log",
            log.to_str().unwrap(),
            "--call",
            r#"notify {"title": "hi", "body": "there"}"#,
        ],
        vec![Permission::Process],
    );
    let (_handle, rx) = ExtHandle::spawn(&ext, tmp.path());
    let lines = logged(&log, |lines| lines.iter().any(|l| l["title"] == "reply"));
    let reply: Value = lines
        .iter()
        .find(|l| l["title"] == "reply")
        .map(|l| serde_json::from_str(l["body"].as_str().unwrap()).unwrap())
        .unwrap();
    assert_eq!(reply["id"], 1);
    assert_eq!(
        reply["error"],
        json!({"code": -32001, "message": "Permission denied: notify"})
    );
    let events: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
    assert!(
        events.iter().all(|e| matches!(e, ExtEvent::Ready { .. })),
        "a denied notify reached wai: {events:?}"
    );
}

fn text(text: &str) -> panel::Node {
    panel::Node::Text {
        text: text.into(),
        muted: false,
    }
}

/// At most 100 renders per second; the latest tree always arrives.
#[test]
fn flood_is_coalesced() {
    let tmp = tempfile::tempdir().unwrap();
    let ext = installed(
        tmp.path(),
        &["--flood", "20000"],
        vec![Permission::Process, Permission::Panels],
    );
    run(Duration::from_secs(10), async {
        let start = Instant::now();
        let (_handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        let events = until(
            &rx,
            |e| matches!(e, ExtEvent::Render { tree, .. } if *tree == Ok(text("19999"))),
        )
        .await;
        let elapsed = start.elapsed();
        let renders: Vec<_> = events
            .iter()
            .filter_map(|e| match e {
                ExtEvent::Render { panel, tree } => Some((panel.as_str(), tree)),
                _ => None,
            })
            .collect();
        assert!(renders.iter().all(|(panel, _)| *panel == "main"));
        // Renders are at least 10 ms apart: n of them span (n - 1) * 10 ms.
        assert!(
            renders.len() as u128 <= elapsed.as_millis() / 10 + 1,
            "{} renders in {elapsed:?}",
            renders.len()
        );
    });
}

/// A tree still waiting for its render slot when the process ends is rendered, not lost.
#[test]
fn last_render_survives_exit() {
    let tmp = tempfile::tempdir().unwrap();
    let ext = installed(
        tmp.path(),
        &["--flood", "50", "--exit"],
        vec![Permission::Process, Permission::Panels],
    );
    run(Duration::from_secs(10), async {
        let (_handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        let mut last_trees = Vec::new();
        let mut tree = None;
        while let Ok(event) = rx.recv().await {
            match event {
                ExtEvent::Render { tree: t, .. } => tree = Some(t),
                ExtEvent::Ready { .. } => last_trees.extend(tree.take()),
                _ => {}
            }
        }
        last_trees.extend(tree);
        // Each of the 4 runs (the first and 3 restarts) ends on its last tree.
        assert_eq!(last_trees, vec![Ok(text("49")); 4]);
    });
}

/// A message above 4 MB is dropped with an error (its id unknown: null); the next one is read.
#[test]
fn oversized_message_rejected() {
    let tmp = tempfile::tempdir().unwrap();
    let ext = installed(
        tmp.path(),
        &["--oversized"],
        vec![Permission::Process, Permission::Notify],
    );
    run(Duration::from_secs(10), async {
        let (_handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        let events = until(&rx, |e| title(e) == Some("reply")).await;
        let titles: Vec<_> = events.iter().filter_map(title).collect();
        assert_eq!(titles, ["initialize", "after", "reply"]);
        let Some(ExtEvent::Notify { body, .. }) = events.last() else {
            unreachable!()
        };
        let reply: Value = serde_json::from_str(body).unwrap();
        assert_eq!(reply["id"], Value::Null);
        assert_eq!(
            reply["error"],
            json!({"code": -32600, "message": "Message larger than 4 MB"})
        );
    });
}

#[test]
fn initialize_timeout_marks_failed() {
    let tmp = tempfile::tempdir().unwrap();
    let pid_file = tmp.path().join("ext.pid");
    let ext = installed(
        tmp.path(),
        &["--no-init", "--pid-file", pid_file.to_str().unwrap()],
        vec![Permission::Process],
    );
    run(Duration::from_secs(20), async {
        let start = Instant::now();
        let (_handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        let event = rx.recv().await.unwrap();
        assert!(
            matches!(&event, ExtEvent::Failed { id, error }
                if id.as_str() == "fake" && error.contains("initialize")),
            "{event:?}"
        );
        assert!(start.elapsed() >= Duration::from_secs(10));
        assert!(rx.recv().await.is_err(), "nothing after Failed");
    });
    #[cfg(unix)]
    wait_gone(&std::fs::read_to_string(&pid_file).unwrap());
}

#[test]
fn crash_loop_stops_after_three() {
    let tmp = tempfile::tempdir().unwrap();
    let ext = installed(tmp.path(), &["--crash"], vec![Permission::Process]);
    // a hang guard on four process starts, not a speed limit
    run(Duration::from_secs(30), async {
        let (_handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        let mut events = Vec::new();
        while let Ok(event) = rx.recv().await {
            events.push(event);
        }
        let readies = events
            .iter()
            .filter(|e| matches!(e, ExtEvent::Ready { .. }))
            .count();
        assert_eq!(readies, 4, "the first run and 3 restarts: {events:?}");
        assert!(
            matches!(events.last(), Some(ExtEvent::Crashed { id, stderr_tail })
                if id.as_str() == "fake" && stderr_tail.contains("fake ext crashed")),
            "{events:?}"
        );
    });
}

/// The extension and what it started (its process group) are gone.
#[cfg(unix)]
#[test]
fn shutdown_kills_process() {
    let tmp = tempfile::tempdir().unwrap();
    let (pid_file, sleep_file) = (tmp.path().join("ext.pid"), tmp.path().join("sleep.pid"));
    let ext = installed(
        tmp.path(),
        &[
            "--pid-file",
            pid_file.to_str().unwrap(),
            "--spawn-sleep",
            sleep_file.to_str().unwrap(),
        ],
        vec![Permission::Process],
    );
    run(Duration::from_secs(10), async {
        let (handle, rx) = ExtHandle::spawn(&ext, tmp.path());
        until(&rx, |e| matches!(e, ExtEvent::Ready { .. })).await;

        handle.shutdown();
        wait_gone(&std::fs::read_to_string(&pid_file).unwrap());
        wait_gone(&std::fs::read_to_string(&sleep_file).unwrap());
        assert!(rx.recv().await.is_err(), "the channel closes");
    });
}
