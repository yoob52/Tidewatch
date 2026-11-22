//! 事件解析回放：样本说明见 fixtures/README.md。
use tidewatch_core::agent::{adapter, AgentEvent, AgentKind, PermissionMode, RunSpec};

fn replay(kind: AgentKind, fixture: &str) -> Vec<AgentEvent> {
    let text = std::fs::read_to_string(format!(
        "{}/tests/fixtures/{fixture}",
        env!("CARGO_MANIFEST_DIR")
    ))
    .unwrap();
    let mut parser = adapter(kind).parser();
    let mut ev: Vec<AgentEvent> = text.lines().flat_map(|l| parser.parse_line(l)).collect();
    ev.extend(parser.finish());
    ev
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

    // 运行中途就有用量（每条消息报一次，不重复计），预算护栏靠它才能在结束前拦住
    let mut running = tidewatch_core::agent::Usage::default();
    let mut reports = 0;
    for e in &ev {
        if let AgentEvent::Usage { usage } = e {
            running.add(usage);
            reports += 1;
        }
    }
    assert_eq!(reports, 4);
    assert_eq!(running.input_tokens, usage.input_tokens);
    assert_eq!(running.cached_tokens, usage.cached_tokens);
    assert!(running.cost_usd.is_none());
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
fn opencode_real_edit_stream() {
    let ev = replay(AgentKind::OpenCode, "opencode_edit.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { session_id, .. } if session_id == "ses_dfcfe32b5ffeMjdNqSS8saqeUr")
    );
    // 会话 id 每行都有，只报一次
    let starts = ev
        .iter()
        .filter(|e| matches!(e, AgentEvent::SessionStarted { .. }))
        .count();
    assert_eq!(starts, 1);
    assert!(ev.contains(&AgentEvent::FileChange {
        path: "/repo/b.txt".into(),
        kind: "add".into()
    }));
    assert!(ev.iter().any(|e| matches!(e, AgentEvent::Command { command, exit_code: Some(1), done: true, .. } if command.starts_with("git diff"))));
    // 中间几步以 tool-calls 结束，不算完成；只有最后一步算
    let done: Vec<_> = ev
        .iter()
        .filter_map(|e| match e {
            AgentEvent::Finished {
                ok,
                summary,
                usage,
                num_turns,
                ..
            } => Some((*ok, summary.clone(), usage.clone().unwrap(), *num_turns)),
            _ => None,
        })
        .collect();
    assert_eq!(done.len(), 1);
    let (ok, summary, usage, turns) = &done[0];
    assert!(ok);
    assert_eq!(summary, "done");
    assert_eq!(turns, &Some(5));
    assert_eq!(
        (usage.input_tokens, usage.output_tokens, usage.cached_tokens),
        (41712, 303, 10752)
    );
    assert_eq!(usage.cost_usd, None);
}

#[test]
fn opencode_real_resume_and_error() {
    let ev = replay(AgentKind::OpenCode, "opencode_resume_readonly.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { session_id, .. } if session_id == "ses_dfcfe32b5ffeMjdNqSS8saqeUr")
    );
    assert!(
        matches!(ev.last(), Some(AgentEvent::Finished { ok: true, summary, .. }) if summary.contains("b.txt"))
    );
    assert!(!ev
        .iter()
        .any(|e| matches!(e, AgentEvent::FileChange { .. })));

    let ev = replay(AgentKind::OpenCode, "opencode_error.jsonl");
    assert!(ev.contains(&AgentEvent::Error {
        message: "Unauthorized: Invalid API key".into()
    }));
    assert!(matches!(
        ev.last(),
        Some(AgentEvent::Finished { ok: false, .. })
    ));
}

#[test]
fn gemini_real_bad_key() {
    let ev = replay(AgentKind::Gemini, "gemini_bad_key.jsonl");
    assert!(
        matches!(&ev[0], AgentEvent::SessionStarted { model, .. } if model.as_deref() == Some("auto"))
    );
    assert!(
        matches!(ev.last(), Some(AgentEvent::Finished { ok: false, summary, .. }) if summary.contains("API key not valid"))
    );
}

#[test]
fn gemini_documented_success_stream() {
    let ev = replay(AgentKind::Gemini, "gemini_success.jsonl");
    assert!(ev.contains(&AgentEvent::FileChange {
        path: "b.txt".into(),
        kind: "write_file".into()
    }));
    assert!(ev.contains(&AgentEvent::ToolResult {
        id: "t1".into(),
        output: "ok".into(),
        is_error: false
    }));
    let done = ev.last().unwrap();
    assert!(
        matches!(done, AgentEvent::Finished { ok: true, summary, usage: Some(u), duration_ms: Some(1200), .. }
            if summary == "写好了。" && u.input_tokens == 120 && u.output_tokens == 30 && u.cached_tokens == 20)
    );
}

#[test]
fn aider_real_edit_output() {
    let ev = replay(AgentKind::Aider, "aider_edit.txt");
    assert!(ev.contains(&AgentEvent::FileChange {
        path: "b.txt".into(),
        kind: "edit".into()
    }));
    let usage = tidewatch_core::agent::Usage {
        input_tokens: 5200,
        output_tokens: 33,
        cached_tokens: 0,
        cost_usd: Some(0.01),
    };
    assert!(ev.contains(&AgentEvent::Usage {
        usage: usage.clone()
    }));
    // 没有真的会话 id：占位 id 让引擎在续接时带上 --restore-chat-history
    assert!(ev.iter().any(
        |e| matches!(e, AgentEvent::SessionStarted { session_id, model: Some(m) }
            if session_id == "aider-chat-history" && m.starts_with("openai/"))
    ));
    // 启动说明不算回复
    assert!(
        matches!(ev.last(), Some(AgentEvent::Finished { ok: true, summary, usage: Some(u), num_turns: Some(1), .. })
            if summary.starts_with("Add `b.txt`") && summary.contains("world") && !summary.contains("Aider v") && u == &usage)
    );
}

#[test]
fn aider_real_bad_key_exits_zero_but_fails() {
    let ev = replay(AgentKind::Aider, "aider_bad_key.txt");
    assert!(ev
        .iter()
        .any(|e| matches!(e, AgentEvent::Error { message } if message.starts_with("litellm.AuthenticationError"))));
    assert!(matches!(
        ev.last(),
        Some(AgentEvent::Finished { ok: false, .. })
    ));
}

#[test]
fn new_agent_command_lines() {
    let dir = tempfile::tempdir().unwrap();
    let spec = |resume: Option<&str>, permission| RunSpec {
        prompt: "改一下".into(),
        cwd: dir.path().to_path_buf(),
        model: Some("m1".into()),
        permission,
        resume_session: resume.map(String::from),
        binary: Some("/bin/x".into()),
        images: vec!["/data/img/a.png".into()],
        ..Default::default()
    };
    let env_of = |c: &tidewatch_core::agent::CommandSpec, k: &str| {
        c.env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone())
    };

    // OpenCode：权限进内联配置；注入了 Provider 时模型带上 Provider 前缀
    let oc = adapter(AgentKind::OpenCode);
    let c = oc
        .build_command(&spec(Some("ses_1"), PermissionMode::ReadOnly))
        .unwrap();
    assert_eq!(
        c.args,
        [
            "run",
            "--format",
            "json",
            "-m",
            "m1",
            "-s",
            "ses_1",
            "-f",
            "/data/img/a.png"
        ]
    );
    assert_eq!(c.stdin, "改一下");
    let cfg: serde_json::Value =
        serde_json::from_str(&env_of(&c, "OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
    assert_eq!(cfg["permission"]["edit"], "deny");
    assert_eq!(cfg["permission"]["bash"], "deny");
    let mut s = spec(None, PermissionMode::FullAuto);
    s.env = vec![(
        "OPENCODE_CONFIG_CONTENT".into(),
        r#"{"provider":{"tidewatch":{"npm":"@ai-sdk/openai"}}}"#.into(),
    )];
    let c = oc.build_command(&s).unwrap();
    assert!(c.args.contains(&"tidewatch/m1".to_string()) && c.args.contains(&"--auto".to_string()));
    let cfg: serde_json::Value =
        serde_json::from_str(&env_of(&c, "OPENCODE_CONFIG_CONTENT").unwrap()).unwrap();
    assert_eq!(cfg["permission"]["edit"], "allow");
    assert_eq!(cfg["provider"]["tidewatch"]["npm"], "@ai-sdk/openai");
    assert_eq!(c.env.len(), 1);

    // Gemini CLI
    let c = adapter(AgentKind::Gemini)
        .build_command(&spec(Some("sid"), PermissionMode::WorkspaceWrite))
        .unwrap();
    assert_eq!(
        c.args,
        [
            "-o",
            "stream-json",
            "--approval-mode",
            "auto_edit",
            "--skip-trust",
            "-m",
            "m1",
            "-r",
            "sid",
            "-p",
            " ",
            "--include-directories",
            "/data/img"
        ]
    );
    assert!(c.stdin.starts_with("改一下") && c.stdin.contains("@/data/img/a.png"));

    // Aider：提示词落成文件；模型前缀来自 Provider，不传给子进程
    let aider = adapter(AgentKind::Aider);
    let mut s = spec(Some("x"), PermissionMode::ReadOnly);
    s.env = vec![
        ("OPENAI_API_KEY".into(), "k".into()),
        ("TIDEWATCH_AIDER_MODEL_PREFIX".into(), "openai/".into()),
    ];
    aider.prepare(&s).unwrap();
    assert!(
        std::fs::read_to_string(dir.path().join(".aider.tidewatch-prompt.md"))
            .unwrap()
            .ends_with("\n\n改一下")
    );
    let c = aider.build_command(&s).unwrap();
    assert!(c.args.windows(2).any(|w| w == ["--model", "openai/m1"]));
    assert!(c.args.contains(&"--dry-run".to_string()));
    assert!(c.args.contains(&"--restore-chat-history".to_string()));
    assert!(c
        .args
        .windows(2)
        .any(|w| w == ["--read", "/data/img/a.png"]));
    assert!(c.args.ends_with(&[
        "--message-file".to_string(),
        ".aider.tidewatch-prompt.md".to_string()
    ]));
    // 模型前缀只给适配器看；COLUMNS 防止 JSON 被按 80 列折行
    assert_eq!(
        c.env,
        [
            ("OPENAI_API_KEY".to_string(), "k".to_string()),
            ("COLUMNS".to_string(), "10000".to_string())
        ]
    );
    assert!(c.stdin.is_empty());
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

/// 只要文字回答的调用（智能创建的 AI 分析）：不给读文件、跑命令的工具。
#[test]
fn no_tools_takes_away_file_and_command_tools() {
    let spec = |no_tools| RunSpec {
        prompt: "p".into(),
        cwd: "/w".into(),
        permission: PermissionMode::ReadOnly,
        binary: Some("/bin/x".into()),
        no_tools,
        ..Default::default()
    };
    let pair = |args: &[String], a: &str, b: &str| args.windows(2).any(|w| w[0] == a && w[1] == b);

    let claude = adapter(AgentKind::ClaudeCode);
    let c = claude.build_command(&spec(true)).unwrap();
    assert!(pair(&c.args, "--tools", ""));
    assert!(c.args.contains(&"--strict-mcp-config".to_string()));
    let c = claude.build_command(&spec(false)).unwrap();
    assert!(!c.args.contains(&"--tools".to_string()));

    let codex = adapter(AgentKind::Codex);
    let c = codex.build_command(&spec(true)).unwrap();
    assert!(pair(&c.args, "--disable", "shell_tool"));
    assert!(pair(&c.args, "--disable", "unified_exec"));
    assert_eq!(c.args.last().unwrap(), "-");
    let c = codex.build_command(&spec(false)).unwrap();
    assert!(!c.args.contains(&"--disable".to_string()));

    let opencode = adapter(AgentKind::OpenCode);
    let config = |c: &tidewatch_core::agent::CommandSpec| -> serde_json::Value {
        let v = &c
            .env
            .iter()
            .find(|(k, _)| k == "OPENCODE_CONFIG_CONTENT")
            .unwrap()
            .1;
        serde_json::from_str(v).unwrap()
    };
    assert_eq!(
        config(&opencode.build_command(&spec(true)).unwrap())["permission"],
        "deny"
    );
    assert_eq!(
        config(&opencode.build_command(&spec(false)).unwrap())["permission"]["bash"],
        "deny"
    );
}

/// 观澜的 MCP 服务：令牌只以环境变量名出现，各家都预先放行它的工具。
#[test]
fn mcp_server_is_injected_without_the_token() {
    use tidewatch_core::agent::McpServer;
    let spec = |permission| RunSpec {
        prompt: "p".into(),
        cwd: "/w".into(),
        permission,
        binary: Some("/bin/x".into()),
        allowed_tools: vec!["Bash(npm test:*)".into()],
        mcp: Some(McpServer {
            url: "http://127.0.0.1:47600/mcp".into(),
            token_env: "TIDEWATCH_MCP_TOKEN".into(),
        }),
        ..Default::default()
    };
    let after = |args: &[String], flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .map(|i| args[i + 1].clone())
    };

    let claude = adapter(AgentKind::ClaudeCode);
    let c = claude
        .build_command(&spec(PermissionMode::WorkspaceWrite))
        .unwrap();
    assert_eq!(
        after(&c.args, "--allowedTools").unwrap(),
        "Bash(npm test:*),mcp__tidewatch"
    );
    let config: serde_json::Value =
        serde_json::from_str(&after(&c.args, "--mcp-config").unwrap()).unwrap();
    assert_eq!(
        config["mcpServers"]["tidewatch"]["headers"]["Authorization"],
        "Bearer ${TIDEWATCH_MCP_TOKEN}"
    );
    let c = claude
        .build_command(&spec(PermissionMode::ReadOnly))
        .unwrap();
    assert_eq!(after(&c.args, "--allowedTools").unwrap(), "mcp__tidewatch");
    let c = claude
        .build_command(&spec(PermissionMode::FullAuto))
        .unwrap();
    assert_eq!(after(&c.args, "--allowedTools"), None);

    let c = adapter(AgentKind::Codex)
        .build_command(&spec(PermissionMode::WorkspaceWrite))
        .unwrap();
    for want in [
        r#"mcp_servers.tidewatch.url="http://127.0.0.1:47600/mcp""#,
        r#"mcp_servers.tidewatch.bearer_token_env_var="TIDEWATCH_MCP_TOKEN""#,
        r#"mcp_servers.tidewatch.default_tools_approval_mode="approve""#,
    ] {
        assert!(c.args.iter().any(|a| a == want), "{want}: {:?}", c.args);
    }

    let c = adapter(AgentKind::OpenCode)
        .build_command(&spec(PermissionMode::WorkspaceWrite))
        .unwrap();
    let config: serde_json::Value = serde_json::from_str(
        &c.env
            .iter()
            .find(|(k, _)| k == "OPENCODE_CONFIG_CONTENT")
            .unwrap()
            .1,
    )
    .unwrap();
    assert_eq!(
        config["mcp"]["tidewatch"]["headers"]["Authorization"],
        "Bearer {env:TIDEWATCH_MCP_TOKEN}"
    );
}
