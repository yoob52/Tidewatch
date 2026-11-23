//! 运行一次 Agent：启动进程树、写入提示词、逐行解析事件、支持中途停止与超时。
use crate::agent::{AgentAdapter, AgentEvent, AgentKind, CommandSpec, RunSpec, Usage};
use crate::process;
use crate::Result;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Clone, Default)]
pub struct RunOptions {
    /// 原始 JSONL 落盘路径（追加写）。
    pub raw_log: Option<PathBuf>,
    /// 置为 true 即停止整棵进程树。
    pub cancel: Arc<AtomicBool>,
    pub timeout: Option<Duration>,
    /// 护栏（预算、卡住、危险命令）置为 true：停掉进程树，结果记为被护栏中止而不是用户停止。
    pub abort: Arc<AtomicBool>,
    /// 交互模式下的追加指令收件箱（Agent 不支持时忽略）。
    pub steer: Option<Arc<SteerInbox>>,
}

#[derive(Debug, Clone)]
pub enum RunnerEvent {
    Spawned {
        pid: u32,
        command: String,
    },
    Agent(AgentEvent),
    Stderr(String),
    /// Agent 已经这么久没有任何输出（每 10 秒报一次）。
    Idle(Duration),
    /// 一条追加指令已写进运行中 Agent 的 stdin。
    Steered(String),
}

/// 运行中追加指令的收件箱：界面投递，运行器写进 Agent 的 stdin。
/// Agent 答完最后一轮、stdin 关闭之后不再收件，投递失败的指令由调用方改为排队到下一轮。
#[derive(Debug, Default)]
pub struct SteerInbox {
    state: std::sync::Mutex<(Vec<String>, bool)>,
}

impl SteerInbox {
    /// 投递成功返回 `true`；通道已关闭返回 `false`。
    pub fn push(&self, text: String) -> bool {
        let mut s = self.state.lock().unwrap();
        if s.1 {
            return false;
        }
        s.0.push(text);
        true
    }

    pub fn is_closed(&self) -> bool {
        self.state.lock().unwrap().1
    }

    /// 关闭后取出还没写进 stdin 的指令。
    pub fn take_left(&self) -> Vec<String> {
        let mut s = self.state.lock().unwrap();
        s.1 = true;
        std::mem::take(&mut s.0)
    }

    fn take(&self) -> Vec<String> {
        std::mem::take(&mut self.state.lock().unwrap().0)
    }

    fn close_if_empty(&self) -> bool {
        let mut s = self.state.lock().unwrap();
        if s.0.is_empty() {
            s.1 = true;
        }
        s.1
    }
}

#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub session_id: Option<String>,
    pub exit_code: Option<i32>,
    /// 被用户停止。
    pub stopped: bool,
    pub timed_out: bool,
    /// 被护栏中止。
    pub aborted: bool,
    /// 护栏中止的原因（引擎填）。
    pub abort_reason: Option<String>,
    /// Agent 自己报告的结果；没收到结束事件为 `None`。
    pub finished_ok: Option<bool>,
    pub summary: String,
    pub usage: Usage,
    pub stderr_tail: String,
    /// 原始日志写入失败（磁盘满等）时的错误；此后的输出不再写入日志。
    pub raw_log_error: Option<String>,
    pub duration_ms: u64,
}

impl RunOutcome {
    fn absorb(&mut self, ev: &AgentEvent) {
        match ev {
            AgentEvent::SessionStarted { session_id, .. } => {
                self.session_id = Some(session_id.clone())
            }
            AgentEvent::Usage { usage } => self.usage.add(usage),
            AgentEvent::Finished {
                ok, summary, usage, ..
            } => {
                self.finished_ok = Some(*ok);
                self.summary = summary.clone();
                if let Some(u) = usage {
                    self.usage = u.clone();
                }
            }
            _ => {}
        }
    }

    pub fn succeeded(&self) -> bool {
        !self.stopped
            && !self.timed_out
            && !self.aborted
            && self.exit_code == Some(0)
            && self.finished_ok != Some(false)
    }
}

enum Msg {
    Out(String),
    Err(String),
    OutClosed,
    ErrClosed,
}

