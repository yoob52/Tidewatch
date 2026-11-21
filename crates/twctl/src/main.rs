//! twctl：观澜命令行。M0 技术验证的入口，也随桌面应用分发（演示 Agent）。
mod mock_agent;

use clap::{Parser, Subcommand};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tidewatch_core::agent::{adapter, AgentEvent, AgentKind, PermissionMode, RunSpec};
use tidewatch_core::runner::{run_agent, RunOptions, RunOutcome, RunnerEvent};
use tidewatch_core::{git, provider};

#[derive(Parser)]
#[command(name = "twctl", version, about = "观澜 Tidewatch 命令行")]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(clap::Args, Clone)]
struct AgentOpts {
    /// 模型名
    #[arg(long)]
    model: Option<String>,
    /// read_only / workspace_write / full_auto
    #[arg(long, default_value = "workspace_write")]
    permission: String,
    /// 使用 DeepSeek（Anthropic 兼容端点），密钥从 --api-key-env 指定的环境变量读取
    #[arg(long)]
    deepseek: bool,
    /// 存放 API Key 的环境变量名
    #[arg(long, default_value = "DEEPSEEK_API_KEY")]
    api_key_env: String,
    /// 指定 CLI 可执行文件
    #[arg(long)]
    binary: Option<PathBuf>,
    /// 超时秒数
    #[arg(long)]
    timeout: Option<u64>,
    /// 运行这么多秒后自动停止（演示「中途停止」）
    #[arg(long)]
    stop_after: Option<f64>,
}

#[derive(Subcommand)]
enum Cmd {
    /// 检查本机环境：git、Claude Code、Codex
    Doctor,
    /// 在指定目录运行一次 Agent，实时打印事件；Ctrl-C 停止整棵进程树
    Run {
        /// claude_code / codex / mock
        #[arg(long)]
        agent: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(long)]
        prompt: String,
        /// 续接会话 id
        #[arg(long)]
        resume: Option<String>,
        /// 原始 JSONL 日志路径
        #[arg(long)]
        log: Option<PathBuf>,
        #[command(flatten)]
        opts: AgentOpts,
    },
    /// 只打印将要执行的命令（密钥脱敏），不启动
    Plan {
        #[arg(long)]
        agent: String,
        #[arg(long, default_value = ".")]
        cwd: PathBuf,
        #[arg(long, default_value = "")]
        prompt: String,
        #[arg(long)]
        resume: Option<String>,
        #[command(flatten)]
        opts: AgentOpts,
    },
    /// M0 演示：建 worktree → 各 Agent 依次做小改动（可中途停止、续接）→ 提交 → 看 diff → 清理
    Demo {
        /// 目标仓库
        #[arg(long)]
        repo: PathBuf,
        /// 逗号分隔：claude_code,codex,mock
        #[arg(long, default_value = "claude_code,codex")]
        agents: String,
        /// 演示结束后保留 worktree 与分支
        #[arg(long)]
        keep: bool,
        #[command(flatten)]
        opts: AgentOpts,
    },
    /// 创建 / 删除 worktree
    Worktree {
        #[command(subcommand)]
        cmd: WorktreeCmd,
    },
    /// 内置演示 Agent（按 Claude Code 的事件格式输出）
    MockAgent {
        #[arg(long, default_value = "mock-1")]
        model: String,
        #[arg(long, default_value = "workspace_write")]
        permission: String,
        #[arg(long)]
        resume: Option<String>,
        #[arg(long, env = "TIDEWATCH_MOCK_STEPS", default_value_t = 3)]
        steps: u32,
        #[arg(long, env = "TIDEWATCH_MOCK_DELAY_MS", default_value_t = 400)]
        delay_ms: u64,
    },
}

#[derive(Subcommand)]
enum WorktreeCmd {
    Add {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        branch: String,
        #[arg(long, default_value = "HEAD")]
        base: String,
    },
    Remove {
        #[arg(long)]
        repo: PathBuf,
        #[arg(long)]
        path: PathBuf,
        #[arg(long)]
        force: bool,
    },
    List {
        #[arg(long)]
        repo: PathBuf,
    },
}

