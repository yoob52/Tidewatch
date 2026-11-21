//! 事件解析回放：样本说明见 fixtures/README.md。
use tidewatch_core::agent::{adapter, AgentEvent, AgentKind, PermissionMode, RunSpec};

fn replay(kind: AgentKind, fixture: &str) -> Vec<AgentEvent> {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut parser = adapter(kind).parser();
    text.lines().flat_map(|l| parser.parse_line(l)).collect()
}

#[test]
fn claude_success_stream() {
    let ev = replay(AgentKind::ClaudeCode, "claude_success.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { session_id, model }
        if session_id == "11111111-2222-3333-4444-555555555555" && model.as_deref() == Some("claude-sonnet-4-5"))
    );
    assert!(ev.contains(&AgentEvent::Thinking {
        text: "先看一下文件".into()
    }));
    assert!(ev.contains(&AgentEvent::FileChange {
        path: "/repo/README.md".into(),
        kind: "edit".into()
    }));
    assert!(ev.contains(&AgentEvent::ToolResult {
        id: "toolu_01".into(),
        output: "The file has been updated.".into(),
        is_error: false
    }));
    assert!(ev.contains(&AgentEvent::ToolResult {
        id: "toolu_02".into(),
        output: "1 failed".into(),
        is_error: true
    }));
    let done = ev.iter().find_map(|e| match e {
        AgentEvent::Finished {
            ok,
            summary,
            usage,
            num_turns,
            ..
        } => Some((ok, summary, usage.clone().unwrap(), num_turns)),
        _ => None,
    });
    let (ok, summary, usage, turns) = done.expect("finished");
    assert!(*ok);
    assert_eq!(summary, "已完成修改。");
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.cached_tokens),
        (1500, 210, 5000)
    );
    assert_eq!(usage.cost_usd, Some(0.0123));
    assert_eq!(*turns, Some(3));
    assert!(matches!(ev.last(), Some(AgentEvent::Raw { line }) if line == "not json at all"));
}

#[test]
fn claude_real_not_logged_in() {
    let ev = replay(AgentKind::ClaudeCode, "claude_not_logged_in.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { session_id, .. } if session_id.len() == 36)
    );
    assert!(ev
        .iter()
        .any(|e| matches!(e, AgentEvent::Text { text } if text.contains("Not logged in"))));
    assert!(ev
        .iter()
        .any(|e| matches!(e, AgentEvent::Finished { ok: false, .. })));
}

/// 真实录制：Claude Code 续接被中途停止的会话，改两个文件后成功结束。
#[test]
fn claude_real_resume_stream() {
    let ev = replay(AgentKind::ClaudeCode, "claude_real_resume.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { session_id, model }
        if session_id == "8d23d512-8c23-4572-8fc6-1f665311b6e6" && model.as_deref() == Some("claude-fable-5-2"))
    );
    for path in ["/repo/calc.py", "/repo/README.md"] {
        assert!(ev.contains(&AgentEvent::FileChange {
            path: path.into(),
            kind: "edit".into()
        }));
    }
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Thinking { .. })));
    let Some(AgentEvent::Finished {
        ok, summary, usage, ..
    }) = ev.last()
    else {
        panic!("最后一个事件应是 Finished：{:?}", ev.last());
    };
    assert!(*ok && summary.contains("sub(a, b)"));
    let usage = usage.as_ref().unwrap();
    assert!(usage.output_tokens == 2577 && usage.cost_usd.is_some_and(|c| c > 0.0));
}

/// 真实录制：上游接口 502，CLI 自动重试——要变成一条看得见的错误事件。
#[test]
fn claude_real_api_retry_is_surfaced() {
    let ev = replay(AgentKind::ClaudeCode, "claude_real_api_retry.jsonl");
    assert!(ev.iter().any(
        |e| matches!(e, AgentEvent::Error { message } if message.contains("502") && message.contains("1/10"))
    ));
}

/// 真实录制：Codex（gpt-6.1-sol，自定义 Provider）改两个文件后成功结束。
#[test]
fn codex_real_success_stream() {
    let ev = replay(AgentKind::Codex, "codex_real_success.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { session_id, .. } if session_id == "01a1fedb-2b8a-7940-beba-e3f69089f8e6")
    );
    for path in ["/repo/calc.py", "/repo/README.md"] {
        assert!(ev
            .iter()
            .any(|e| matches!(e, AgentEvent::FileChange { path: p, .. } if p == path)));
    }
    assert!(ev.iter().any(|e| matches!(
        e,
        AgentEvent::Command {
            done: true,
            exit_code: Some(0),
            ..
        }
    )));
    let Some(AgentEvent::Finished {
        ok, summary, usage, ..
    }) = ev.last()
    else {
        panic!("最后一个事件应是 Finished：{:?}", ev.last());
    };
    assert!(*ok && summary.contains("mul(a, b)"));
    assert_eq!(usage.as_ref().unwrap().output_tokens, 486);
}

