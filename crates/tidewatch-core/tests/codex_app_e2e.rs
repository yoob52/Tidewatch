//! 真 Codex CLI + 假 Responses 服务的端到端检查：`TW_CODEX_E2E=<base_url> cargo test -- --ignored codex_app`。
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tidewatch_core::agent::{adapter, AgentEvent, AgentKind, RunSpec};
use tidewatch_core::runner::{run_agent, RunOptions, RunnerEvent, SteerInbox};

#[test]
#[ignore]
fn codex_app_server_takes_a_steer_mid_turn() {
    let Ok(base) = std::env::var("TW_CODEX_E2E") else { return };
    let dir = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let spec = RunSpec {
        prompt: "first SLOW".into(),
        cwd: dir.path().into(),
        interactive: true,
        env: vec![
            ("CODEX_HOME".into(), home.path().display().to_string()),
            ("FAKE_KEY".into(), "x".into()),
        ],
        extra_args: [
            "model_provider=\"fake\"".to_string(),
            "model_providers.fake.name=\"fake\"".into(),
            format!("model_providers.fake.base_url={base:?}"),
            "model_providers.fake.wire_api=\"responses\"".into(),
            "model_providers.fake.env_key=\"FAKE_KEY\"".into(),
        ]
        .into_iter()
        .flat_map(|c| ["-c".to_string(), c])
        .collect(),
        model: Some("fake-model".into()),
        ..Default::default()
    };
    let inbox = Arc::new(SteerInbox::default());
    let opts = RunOptions {
        raw_log: None,
        cancel: Arc::new(AtomicBool::new(false)),
        timeout: Some(std::time::Duration::from_secs(60)),
        abort: Arc::new(AtomicBool::new(false)),
        steer: Some(inbox.clone()),
    };
    let (mut texts, mut steered, mut finished) = (vec![], 0, 0);
    let out = run_agent(adapter(AgentKind::Codex).as_ref(), &spec, &opts, &mut |e| match e {
        RunnerEvent::Agent(AgentEvent::SessionStarted { .. }) => assert!(inbox.push("STEER now".into())),
        RunnerEvent::Agent(AgentEvent::Text { text }) => texts.push(text),
        RunnerEvent::Agent(AgentEvent::Finished { ok, .. }) => {
            assert!(ok);
            finished += 1
        }
        RunnerEvent::Steered(_) => steered += 1,
        _ => {}
    })
    .unwrap();
    assert_eq!(steered, 1);
    assert_eq!(finished, 1);
    assert!(texts.iter().any(|t| t.contains("STEER now")), "{texts:?}");
    assert_eq!(out.exit_code, Some(0));
}