/// 任何 Agent 都要的系统变量：可执行文件查找、用户目录、临时目录、区域、代理与证书、语言工具链位置。
const SYSTEM_ENV: &[&str] = &[
    "PATH",
    "PATHEXT",
    "HOME",
    "USER",
    "LOGNAME",
    "SHELL",
    "LANG",
    "LANGUAGE",
    "TERM",
    "COLORTERM",
    "TZ",
    "TMPDIR",
    "TEMP",
    "TMP",
    // Windows
    "USERPROFILE",
    "USERNAME",
    "USERDOMAIN",
    "HOMEDRIVE",
    "HOMEPATH",
    "APPDATA",
    "LOCALAPPDATA",
    "PROGRAMDATA",
    "PROGRAMFILES",
    "PROGRAMFILES(X86)",
    "PROGRAMW6432",
    "COMMONPROGRAMFILES",
    "COMMONPROGRAMFILES(X86)",
    "COMMONPROGRAMW6432",
    "ALLUSERSPROFILE",
    "PUBLIC",
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "OS",
    "COMPUTERNAME",
    "NUMBER_OF_PROCESSORS",
    "PROCESSOR_ARCHITECTURE",
    "PROCESSOR_IDENTIFIER",
    "PSMODULEPATH",
    // 代理与证书
    "HTTP_PROXY",
    "HTTPS_PROXY",
    "NO_PROXY",
    "ALL_PROXY",
    "SSL_CERT_FILE",
    "SSL_CERT_DIR",
    "NODE_EXTRA_CA_CERTS",
    // 工具链位置（Agent 要能跑项目的测试 / 构建命令）
    "JAVA_HOME",
    "GOPATH",
    "GOROOT",
    "CARGO_HOME",
    "RUSTUP_HOME",
    "PNPM_HOME",
    "VOLTA_HOME",
    "PYENV_ROOT",
    "VIRTUAL_ENV",
    "CONDA_PREFIX",
    "ANDROID_HOME",
    "ANDROID_SDK_ROOT",
    "DOTNET_ROOT",
    "MAVEN_HOME",
    "M2_HOME",
    "GRADLE_USER_HOME",
];
const SYSTEM_ENV_PREFIXES: &[&str] = &["LC_", "XDG_", "NVM_"];

/// 系统变量（PATH、用户目录、代理、证书、工具链位置）。项目的初始化 / 检查命令只继承这些：
/// 它们跑的是仓库里的脚本，Agent 改得到，不能让脚本读到没给 Agent 的密钥。
pub fn is_system_env(name: &str) -> bool {
    let name = name.to_ascii_uppercase();
    SYSTEM_ENV.contains(&name.as_str()) || SYSTEM_ENV_PREFIXES.iter().any(|p| name.starts_with(p))
}

/// 子进程能从观澜自身环境里继承哪些变量。
///
/// 只放行系统变量、Agent 自己的配置变量和用户点名的变量（`passthrough`）；启动观澜的 shell 里
/// 的其它密钥不会传给 Agent（docs/04-architecture/security-storage.md §3）。Provider 已经注入
/// 凭据时，环境里现成的同类凭据（`ANTHROPIC_*` / `OPENAI_*`）也不继承，免得两套混用。
pub fn inherits_env(
    kind: AgentKind,
    name: &str,
    injected: &[(String, String)],
    passthrough: &[String],
) -> bool {
    let name = name.to_ascii_uppercase();
    if is_system_env(&name) || passthrough.iter().any(|p| p.eq_ignore_ascii_case(&name)) {
        return true;
    }
    let (config, credentials): (&[&str], &[&str]) = match kind {
        AgentKind::ClaudeCode => (&["CLAUDE_"], &["ANTHROPIC_"]),
        AgentKind::Codex => (&["CODEX_"], &["OPENAI_"]),
        AgentKind::OpenCode => (
            &["OPENCODE_"],
            &["ANTHROPIC_", "OPENAI_", "GEMINI_", "GOOGLE_"],
        ),
        AgentKind::Gemini => (&["GEMINI_CLI_"], &["GEMINI_", "GOOGLE_"]),
        AgentKind::Aider => (
            &["AIDER_"],
            &[
                "ANTHROPIC_",
                "OPENAI_",
                "GEMINI_",
                "DEEPSEEK_",
                "OPENROUTER_",
            ],
        ),
        AgentKind::Mock => (&["TIDEWATCH_MOCK_"], &[]),
    };
    if config.iter().any(|p| name.starts_with(p)) {
        return true;
    }
    // 只注入了模型名之类的覆盖项不算：这时登录凭据仍然来自环境
    let provider_injected = injected.iter().any(|(k, v)| {
        let k = k.to_ascii_uppercase();
        k == crate::provider::CODEX_KEY_ENV
            // OpenCode 的内联配置每次都有（权限、MCP）；里面带了 Provider 才算注入
            || (k == crate::agent::opencode::CONFIG_ENV
                && crate::agent::opencode::config_has_provider(v))
            || credentials.iter().any(|p| {
                k.strip_prefix(p).is_some_and(|rest| {
                    matches!(rest, "API_KEY" | "AUTH_TOKEN" | "BASE_URL" | "API_BASE")
                })
            })
    });
    !provider_injected && credentials.iter().any(|p| name.starts_with(p))
}

