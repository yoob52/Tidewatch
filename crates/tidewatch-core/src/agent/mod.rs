//! Agent 适配层：把各家 CLI 的命令行与事件流统一成 `RunSpec` → `AgentEvent`。
pub mod aider;
pub mod claude;
pub mod codex;
pub mod gemini;
pub mod mock;
pub mod mock_process;
pub mod opencode;

use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentKind {
    ClaudeCode,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
    Gemini,
    Aider,
    /// 内置演示 Agent：不联网、不需要账号，按 Claude 的事件格式输出。
    Mock,
}

impl AgentKind {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "claude_code",
            AgentKind::Codex => "codex",
            AgentKind::OpenCode => "opencode",
            AgentKind::Gemini => "gemini",
            AgentKind::Aider => "aider",
            AgentKind::Mock => "mock",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "claude_code" | "claude" => Ok(AgentKind::ClaudeCode),
            "codex" => Ok(AgentKind::Codex),
            "opencode" => Ok(AgentKind::OpenCode),
            "gemini" | "gemini_cli" => Ok(AgentKind::Gemini),
            "aider" => Ok(AgentKind::Aider),
            "mock" => Ok(AgentKind::Mock),
            other => Err(Error::invalid(format!("未知 Agent 类型：{other}"))),
        }
    }
    /// 真实 Agent（不含演示 Agent），按界面上的顺序。
    pub const REAL: [AgentKind; 5] = [
        AgentKind::ClaudeCode,
        AgentKind::Codex,
        AgentKind::OpenCode,
        AgentKind::Gemini,
        AgentKind::Aider,
    ];
    pub fn display_name(self) -> &'static str {
        match self {
            AgentKind::ClaudeCode => "Claude Code",
            AgentKind::Codex => "Codex",
            AgentKind::OpenCode => "OpenCode",
            AgentKind::Gemini => "Gemini CLI",
            AgentKind::Aider => "Aider",
            AgentKind::Mock => "演示 Agent",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    ReadOnly,
    #[default]
    WorkspaceWrite,
    FullAuto,
}

impl PermissionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::ReadOnly => "read_only",
            PermissionMode::WorkspaceWrite => "workspace_write",
            PermissionMode::FullAuto => "full_auto",
        }
    }
    pub fn parse(s: &str) -> Result<Self> {
        match s {
            "read_only" => Ok(PermissionMode::ReadOnly),
            "workspace_write" => Ok(PermissionMode::WorkspaceWrite),
            "full_auto" => Ok(PermissionMode::FullAuto),
            other => Err(Error::invalid(format!("未知权限模式：{other}"))),
        }
    }
}

/// 一次 Agent 调用的全部输入。
#[derive(Debug, Clone, Default)]
pub struct RunSpec {
    pub prompt: String,
    pub cwd: PathBuf,
    pub model: Option<String>,
    pub permission: PermissionMode,
    /// 续接已有会话。
    pub resume_session: Option<String>,
    /// 这次调用只要一段文字回答：不给 Agent 读文件、跑命令、联网的工具。
    /// 只读模式拦不住读工作目录之外的文件，分析不可信的正文时要用这个。
    /// Gemini CLI 和 Aider 没有对应的开关，仍然只是只读。
    pub no_tools: bool,
    /// 追加到白名单的工具 / 命令（Claude `--allowedTools`）。
    pub allowed_tools: Vec<String>,
    /// 指定可执行文件；为空则从 PATH 查找。
    pub binary: Option<PathBuf>,
    /// 仅注入本次子进程的环境变量（Provider 密钥等）。
    pub env: Vec<(String, String)>,
    /// 额外允许从观澜自身环境继承的变量名；默认只继承系统变量和 Agent 自己的配置变量。
    pub env_passthrough: Vec<String>,
    /// 追加到命令行末尾之前的参数（Provider 的 `-c` 覆盖、实例启动参数）。
    pub extra_args: Vec<String>,
    /// 随 Prompt 发给 Agent 的图片（绝对路径，在 worktree 之外）。
    pub images: Vec<PathBuf>,
    /// 观澜自己的 MCP 服务（委派、提问）；不支持 MCP 的 Agent 忽略它。
    pub mcp: Option<McpServer>,
}

/// 注入给 Agent 的 MCP 服务：地址进配置，令牌只放在环境变量里。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct McpServer {
    /// `http://127.0.0.1:<端口>/mcp`
    pub url: String,
    /// 存着 Bearer 令牌的环境变量名（值在 `RunSpec::env` 里）。
    pub token_env: String,
}

/// MCP 服务在各 Agent 配置里的名字。
pub const MCP_SERVER_NAME: &str = "tidewatch";

