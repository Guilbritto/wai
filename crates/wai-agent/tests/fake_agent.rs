use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use async_channel::Receiver;
use wai_agent::{
    AgentCaps, AgentError, AgentEvent, AgentHandle, AgentSpec, BrokerError, ChoiceKind,
    PermissionRequest, PromptBlock, SessionEvent, StopReason, ToolContent,
};

fn run(test: impl Future<Output = ()>) {
    smol::block_on(smol::future::or(test, async {
        smol::Timer::after(Duration::from_secs(10)).await;
        panic!("test timed out after 10 s");
    }))
}

fn spawn(args: &[&str]) -> (AgentHandle, Receiver<AgentEvent>) {
    let command = std::iter::once(env!("CARGO_BIN_EXE_wai-fake-agent"))
        .chain(args.iter().copied())
        .map(|a| format!("'{a}'"))
        .collect::<Vec<_>>()
        .join(" ");
    AgentHandle::spawn(AgentSpec {
        name: "fake".into(),
        command,
    })
}

/// Collects events until `pred` matches; the matching event is the last one returned.
async fn until(
    rx: &Receiver<AgentEvent>,
    mut pred: impl FnMut(&AgentEvent) -> bool,
) -> Vec<AgentEvent> {
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

async fn ready(rx: &Receiver<AgentEvent>) -> AgentEvent {
    until(rx, |e| matches!(e, AgentEvent::Ready { .. }))
        .await
        .pop()
        .unwrap()
}

fn agent_text(events: &[AgentEvent]) -> String {
    events
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Update {
                event: SessionEvent::AgentText(t),
                ..
            } => Some(t.as_str()),
            _ => None,
        })
        .collect()
}

fn cwd() -> PathBuf {
    std::env::temp_dir()
}

async fn permission(rx: &Receiver<AgentEvent>) -> PermissionRequest {
    match until(rx, |e| matches!(e, AgentEvent::PermissionRequested(_)))
        .await
        .pop()
    {
        Some(AgentEvent::PermissionRequested(p)) => p,
        other => unreachable!("{other:?}"),
    }
}

fn hello() -> Vec<PromptBlock> {
    vec![PromptBlock::Text("hello".into())]
}

#[test]
fn streams_agent_text_and_ends_turn() {
    run(async {
        let (agent, rx) = spawn(&["--reply", "one two three"]);
        match ready(&rx).await {
            AgentEvent::Ready { name, caps, .. } => {
                assert_eq!(name, "wai-fake-agent");
                assert_eq!(
                    caps,
                    AgentCaps {
                        load_session: true,
                        list_sessions: true,
                        images: true
                    }
                );
            }
            other => unreachable!("{other:?}"),
        }
        let session = agent.new_session(cwd()).await.unwrap();
        assert_eq!(session.current_mode.as_deref(), Some("default"));
        assert_eq!(session.modes.len(), 2);

        agent.prompt(session.id.clone(), hello());
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert_eq!(agent_text(&events), "one two three");
        assert_eq!(
            events.last(),
            Some(&AgentEvent::TurnEnded {
                session: session.id,
                reason: StopReason::EndTurn
            })
        );
    });
}

#[test]
fn permission_request_roundtrip() {
    run(async {
        let (agent, rx) = spawn(&["--ask-permission"]);
        ready(&rx).await;
        let session = agent.new_session(cwd()).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        let request = permission(&rx).await;
        assert_eq!(request.session, session.id);
        assert_eq!(request.diff.clone().unwrap().new_text, "hi\n");
        assert_eq!(request.options.len(), 3);
        let allow = request
            .options
            .iter()
            .find(|o| o.kind == ChoiceKind::AllowOnce)
            .unwrap()
            .id
            .clone();

        assert_eq!(
            agent.answer_permission(request.id, Some(allow.clone())),
            Ok(())
        );
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert!(
            agent_text(&events).contains(&format!("chose:{allow}")),
            "{events:?}"
        );
    });
}

#[test]
fn late_permission_answer_is_stale() {
    run(async {
        let (agent, rx) = spawn(&["--ask-permission"]);
        ready(&rx).await;
        let session = agent.new_session(cwd()).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let request = permission(&rx).await;

        agent.cancel(session.id.clone());
        until(&rx, |e| {
            *e == AgentEvent::PermissionResolved { id: request.id }
        })
        .await;
        assert_eq!(
            agent.answer_permission(request.id, Some(request.options[0].id.clone())),
            Err(BrokerError::Stale)
        );
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert_eq!(
            events.last(),
            Some(&AgentEvent::TurnEnded {
                session: session.id,
                reason: StopReason::Cancelled
            })
        );
    });
}

