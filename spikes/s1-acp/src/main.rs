//! S1 spike (throwaway): drive claude-agent-acp over ACP v1 and record what works.
use agent_client_protocol::schema::ProtocolVersion;
use agent_client_protocol::v1::ConnectionTo;
use agent_client_protocol::v1::schema::*;
use agent_client_protocol::{AcpAgent, Agent};
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const CMD: &str = "npx -y @agentclientprotocol/claude-agent-acp@0.101.0";

fn log(t0: Instant, msg: impl AsRef<str>) {
    println!("[{:>6.2}s] {}", t0.elapsed().as_secs_f32(), msg.as_ref());
}

fn short(s: &str) -> String {
    let s = s.replace('\n', "⏎");
    if s.chars().count() > 140 {
        format!("{}…", s.chars().take(140).collect::<String>())
    } else {
        s
    }
}

async fn run(
    cwd: PathBuf,
    resume: Option<SessionId>,
    prompt: &str,
    t0: Instant,
) -> anyhow::Result<SessionId> {
    let agent = AcpAgent::from_str(CMD)?;
    let first_token: Arc<Mutex<Option<f32>>> = Arc::default();
    let ft = first_token.clone();
    let out: Arc<Mutex<Option<SessionId>>> = Arc::default();
    let out2 = out.clone();
    let prompt = prompt.to_string();
    agent_client_protocol::Client
        .v1()
        .on_receive_notification(
            async move |n: SessionNotification, _cx| {
                let line = match &n.update {
                    SessionUpdate::AgentMessageChunk(c) => {
                        let mut f = ft.lock().unwrap();
                        if f.is_none() { *f = Some(t0.elapsed().as_secs_f32()); }
                        match &c.content { ContentBlock::Text(t) => format!("agent_text {:?}", short(&t.text)), o => format!("agent_other {o:?}") }
                    }
                    SessionUpdate::AgentThoughtChunk(_) => "thought_chunk".into(),
                    SessionUpdate::UserMessageChunk(_) => "user_chunk (replay)".into(),
                    SessionUpdate::ToolCall(tc) => format!("tool_call id={} kind={:?} title={:?} status={:?} content={:?}", tc.tool_call_id, tc.kind, short(&tc.title), tc.status, tc.content.iter().map(|c| short(&format!("{c:?}"))).collect::<Vec<_>>()),
                    SessionUpdate::UsageUpdate(u) => format!("usage used={} size={} cost={:?} meta={:?}", u.used, u.size, u.cost.as_ref().map(|c| c.amount), u.meta),
                    SessionUpdate::ToolCallUpdate(u) => format!("tool_call_update id={} status={:?} content={:?}", u.tool_call_id, u.fields.status, u.fields.content.as_ref().map(|c| c.iter().map(|x| short(&format!("{x:?}"))).collect::<Vec<_>>())),
                    SessionUpdate::Plan(p) => format!("plan entries={}", p.entries.len()),
                    other => short(&format!("{other:?}")),
                };
                log(t0, format!("update: {line}"));
                Ok(())
            },
            agent_client_protocol::on_receive_notification!(),
        )
        .on_receive_request(
            async move |req: RequestPermissionRequest, responder, _cx| {
                log(t0, format!("PERMISSION REQUEST title={:?} options={:?}", req.tool_call.fields.title, req.options.iter().map(|o| (o.kind, o.name.clone())).collect::<Vec<_>>()));
                let id = req.options.iter().find(|o| o.kind == PermissionOptionKind::AllowOnce).or(req.options.first()).map(|o| o.option_id.clone());
                match id {
                    Some(id) => responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Selected(SelectedPermissionOutcome::new(id)))),
                    None => responder.respond(RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: ReadTextFileRequest, responder, _cx| {
                log(t0, format!("FS READ {:?}", req.path));
                match std::fs::read_to_string(&req.path) {
                    Ok(c) => responder.respond(ReadTextFileResponse::new(c)),
                    Err(e) => responder.respond_with_error(agent_client_protocol::Error::internal_error().data(e.to_string())),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .on_receive_request(
            async move |req: WriteTextFileRequest, responder, _cx| {
                log(t0, format!("FS WRITE {:?} ({} bytes)", req.path, req.content.len()));
                match std::fs::write(&req.path, &req.content) {
                    Ok(()) => responder.respond(WriteTextFileResponse::new()),
                    Err(e) => responder.respond_with_error(agent_client_protocol::Error::internal_error().data(e.to_string())),
                }
            },
            agent_client_protocol::on_receive_request!(),
        )
        .connect_with(agent, async move |cx: ConnectionTo<Agent>| {
            let caps = ClientCapabilities::new().fs(FileSystemCapabilities::new().read_text_file(true).write_text_file(true));
            let init = cx.send_request(InitializeRequest::new(ProtocolVersion::V1).client_capabilities(caps)).block_task().await?;
            log(t0, format!("initialized agent_info={:?}", init.agent_info));
            log(t0, format!("agent caps={:?}", init.agent_capabilities));
            log(t0, format!("auth_methods={:?}", init.auth_methods.iter().map(|m| format!("{m:?}")).collect::<Vec<_>>()));
            if std::env::var("S1_LIST").is_ok() {
                let t = Instant::now();
                let mut n = 0; let mut cursor = None; let mut pages = 0;
                loop {
                    let mut req = ListSessionsRequest::new();
                    if let Some(c) = cursor.take() { req = req.cursor(c); }
                    let r = cx.send_request(req).block_task().await?;
                    pages += 1; n += r.sessions.len();
                    if pages == 1 { for s in r.sessions.iter().take(3) { log(t0, format!("session {} cwd={} title={:?} updated={:?}", s.session_id, s.cwd.display(), s.title, s.updated_at)); } }
                    match r.next_cursor { Some(c) => cursor = Some(c), None => break }
                }
                log(t0, format!("list_sessions: {n} sessions, {pages} pages, {:?}", t.elapsed()));
                std::process::exit(0);
            }
            let sid = match resume {
                Some(sid) => {
                    cx.send_request(LoadSessionRequest::new(sid.clone(), cwd.clone())).block_task().await?;
                    log(t0, format!("load_session ok id={sid}"));
                    sid
                }
                None => {
                    let mut req = NewSessionRequest::new(cwd.clone());
                    if std::env::var("S1_PERM").is_ok() {
                        let meta: serde_json::Map<String, serde_json::Value> = serde_json::from_value(serde_json::json!({"claudeCode": {"options": {"settingSources": []}}}))?;
                        req = req.meta(meta);
                    }
                    let r = cx.send_request(req).block_task().await?;
                    log(t0, format!("new_session id={} modes={:?}", r.session_id, r.modes.as_ref().map(|m| m.available_modes.iter().map(|x| x.id.to_string()).collect::<Vec<_>>())));
                    log(t0, format!("current_mode={:?}", r.modes.as_ref().map(|m| m.current_mode_id.to_string())));
                    if std::env::var("S1_PERM").is_ok() {
                        cx.send_request(SetSessionModeRequest::new(r.session_id.clone(), "default")).block_task().await?;
                        log(t0, "set_mode default ok");
                    }
                    r.session_id
                }
            };
            log(t0, format!("prompt: {prompt}"));
            let r = cx.send_request(PromptRequest::new(sid.clone(), vec![ContentBlock::Text(TextContent::new(prompt.clone()))])).block_task().await?;
            log(t0, format!("prompt done stop_reason={:?}", r.stop_reason));
            *out2.lock().unwrap() = Some(sid);
            Ok(())
        })
        .await?;
    log(
        t0,
        format!("time to first token: {:?}s", first_token.lock().unwrap()),
    );
    Ok(out.lock().unwrap().clone().expect("session id"))
}

fn main() -> anyhow::Result<()> {
    let cwd = std::env::temp_dir().join(format!("wai-s1-{}", std::process::id()));
    std::fs::create_dir_all(&cwd)?;
    let t0 = Instant::now();
    println!("cwd={}", cwd.display());
    smol::block_on(async {
        if std::env::var("S1_PERM").is_ok() {
            run(cwd.clone(), None, "Use your Write tool (not the shell) to create notes.md containing the single line: hi. Then reply DONE.", t0).await?;
            println!("notes.md exists={}", cwd.join("notes.md").exists());
            return anyhow::Ok(());
        }
        let sid = run(
            cwd.clone(),
            None,
            "Create hello.txt containing the word wai, then reply with DONE.",
            t0,
        )
        .await?;
        println!(
            "hello.txt exists={} content={:?}",
            cwd.join("hello.txt").exists(),
            std::fs::read_to_string(cwd.join("hello.txt")).ok()
        );
        println!("--- second process: resume via load_session ---");
        run(
            cwd.clone(),
            Some(sid),
            "What is the name of the file you created? Reply with only the file name.",
            t0,
        )
        .await?;
        anyhow::Ok(())
    })
}