type AnyResult<T> = Result<T, Box<dyn std::error::Error>>;

fn main() {
    let cli = Cli::parse();
    let code = match dispatch(cli.cmd) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("错误：{e}");
            1
        }
    };
    std::process::exit(code);
}

fn build_spec(
    agent: AgentKind,
    cwd: &Path,
    prompt: &str,
    resume: Option<String>,
    o: &AgentOpts,
) -> AnyResult<RunSpec> {
    let mut spec = RunSpec {
        prompt: prompt.to_string(),
        cwd: tidewatch_core::util::canonical(cwd)?,
        model: o.model.clone(),
        permission: PermissionMode::parse(&o.permission)?,
        resume_session: resume,
        binary: o.binary.clone(),
        ..Default::default()
    };
    if agent == AgentKind::Mock && spec.binary.is_none() {
        spec.binary = Some(std::env::current_exe()?);
    }
    if o.deepseek {
        let key = std::env::var(&o.api_key_env)
            .map_err(|_| format!("环境变量 {} 未设置", o.api_key_env))?;
        let inj = provider::inject(agent, &provider::deepseek(), Some(&key))?;
        spec.env.extend(inj.env);
        spec.extra_args.extend(inj.args);
        if spec.model.is_none() {
            spec.model = inj.model;
        }
    }
    Ok(spec)
}

fn print_event(ev: &RunnerEvent) {
    let clip = |s: &str| -> String {
        let one = s.replace('\n', " ⏎ ");
        if one.chars().count() > 200 {
            format!("{}…", one.chars().take(200).collect::<String>())
        } else {
            one
        }
    };
    match ev {
        RunnerEvent::Spawned { pid, command } => println!("▶ pid {pid}  {command}"),
        RunnerEvent::Stderr(l) => eprintln!("  stderr │ {l}"),
        RunnerEvent::Agent(a) => match a {
            AgentEvent::SessionStarted { session_id, model } => {
                println!(
                    "● 会话 {session_id}  模型 {}",
                    model.as_deref().unwrap_or("默认")
                )
            }
            AgentEvent::Text { text } => println!("  说 │ {}", clip(text)),
            AgentEvent::Thinking { text } => println!("  想 │ {}", clip(text)),
            AgentEvent::ToolCall { name, input, .. } => {
                println!("  工具 │ {name} {}", clip(&input.to_string()))
            }
            AgentEvent::ToolResult {
                output, is_error, ..
            } => {
                println!(
                    "  {} │ {}",
                    if *is_error { "失败" } else { "结果" },
                    clip(output)
                )
            }
            AgentEvent::Command {
                command,
                exit_code,
                done,
                ..
            } => {
                if *done {
                    println!(
                        "  命令 │ {} → 退出码 {}",
                        clip(command),
                        exit_code.map_or("?".into(), |c| c.to_string())
                    )
                } else {
                    println!("  命令 │ {} …", clip(command))
                }
            }
            AgentEvent::FileChange { path, kind } => println!("  文件 │ {kind} {path}"),
            AgentEvent::Usage { usage } => println!(
                "  用量 │ 入 {} 出 {}",
                usage.input_tokens, usage.output_tokens
            ),
            AgentEvent::Finished { ok, summary, .. } => {
                println!("■ {}：{}", if *ok { "完成" } else { "失败" }, clip(summary))
            }
            AgentEvent::Error { message } => println!("  错误 │ {}", clip(message)),
            AgentEvent::Raw { line } => println!("  原始 │ {}", clip(line)),
        },
    }
}

