//! Own test binary: `enter_neutral_dir` changes the process-wide cwd, which would race
//! other tests in a shared binary.

use std::time::Duration;

use wai_agent::{AgentEvent, AgentHandle, AgentSpec, quote};

#[test]
fn agent_process_cwd_is_neutral() {
    let tmp = tempfile::tempdir().unwrap();
    let neutral = wai_settings::canonical(tmp.path()).join("config");
    let cwd_file = tmp.path().join("cwd.txt");

    wai_agent::enter_neutral_dir(&neutral).unwrap();
    let command = format!(
        "{} --cwd-file {}",
        quote(env!("CARGO_BIN_EXE_wai-fake-agent")),
        quote(&cwd_file)
    );
    let (_handle, events) = AgentHandle::spawn(AgentSpec {
        name: "fake".into(),
        command,
    });
    smol::block_on(smol::future::or(
        async { while !matches!(events.recv().await, Ok(AgentEvent::Ready { .. })) {} },
        async {
            smol::Timer::after(Duration::from_secs(10)).await;
            panic!("agent never became ready");
        },
    ));

    let written = std::fs::read_to_string(&cwd_file).unwrap();
    assert_eq!(wai_settings::canonical(written.as_ref()), neutral);
}
