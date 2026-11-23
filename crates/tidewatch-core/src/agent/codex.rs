//! Codex：`codex exec --json`，提示词走 stdin（`-`）。
//! 续接用 `codex exec resume <id>`，该子命令没有 `-s` / `-C`，所以沙箱统一用 `-c sandbox_mode=` 覆盖、目录用进程 cwd。
//! `-c` 的值不加引号：Codex 对无法按 TOML 解析的值按字面字符串处理，Windows 的 .cmd 包装下也没有转义问题。
use super::*;
use serde_json::Value;

pub struct CodexAdapter;

impl AgentAdapter for CodexAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Codex
    }
    fn default_binary(&self) -> &'static str {
        "codex"
    }

    fn build_command(&self, spec: &RunSpec) -> Result<CommandSpec> {
        let sandbox = match spec.permission {
            PermissionMode::ReadOnly => "read-only",
            PermissionMode::WorkspaceWrite => "workspace-write",
            PermissionMode::FullAuto => "danger-full-access",
        };
        let app = app_server(spec);
        let mut args: Vec<String> = vec![if app { "app-server" } else { "exec" }.into()];
        if app {
            push_config(spec, sandbox, &mut args);
            args.extend(spec.extra_args.iter().cloned());
            return Ok(CommandSpec {
                program: self.resolve_binary(spec)?,
                args,
                env: spec.env.clone(),
                cwd: spec.cwd.clone(),
                stdin: super::codex_app::handshake(spec),
                interactive: true,
            });
        }
        if spec.resume_session.is_some() {
            args.push("resume".into());
        }
        args.extend(["--json".into(), "--skip-git-repo-check".into()]);
        // `-i` 可以接多个值，所以每张图单独写一次，并且放在其它选项之前，免得把末尾的 `-` 吃掉
        for f in &spec.images {
            args.extend(["-i".into(), f.display().to_string()]);
        }
        if let Some(m) = &spec.model {
            args.extend(["-m".into(), m.clone()]);
        }
        push_config(spec, sandbox, &mut args);
        args.extend(spec.extra_args.iter().cloned());
        if let Some(id) = &spec.resume_session {
            args.push(id.clone());
        }
        args.push("-".into());
        Ok(CommandSpec {
            program: self.resolve_binary(spec)?,
            args,
            env: spec.env.clone(),
            cwd: spec.cwd.clone(),
            stdin: spec.prompt.clone(),
            interactive: false,
        })
    }

    fn parser(&self) -> Box<dyn EventParser> {
        Box::new(CodexParser::default())
    }

    fn parser_for(&self, spec: &RunSpec) -> Box<dyn EventParser> {
        if app_server(spec) {
            Box::new(super::codex_app::CodexAppParser::new(spec))
        } else {
            self.parser()
        }
    }

    /// 实际内容由 app-server 解析器生成（要带会话和这一轮的 id）。
    fn steer_message(&self, text: &str) -> Option<String> {
        Some(text.to_string())
    }
}

/// 子任务运行走 app-server，运行中可以追加指令；只要文字回答的调用仍用一次性的 `codex exec`。
fn app_server(spec: &RunSpec) -> bool {
    spec.interactive && !spec.no_tools
}

/// `exec` 和 `app-server` 共用的 `-c` 覆盖（沙箱、Windows 沙箱、工具开关、MCP、Provider 参数）。
fn push_config(spec: &RunSpec, sandbox: &str, args: &mut Vec<String>) {
    {
        args.extend(["-c".into(), format!("sandbox_mode={sandbox}")]);
        if cfg!(windows) && !user_sets_windows_sandbox(spec) {
            // 不指定时 Codex 用旧版受限令牌沙箱，在很多 Windows 上任何命令都被 policy 拒绝、工作区也写不了
            args.extend(["-c".into(), "windows.sandbox=unelevated".into()]);
        }
        if spec.no_tools {
            // 只读沙箱里仍然能 cat 任意文件；把跑命令的工具整个关掉
            for f in ["shell_tool", "unified_exec"] {
                args.extend(["--disable".into(), f.into()]);
            }
        }
        if let Some(mcp) = &spec.mcp {
            let name = super::MCP_SERVER_NAME;
            args.extend([
                "-c".into(),
                format!("mcp_servers.{name}.url={:?}", mcp.url),
                "-c".into(),
                format!(
                    "mcp_servers.{name}.bearer_token_env_var={:?}",
                    mcp.token_env
                ),
                // exec 的审批策略是 never，MCP 工具默认要审批，不预先放行就全部被拒
                "-c".into(),
                format!("mcp_servers.{name}.default_tools_approval_mode=\"approve\""),
            ]);
        }
    }
}

/// 用户在 Codex 的 config.toml 里自己选过 Windows 沙箱实现时，不用命令行覆盖它。
fn user_sets_windows_sandbox(spec: &RunSpec) -> bool {
    let home = spec
        .env
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("CODEX_HOME"))
        .map(|(_, v)| std::path::PathBuf::from(v))
        .or_else(|| std::env::var_os("CODEX_HOME").map(Into::into))
        .or_else(|| {
            std::env::var_os("USERPROFILE").map(|h| std::path::PathBuf::from(h).join(".codex"))
        });
    home.and_then(|h| std::fs::read_to_string(h.join("config.toml")).ok())
        .is_some_and(|t| sets_windows_sandbox(&t))
}

