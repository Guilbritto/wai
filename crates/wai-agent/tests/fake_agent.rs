use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use async_channel::Receiver;
use wai_agent::{
    AgentCaps, AgentError, AgentEvent, AgentHandle, AgentSpec, BrokerError, Choice, ChoiceKind,
    CommandInfo, ConfigCategory, ConfigOption, ConfigValue, FieldKind, McpServer,
    PermissionRequest, PromptBlock, QuestionAnswer, QuestionRequest, SessionEvent, StopReason,
    SubagentPatch, SubagentState, ToolContent, program, quote,
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
        .map(quote)
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

async fn question(rx: &Receiver<AgentEvent>) -> QuestionRequest {
    match until(rx, |e| matches!(e, AgentEvent::QuestionAsked(_)))
        .await
        .pop()
    {
        Some(AgentEvent::QuestionAsked(q)) => q,
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
                        fork: true,
                        images: true,
                        steering: false,
                    }
                );
            }
            other => unreachable!("{other:?}"),
        }
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        assert_eq!(session.current_mode.as_deref(), Some("default"));
        assert_eq!(session.modes.len(), 2);

        agent.prompt(session.id.clone(), hello());
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert_eq!(agent_text(&events), "one two three");
        assert_eq!(
            events.last(),
            Some(&AgentEvent::TurnEnded {
                session: session.id.clone(),
                reason: StopReason::EndTurn
            })
        );
        // An agent without steering does not know the request.
        let steered = agent.steer(session.id, hello()).await;
        assert!(
            matches!(steered, Err(AgentError::Protocol(_))),
            "{steered:?}"
        );
    });
}

/// `--steering`: a message steered during a turn joins it; with no turn running, the agent
/// asks for a prompt instead.
#[test]
fn steering_joins_the_running_turn() {
    run(async {
        let flags = [
            "--steering",
            "--reply",
            "a b c d e f",
            "--chunk-delay-ms",
            "50",
        ];
        let (agent, rx) = spawn(&flags);
        match ready(&rx).await {
            AgentEvent::Ready { caps, .. } => assert!(caps.steering),
            other => unreachable!("{other:?}"),
        }
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let text = |e: &AgentEvent| {
            matches!(
                e,
                AgentEvent::Update {
                    event: SessionEvent::AgentText(_),
                    ..
                }
            )
        };
        let mut events = until(&rx, text).await;

        let more = vec![PromptBlock::Text("more".into())];
        assert_eq!(
            agent.steer(session.id.clone(), more.clone()).await,
            Ok(true)
        );
        events.extend(until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await);
        assert_eq!(agent_text(&events), "a b c d e f Steered: more");
        assert_eq!(agent.steer(session.id.clone(), more).await, Ok(false));
    });
}