fn run_once(
    agent: AgentKind,
    spec: &RunSpec,
    log: Option<PathBuf>,
    o: &AgentOpts,
    cancel: Arc<AtomicBool>,
) -> AnyResult<RunOutcome> {
    let opts = RunOptions {
        raw_log: log,
        cancel: cancel.clone(),
        timeout: o.timeout.map(Duration::from_secs),
    };
    if let Some(secs) = o.stop_after {
        let c = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs_f64(secs));
            c.store(true, Ordering::SeqCst);
        });
    }
    let outcome = run_agent(adapter(agent).as_ref(), spec, &opts, &mut |ev| {
        print_event(&ev)
    })?;
    let state = if outcome.stopped {
        "已停止"
    } else if outcome.timed_out {
        "超时"
    } else if outcome.succeeded() {
        "成功"
    } else {
        "失败"
    };
    println!(
        "— {state} · 退出码 {} · {:.1}s · 会话 {} · Token 入 {} / 出 {}{}",
        outcome.exit_code.map_or("无".into(), |c| c.to_string()),
        outcome.duration_ms as f64 / 1000.0,
        outcome.session_id.as_deref().unwrap_or("无"),
        outcome.usage.input_tokens,
        outcome.usage.output_tokens,
        outcome
            .usage
            .cost_usd
            .map(|c| format!(" · ${c:.4}"))
            .unwrap_or_default()
    );
    if let Some(e) = &outcome.raw_log_error {
        eprintln!("警告：原始日志写入失败，日志不完整：{e}");
    }
    if !outcome.succeeded() && !outcome.stopped && !outcome.stderr_tail.is_empty() {
        eprintln!("{}", outcome.stderr_tail);
    }
    Ok(outcome)
}

fn cancel_on_ctrlc() -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let _ = ctrlc::set_handler(move || {
        eprintln!("\n收到 Ctrl-C，正在停止进程树…");
        c.store(true, Ordering::SeqCst);
    });
    cancel
}

fn dispatch(cmd: Cmd) -> AnyResult<i32> {
    match cmd {
        Cmd::Doctor => {
            println!(
                "git        {}",
                git::version().unwrap_or_else(|| "未安装".into())
            );
            for kind in [AgentKind::ClaudeCode, AgentKind::Codex] {
                let d = adapter(kind).detect();
                match (d.installed, d.path, d.version) {
                    (true, Some(p), v) => println!(
                        "{:<10} {}  {}",
                        kind.as_str(),
                        v.unwrap_or_else(|| "版本未知".into()),
                        p
                    ),
                    _ => println!("{:<10} 未安装", kind.as_str()),
                }
            }
            if let Some(hint) = git::long_paths_hint() {
                println!("提示       {hint}");
            }
            Ok(0)
        }
        Cmd::Plan {
            agent,
            cwd,
            prompt,
            resume,
            opts,
        } => {
            let kind = AgentKind::parse(&agent)?;
            let spec = build_spec(kind, &cwd, &prompt, resume, &opts)?;
            println!("{}", adapter(kind).build_command(&spec)?.display());
            Ok(0)
        }
        Cmd::Run {
            agent,
            cwd,
            prompt,
            resume,
            log,
            opts,
        } => {
            let kind = AgentKind::parse(&agent)?;
            let spec = build_spec(kind, &cwd, &prompt, resume, &opts)?;
            let outcome = run_once(kind, &spec, log, &opts, cancel_on_ctrlc())?;
            Ok(if outcome.succeeded() {
                0
            } else if outcome.stopped {
                130
            } else {
                1
            })
        }
        Cmd::Demo {
            repo,
            agents,
            keep,
            opts,
        } => demo(&repo, &agents, keep, &opts),
        Cmd::Worktree { cmd } => {
            match cmd {
                WorktreeCmd::Add {
                    repo,
                    path,
                    branch,
                    base,
                } => {
                    git::worktree_add(&repo, &path, &branch, &base)?;
                    println!("已创建 {}（分支 {branch}）", path.display());
                }
                WorktreeCmd::Remove { repo, path, force } => {
                    git::worktree_remove(&repo, &path, force)?;
                    println!("已删除 {}", path.display());
                }
                WorktreeCmd::List { repo } => {
                    for w in git::worktree_list(&repo)? {
                        println!(
                            "{}  {}  {}",
                            &w.head[..w.head.len().min(8)],
                            w.branch.as_deref().unwrap_or("(detached)"),
                            w.path.display()
                        );
                    }
                }
            }
            Ok(0)
        }
        Cmd::MockAgent {
            model,
            permission,
            resume,
            steps,
            delay_ms,
        } => Ok(mock_agent::run(mock_agent::MockArgs {
            model,
            permission,
            resume,
            steps,
            delay_ms,
        })),
    }
}

