//! Opt-in smoke tests: each built-in agent starts from its `BUILTIN_AGENTS` command, answers
//! `initialize` and opens a session (`session/new`), through the developer's own CLI and login:
//! `cargo test -p wai-agent --test real_agents -- --ignored --nocapture [claude|codex|antigravity|copilot]`
//! Never run by plain `cargo test`.
//!
//! Verified 2026-11-10: claude (initialize and session/new). codex: the npx command downloads and
//! answers `initialize`; `session/new` then fails with AuthRequired (not signed in here).
//! Not verified: antigravity and copilot (their CLIs were not installed), and every install and
//! login hint.

use std::path::PathBuf;
use std::time::Duration;

use wai_agent::{AgentEvent, AgentHandle, AgentSpec};

fn run(test: impl Future<Output = ()>) {
    smol::block_on(smol::future::or(test, async {
        smol::Timer::after(Duration::from_secs(240)).await;
        panic!("test timed out after 240 s");
    }))
}

fn check(id: &str) {
    run(async {
        let builtin = wai_settings::builtin(id).expect("a built-in agent");
        let dir: PathBuf =
            std::env::temp_dir().join(format!("wai-real-{id}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = wai_settings::canonical(&dir);

        let (agent, rx) = AgentHandle::spawn(AgentSpec {
            name: id.into(),
            command: builtin.command.into(),
        });
        loop {
            match rx.recv().await.expect("event channel closed") {
                AgentEvent::Ready { name, version, .. } => {
                    println!("ready: {name} {version}");
                    break;
                }
                AgentEvent::AuthRequired { .. } => panic!("not signed in: {}", builtin.login),
                AgentEvent::Exited { error } => panic!("agent exited before ready: {error:?}"),
                _ => {}
            }
        }
        let session = agent
            .new_session(dir.clone(), vec![])
            .await
            .expect("new_session");
        println!("session: {}", session.id);

        agent.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
#[ignore = "needs network, npx and a Claude Code login"]
fn claude() {
    check("claude");
}

#[test]
#[ignore = "needs network, npx and a Codex login"]
fn codex() {
    check("codex");
}

#[test]
#[ignore = "needs the agy CLI and a login"]
fn antigravity() {
    check("antigravity");
}

#[test]
#[ignore = "needs the copilot CLI and a login"]
fn copilot() {
    check("copilot");
}
