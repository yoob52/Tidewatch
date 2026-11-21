//! 运行一次 Agent：启动进程树、写入提示词、逐行解析事件、支持中途停止与超时。
use crate::agent::{AgentAdapter, AgentEvent, CommandSpec, RunSpec, Usage};
use crate::process;
use crate::Result;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
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
}

#[derive(Debug, Clone)]
pub enum RunnerEvent {
    Spawned { pid: u32, command: String },
    Agent(AgentEvent),
    Stderr(String),
}

#[derive(Debug, Clone, Default)]
pub struct RunOutcome {
    pub session_id: Option<String>,
    pub exit_code: Option<i32>,
    /// 被用户停止。
    pub stopped: bool,
    pub timed_out: bool,
    /// Agent 自己报告的结果；没收到结束事件为 `None`。
    pub finished_ok: Option<bool>,
    pub summary: String,
    pub usage: Usage,
    pub stderr_tail: String,
    pub duration_ms: u64,
}

impl RunOutcome {
    pub fn succeeded(&self) -> bool {
        !self.stopped
            && !self.timed_out
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

fn to_command(spec: &CommandSpec) -> Command {
    let mut cmd = Command::new(&spec.program);
    cmd.args(&spec.args).current_dir(&spec.cwd);
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
    let command = adapter.build_command(spec)?;
    let mut parser = adapter.parser();
    let started = Instant::now();
    let mut tree = process::spawn(to_command(&command))?;
    on_event(RunnerEvent::Spawned {
        pid: tree.id(),
        command: command.display(),
    });

    if let Some(mut stdin) = tree.take_stdin() {
        let prompt = command.stdin.clone();
        std::thread::spawn(move || {
            let _ = stdin.write_all(prompt.as_bytes());
        });
    }

    let (tx, rx) = mpsc::channel::<Msg>();
    let stdout = tree.take_stdout().expect("stdout piped");
    let stderr = tree.take_stderr().expect("stderr piped");
    let tx_out = tx.clone();
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
        let _ = tx_out.send(Msg::OutClosed);
    });
    std::thread::spawn(move || {
        for line in BufReader::new(stderr).lines().map_while(|l| l.ok()) {
            if tx.send(Msg::Err(line)).is_err() {
                return;
            }
        }
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

    loop {
        let msg = rx.recv_timeout(Duration::from_millis(50));
        if msg.is_ok() {
            last_msg = Instant::now();
        }
        match msg {
            Ok(Msg::Out(line)) => {
                if let Some(f) = raw.as_mut() {
                    let _ = writeln!(f, "{line}");
                }
                for ev in parser.parse_line(&line) {
                    match &ev {
                        AgentEvent::SessionStarted { session_id, .. } => {
                            outcome.session_id = Some(session_id.clone())
                        }
                        AgentEvent::Usage { usage } => outcome.usage.add(usage),
                        AgentEvent::Finished {
                            ok, summary, usage, ..
                        } => {
                            outcome.finished_ok = Some(*ok);
                            outcome.summary = summary.clone();
                            if let Some(u) = usage {
                                outcome.usage = u.clone();
                            }
                        }
                        _ => {}
                    }
                    on_event(RunnerEvent::Agent(ev));
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

        if opts.cancel.load(Ordering::SeqCst) && !outcome.stopped {
            outcome.stopped = true;
            tree.kill_tree(Duration::from_secs(2))?;
        } else if opts.timeout.is_some_and(|t| started.elapsed() > t)
            && !outcome.timed_out
            && !outcome.stopped
        {
            outcome.timed_out = true;
            tree.kill_tree(Duration::from_secs(2))?;
        }

        if exited_at.is_none() {
            if let Some(status) = tree.try_wait()? {
                outcome.exit_code = status.code();
                exited_at = Some(Instant::now());
            }
        }
        // 进程已退出：等 stdout 和 stderr 都读完。若有脱离进程树的后代仍占着管道，
        // 则在管道连续 1 秒没有新内容后放弃等待；已排队的输出总会先处理完。
        if exited_at.is_some()
            && ((out_closed && err_closed) || last_msg.elapsed() > Duration::from_secs(1))
        {
            break;
        }
    }

    let n = stderr_lines.len();
    outcome.stderr_tail = stderr_lines[n.saturating_sub(20)..].join("\n");
    outcome.duration_ms = started.elapsed().as_millis() as u64;
    Ok(outcome)
}