/// The agent's commands come once the session started, each with its input hint if it has one.
#[test]
fn commands_come_with_their_hints() {
    run(async {
        let (agent, rx) = spawn(&[]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        let events = until(&rx, |e| {
            matches!(
                e,
                AgentEvent::Update {
                    event: SessionEvent::Commands(_),
                    ..
                }
            )
        })
        .await;
        let Some(AgentEvent::Update {
            session: id,
            event: SessionEvent::Commands(commands),
        }) = events.last()
        else {
            unreachable!()
        };
        assert_eq!(*id, session.id);
        let command = |name: &str, description: &str, hint: Option<&str>| CommandInfo {
            name: name.into(),
            description: description.into(),
            hint: hint.map(str::to_string),
        };
        assert_eq!(
            *commands,
            [
                command("init", "Initialize a CLAUDE.md file", None),
                command("review", "Review a pull request", None),
                command(
                    "compact",
                    "Clear the history but keep a summary",
                    Some("custom instructions")
                ),
                command("pr-review", "Comment on the open pull request", None),
            ]
        );
    });
}

#[test]
fn permission_request_roundtrip() {
    run(async {
        let (agent, rx) = spawn(&["--ask-permission"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
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
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
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

/// The fake asks only when wai advertises form elicitation, as claude-agent-acp does.
#[test]
fn question_roundtrip() {
    run(async {
        let (agent, rx) = spawn(&["--ask-question"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        let q = question(&rx).await;
        assert_eq!(q.session, session.id);
        assert_eq!(q.message, "Pick a color");
        let choice = |value: &str, label: &str| Choice {
            value: value.into(),
            label: label.into(),
            description: None,
        };
        let fields: Vec<_> = q
            .fields
            .iter()
            .map(|f| (f.key.as_str(), f.title.as_str(), f.required, f.kind.clone()))
            .collect();
        assert_eq!(
            fields,
            [
                (
                    "color",
                    "Color",
                    true,
                    FieldKind::SingleChoice(vec![choice("red", "Red"), choice("blue", "Blue")])
                ),
                ("note", "Note", false, FieldKind::Text),
            ]
        );

        let serde_json::Value::Object(content) = serde_json::json!({"color": "blue", "note": "x"})
        else {
            unreachable!()
        };
        assert_eq!(
            agent.answer_question(q.id, QuestionAnswer::Accept(content)),
            Ok(())
        );
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert!(
            events.contains(&AgentEvent::QuestionResolved { id: q.id }),
            "{events:?}"
        );
        let text = agent_text(&events);
        assert!(
            text.starts_with("answered:") && text.contains(r#""color":"blue""#),
            "{text}"
        );
    });
}

/// Review Focus 5: the turn is cancelled while the form is open.
#[test]
fn late_question_answer_is_stale() {
    run(async {
        let (agent, rx) = spawn(&["--ask-question"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let q = question(&rx).await;

        agent.cancel(session.id.clone());
        until(&rx, |e| *e == AgentEvent::QuestionResolved { id: q.id }).await;
        assert_eq!(
            agent.answer_question(q.id, QuestionAnswer::Decline),
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

/// Review Focus 5: the agent withdraws the form (`$/cancel_request`) while the turn goes on.
#[test]
fn withdrawn_question_resolves_on_its_own() {
    run(async {
        let (agent, rx) = spawn(&["--ask-question", "--withdraw-question"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let withdrawn = question(&rx).await;

        // A second, regular question keeps the turn open: only the withdrawal resolves the first.
        let (mut resolved, mut next) = (false, None);
        until(&rx, |e| {
            match e {
                AgentEvent::QuestionResolved { id } if *id == withdrawn.id => resolved = true,
                AgentEvent::QuestionAsked(q) => next = Some(q.id),
                _ => {}
            }
            resolved && next.is_some()
        })
        .await;
        assert_eq!(
            agent.answer_question(withdrawn.id, QuestionAnswer::Decline),
            Err(BrokerError::Stale)
        );
        assert_eq!(
            agent.answer_question(next.unwrap(), QuestionAnswer::Decline),
            Ok(())
        );
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert!(
            agent_text(&events).contains("answered:declined"),
            "{events:?}"
        );
    });
}

#[test]
fn cancel_ends_turn_with_cancelled() {
    run(async {
        let reply = vec!["word"; 200].join(" ");
        let (agent, rx) = spawn(&["--reply", &reply, "--chunk-delay-ms", "20"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
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
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id, hello());

        let events = until(&rx, |e| matches!(e, AgentEvent::Exited { .. })).await;
        match events.last() {
            Some(AgentEvent::Exited { error: Some(e) }) => {
                assert!(e.contains("fake agent crashed"), "{e}")
            }
            other => panic!("expected an exit error, got {other:?}"),
        }
        assert_eq!(
            agent.new_session(cwd(), vec![]).await,
            Err(AgentError::Exited)
        );
    });
}

#[test]
fn require_auth_emits_auth_required() {
    run(async {
        let (agent, rx) = spawn(&["--require-auth"]);
        ready(&rx).await;
        assert_eq!(
            agent.new_session(cwd(), vec![]).await,
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
fn prompt_auth_required_emits_auth_required() {
    run(async {
        let (agent, rx) = spawn(&["--require-auth-on-prompt"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert!(
            events.contains(&AgentEvent::AuthRequired {
                agent: "fake".into()
            }),
            "{events:?}"
        );
        assert_eq!(
            events.last(),
            Some(&AgentEvent::TurnEnded {
                session: session.id,
                reason: StopReason::Other("auth_required".into())
            })
        );
    });
}

#[test]
fn flood_tool_output_arrives_intact() {
    run(async {
        let (agent, rx) = spawn(&["--flood", "5000000"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
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
        let started = agent
            .load_session("abc".into(), cwd(), vec![])
            .await
            .unwrap();
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

#[test]
fn fork_replays_source_history() {
    run(async {
        let (agent, rx) = spawn(&[]);
        ready(&rx).await;
        let forked = agent
            .fork_session("abc".into(), cwd(), vec![])
            .await
            .unwrap();
        assert_ne!(forked.id, "abc");
        assert_eq!(forked.modes.len(), 2);

        // The fork holds the source's conversation: loading it replays it under the new id.
        agent
            .load_session(forked.id.clone(), cwd(), vec![])
            .await
            .unwrap();
        let replay: Vec<_> = std::iter::from_fn(|| rx.try_recv().ok()).collect();
        assert_eq!(
            replay,
            vec![
                AgentEvent::Update {
                    session: forked.id.clone(),
                    event: SessionEvent::UserText("previous prompt".into())
                },
                AgentEvent::Update {
                    session: forked.id,
                    event: SessionEvent::AgentText("previous answer".into())
                },
            ]
        );

        let missing = agent.fork_session("missing".into(), cwd(), vec![]).await;
        assert!(
            matches!(missing, Err(AgentError::Protocol(_))),
            "{missing:?}"
        );
    });
}

/// R46: the MCP servers given to `new_session`, `load_session` and `fork_session` reach the
/// agent (`--echo-mcp` answers each turn with the session's servers).
#[test]
fn mcp_servers_reach_the_agent() {
    run(async {
        let (agent, rx) = spawn(&["--echo-mcp"]);
        ready(&rx).await;
        let server = |name: &str| McpServer {
            name: name.into(),
            command: format!("/opt/{name}-mcp").into(),
            args: vec!["--stdio".into()],
            env: vec![("MODE".into(), "test".into())],
        };
        let turn = |session: String| {
            agent.prompt(session, hello());
            let rx = rx.clone();
            async move {
                let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
                agent_text(&events)
            }
        };

        let both = vec![server("jira"), server("docs")];
        let session = agent.new_session(cwd(), both).await.unwrap();
        assert_eq!(
            turn(session.id).await,
            "mcp:jira=/opt/jira-mcp,docs=/opt/docs-mcp"
        );

        agent
            .load_session("abc".into(), cwd(), vec![server("jira")])
            .await
            .unwrap();
        while rx.try_recv().is_ok() {} // the replay
        assert_eq!(turn("abc".into()).await, "mcp:jira=/opt/jira-mcp");

        let forked = agent.fork_session("abc".into(), cwd(), vec![]).await;
        assert_eq!(turn(forked.unwrap().id).await, "mcp:");
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

        agent.shutdown().await;
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

#[test]
fn config_options_come_with_the_session() {
    run(async {
        let (agent, rx) = spawn(&["--models"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        assert_eq!(session.config.len(), 2);
        let value = |id: &str, name: &str| ConfigValue {
            id: id.into(),
            name: name.into(),
        };
        assert_eq!(
            session.config[0],
            ConfigOption {
                id: "model".into(),
                name: "Model".into(),
                category: ConfigCategory::Model,
                current: "fake-small".into(),
                values: vec![
                    value("fake-small", "Fake Small"),
                    value("fake-large", "Fake Large")
                ],
            }
        );
        assert_eq!(session.config[1].category, ConfigCategory::ThoughtLevel);
        assert_eq!(session.config[1].current, "low");
    });
}

#[test]
fn set_config_switches_the_model() {
    run(async {
        let (agent, rx) = spawn(&["--models"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        let options = agent
            .set_config(session.id.clone(), "model".into(), "fake-large".into())
            .await
            .unwrap();
        assert_eq!(options[0].current, "fake-large");

        agent.prompt(session.id, hello());
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert_eq!(agent_text(&events), "model:fake-large");
    });
}

/// A subagent's traffic (`--subagent`) comes out as the parent's: its announcement and states,
/// its tool call tagged with it, its reply; not its text or its plan.
#[test]
fn subagent_traffic_reaches_the_parent() {
    run(async {
        let (agent, rx) = spawn(&["--subagent"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;

        let sub = format!("{}-sub", session.id);
        let updates: Vec<&SessionEvent> = events
            .iter()
            .filter_map(|e| match e {
                AgentEvent::Update { session: s, event } => {
                    assert_eq!(*s, session.id, "{event:?}");
                    Some(event)
                }
                _ => None,
            })
            .collect();
        let patch = |state| {
            SessionEvent::Subagent(SubagentPatch {
                id: sub.clone(),
                title: Some("Explore the code".into()),
                state: Some(state),
            })
        };
        let position = |want: &SessionEvent| updates.iter().position(|e| *e == want);
        let running = position(&patch(SubagentState::Running)).expect("running");
        let done = updates
            .iter()
            .position(|e| {
                matches!(e, SessionEvent::Subagent(p) if p.id == sub && p.state == Some(SubagentState::Done))
            })
            .expect("done");
        assert!(running < done);
        assert!(
            updates.iter().any(|e| matches!(e, SessionEvent::ToolCall(v)
                if v.id == "sub-read" && v.subagent.as_deref() == Some(sub.as_str()))),
            "{updates:?}"
        );
        assert!(updates.contains(&&SessionEvent::SubagentReply {
            id: sub.clone(),
            text: "found it".into(),
        }));
        assert!(!updates.iter().any(|e| matches!(e, SessionEvent::Plan(_))));
        assert_eq!(agent_text(&events), "Hello from fake agent");
    });
}

/// With `--subagent`, the permission is asked by the subagent: the card shows in the parent
/// session, tagged with the subagent, and the answer reaches the agent.
#[test]
fn subagent_permission_reaches_the_parent() {
    run(async {
        let (agent, rx) = spawn(&["--subagent", "--ask-permission"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        let request = permission(&rx).await;
        assert_eq!(request.session, session.id);
        assert_eq!(request.subagent, Some(format!("{}-sub", session.id)));
        assert_eq!(
            agent.answer_permission(request.id, Some("allow-once".into())),
            Ok(())
        );
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert!(
            agent_text(&events).contains("chose:allow-once"),
            "{events:?}"
        );
    });
}

#[test]
fn subagent_question_reaches_the_parent() {
    run(async {
        let (agent, rx) = spawn(&["--subagent", "--ask-question"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        let q = question(&rx).await;
        assert_eq!(q.session, session.id);
        assert_eq!(q.subagent, Some(format!("{}-sub", session.id)));
        assert_eq!(agent.answer_question(q.id, QuestionAnswer::Decline), Ok(()));
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert!(
            agent_text(&events).contains("answered:declined"),
            "{events:?}"
        );
    });
}

fn subagent_state(e: &AgentEvent, want: SubagentState) -> bool {
    matches!(
        e,
        AgentEvent::Update {
            event: SessionEvent::Subagent(SubagentPatch { state: Some(s), .. }),
            ..
        } if *s == want
    )
}

/// Review focus 2: Stop cancels only the session's own requests; a background subagent's card
/// stays open and still answers.
#[test]
fn stop_leaves_a_background_subagents_request_open() {
    run(async {
        let reply = vec!["word"; 200].join(" ");
        let (agent, rx) = spawn(&[
            "--subagent",
            "--background",
            "--ask-permission",
            "--reply",
            &reply,
            "--chunk-delay-ms",
            "20",
        ]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let request = permission(&rx).await;

        agent.cancel(session.id.clone());
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert_eq!(
            events.last(),
            Some(&AgentEvent::TurnEnded {
                session: session.id,
                reason: StopReason::Cancelled
            })
        );
        assert!(
            !events.contains(&AgentEvent::PermissionResolved { id: request.id }),
            "{events:?}"
        );
        assert_eq!(
            agent.answer_permission(request.id, Some("allow-once".into())),
            Ok(())
        );
        until(&rx, |e| subagent_state(e, SubagentState::Done)).await;
    });
}

/// Review focus 2: the agent withdraws a permission request (`$/cancel_request`), as it does
/// for a foreground subagent it stops; the card closes on its own.
#[test]
fn withdrawn_permission_resolves_on_its_own() {
    run(async {
        let (agent, rx) = spawn(&["--ask-permission", "--withdraw-permission"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());
        let withdrawn = permission(&rx).await;

        let (mut resolved, mut next) = (false, None);
        until(&rx, |e| {
            match e {
                AgentEvent::PermissionResolved { id } if *id == withdrawn.id => resolved = true,
                AgentEvent::PermissionRequested(p) => next = Some(p.id),
                _ => {}
            }
            resolved && next.is_some()
        })
        .await;
        assert_eq!(
            agent.answer_permission(withdrawn.id, Some("allow-once".into())),
            Err(BrokerError::Stale)
        );
        assert_eq!(
            agent.answer_permission(next.unwrap(), Some("allow-once".into())),
            Ok(())
        );
        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert!(
            agent_text(&events).contains("chose:allow-once"),
            "{events:?}"
        );
    });
}

#[test]
fn background_subagent_ends_after_the_turn() {
    run(async {
        let (agent, rx) = spawn(&["--subagent", "--background"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        assert_eq!(
            events.last(),
            Some(&AgentEvent::TurnEnded {
                session: session.id,
                reason: StopReason::EndTurn
            })
        );
        assert!(
            events
                .iter()
                .any(|e| subagent_state(e, SubagentState::Running))
        );
        assert!(
            !events
                .iter()
                .any(|e| subagent_state(e, SubagentState::Done))
        );
        until(&rx, |e| subagent_state(e, SubagentState::Done)).await;
    });
}

/// Review focus 1: a background subagent's permission outlives the turn, and its answer still
/// reaches the agent.
#[test]
fn background_permission_outlives_the_turn() {
    run(async {
        let (agent, rx) = spawn(&["--subagent", "--background", "--ask-permission"]);
        ready(&rx).await;
        let session = agent.new_session(cwd(), vec![]).await.unwrap();
        agent.prompt(session.id.clone(), hello());

        let events = until(&rx, |e| matches!(e, AgentEvent::TurnEnded { .. })).await;
        let request = events
            .iter()
            .find_map(|e| match e {
                AgentEvent::PermissionRequested(p) => Some(p.clone()),
                _ => None,
            })
            .expect("the permission before the turn ended");
        assert!(request.subagent.is_some());
        assert!(
            events
                .iter()
                .any(|e| subagent_state(e, SubagentState::NeedsYou))
        );
        assert!(
            !events.contains(&AgentEvent::PermissionResolved { id: request.id }),
            "{events:?}"
        );
        assert_eq!(
            agent.answer_permission(request.id, Some("allow-once".into())),
            Ok(())
        );
        until(&rx, |e| subagent_state(e, SubagentState::Done)).await;
    });
}

#[test]
fn program_parses_like_the_spawn() {
    if cfg!(unix) {
        assert_eq!(program("'/bin/sh' -c x"), Some("/bin/sh".into()));
    }
    assert_eq!(program("no-such-program-xyz --acp"), None);
    assert_eq!(program(""), None);
}
