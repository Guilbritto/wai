//! Opt-in smoke test against the real Claude agent and the developer's own Claude Code login:
//! `cargo test -p wai-agent --test real_claude -- --ignored --nocapture`
//! Never run by plain `cargo test`; the prompts are tiny (subscription quota).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_channel::Receiver;
use wai_agent::{
    AgentCaps, AgentEvent, AgentHandle, AgentSpec, ChoiceKind, FieldKind, PromptBlock,
    QuestionAnswer, SessionEvent, StopReason, SubagentState, ToolContent,
};

const COMMAND: &str = "npx -y @agentclientprotocol/claude-agent-acp@0.103.0";

fn run(test: impl Future<Output = ()>) {
    smol::block_on(smol::future::or(test, async {
        smol::Timer::after(Duration::from_secs(240)).await;
        panic!("test timed out after 240 s");
    }))
}

fn spawn() -> (AgentHandle, Receiver<AgentEvent>) {
    AgentHandle::spawn(AgentSpec {
        name: "claude".into(),
        command: COMMAND.into(),
    })
}

async fn ready(rx: &Receiver<AgentEvent>) -> AgentCaps {
    loop {
        match rx.recv().await.expect("event channel closed") {
            AgentEvent::Ready {
                name,
                version,
                caps,
            } => {
                println!("ready: {name} {version}");
                return caps;
            }
            AgentEvent::Exited { error } => panic!("agent exited before ready: {error:?}"),
            _ => {}
        }
    }
}

