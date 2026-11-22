//! Claude Code：`claude -p --output-format stream-json --verbose`，提示词走 stdin。
use super::*;
use serde_json::Value;

pub struct ClaudeAdapter;

impl AgentAdapter for ClaudeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::ClaudeCode
    }
    fn default_binary(&self) -> &'static str {
        "claude"
    }

    fn build_command(&self, spec: &RunSpec) -> Result<CommandSpec> {
        let mut args: Vec<String> = ["-p", "--output-format", "stream-json", "--verbose"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        if let Some(m) = &spec.model {
            args.extend(["--model".into(), m.clone()]);
        }
        let mode = match spec.permission {
            PermissionMode::ReadOnly => "plan",
            PermissionMode::WorkspaceWrite => "acceptEdits",
            PermissionMode::FullAuto => "bypassPermissions",
        };
        args.extend(["--permission-mode".into(), mode.into()]);
        let mut allowed = if spec.permission == PermissionMode::WorkspaceWrite {
            spec.allowed_tools.clone()
        } else {
            Vec::new()
        };
        // 无人值守时没放行的 MCP 工具会被拒；观澜自己的服务整个放行
        if spec.mcp.is_some() && spec.permission != PermissionMode::FullAuto {
            allowed.push(format!("mcp__{}", super::MCP_SERVER_NAME));
        }
        if !allowed.is_empty() {
            args.extend(["--allowedTools".into(), allowed.join(",")]);
        }
        if spec.no_tools {
            // 内置工具一个不给；用户自己配的 MCP 服务也不加载
            args.extend([
                "--tools".into(),
                String::new(),
                "--strict-mcp-config".into(),
            ]);
        }
        if let Some(id) = &spec.resume_session {
            args.extend(["--resume".into(), id.clone()]);
        }
        // 图片按 Prompt 里的路径读取；它们在 worktree 之外，要把所在目录放行
        let mut dirs: Vec<String> = spec
            .images
            .iter()
            .filter_map(|f| f.parent())
            .map(|d| d.display().to_string())
            .collect();
        dirs.dedup();
        for d in dirs {
            args.extend(["--add-dir".into(), d]);
        }
        if let Some(mcp) = &spec.mcp {
            // 令牌用 ${VAR} 引用环境变量，不出现在命令行里
            let config = serde_json::json!({ "mcpServers": { super::MCP_SERVER_NAME: {
                "type": "http",
                "url": mcp.url,
                "headers": { "Authorization": format!("Bearer ${{{}}}", mcp.token_env) },
            }}});
            args.extend(["--mcp-config".into(), config.to_string()]);
        }
        args.extend(spec.extra_args.iter().cloned());
        Ok(CommandSpec {
            program: self.resolve_binary(spec)?,
            args,
            env: spec.env.clone(),
            cwd: spec.cwd.clone(),
            stdin: spec.prompt.clone(),
        })
    }

    fn parser(&self) -> Box<dyn EventParser> {
        Box::new(ClaudeParser::default())
    }
}

#[derive(Default)]
pub struct ClaudeParser {
    session_seen: bool,
    /// 已经报过用量的那条消息：同一条消息的每个内容块各占一行，用量是重复的。
    usage_reported: Option<String>,
}

fn usage_of(v: &Value) -> Option<Usage> {
    let u = v.get("usage")?;
    Some(Usage {
        input_tokens: u64_of(u, "input_tokens") + u64_of(u, "cache_creation_input_tokens"),
        output_tokens: u64_of(u, "output_tokens"),
        cached_tokens: u64_of(u, "cache_read_input_tokens"),
        cost_usd: v.get("total_cost_usd").and_then(|c| c.as_f64()),
    })
}

