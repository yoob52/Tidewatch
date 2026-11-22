//! OpenCode：`opencode run --format json`，提示词走 stdin。
//! Provider 和权限都通过 `OPENCODE_CONFIG_CONTENT` 这个环境变量传（一段 JSON 配置），不动用户自己的配置文件。
use super::*;
use serde_json::{json, Value};

pub struct OpenCodeAdapter;

/// OpenCode 读取内联配置的环境变量。
pub const CONFIG_ENV: &str = "OPENCODE_CONFIG_CONTENT";
/// 注入的 Provider 在 OpenCode 配置里的名字；模型写成 `tidewatch/<模型>`。
pub const PROVIDER_ID: &str = "tidewatch";

impl AgentAdapter for OpenCodeAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::OpenCode
    }
    fn default_binary(&self) -> &'static str {
        "opencode"
    }

    fn build_command(&self, spec: &RunSpec) -> Result<CommandSpec> {
        let mut args: Vec<String> = vec!["run".into(), "--format".into(), "json".into()];
        let injected = spec.env.iter().find(|(k, _)| k == CONFIG_ENV);
        if let Some(m) = &spec.model {
            let prefix = format!("{PROVIDER_ID}/");
            let m = match injected {
                Some(_) if !m.starts_with(&prefix) => format!("{prefix}{m}"),
                _ => m.clone(),
            };
            args.extend(["-m".into(), m]);
        }
        if let Some(id) = &spec.resume_session {
            args.extend(["-s".into(), id.clone()]);
        }
        for f in &spec.images {
            args.extend(["-f".into(), f.display().to_string()]);
        }
        if spec.permission == PermissionMode::FullAuto {
            args.push("--auto".into());
        }
        args.extend(spec.extra_args.iter().cloned());

        // 权限写进内联配置：只读时不给改文件和跑命令的工具
        let mut config = injected
            .and_then(|(_, v)| serde_json::from_str::<Value>(v).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        let rule = if spec.permission == PermissionMode::ReadOnly {
            "deny"
        } else {
            "allow"
        };
        config["permission"] = json!({ "edit": rule, "bash": rule });
        if let Some(mcp) = &spec.mcp {
            config["mcp"][super::MCP_SERVER_NAME] = json!({
                "type": "remote",
                "url": mcp.url,
                "enabled": true,
                "oauth": false,
                "headers": { "Authorization": format!("Bearer {{env:{}}}", mcp.token_env) },
            });
        }
        let mut env: Vec<(String, String)> = spec
            .env
            .iter()
            .filter(|(k, _)| k != CONFIG_ENV)
            .cloned()
            .collect();
        env.push((CONFIG_ENV.into(), config.to_string()));

        Ok(CommandSpec {
            program: self.resolve_binary(spec)?,
            args,
            env,
            cwd: spec.cwd.clone(),
            stdin: spec.prompt.clone(),
        })
    }

    fn parser(&self) -> Box<dyn EventParser> {
        Box::new(OpenCodeParser::default())
    }
}

#[derive(Default)]
pub struct OpenCodeParser {
    session: Option<String>,
    last_text: String,
    total: Usage,
    steps: u64,
}

impl EventParser for OpenCodeParser {
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
        let sid = str_of(&v, "sessionID");
        if !sid.is_empty() && self.session.as_deref() != Some(&sid) {
            self.session = Some(sid.clone());
            out.push(AgentEvent::SessionStarted {
                session_id: sid,
                model: None,
            });
        }
        let part = v.get("part").cloned().unwrap_or(Value::Null);
        match v.get("type").and_then(|t| t.as_str()).unwrap_or_default() {
            "step_start" => {}
            "text" => {
                self.last_text = str_of(&part, "text");
                out.push(AgentEvent::Text {
                    text: self.last_text.clone(),
                });
            }
            "reasoning" => out.push(AgentEvent::Thinking {
                text: str_of(&part, "text"),
            }),
            "tool_use" => {
                let tool = str_of(&part, "tool");
                let id = str_of(&part, "callID");
                let state = part.get("state").cloned().unwrap_or(Value::Null);
                let input = state.get("input").cloned().unwrap_or(Value::Null);
                let output = str_of(&state, "output");
                let failed = str_of(&state, "status") == "error";
                if tool == "bash" {
                    out.push(AgentEvent::Command {
                        command: str_of(&input, "command"),
                        output: if failed {
                            str_of(&state, "error")
                        } else {
                            output
                        },
                        exit_code: state
                            .get("metadata")
                            .and_then(|m| m.get("exit"))
                            .and_then(|c| c.as_i64()),
                        done: true,
                    });
                } else {
                    if !failed {
                        // apply_patch 一次可以改多个文件，列表在 metadata.files 里
                        let files = state
                            .get("metadata")
                            .and_then(|m| m.get("files"))
                            .and_then(|f| f.as_array());
                        match files {
                            Some(files) => {
                                for f in files {
                                    out.push(AgentEvent::FileChange {
                                        path: str_of(f, "filePath"),
                                        kind: str_of(f, "type"),
                                    });
                                }
                            }
                            None if matches!(tool.as_str(), "edit" | "write") => {
                                out.push(AgentEvent::FileChange {
                                    path: str_of(&input, "filePath"),
                                    kind: tool.clone(),
                                });
                            }
                            None => {}
                        }
                    }
                    out.push(AgentEvent::ToolCall {
                        id: id.clone(),
                        name: tool,
                        input,
                    });
                    out.push(AgentEvent::ToolResult {
                        id,
                        output: if failed {
                            str_of(&state, "error")
                        } else {
                            output
                        },
                        is_error: failed,
                    });
                }
            }
            "step_finish" => {
                let tokens = part.get("tokens").cloned().unwrap_or(Value::Null);
                let cache = tokens.get("cache").cloned().unwrap_or(Value::Null);
                let usage = Usage {
                    input_tokens: u64_of(&tokens, "input"),
                    output_tokens: u64_of(&tokens, "output") + u64_of(&tokens, "reasoning"),
                    cached_tokens: u64_of(&cache, "read"),
                    cost_usd: part
                        .get("cost")
                        .and_then(|c| c.as_f64())
                        .filter(|c| *c > 0.0),
                };
                self.total.add(&usage);
                self.steps += 1;
                out.push(AgentEvent::Usage { usage });
                // 只有不再调用工具的那一步才是结束
                if str_of(&part, "reason") != "tool-calls" {
                    out.push(AgentEvent::Finished {
                        ok: true,
                        summary: self.last_text.clone(),
                        usage: Some(self.total.clone()),
                        num_turns: Some(self.steps),
                        duration_ms: None,
                    });
                }
            }
            "error" => {
                let err = v.get("error").cloned().unwrap_or(Value::Null);
                let data = err.get("data").cloned().unwrap_or(Value::Null);
                let message = [str_of(&data, "message"), str_of(&err, "message")]
                    .into_iter()
                    .find(|m| !m.is_empty())
                    .unwrap_or_else(|| str_of(&err, "name"));
                out.push(AgentEvent::Error {
                    message: message.clone(),
                });
                out.push(AgentEvent::Finished {
                    ok: false,
                    summary: message,
                    usage: Some(self.total.clone()),
                    num_turns: Some(self.steps),
                    duration_ms: None,
                });
            }
            _ => out.push(AgentEvent::Raw {
                line: line.to_string(),
            }),
        }
        out
    }
}