#[test]
fn codex_success_stream() {
    let ev = replay(AgentKind::Codex, "codex_success.jsonl");
    assert_eq!(
        ev[0],
        AgentEvent::SessionStarted {
            session_id: "0199a213-81c0-7800-8aa1-bbab2a035a53".into(),
            model: None
        }
    );
    assert!(ev.contains(&AgentEvent::Command {
        command: "bash -lc ls".into(),
        output: "".into(),
        exit_code: None,
        done: false
    }));
    assert!(ev.contains(&AgentEvent::Command {
        command: "bash -lc ls".into(),
        output: "README.md\n".into(),
        exit_code: Some(0),
        done: true
    }));
    assert!(ev.contains(&AgentEvent::FileChange {
        path: "docs/a.md".into(),
        kind: "add".into()
    }));
    match ev.last().unwrap() {
        AgentEvent::Finished {
            ok, summary, usage, ..
        } => {
            assert!(*ok);
            assert_eq!(summary, "改好了。");
            let u = usage.clone().unwrap();
            assert_eq!(
                (u.input_tokens, u.cached_tokens, u.output_tokens),
                (315, 24448, 122)
            );
        }
        other => panic!("unexpected {other:?}"),
    }
}

#[test]
fn codex_real_unauthorized() {
    let ev = replay(AgentKind::Codex, "codex_unauthorized.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { session_id, .. } if !session_id.is_empty())
    );
    assert!(ev
        .iter()
        .any(|e| matches!(e, AgentEvent::Error { message } if message.contains("401"))));
    assert!(matches!(
        ev.last(),
        Some(AgentEvent::Finished { ok: false, .. })
    ));
}

#[test]
fn command_lines() {
    let spec = |resume: Option<&str>, permission| RunSpec {
        prompt: "改一下".into(),
        cwd: "/w".into(),
        model: Some("m1".into()),
        permission,
        resume_session: resume.map(String::from),
        binary: Some("/bin/x".into()),
        allowed_tools: vec!["Bash(npm test:*)".into()],
        ..Default::default()
    };
    let claude = adapter(AgentKind::ClaudeCode);
    let c = claude
        .build_command(&spec(None, PermissionMode::WorkspaceWrite))
        .unwrap();
    assert_eq!(
        c.args,
        [
            "-p",
            "--output-format",
            "stream-json",
            "--verbose",
            "--model",
            "m1",
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            "Bash(npm test:*)"
        ]
    );
    assert_eq!(c.stdin, "改一下");
    let c = claude
        .build_command(&spec(Some("sid"), PermissionMode::ReadOnly))
        .unwrap();
    assert!(c.args.ends_with(&[
        "--permission-mode".into(),
        "plan".into(),
        "--resume".into(),
        "sid".into()
    ]));

    let codex = adapter(AgentKind::Codex);
    let c = codex
        .build_command(&spec(None, PermissionMode::WorkspaceWrite))
        .unwrap();
    assert_eq!(
        c.args,
        [
            "exec",
            "--json",
            "--skip-git-repo-check",
            "-m",
            "m1",
            "-c",
            "sandbox_mode=workspace-write",
            "-"
        ]
    );
    let c = codex
        .build_command(&spec(Some("sid"), PermissionMode::ReadOnly))
        .unwrap();
    assert_eq!(
        c.args,
        [
            "exec",
            "resume",
            "--json",
            "--skip-git-repo-check",
            "-m",
            "m1",
            "-c",
            "sandbox_mode=read-only",
            "sid",
            "-"
        ]
    );
}

#[test]
fn display_masks_only_secrets() {
    let mut spec = RunSpec {
        binary: Some("/bin/claude".into()),
        ..Default::default()
    };
    spec.env = vec![
        (
            "ANTHROPIC_BASE_URL".into(),
            "https://api.deepseek.com/anthropic".into(),
        ),
        ("ANTHROPIC_AUTH_TOKEN".into(), "sk-secret-1234567890".into()),
    ];
    let shown = adapter(AgentKind::ClaudeCode)
        .build_command(&spec)
        .unwrap()
        .display();
    assert!(shown.contains("ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic"));
    assert!(shown.contains("ANTHROPIC_AUTH_TOKEN=sk-****7890"));
    assert!(!shown.contains("sk-secret"));
}
