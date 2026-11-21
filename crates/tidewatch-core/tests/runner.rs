//! 用假 CLI 脚本验证运行器：流式事件、中途停止（含孙进程）、超时。仅 Unix；Windows 上由 twctl 的演示 Agent 覆盖。
#![cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tidewatch_core::agent::{adapter, AgentEvent, AgentKind, RunSpec};
use tidewatch_core::process::pid_alive;
use tidewatch_core::runner::{run_agent, RunOptions, RunnerEvent};

fn script(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("fake-agent.sh");
    std::fs::write(&p, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn spec(dir: &Path, bin: PathBuf) -> RunSpec {
    RunSpec {
        prompt: "你好".into(),
        cwd: dir.into(),
        binary: Some(bin),
        env: vec![("TW_TEST_ENV".into(), "injected".into())],
        ..Default::default()
    }
}

#[test]
fn streams_events_and_writes_raw_log() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        r#"prompt=$(cat)
echo '{"type":"system","subtype":"init","session_id":"s-1","model":"m"}'
echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"$prompt $TW_TEST_ENV\"}]}}"
echo "warn" >&2
echo '{"type":"result","subtype":"success","is_error":false,"result":"ok","session_id":"s-1","total_cost_usd":0.5,"usage":{"input_tokens":10,"output_tokens":2}}'"#,
    );
    let log = dir.path().join("logs/run.jsonl");
    let opts = RunOptions {
        raw_log: Some(log.clone()),
        ..Default::default()
    };
    let mut events = vec![];
    let out = run_agent(
        adapter(AgentKind::ClaudeCode).as_ref(),
        &spec(dir.path(), bin),
        &opts,
        &mut |e| events.push(e),
    )
    .unwrap();
    assert!(out.succeeded());
    assert_eq!(out.session_id.as_deref(), Some("s-1"));
    assert_eq!(out.usage.input_tokens, 10);
    assert_eq!(out.usage.cost_usd, Some(0.5));
    assert_eq!(out.stderr_tail, "warn");
    assert!(matches!(events[0], RunnerEvent::Spawned { .. }));
    assert!(events.iter().any(
        |e| matches!(e, RunnerEvent::Agent(AgentEvent::Text { text }) if text == "你好 injected")
    ));
    assert_eq!(std::fs::read_to_string(log).unwrap().lines().count(), 3);
}

#[test]
fn cancel_kills_whole_tree() {
    let dir = tempfile::tempdir().unwrap();
    let bin = script(
        dir.path(),
        r#"echo '{"type":"system","subtype":"init","session_id":"s-2"}'
sleep 300 &
echo "{\"type\":\"assistant\",\"message\":{\"content\":[{\"type\":\"text\",\"text\":\"child $!\"}]}}"
wait"#,
    );
    let cancel = Arc::new(AtomicBool::new(false));
    let opts = RunOptions {
        cancel: cancel.clone(),
        ..Default::default()
    };
    let mut child_pid = 0u32;
    let started = Instant::now();
    let out = run_agent(
        adapter(AgentKind::ClaudeCode).as_ref(),
        &spec(dir.path(), bin),
        &opts,
        &mut |e| {
            if let RunnerEvent::Agent(AgentEvent::Text { text }) = e {
                child_pid = text.trim_start_matches("child ").parse().unwrap();
                assert!(pid_alive(child_pid));
                cancel.store(true, Ordering::SeqCst);
            }
        },
    )
    .unwrap();
    assert!(out.stopped && !out.succeeded());
    assert_eq!(
        out.session_id.as_deref(),
        Some("s-2"),
        "停止后仍保留会话 id 以便续接"
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    std::thread::sleep(Duration::from_millis(200));
    assert!(child_pid > 0 && !pid_alive(child_pid), "孙进程应一并结束");
}

#[test]
fn timeout_and_failure_exit() {
    let dir = tempfile::tempdir().unwrap();
    let slow = script(dir.path(), "sleep 300");
    let opts = RunOptions {
        timeout: Some(Duration::from_millis(300)),
        ..Default::default()
    };
    let out = run_agent(
        adapter(AgentKind::Codex).as_ref(),
        &spec(dir.path(), slow),
        &opts,
        &mut |_| {},
    )
    .unwrap();
    assert!(out.timed_out && !out.succeeded());

    let bad = script(dir.path(), "echo boom >&2; exit 3");
    let out = run_agent(
        adapter(AgentKind::Codex).as_ref(),
        &spec(dir.path(), bad),
        &RunOptions::default(),
        &mut |_| {},
    )
    .unwrap();
    assert_eq!(out.exit_code, Some(3));
    assert_eq!(out.stderr_tail, "boom");
    assert!(!out.succeeded());
}

#[test]
fn missing_binary_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let s = RunSpec {
        cwd: dir.path().into(),
        binary: Some(dir.path().join("nope")),
        ..Default::default()
    };
    assert!(run_agent(
        adapter(AgentKind::ClaudeCode).as_ref(),
        &s,
        &RunOptions::default(),
        &mut |_| {}
    )
    .is_err());
}
