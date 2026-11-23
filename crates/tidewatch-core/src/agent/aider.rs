//! Aider：`aider --message-file`，一次性跑完就退出。
//! 输出是给人看的纯文本，没有 JSON 事件，也没有会话 id：只能按行认出改了哪些文件、用量和报错。
use super::*;

pub struct AiderAdapter;

/// 提示词文件名（在 worktree 根目录）。以 `.aider` 开头，和 Aider 自己的缓存文件一起被排除在提交之外。
pub const PROMPT_FILE: &str = ".aider.tidewatch-prompt.md";
/// Provider 注入时带上的模型名前缀（`openai/`、`anthropic/`…）。只给适配器看，不传给 Aider。
pub const MODEL_PREFIX_ENV: &str = "TIDEWATCH_AIDER_MODEL_PREFIX";
/// Aider 默认把自己当成结对编程的一方，会反问"要不要把某文件加进对话"；一次性运行时没人回答。
const ONE_SHOT_NOTE: &str = "（本次是非交互运行：没有人能回答问题，也没有人能把文件加进对话。需要改的文件直接改，不存在的文件直接新建，不要反问。）";
/// Aider 没有会话 id：对话记录就在 worktree 的 `.aider.chat.history.md` 里。
/// 给一个固定的占位 id，引擎才会在续接时带上 `resume_session`。
pub const SESSION_ID: &str = "aider-chat-history";
const EXCLUDE_PATTERN: &str = ".aider*";

impl AgentAdapter for AiderAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Aider
    }
    fn default_binary(&self) -> &'static str {
        "aider"
    }

    /// Aider 会在仓库根目录留下 `.aider.chat.history.md` 等文件：写进 `info/exclude`，
    /// 免得被自动提交带走；提示词也落成文件，避开 Windows 命令行长度上限。
    fn prepare(&self, spec: &RunSpec) -> Result<()> {
        if let Ok(dir) = crate::git::git(&spec.cwd, &["rev-parse", "--git-common-dir"]) {
            let dir = spec.cwd.join(dir.trim());
            let exclude = dir.join("info").join("exclude");
            let current = std::fs::read_to_string(&exclude).unwrap_or_default();
            if !current.lines().any(|l| l.trim() == EXCLUDE_PATTERN) {
                std::fs::create_dir_all(dir.join("info"))?;
                let sep = if current.is_empty() || current.ends_with('\n') {
                    ""
                } else {
                    "\n"
                };
                std::fs::write(&exclude, format!("{current}{sep}{EXCLUDE_PATTERN}\n"))?;
            }
        }
        std::fs::write(
            spec.cwd.join(PROMPT_FILE),
            format!("{ONE_SHOT_NOTE}\n\n{}", spec.prompt),
        )?;
        Ok(())
    }

    fn build_command(&self, spec: &RunSpec) -> Result<CommandSpec> {
        let mut args: Vec<String> = [
            "--yes-always",
            // 提交由观澜统一做
            "--no-auto-commits",
            "--no-stream",
            "--no-pretty",
            "--no-check-update",
            "--no-show-model-warnings",
            "--no-analytics",
            "--no-gitignore",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let prefix = spec
            .env
            .iter()
            .find(|(k, _)| k == MODEL_PREFIX_ENV)
            .map(|(_, v)| v.as_str())
            .unwrap_or_default();
        if let Some(m) = &spec.model {
            let m = if m.starts_with(prefix) {
                m.clone()
            } else {
                format!("{prefix}{m}")
            };
            args.extend(["--model".into(), m]);
        }
        if spec.permission == PermissionMode::ReadOnly {
            args.push("--dry-run".into());
        }
        // 没有会话 id：续接靠 worktree 里的聊天记录文件
        if spec.resume_session.is_some() {
            args.push("--restore-chat-history".into());
        } else {
            args.push("--no-restore-chat-history".into());
        }
        for f in &spec.images {
            args.extend(["--read".into(), f.display().to_string()]);
        }
        args.extend(spec.extra_args.iter().cloned());
        args.extend(["--message-file".into(), PROMPT_FILE.into()]);
        Ok(CommandSpec {
            program: self.resolve_binary(spec)?,
            args,
            // 不是终端时 Aider 按 80 列折行，会把 JSON 字符串从中间断开
            env: spec
                .env
                .iter()
                .filter(|(k, _)| k != MODEL_PREFIX_ENV)
                .cloned()
                .chain([("COLUMNS".to_string(), "10000".to_string())])
                .collect(),
            cwd: spec.cwd.clone(),
            stdin: String::new(),
            interactive: false,
        })
    }

    fn parser(&self) -> Box<dyn EventParser> {
        Box::new(AiderParser::default())
    }
}

