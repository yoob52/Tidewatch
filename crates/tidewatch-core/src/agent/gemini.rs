//! Gemini CLI：`gemini -o stream-json`，提示词走 stdin（`-p` 的内容会接在 stdin 后面，所以给一个空格）。
//! 事件格式按官方文档的 headless 模式写的，没有用真实账号跑过。
use super::*;
use serde_json::Value;

pub struct GeminiAdapter;

impl AgentAdapter for GeminiAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Gemini
    }
    fn default_binary(&self) -> &'static str {
        "gemini"
    }

    fn build_command(&self, spec: &RunSpec) -> Result<CommandSpec> {
        let approval = match spec.permission {
            PermissionMode::ReadOnly => "plan",
            PermissionMode::WorkspaceWrite => "auto_edit",
            PermissionMode::FullAuto => "yolo",
        };
        let mut args: Vec<String> = vec![
            "-o".into(),
            "stream-json".into(),
            "--approval-mode".into(),
            approval.into(),
            // headless 下没法回答"是否信任这个目录"
            "--skip-trust".into(),
        ];
        if let Some(m) = &spec.model {
            args.extend(["-m".into(), m.clone()]);
        }
        if let Some(id) = &spec.resume_session {
            args.extend(["-r".into(), id.clone()]);
        }
        args.extend(spec.extra_args.iter().cloned());
        args.extend(["-p".into(), " ".into()]);
        // Gemini CLI 没有传图片的参数：在提示词里用 @路径 引用，并把图片所在目录加进工作区
        let mut prompt = spec.prompt.clone();
        if !spec.images.is_empty() {
            let mut dirs: Vec<String> = spec
                .images
                .iter()
                .filter_map(|p| p.parent().map(|d| d.display().to_string()))
                .collect();
            dirs.dedup();
            for d in dirs {
                args.extend(["--include-directories".into(), d]);
            }
            prompt.push_str("\n\n附图：\n");
            for f in &spec.images {
                prompt.push_str(&format!("@{}\n", f.display()));
            }
        }
        Ok(CommandSpec {
            program: self.resolve_binary(spec)?,
            args,
            env: spec.env.clone(),
            cwd: spec.cwd.clone(),
            stdin: prompt,
        })
    }

    fn parser(&self) -> Box<dyn EventParser> {
        Box::new(GeminiParser::default())
    }
}

#[derive(Default)]
pub struct GeminiParser {
    text: String,
    turns: u64,
}

impl EventParser for GeminiParser {
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
            "init" => out.push(AgentEvent::SessionStarted {
                session_id: str_of(&v, "session_id"),
                model: v.get("model").and_then(|m| m.as_str()).map(String::from),
            }),
            "message" => {
                if str_of(&v, "role") == "assistant" {
                    let text = str_of(&v, "content");
                    // delta 是流式的片段：攒成一整段，界面上显示片段
                    if v.get("delta").and_then(|d| d.as_bool()) == Some(true) {
                        self.text.push_str(&text);
                    } else {
                        self.text = text.clone();
                    }
                    if !text.is_empty() {
                        out.push(AgentEvent::Text { text });
                    }
                }
            }
            "tool_use" => {
                self.text.clear();
                self.turns += 1;
                let name = str_of(&v, "tool_name");
                let input = v.get("parameters").cloned().unwrap_or(Value::Null);
                if matches!(name.as_str(), "write_file" | "replace" | "edit") {
                    let path = [str_of(&input, "file_path"), str_of(&input, "path")]
                        .into_iter()
                        .find(|p| !p.is_empty())
                        .unwrap_or_default();
                    out.push(AgentEvent::FileChange {
                        path,
                        kind: name.clone(),
                    });
                }
                if name == "run_shell_command" {
                    out.push(AgentEvent::Command {
                        command: str_of(&input, "command"),
                        output: String::new(),
                        exit_code: None,
                        done: false,
                    });
                }
                out.push(AgentEvent::ToolCall {
                    id: str_of(&v, "tool_id"),
                    name,
                    input,
                });
            }
            "tool_result" => {
                let failed = str_of(&v, "status") == "error";
                let output = if failed {
                    v.get("error")
                        .map(|e| str_of(e, "message"))
                        .unwrap_or_default()
                } else {
                    str_of(&v, "output")
                };
                out.push(AgentEvent::ToolResult {
                    id: str_of(&v, "tool_id"),
                    output,
                    is_error: failed,
                });
            }
            "error" => out.push(AgentEvent::Error {
                message: str_of(&v, "message"),
            }),
            "result" => {
                let stats = v.get("stats").cloned().unwrap_or(Value::Null);
                let usage = Usage {
                    input_tokens: u64_of(&stats, "input_tokens"),
                    output_tokens: u64_of(&stats, "output_tokens"),
                    cached_tokens: u64_of(&stats, "cached"),
                    cost_usd: None,
                };
                let ok = str_of(&v, "status") != "error";
                let summary = if ok {
                    self.text.clone()
                } else {
                    v.get("error")
                        .map(|e| str_of(e, "message"))
                        .unwrap_or_default()
                };
                out.push(AgentEvent::Finished {
                    ok,
                    summary,
                    usage: Some(usage),
                    num_turns: Some(self.turns.max(1)),
                    duration_ms: stats.get("duration_ms").and_then(|d| d.as_u64()),
                });
            }
            _ => out.push(AgentEvent::Raw {
                line: line.to_string(),
            }),
        }
        out
    }
}
