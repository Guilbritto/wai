//! Opt-in smoke test against the real Claude agent and the developer's own Claude Code login:
//! `cargo test -p wai-agent --test real_claude -- --ignored --nocapture`
//! Never run by plain `cargo test`; the prompts are tiny (subscription quota).

use std::path::PathBuf;
use std::time::{Duration, Instant};

use async_channel::Receiver;
use wai_agent::{
    AgentCaps, AgentEvent, AgentHandle, AgentSpec, ChoiceKind, FieldKind, PromptBlock,
    QuestionAnswer, SessionEvent, StopReason, ToolContent,
};

const COMMAND: &str = "npx -y @agentclientprotocol/claude-agent-acp@0.101.0";

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