fn sets_windows_sandbox(toml: &str) -> bool {
    let mut table = String::new();
    toml.lines().map(str::trim).any(|l| {
        if let Some(h) = l.strip_prefix('[') {
            table = h.split(']').next().unwrap_or("").trim().to_string();
            return false;
        }
        let Some((key, _)) = l.split_once('=') else {
            return false;
        };
        let key = key.trim();
        (table.is_empty() && key == "windows.sandbox") || (table == "windows" && key == "sandbox")
    })
}

#[cfg(test)]
mod sandbox_tests {
    use super::sets_windows_sandbox;

    #[test]
    fn finds_user_windows_sandbox() {
        assert!(sets_windows_sandbox(
            "model = \"x\"
[windows]
sandbox = \"elevated\"
"
        ));
        assert!(sets_windows_sandbox("windows.sandbox=\"elevated\""));
        assert!(!sets_windows_sandbox(
            "[tools]
sandbox = 1
[windows]
# sandbox = \"elevated\"
"
        ));
        assert!(!sets_windows_sandbox(""));
        assert!(sets_windows_sandbox(
            "[windows] # 本机
sandbox = \"elevated\""
        ));
    }
}

#[derive(Default)]
pub struct CodexParser {
    last_message: String,
    total: Usage,
    turns: u64,
}

impl EventParser for CodexParser {
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
        let ty = v.get("type").and_then(|t| t.as_str()).unwrap_or_default();
        match ty {
            "thread.started" => out.push(AgentEvent::SessionStarted {
                session_id: str_of(&v, "thread_id"),
                model: None,
            }),
            "turn.started" => {}
            "item.started" | "item.updated" | "item.completed" => {
                let done = ty == "item.completed";
                let item = v.get("item").cloned().unwrap_or(Value::Null);
                let id = str_of(&item, "id");
                match item
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or_default()
                {
                    "agent_message" if done => {
                        self.last_message = str_of(&item, "text");
                        out.push(AgentEvent::Text {
                            text: self.last_message.clone(),
                        });
                    }
                    "reasoning" if done => out.push(AgentEvent::Thinking {
                        text: str_of(&item, "text"),
                    }),
                    "command_execution" if ty != "item.updated" => out.push(AgentEvent::Command {
                        command: str_of(&item, "command"),
                        output: str_of(&item, "aggregated_output"),
                        exit_code: item.get("exit_code").and_then(|c| c.as_i64()),
                        done,
                    }),
                    "file_change" if done => {
                        for c in item
                            .get("changes")
                            .and_then(|c| c.as_array())
                            .cloned()
                            .unwrap_or_default()
                        {
                            out.push(AgentEvent::FileChange {
                                path: str_of(&c, "path"),
                                kind: str_of(&c, "kind"),
                            });
                        }
                    }
                    "mcp_tool_call" => {
                        if done {
                            out.push(AgentEvent::ToolResult {
                                id,
                                output: item
                                    .get("result")
                                    .map(|r| r.to_string())
                                    .unwrap_or_default(),
                                is_error: item.get("status").and_then(|s| s.as_str())
                                    == Some("failed"),
                            });
                        } else if ty == "item.started" {
                            out.push(AgentEvent::ToolCall {
                                id,
                                name: format!(
                                    "{}.{}",
                                    str_of(&item, "server"),
                                    str_of(&item, "tool")
                                ),
                                input: item.get("arguments").cloned().unwrap_or(Value::Null),
                            });
                        }
                    }
                    "web_search" if done => out.push(AgentEvent::ToolCall {
                        id,
                        name: "web_search".into(),
                        input: serde_json::json!({ "query": str_of(&item, "query") }),
                    }),
                    "error" if done => out.push(AgentEvent::Error {
                        message: str_of(&item, "message"),
                    }),
                    _ => {}
                }
            }
            "turn.completed" => {
                self.turns += 1;
                let u = v.get("usage").cloned().unwrap_or(Value::Null);
                let cached = u64_of(&u, "cached_input_tokens");
                let usage = Usage {
                    input_tokens: u64_of(&u, "input_tokens").saturating_sub(cached),
                    output_tokens: u64_of(&u, "output_tokens"),
                    cached_tokens: cached,
                    cost_usd: None,
                };
                self.total.add(&usage);
                out.push(AgentEvent::Finished {
                    ok: true,
                    summary: self.last_message.clone(),
                    usage: Some(self.total.clone()),
                    num_turns: Some(self.turns),
                    duration_ms: None,
                });
            }
            "turn.failed" => {
                let message = v
                    .pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .unwrap_or("turn failed")
                    .to_string();
                out.push(AgentEvent::Error {
                    message: message.clone(),
                });
                out.push(AgentEvent::Finished {
                    ok: false,
                    summary: message,
                    usage: Some(self.total.clone()),
                    num_turns: Some(self.turns),
                    duration_ms: None,
                });
            }
            "error" => out.push(AgentEvent::Error {
                message: str_of(&v, "message"),
            }),
            _ => out.push(AgentEvent::Raw {
                line: line.to_string(),
            }),
        }
        out
    }
}