#[derive(Default)]
pub struct AiderParser {
    /// 当前这段文字（空行分段）。
    block: Vec<String>,
    /// 本轮回复里已经结束的各段。
    reply: Vec<String>,
    /// 最近一轮完整的回复。
    last_reply: String,
    usage: Usage,
    turns: u64,
    error: Option<String>,
}

/// 启动时的说明和各种提示：记进日志，但不算回复内容。
const NOTICE_PREFIXES: &[&str] = &[
    "Aider v",
    "Model: ",
    "Main model: ",
    "Weak model: ",
    "Editor model: ",
    "Git repo: ",
    "Repo-map: ",
    "Added ",
    "Warning: ",
    "Retrying in ",
    "Restored previous conversation history",
    "https://aider.chat/",
];

impl AiderParser {
    fn flush(&mut self, out: &mut Vec<AgentEvent>) {
        if self.block.is_empty() {
            return;
        }
        let text = std::mem::take(&mut self.block).join("\n");
        self.reply.push(text.clone());
        out.push(AgentEvent::Text { text });
    }
    fn end_turn(&mut self) {
        if !self.reply.is_empty() {
            self.last_reply = std::mem::take(&mut self.reply).join("\n\n");
        }
    }
}

/// `5.2k`、`33`、`1.2M` 这样的数量。
fn count(s: &str) -> u64 {
    let s = s.trim().replace(',', "");
    let (num, mul) = match s.chars().last() {
        Some('k') | Some('K') => (&s[..s.len() - 1], 1_000.0),
        Some('M') => (&s[..s.len() - 1], 1_000_000.0),
        _ => (s.as_str(), 1.0),
    };
    num.parse::<f64>().map(|n| (n * mul) as u64).unwrap_or(0)
}

/// `Tokens: 5.2k sent, 33 received. Cost: $0.01 message, $0.01 session.`
fn usage_line(line: &str) -> Option<Usage> {
    let rest = line.strip_prefix("Tokens: ")?;
    let (tokens, cost) = match rest.split_once(" Cost: ") {
        Some((t, c)) => (t, Some(c)),
        None => (rest, None),
    };
    let mut usage = Usage::default();
    for part in tokens.trim_end_matches('.').split(", ") {
        let (n, what) = part.trim().split_once(' ')?;
        match what.trim_end_matches('.') {
            "sent" => usage.input_tokens = count(n),
            "received" => usage.output_tokens = count(n),
            "cache hit" => usage.cached_tokens = count(n),
            _ => {}
        }
    }
    usage.cost_usd = cost
        .and_then(|c| c.split(" message").next())
        .and_then(|c| c.trim().trim_start_matches('$').parse::<f64>().ok())
        .filter(|c| *c > 0.0);
    Some(usage)
}

/// 模型调用失败时 Aider 照样以 0 退出，只能从输出里认。
fn error_line(line: &str) -> bool {
    line.split_once(':').is_some_and(|(head, _)| {
        head.starts_with("litellm.") && (head.ends_with("Error") || head.ends_with("Exception"))
    })
}

impl EventParser for AiderParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let line = line.trim_end();
        let mut out = vec![];
        if line.trim().is_empty() {
            self.flush(&mut out);
        } else if let Some(usage) = usage_line(line) {
            self.flush(&mut out);
            self.end_turn();
            self.usage.add(&usage);
            self.turns += 1;
            // 限流之类的报错之后重试成功了
            self.error = None;
            out.push(AgentEvent::Usage { usage });
        } else if let Some(path) = line.strip_prefix("Applied edit to ") {
            self.flush(&mut out);
            out.push(AgentEvent::FileChange {
                path: path.trim().to_string(),
                kind: "edit".into(),
            });
        } else if error_line(line) {
            self.flush(&mut out);
            self.error = Some(line.to_string());
            out.push(AgentEvent::Error {
                message: line.to_string(),
            });
        } else if NOTICE_PREFIXES.iter().any(|p| line.starts_with(p)) {
            self.flush(&mut out);
            if let Some(model) = line
                .strip_prefix("Model: ")
                .or(line.strip_prefix("Main model: "))
            {
                out.push(AgentEvent::SessionStarted {
                    session_id: SESSION_ID.into(),
                    model: model.split_whitespace().next().map(String::from),
                });
            }
            out.push(AgentEvent::Raw {
                line: line.to_string(),
            });
        } else {
            self.block.push(line.to_string());
        }
        out
    }

    fn finish(&mut self) -> Vec<AgentEvent> {
        let mut out = vec![];
        self.flush(&mut out);
        // 用量行之后的输出是"改了哪些文件"之类的收尾，不是回复
        if self.last_reply.is_empty() {
            self.end_turn();
        }
        out.push(AgentEvent::Finished {
            ok: self.error.is_none(),
            summary: self.error.clone().unwrap_or(self.last_reply.clone()),
            usage: Some(self.usage.clone()),
            num_turns: Some(self.turns),
            duration_ms: None,
        });
        out
    }
}