/// 可直接交给 `std::process::Command` 的命令描述。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cwd: PathBuf,
    /// 写入 stdin 后关闭。
    pub stdin: String,
}

impl CommandSpec {
    /// 日志 / 界面展示用：环境变量值一律脱敏。
    pub fn display(&self) -> String {
        let env: Vec<String> = self
            .env
            .iter()
            .map(|(k, v)| {
                let up = k.to_ascii_uppercase();
                let secret = ["KEY", "TOKEN", "SECRET", "PASSWORD"]
                    .iter()
                    .any(|w| up.contains(w));
                format!(
                    "{k}={}",
                    if secret {
                        crate::util::mask_secret(v)
                    } else {
                        v.clone()
                    }
                )
            })
            .collect();
        let mut parts = env;
        parts.push(self.program.display().to_string());
        parts.extend(self.args.iter().map(|a| {
            if a.contains(' ') {
                format!("{a:?}")
            } else {
                a.clone()
            }
        }));
        parts.join(" ")
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cached_tokens: u64,
    /// CLI 自报的费用（Claude 有，Codex 没有）。
    pub cost_usd: Option<f64>,
}

impl Usage {
    pub fn add(&mut self, other: &Usage) {
        self.input_tokens += other.input_tokens;
        self.output_tokens += other.output_tokens;
        self.cached_tokens += other.cached_tokens;
        if let Some(c) = other.cost_usd {
            self.cost_usd = Some(self.cost_usd.unwrap_or(0.0) + c);
        }
    }
}

/// 归一化事件：界面与落库都只认这一套。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AgentEvent {
    SessionStarted {
        session_id: String,
        model: Option<String>,
    },
    Text {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    ToolResult {
        id: String,
        output: String,
        is_error: bool,
    },
    Command {
        command: String,
        output: String,
        exit_code: Option<i64>,
        done: bool,
    },
    FileChange {
        path: String,
        kind: String,
    },
    Usage {
        usage: Usage,
    },
    Finished {
        ok: bool,
        summary: String,
        usage: Option<Usage>,
        num_turns: Option<u64>,
        duration_ms: Option<u64>,
    },
    Error {
        message: String,
    },
    /// 无法识别的行，原样保留，不丢。
    Raw {
        line: String,
    },
}

/// 逐行解析 CLI 的 JSONL 输出；有状态（会话 id、累计用量）。
pub trait EventParser: Send {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent>;
    /// 输出读完之后调用一次：没有结束事件的 CLI（Aider）在这里补上。
    fn finish(&mut self) -> Vec<AgentEvent> {
        vec![]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Detection {
    pub kind: AgentKind,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

pub trait AgentAdapter: Send + Sync {
    fn kind(&self) -> AgentKind;
    /// PATH 里的默认命令名。
    fn default_binary(&self) -> &'static str;
    fn build_command(&self, spec: &RunSpec) -> Result<CommandSpec>;
    fn parser(&self) -> Box<dyn EventParser>;
    /// 启动前在工作目录里要做的准备（写提示词文件等）；多数 Agent 不需要。
    fn prepare(&self, _spec: &RunSpec) -> Result<()> {
        Ok(())
    }

    fn resolve_binary(&self, spec: &RunSpec) -> Result<PathBuf> {
        match &spec.binary {
            Some(p) => Ok(p.clone()),
            None => which::which(self.default_binary())
                .map_err(|_| Error::BinaryNotFound(self.default_binary().to_string())),
        }
    }

    fn detect(&self) -> Detection {
        let path = which::which(self.default_binary()).ok();
        let version = path.as_ref().and_then(|p| {
            let out = std::process::Command::new(p)
                .arg("--version")
                .output()
                .ok()?;
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            (!s.is_empty()).then_some(s)
        });
        Detection {
            kind: self.kind(),
            installed: path.is_some(),
            path: path.map(|p| p.display().to_string()),
            version,
        }
    }
}

pub fn adapter(kind: AgentKind) -> Box<dyn AgentAdapter> {
    match kind {
        AgentKind::ClaudeCode => Box::new(claude::ClaudeAdapter),
        AgentKind::Codex => Box::new(codex::CodexAdapter),
        AgentKind::OpenCode => Box::new(opencode::OpenCodeAdapter),
        AgentKind::Gemini => Box::new(gemini::GeminiAdapter),
        AgentKind::Aider => Box::new(aider::AiderAdapter),
        AgentKind::Mock => Box::new(mock::MockAdapter),
    }
}

pub(crate) fn str_of(v: &serde_json::Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or_default()
        .to_string()
}

pub(crate) fn u64_of(v: &serde_json::Value, key: &str) -> u64 {
    v.get(key).and_then(|x| x.as_u64()).unwrap_or(0)
}