fn to_command(kind: AgentKind, spec: &CommandSpec, passthrough: &[String]) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd).env_clear();
    for (k, v) in std::env::vars_os() {
        if k.to_str()
            .is_some_and(|k| inherits_env(kind, k, &spec.env, passthrough))
        {
            cmd.env(k, v);
        }
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    cmd
}

pub fn run_agent(
    adapter: &dyn AgentAdapter,
    spec: &RunSpec,
    opts: &RunOptions,
    on_event: &mut dyn FnMut(RunnerEvent),
) -> Result<RunOutcome> {
    adapter.prepare(spec)?;
    let command = adapter.build_command(spec)?;
    let mut parser = adapter.parser_for(spec);
    let started = Instant::now();
    let mut tree = process::spawn(to_command(adapter.kind(), &command, &spec.env_passthrough))?;
    on_event(RunnerEvent::Spawned {
        pid: tree.id(),
        command: command.display(),
    });

    let steer = opts
        .steer
        .clone()
        .filter(|_| command.interactive && adapter.steer_message("").is_some());
    if let (None, Some(inbox)) = (&steer, &opts.steer) {
        // 这次是一次性输入，运行中收不了追加指令：让引擎改走排队
        inbox.close_if_empty();
    }
    let mut stdin_tx: Option<mpsc::Sender<String>> = None;
    if let Some(mut stdin) = tree.take_stdin() {
        let prompt = command.stdin.clone();
        if steer.is_some() {
            let (stx, srx) = mpsc::channel::<String>();
            std::thread::spawn(move || {
                if stdin
                    .write_all(prompt.as_bytes())
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    return;
                }
                for line in srx {
                    if stdin
                        .write_all(line.as_bytes())
                        .and_then(|_| stdin.flush())
                        .is_err()
                    {
                        return;
                    }
                }
            });
            stdin_tx = Some(stx);
        } else {
            std::thread::spawn(move || {
                let _ = stdin.write_all(prompt.as_bytes());
            });
        }
    }
    // 交互模式：每条写进去的消息对应一轮，等这么多个 result 后关闭 stdin 让 Agent 退出
    let mut pending_turns: u32 = 1;
    let mut finished_once = false;

    let (tx, rx) = mpsc::channel::<Msg>();
    let stdout = tree.take_stdout().expect("stdout piped");
    let stderr = tree.take_stderr().expect("stderr piped");
    let tx_out = tx.clone();
    // 已读到 EOF 的管道数；两条都到 EOF 说明剩下的输出全在队列里。
    let eof = Arc::new(AtomicUsize::new(0));
    let (eof_out, eof_err) = (eof.clone(), eof.clone());
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            match line {
                Ok(l) => {
                    if tx_out.send(Msg::Out(l)).is_err() {
                        return;
                    }
                }
                Err(_) => break,
            }
        }
        eof_out.fetch_add(1, Ordering::SeqCst);
        let _ = tx_out.send(Msg::OutClosed);
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            if tx.send(Msg::Err(line)).is_err() {
                return;
            }
        }
        eof_err.fetch_add(1, Ordering::SeqCst);
        let _ = tx.send(Msg::ErrClosed);
    });

    let mut raw = match &opts.raw_log {
        Some(p) => {
            if let Some(dir) = p.parent() {
                std::fs::create_dir_all(dir)?;
            }
            Some(
                std::fs::OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(p)?,
            )
        }
        None => None,
    };

    let mut outcome = RunOutcome::default();
    let mut stderr_lines: Vec<String> = vec![];
    let mut out_closed = false;
    let mut err_closed = false;
    let mut exited_at: Option<Instant> = None;

    let mut last_msg = Instant::now();
    let mut last_idle = Instant::now();

    loop {
        let msg = rx.recv_timeout(Duration::from_millis(50));
        if msg.is_ok() {
            last_msg = Instant::now();
            last_idle = last_msg;
        } else if exited_at.is_none() && last_idle.elapsed() >= Duration::from_secs(10) {
            last_idle = Instant::now();
            on_event(RunnerEvent::Idle(last_msg.elapsed()));
        }
        match msg {
            Ok(Msg::Out(line)) => {
                if let Some(f) = raw.as_mut() {
                    if let Err(e) = writeln!(f, "{line}") {
                        // 日志写不进去不该打断 Agent，但要让调用方知道日志不完整
                        outcome.raw_log_error = Some(e.to_string());
                        raw = None;
                    }
                }
                for ev in parser.parse_line(&line) {
                    if matches!(ev, AgentEvent::Finished { .. }) {
                        pending_turns = pending_turns.saturating_sub(1);
                        finished_once = true;
                    }
                    outcome.absorb(&ev);
                    on_event(RunnerEvent::Agent(ev));
                }
                if let Some(tx) = &stdin_tx {
                    for l in parser.take_input() {
                        let _ = tx.send(l);
                    }
                }
            }
            Ok(Msg::Err(line)) => {
                on_event(RunnerEvent::Stderr(line.clone()));
                stderr_lines.push(line);
                if stderr_lines.len() > 200 {
                    stderr_lines.remove(0);
                }
            }
            Ok(Msg::OutClosed) => out_closed = true,
            Ok(Msg::ErrClosed) => err_closed = true,
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                out_closed = true;
                err_closed = true;
            }
        }

        if let (Some(inbox), Some(tx)) = (&steer, &stdin_tx) {
            for text in inbox.take() {
                if let Some(line) = parser.steer(&text).or_else(|| adapter.steer_message(&text)) {
                    if tx.send(line).is_ok() {
                        pending_turns += 1;
                        on_event(RunnerEvent::Steered(text));
                    }
                }
            }
            // 有的 Agent 会把运行中收到的消息并进当前这一轮：答完后 5 秒没动静也视为结束
            let quiet = last_msg.elapsed() > Duration::from_secs(5);
            let idle = parser
                .idle()
                .unwrap_or(finished_once && (pending_turns == 0 || quiet));
            if idle && inbox.close_if_empty() {
                stdin_tx = None;
            }
        }

        if opts.cancel.load(Ordering::SeqCst) && !outcome.stopped {
            outcome.stopped = true;
            tree.kill_tree(Duration::from_secs(2))?;
        } else if opts.timeout.is_some_and(|t| started.elapsed() > t)
            && !outcome.timed_out
            && !outcome.stopped
        {
            outcome.timed_out = true;
            tree.kill_tree(Duration::from_secs(2))?;
        } else if opts.abort.load(Ordering::SeqCst)
            && !outcome.aborted
            && !outcome.timed_out
            && !outcome.stopped
        {
            outcome.aborted = true;
            tree.kill_tree(Duration::from_secs(2))?;
        }

        if exited_at.is_none() {
            if let Some(status) = tree.try_wait()? {
                outcome.exit_code = status.code();
                exited_at = Some(Instant::now());
                tree.kill_leftovers();
            }
        }
        // 进程已退出：把 stdout 和 stderr 读完、处理完再返回。只有脱离了进程树的后代还占着管道时
        // 才放弃等待：管道连续 1 秒没有新内容，或退出已超过 5 秒（它一直在写）。
        if let Some(t) = exited_at {
            let pipes_held = eof.load(Ordering::SeqCst) < 2;
            let give_up = pipes_held
                && (last_msg.elapsed() > Duration::from_secs(1)
                    || t.elapsed() > Duration::from_secs(5));
            if (out_closed && err_closed) || give_up {
                break;
            }
        }
    }

    drop(stdin_tx);
    if let Some(inbox) = &opts.steer {
        let _ = inbox.is_closed() || inbox.close_if_empty();
    }
    for ev in parser.finish() {
        outcome.absorb(&ev);
        on_event(RunnerEvent::Agent(ev));
    }
    let n = stderr_lines.len();
    outcome.stderr_tail = stderr_lines[n.saturating_sub(20)..].join("\n");
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    Ok(outcome)
}