/// 同一个 worktree 里让每种 Agent 依次改一点东西：先跑一段后停止，再用会话 id 续接跑完。
fn demo(repo: &Path, agents: &str, keep: bool, o: &AgentOpts) -> AnyResult<i32> {
    let repo = tidewatch_core::util::canonical(repo)?;
    if !git::is_repo(&repo) {
        return Err(format!("{} 不是 git 仓库", repo.display()).into());
    }
    let base = git::current_branch(&repo)?;
    let id = tidewatch_core::util::new_id().to_lowercase();
    let branch = format!("tw/demo-{}", &id[id.len() - 8..]);
    let wt = repo
        .parent()
        .unwrap_or(&repo)
        .join(".tidewatch-demo")
        .join(branch.replace('/', "-"));
    println!(
        "【1】创建 worktree：{}（分支 {branch}，基于 {base}）",
        wt.display()
    );
    git::worktree_add(&repo, &wt, &branch, &base)?;

    let cancel = cancel_on_ctrlc();
    let mut failed = false;
    for (i, name) in agents
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .enumerate()
    {
        let kind = AgentKind::parse(name)?;
        println!("\n【{}】{}：做一个小改动", i + 2, kind.display_name());
        let prompt = format!(
            "在仓库根目录的 TIDEWATCH_DEMO.md 末尾追加一行：「{} 到此一游」。文件不存在就创建。只改这一个文件，不要运行其它命令。",
            kind.display_name()
        );
        let spec = match build_spec(kind, &wt, &prompt, None, o) {
            Ok(s) => s,
            Err(e) => {
                println!("  跳过：{e}");
                failed = true;
                continue;
            }
        };
        let mut first = o.clone();
        if first.stop_after.is_none() && kind == AgentKind::Mock {
            first.stop_after = Some(0.6);
        }
        cancel.store(false, Ordering::SeqCst);
        let outcome = match run_once(kind, &spec, None, &first, cancel.clone()) {
            Ok(out) => out,
            Err(e) => {
                println!("  跳过：{e}");
                failed = true;
                continue;
            }
        };
        if outcome.stopped {
            if let Some(session) = outcome.session_id.clone() {
                println!("  ↻ 已中途停止，用会话 {session} 续接");
                cancel.store(false, Ordering::SeqCst);
                let mut second = o.clone();
                second.stop_after = None;
                let spec = build_spec(kind, &wt, "继续完成刚才的任务。", Some(session), o)?;
                failed |= !run_once(kind, &spec, None, &second, cancel.clone())?.succeeded();
            }
        } else {
            failed |= !outcome.succeeded();
        }
        match git::commit_all(&wt, &format!("demo: {} 的改动", kind.display_name()))? {
            Some(sha) => println!("  ✔ 已提交 {}", &sha[..8]),
            None => println!("  （没有文件改动）"),
        }
    }

    println!("\n【diff】{base}...{branch}");
    for f in git::diff_stat(&repo, &base, Some(&branch))? {
        println!("  {}  +{} -{}", f.path, f.added, f.removed);
    }
    println!("{}", git::diff(&repo, &base, Some(&branch))?);

    if keep {
        println!("\n保留 worktree：{}", wt.display());
    } else {
        git::worktree_remove(&repo, &wt, true)?;
        git::delete_branch(&repo, &branch, true)?;
        println!("\n【清理】已删除 worktree 与分支 {branch}");
    }
    Ok(if failed { 1 } else { 0 })
}
