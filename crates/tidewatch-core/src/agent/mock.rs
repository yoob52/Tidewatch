//! 演示 Agent：调用 `twctl mock-agent`（或 `TIDEWATCH_MOCK_AGENT` 指定的程序），
//! 输出与 Claude Code 相同格式的 JSONL，用于没有账号时走通全流程，以及自动化测试。
use super::claude::ClaudeParser;
use super::*;

pub struct MockAdapter;

pub const MOCK_BINARY_ENV: &str = "TIDEWATCH_MOCK_AGENT";
/// 演示 Agent 从这个环境变量拿观澜 MCP 接口的地址。
pub const MCP_URL_ENV: &str = "TIDEWATCH_MCP_URL";

impl AgentAdapter for MockAdapter {
    fn kind(&self) -> AgentKind {
        AgentKind::Mock
    }
    fn default_binary(&self) -> &'static str {
        "twctl"
    }

    fn resolve_binary(&self, spec: &RunSpec) -> Result<PathBuf> {
        if let Some(p) = &spec.binary {
            return Ok(p.clone());
        }
        if let Some(p) = std::env::var_os(MOCK_BINARY_ENV) {
            return Ok(PathBuf::from(p));
        }
        // 与当前可执行文件同目录的 twctl（安装包内随应用分发）
        if let Ok(exe) = std::env::current_exe() {
            let sibling = exe.with_file_name(format!("twctl{}", std::env::consts::EXE_SUFFIX));
            if sibling.exists() {
                return Ok(sibling);
            }
        }
        which::which("twctl").map_err(|_| Error::BinaryNotFound("twctl".into()))
    }

    fn build_command(&self, spec: &RunSpec) -> Result<CommandSpec> {
        let mut args: Vec<String> = vec!["mock-agent".into()];
        if let Some(m) = &spec.model {
            args.extend(["--model".into(), m.clone()]);
        }
        args.extend(["--permission".into(), spec.permission.as_str().into()]);
        if let Some(id) = &spec.resume_session {
            args.extend(["--resume".into(), id.clone()]);
        }
        args.extend(spec.extra_args.iter().cloned());
        let mut env = spec.env.clone();
        if let Some(mcp) = &spec.mcp {
            env.push((MCP_URL_ENV.into(), mcp.url.clone()));
        }
        Ok(CommandSpec {
            program: self.resolve_binary(spec)?,
            args,
            env,
            cwd: spec.cwd.clone(),
            stdin: spec.prompt.clone(),
        })
    }

    fn parser(&self) -> Box<dyn EventParser> {
        Box::new(ClaudeParser::default())
    }

    fn detect(&self) -> Detection {
        let path = self.resolve_binary(&RunSpec::default()).ok();
        Detection {
            kind: AgentKind::Mock,
            installed: path.is_some(),
            path: path.map(|p| p.display().to_string()),
            version: Some("内置".into()),
        }
    }
}