#[test]
fn cancel_ends_turn_with_cancelled() {
    run(async {
        let reply = vec!["word"; 200].join(" ");
        let (agent, rx) = spawn(&["--reply", &reply, "--chunk-delay-ms", "20"]);
        ready(&rx).await;
        let session = agent.new_session(cwd()).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        until(&rx, |e| {
            matches!(
                e,
                AgentEvent::Update {
                    event: SessionEvent::AgentText(_),
                    ..
                }
            )
        })
        .await;
        agent.cancel(session.id.clone());
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert_eq!(
            events.last(),
            Some(&AgentEvent::TurnEnded {
                session: session.id,
                reason: StopReason::Cancelled
            })
        );
    });
}

#[test]
fn crash_mid_stream_reports_exit_with_stderr() {
    run(async {
        let (agent, rx) = spawn(&["--reply", "a b c d e", "--crash-after", "2"]);
        ready(&rx).await;
        let session = agent.new_session(cwd()).await.unwrap();
        agent.prompt(session.id, hello());

        let events = until(&rx, |e| matches!(e, AgentEvent::Exited { .. })).await;
        match events.last() {
            Some(AgentEvent::Exited { error: Some(e) }) => {
                assert!(e.contains("fake agent crashed"), "{e}")
            }
            other => panic!("expected an exit error, got {other:?}"),
        }
        assert_eq!(agent.new_session(cwd()).await, Err(AgentError::Exited));
    });
}

#[test]
fn require_auth_emits_auth_required() {
    run(async {
        let (agent, rx) = spawn(&["--require-auth"]);
        ready(&rx).await;
        assert_eq!(
            agent.new_session(cwd()).await,
            Err(AgentError::AuthRequired)
        );
        until(&rx, |e| {
            *e == AgentEvent::AuthRequired {
                agent: "fake".into(),
            }
        })
        .await;
    });
}

#[test]
fn flood_tool_output_arrives_intact() {
    run(async {
        let (agent, rx) = spawn(&["--flood", "5000000"]);
        ready(&rx).await;
        let session = agent.new_session(cwd()).await.unwrap();
        agent.prompt(session.id, hello());

        let events = until(&rx, |e| {
            matches!(
                e,
                AgentEvent::Update {
                    event: SessionEvent::ToolCall(_),
                    ..
                }
            )
        })
        .await;
        match events.last() {
            Some(AgentEvent::Update {
                event: SessionEvent::ToolCall(call),
                ..
            }) => match call.content.as_slice() {
                [ToolContent::Text(text)] => assert_eq!(text.len(), 5_000_000),
                other => panic!("unexpected content: {} items", other.len()),
            },
            other => unreachable!("{other:?}"),
        }
    });
}

#[test]
fn list_sessions_follows_pages() {
    run(async {
        let (agent, rx) = spawn(&["--sessions", "25"]);
        ready(&rx).await;
        let sessions = agent.list_sessions().await.unwrap();
        assert_eq!(sessions.len(), 25);
        assert_eq!(sessions[0].title.as_deref(), Some("Session 0"));
        assert_eq!(sessions[24].title.as_deref(), Some("Session 24"));
        assert_eq!(sessions[0].cwd, PathBuf::from("/tmp"));
    });
}

#[test]
fn load_session_replays_history() {
    run(async {
        let (agent, rx) = spawn(&[]);
        ready(&rx).await;
        let started = agent.load_session("abc".into(), cwd()).await.unwrap();
        assert_eq!(started.id, "abc");

        let replay: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(
            replay,
            vec![
                AgentEvent::Update {
                    session: "abc".into(),
                    event: SessionEvent::UserText("previous prompt".into())
                },
                AgentEvent::Update {
                    session: "abc".into(),
                    event: SessionEvent::AgentText("previous answer".into())
                },
            ]
        );
        assert_eq!(agent.set_mode("abc".into(), "plan".into()).await, Ok(()));
    });
}

#[cfg(unix)]
#[test]
fn shutdown_kills_agent_process() {
    run(async {
        let pid_file =
            std::env::temp_dir().join(format!("wai-fake-agent-{}.pid", std::process::id()));
        let (agent, rx) = spawn(&["--pid-file", pid_file.to_str().unwrap()]);
        ready(&rx).await;
        let pid = std::fs::read_to_string(&pid_file).unwrap();
        std::fs::remove_file(&pid_file).unwrap();

        agent.shutdown();
        let events = until(&rx, |e| matches!(e, AgentEvent::Exited { .. })).await;
        assert_eq!(events.last(), Some(&AgentEvent::Exited { error: None }));

        let alive = || {
            std::process::Command::new("kill")
                .args(["-0", pid.trim()])
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap()
                .success()
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while alive() {
            assert!(
                std::time::Instant::now() < deadline,
                "agent {pid} still alive"
            );
            smol::Timer::after(Duration::from_millis(20)).await;
        }
    });
}

#[test]
fn handle_is_clone_send_sync() {
    fn check<T: Clone + Send + Sync>() {}
    check::<AgentHandle>();
}