fn tool_result_text(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(|i| i.get("text").and_then(|t| t.as_str()))
            .collect::<Vec<_>>()
            .join("\n"),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

impl EventParser for ClaudeParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let line = line.trim();
        if line.is_empty() {
            return vec![];
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return vec![AgentEvent::Raw {
                line: line.to_string(),
            }];
        };
        let mut out = vec![];
        match v.get("type").and_then(|t| t.as_str()).unwrap_or_default() {
            "system" => match v.get("subtype").and_then(|s| s.as_str()) {
                Some("init") if !self.session_seen => {
                    self.session_seen = true;
                    out.push(AgentEvent::SessionStarted {
                        session_id: str_of(&v, "session_id"),
                        model: v.get("model").and_then(|m| m.as_str()).map(String::from),
                    });
                }
                // 上游接口出错、CLI 在自动重试：不提示的话界面上只会看到长时间没动静
                Some("api_retry") => {
                    let num = |k: &str| v.get(k).and_then(|n| n.as_u64()).unwrap_or_default();
                    out.push(AgentEvent::Error {
                        message: format!(
                            "接口请求失败（{} {}），{} 秒后第 {}/{} 次重试",
                            num("error_status"),
                            str_of(&v, "error"),
                            num("retry_delay_ms").div_ceil(1000),
                            num("attempt"),
                            num("max_retries"),
                        ),
                    });
                }
                _ => {}
            },
            "assistant" => {
                // 每条消息自带这一次请求的用量：边跑边报，预算护栏才能在运行中途生效。
                // 输出 Token 在这里只是开头的一小部分，准确的总数以最后的 result 为准。
                let msg = v.get("message").unwrap_or(&Value::Null);
                let id = msg.get("id").and_then(|i| i.as_str());
                if id.is_some() && id != self.usage_reported.as_deref() {
                    self.usage_reported = id.map(String::from);
                    if let Some(usage) = usage_of(msg) {
                        out.push(AgentEvent::Usage { usage });
                    }
                }
                let blocks = v
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                for b in blocks {
                    match b.get("type").and_then(|t| t.as_str()).unwrap_or_default() {
                        "text" => out.push(AgentEvent::Text {
                            text: str_of(&b, "text"),
                        }),
                        "thinking" => out.push(AgentEvent::Thinking {
                            text: str_of(&b, "thinking"),
                        }),
                        "tool_use" => {
                            let name = str_of(&b, "name");
                            let input = b.get("input").cloned().unwrap_or(Value::Null);
                            if matches!(
                                name.as_str(),
                                "Edit" | "Write" | "MultiEdit" | "NotebookEdit"
                            ) {
                                if let Some(p) = input.get("file_path").and_then(|p| p.as_str()) {
                                    let kind = if name == "Write" { "write" } else { "edit" };
                                    out.push(AgentEvent::FileChange {
                                        path: p.to_string(),
                                        kind: kind.into(),
                                    });
                                }
                            }
                            out.push(AgentEvent::ToolCall {
                                id: str_of(&b, "id"),
                                name,
                                input,
                            });
                        }
                        _ => {}
                    }
                }
            }
            "user" => {
                let blocks = v
                    .pointer("/message/content")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default();
                for b in blocks {
                    if b.get("type").and_then(|t| t.as_str()) == Some("tool_result") {
                        out.push(AgentEvent::ToolResult {
                            id: str_of(&b, "tool_use_id"),
                            output: tool_result_text(b.get("content").unwrap_or(&Value::Null)),
                            is_error: b.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false),
                        });
                    }
                }
            }
            "result" => {
                if !self.session_seen {
                    let sid = str_of(&v, "session_id");
                    if !sid.is_empty() {
                        self.session_seen = true;
                        out.push(AgentEvent::SessionStarted {
                            session_id: sid,
                            model: None,
                        });
                    }
                }
                let is_error = v.get("is_error").and_then(|e| e.as_bool()).unwrap_or(false)
                    || v.get("subtype")
                        .and_then(|s| s.as_str())
                        .is_some_and(|s| s != "success");
                out.push(AgentEvent::Finished {
                    ok: !is_error,
                    summary: str_of(&v, "result"),
                    usage: usage_of(&v),
                    num_turns: v.get("num_turns").and_then(|n| n.as_u64()),
                    duration_ms: v.get("duration_ms").and_then(|n| n.as_u64()),
                });
            }
            _ => out.push(AgentEvent::Raw {
                line: line.to_string(),
            }),
        }
        out
    }
}