#[test]
#[ignore = "needs network, npx and a Claude Code login"]
fn write_a_file_then_resume() {
    run(async {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("wai-real-claude-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = wai_settings::canonical(&dir);
        let t0 = Instant::now();

        let (agent, rx) = spawn();
        ready(&rx).await;
        println!("spawn -> ready: {:?}", t0.elapsed());

        let t = Instant::now();
        let session = agent
            .new_session(dir.clone(), vec![])
            .await
            .expect("new_session");
        println!("new_session: {:?} (id {})", t.elapsed(), session.id);
        agent
            .set_mode(session.id.clone(), "default".into())
            .await
            .expect("set_mode default");

        let prompt = "Create a file named hello.txt in the current directory containing just the word wai. Use your Write tool. Reply with one short sentence.";
        let t = Instant::now();
        agent.prompt(session.id.clone(), vec![PromptBlock::Text(prompt.into())]);

        let (mut text, mut diff, mut permission, mut first_text) =
            (String::new(), false, false, None);
        let reason = loop {
            match rx.recv().await.expect("event channel closed") {
                AgentEvent::Update { event, .. } => match event {
                    SessionEvent::AgentText(s) => {
                        first_text.get_or_insert(t.elapsed());
                        text.push_str(&s)
                    }
                    SessionEvent::ToolCall(call) => {
                        println!("tool call: {} ({})", call.title, call.kind);
                        diff |= call
                            .content
                            .iter()
                            .any(|c| matches!(c, ToolContent::Diff(_)));
                    }
                    SessionEvent::ToolCallUpdate(p) => {
                        diff |= p
                            .content
                            .iter()
                            .flatten()
                            .any(|c| matches!(c, ToolContent::Diff(_)));
                    }
                    _ => {}
                },
                AgentEvent::PermissionRequested(request) => {
                    permission = true;
                    diff |= request.diff.is_some();
                    let allow = request
                        .options
                        .iter()
                        .find(|o| o.kind == ChoiceKind::AllowOnce)
                        .expect("an allow-once option")
                        .id
                        .clone();
                    agent.answer_permission(request.id, Some(allow)).unwrap();
                }
                AgentEvent::TurnEnded { reason, .. } => break reason,
                AgentEvent::AuthRequired { .. } => panic!("not signed in to Claude Code"),
                AgentEvent::Exited { error } => panic!("agent exited mid-turn: {error:?}"),
                _ => {}
            }
        };
        println!(
            "turn: {:?} total, first text after {:?}; permission requested: {permission}; diff seen: {diff}; reply: {text:?}",
            t.elapsed(),
            first_text
        );
        assert_eq!(reason, StopReason::EndTurn);
        assert!(!text.is_empty(), "no AgentText arrived");
        assert!(diff, "no Diff arrived in a tool call or permission request");
        let written = std::fs::read_to_string(dir.join("hello.txt")).expect("hello.txt");
        assert!(written.contains("wai"), "{written:?}");

        agent.shutdown().await;

        let (agent, rx) = spawn();
        let caps = ready(&rx).await;
        let t = Instant::now();
        agent
            .load_session(session.id.clone(), dir.clone(), vec![])
            .await
            .expect("load_session");
        let mut replayed = false;
        let deadline = Instant::now() + Duration::from_secs(15);
        while !replayed && Instant::now() < deadline {
            let Ok(Ok(event)) = smol::future::or(async { Ok(rx.recv().await) }, async {
                smol::Timer::after(Duration::from_secs(1)).await;
                Err(())
            })
            .await
            else {
                continue;
            };
            replayed = matches!(
                event,
                AgentEvent::Update { event: SessionEvent::UserText(ref s), .. } if s.contains("hello.txt")
            );
        }
        println!(
            "load_session + replay: {:?}, user prompt replayed: {replayed}",
            t.elapsed()
        );
        assert!(replayed, "replay did not contain the user prompt");

        println!("fork advertised: {}", caps.fork);
        if caps.fork {
            let t = Instant::now();
            let forked = agent
                .fork_session(session.id.clone(), dir.clone(), vec![])
                .await
                .expect("fork_session");
            assert_ne!(forked.id, session.id);
            // claude-agent-acp answers a fork without replaying it: loading the copy does.
            agent
                .load_session(forked.id.clone(), dir.clone(), vec![])
                .await
                .expect("load_session of the fork");
            let mut replayed = false;
            let deadline = Instant::now() + Duration::from_secs(15);
            while !replayed && Instant::now() < deadline {
                let Ok(Ok(event)) = smol::future::or(async { Ok(rx.recv().await) }, async {
                    smol::Timer::after(Duration::from_secs(1)).await;
                    Err(())
                })
                .await
                else {
                    continue;
                };
                replayed = matches!(
                    event,
                    AgentEvent::Update { session: ref id, event: SessionEvent::UserText(ref s) }
                        if *id == forked.id && s.contains("hello.txt")
                );
            }
            println!("fork + load: {:?}, replayed: {replayed}", t.elapsed());
            assert!(replayed, "the fork did not carry the conversation");
        }

        let sessions = agent.list_sessions().await.expect("list_sessions");
        println!("list_sessions: {} sessions", sessions.len());
        assert!(sessions.iter().any(|s| s.id == session.id));

        agent.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
#[ignore = "needs network, npx and a Claude Code login"]
fn answer_a_question() {
    run(async {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("wai-real-claude-q-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = wai_settings::canonical(&dir);

        let (agent, rx) = spawn();
        ready(&rx).await;
        let session = agent
            .new_session(dir.clone(), vec![])
            .await
            .expect("new_session");

        let prompt = "Use the AskUserQuestion tool to ask me to pick red or blue, then reply with only the color I picked.";
        let t = Instant::now();
        agent.prompt(session.id.clone(), vec![PromptBlock::Text(prompt.into())]);

        let (mut text, mut chosen, mut asked) = (String::new(), None::<String>, false);
        let reason = loop {
            match rx.recv().await.expect("event channel closed") {
                AgentEvent::Update { event, .. } => match event {
                    SessionEvent::AgentText(s) => text.push_str(&s),
                    SessionEvent::ToolCall(call) => {
                        println!("tool call: {} ({})", call.title, call.kind)
                    }
                    _ => {}
                },
                AgentEvent::QuestionAsked(question) => {
                    asked = true;
                    println!(
                        "question after {:?}: {:?}, fields: {:?}",
                        t.elapsed(),
                        question.message,
                        question.fields
                    );
                    let mut values = serde_json::Map::new();
                    for field in &question.fields {
                        match &field.kind {
                            FieldKind::SingleChoice(choices) => {
                                let first = choices.first().expect("a choice");
                                chosen.get_or_insert(first.label.clone());
                                values.insert(field.key.clone(), first.value.clone().into());
                            }
                            FieldKind::MultiChoice(choices) => {
                                let first = choices.first().expect("a choice");
                                values.insert(field.key.clone(), vec![first.value.clone()].into());
                            }
                            FieldKind::Text => {
                                values.insert(field.key.clone(), "ok".into());
                            }
                            FieldKind::Boolean => {
                                values.insert(field.key.clone(), true.into());
                            }
                            FieldKind::Number => {
                                values.insert(field.key.clone(), 1.into());
                            }
                        }
                    }
                    agent
                        .answer_question(question.id, QuestionAnswer::Accept(values))
                        .unwrap();
                }
                AgentEvent::PermissionRequested(request) => {
                    println!("permission requested: {}", request.id);
                    let allow = request
                        .options
                        .iter()
                        .find(|o| o.kind == ChoiceKind::AllowOnce)
                        .map(|o| o.id.clone());
                    agent.answer_permission(request.id, allow).unwrap();
                }
                AgentEvent::TurnEnded { reason, .. } => break reason,
                AgentEvent::AuthRequired { .. } => panic!("not signed in to Claude Code"),
                AgentEvent::Exited { error } => panic!("agent exited mid-turn: {error:?}"),
                _ => {}
            }
        };
        println!(
            "turn: {:?}; question asked: {asked}; chose {chosen:?}; reply: {text:?}",
            t.elapsed()
        );
        assert_eq!(reason, StopReason::EndTurn);
        // The model may answer without asking; then there is nothing to check.
        if let Some(label) = chosen {
            assert!(
                text.to_lowercase().contains(&label.to_lowercase()),
                "reply {text:?} does not name {label:?}"
            );
        }

        agent.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    });
}

/// A Claude Code subagent (the Agent tool) comes as a child session: announced on the parent,
/// its tool calls tagged with it, and reported done.
#[test]
#[ignore = "needs network, npx and a Claude Code login"]
fn subagent_runs_as_a_child_session() {
    run(async {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("wai-real-claude-sub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = wai_settings::canonical(&dir);
        std::fs::write(dir.join("hello.txt"), "wai").unwrap();

        let (agent, rx) = spawn();
        ready(&rx).await;
        let session = agent
            .new_session(dir.clone(), vec![])
            .await
            .expect("new_session");

        let prompt = "Use the Agent tool exactly once, with subagent_type general-purpose, to read hello.txt and report its content. Then reply with that content only.";
        let t = Instant::now();
        agent.prompt(session.id.clone(), vec![PromptBlock::Text(prompt.into())]);

        let (mut text, mut title, mut running, mut finished, mut tagged) =
            (String::new(), None::<String>, None::<String>, false, 0);
        let reason = loop {
            match rx.recv().await.expect("event channel closed") {
                AgentEvent::Update { session: s, event } => {
                    assert_eq!(s, session.id, "{event:?}");
                    match event {
                        SessionEvent::AgentText(s) => text.push_str(&s),
                        SessionEvent::Subagent(patch) => {
                            println!("subagent after {:?}: {patch:?}", t.elapsed());
                            if let Some(t) = patch.title {
                                title = Some(t);
                            }
                            match patch.state {
                                Some(SubagentState::Running) => running = Some(patch.id),
                                Some(SubagentState::Done | SubagentState::Ended) => {
                                    finished |= running.as_deref() == Some(patch.id.as_str())
                                }
                                _ => {}
                            }
                        }
                        SessionEvent::SubagentReply { id, text } => {
                            println!("subagent {id} replied: {text:?}")
                        }
                        SessionEvent::ToolCall(call) => {
                            println!(
                                "tool call: {} ({}) subagent={:?}",
                                call.title, call.kind, call.subagent
                            );
                            tagged += usize::from(call.subagent.is_some());
                        }
                        _ => {}
                    }
                }
                AgentEvent::PermissionRequested(request) => {
                    println!("permission requested: {:?}", request.title);
                    let allow = request
                        .options
                        .iter()
                        .find(|o| o.kind == ChoiceKind::AllowOnce)
                        .map(|o| o.id.clone());
                    agent.answer_permission(request.id, allow).unwrap();
                }
                AgentEvent::TurnEnded { reason, .. } => break reason,
                AgentEvent::AuthRequired { .. } => panic!("not signed in to Claude Code"),
                AgentEvent::Exited { error } => panic!("agent exited mid-turn: {error:?}"),
                _ => {}
            }
        };
        println!(
            "turn: {:?}; subagent title: {title:?}; tagged tool calls: {tagged}; reply: {text:?}",
            t.elapsed()
        );
        assert_eq!(reason, StopReason::EndTurn);
        assert!(running.is_some(), "no subagent was announced running");
        assert!(finished, "the subagent was not reported done");
        assert!(tagged > 0, "no tool call was tagged with a subagent");

        agent.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    });
}

/// A message steered while a turn runs (`_session/steering`) joins that turn: Claude reads it at
/// its next step, and the turn's reply follows it.
#[test]
#[ignore = "needs network, npx and a Claude Code login"]
fn steer_a_running_turn() {
    run(async {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("wai-real-claude-steer-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = wai_settings::canonical(&dir);

        let (agent, rx) = spawn();
        let caps = ready(&rx).await;
        assert!(caps.steering, "steering not advertised");
        let session = agent
            .new_session(dir.clone(), vec![])
            .await
            .expect("new_session");

        let prompt =
            "Run the shell command `sleep 4` with your Bash tool, then reply with the word DONE.";
        let t = Instant::now();
        agent.prompt(session.id.clone(), vec![PromptBlock::Text(prompt.into())]);

        let (mut text, mut steered, mut echoed) = (String::new(), None, Vec::new());
        let reason = loop {
            match rx.recv().await.expect("event channel closed") {
                AgentEvent::Update { event, .. } => match event {
                    SessionEvent::AgentText(s) => text.push_str(&s),
                    SessionEvent::UserText(s) => echoed.push(s),
                    SessionEvent::ToolCall(call) => {
                        println!(
                            "tool call after {:?}: {} ({})",
                            t.elapsed(),
                            call.title,
                            call.kind
                        );
                        if call.kind == "execute" && steered.is_none() {
                            let more = "Also end your reply with the word STEERED";
                            let outcome = agent
                                .steer(session.id.clone(), vec![PromptBlock::Text(more.into())])
                                .await;
                            println!("steer after {:?}: {outcome:?}", t.elapsed());
                            steered = Some(outcome);
                        }
                    }
                    _ => {}
                },
                AgentEvent::PermissionRequested(request) => {
                    println!("permission requested: {:?}", request.title);
                    let allow = request
                        .options
                        .iter()
                        .find(|o| o.kind == ChoiceKind::AllowOnce)
                        .map(|o| o.id.clone());
                    agent.answer_permission(request.id, allow).unwrap();
                }
                AgentEvent::TurnEnded { reason, .. } => break reason,
                AgentEvent::AuthRequired { .. } => panic!("not signed in to Claude Code"),
                AgentEvent::Exited { error } => panic!("agent exited mid-turn: {error:?}"),
                _ => {}
            }
        };
        println!(
            "turn: {:?}; steer: {steered:?}; user text echoed: {echoed:?}; reply: {text:?}",
            t.elapsed()
        );
        assert_eq!(reason, StopReason::EndTurn);
        assert_eq!(steered, Some(Ok(true)), "no Bash call to steer during");
        assert!(
            text.contains("STEERED"),
            "reply {text:?} missed the steered message"
        );

        agent.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    });
}

#[test]
#[ignore = "needs network, npx and a Claude Code login"]
fn new_session_id_names_the_claude_code_transcript() {
    run(async {
        let dir = std::env::temp_dir().join(format!("wai-real-claude-id-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = wai_settings::canonical(&dir);
        let (agent, rx) = spawn();
        ready(&rx).await;
        let session = agent.new_session(dir, vec![]).await.expect("new_session");
        agent.prompt(
            session.id.clone(),
            vec![PromptBlock::Text("Reply with the word ok.".into())],
        );
        loop {
            match rx.recv().await.expect("event channel closed") {
                AgentEvent::TurnEnded { .. } => break,
                AgentEvent::AuthRequired { .. } => panic!("not signed in to Claude Code"),
                AgentEvent::Exited { error } => panic!("agent exited mid-turn: {error:?}"),
                _ => {}
            }
        }
        let projects = PathBuf::from(std::env::var("HOME").unwrap()).join(".claude/projects");
        let name = format!("{}.jsonl", session.id);
        let found = std::fs::read_dir(&projects)
            .unwrap()
            .flatten()
            .any(|p| p.path().join(&name).exists());
        agent.shutdown().await;
        assert!(found, "no {name} under {}", projects.display());
    })
}

/// In `default` mode an Edit, a Bash command and leaving plan mode each arrive as a permission
/// request, and the allow answer reaches the agent (the edit and the command take effect).
#[test]
#[ignore = "needs network, npx and a Claude Code login"]
fn permission_requests_in_default_mode() {
    run(async {
        let dir: PathBuf =
            std::env::temp_dir().join(format!("wai-real-claude-perm-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let dir = wai_settings::canonical(&dir);
        std::fs::write(dir.join("note.txt"), "before\n").unwrap();
        // The developer's user settings may allow Edit and Bash outright; `ask` outranks `allow`.
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(
            dir.join(".claude/settings.json"),
            r#"{"permissions": {"ask": ["Edit", "Write", "Bash"]}}"#,
        )
        .unwrap();

        let (agent, rx) = spawn();
        ready(&rx).await;
        let session = agent
            .new_session(dir.clone(), vec![])
            .await
            .expect("new_session");

        let steps = [
            (
                "default",
                "Use your Edit tool to replace the word before with after in note.txt. Reply with one short sentence.",
            ),
            (
                "default",
                "Run the shell command `touch ran.txt` with your Bash tool. Reply with one short sentence.",
            ),
            (
                "plan",
                "Plan a one-line change: add a line to note.txt. Then call ExitPlanMode with the plan. Reply with one short sentence afterwards.",
            ),
        ];
        let mut titles = Vec::new();
        for (mode, prompt) in steps {
            agent
                .set_mode(session.id.clone(), mode.into())
                .await
                .expect("set_mode");
            agent.prompt(session.id.clone(), vec![PromptBlock::Text(prompt.into())]);
            let mut asked = Vec::new();
            let reason = loop {
                match rx.recv().await.expect("event channel closed") {
                    AgentEvent::PermissionRequested(request) => {
                        println!(
                            "[{mode}] permission requested: {:?} diff={} options={:?}",
                            request.title,
                            request.diff.is_some(),
                            request.options
                        );
                        asked.push(request.title.clone());
                        let allow = request
                            .options
                            .iter()
                            .find(|o| o.kind == ChoiceKind::AllowOnce)
                            .expect("an allow-once option")
                            .id
                            .clone();
                        agent.answer_permission(request.id, Some(allow)).unwrap();
                    }
                    AgentEvent::TurnEnded { reason, .. } => break reason,
                    AgentEvent::AuthRequired { .. } => panic!("not signed in to Claude Code"),
                    AgentEvent::Exited { error } => panic!("agent exited mid-turn: {error:?}"),
                    _ => {}
                }
            };
            println!("[{mode}] turn ended {reason:?}, requests: {asked:?}");
            assert_eq!(reason, StopReason::EndTurn);
            assert!(
                !asked.is_empty(),
                "no permission request in step {prompt:?}"
            );
            titles.extend(asked);
        }
        println!("all permission titles: {titles:?}");

        assert!(titles.iter().any(|t| t == "Approve Plan"), "{titles:?}");
        let note = std::fs::read_to_string(dir.join("note.txt")).unwrap();
        assert!(
            note.contains("after"),
            "the allowed edit did not apply: {note:?}"
        );
        assert!(
            dir.join("ran.txt").exists(),
            "the allowed command did not run"
        );

        agent.shutdown().await;
        std::fs::remove_dir_all(&dir).ok();
    });
}
