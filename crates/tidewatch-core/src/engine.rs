//! 执行引擎：排队与并发、每个子任务一个 worktree、运行 Agent 并把事件落库、
//! 收尾（提交、检查点、Diff、项目检查命令）、停止 / 续接、验收与合并。
//!
//! 约定：持有数据库锁时不做 git / 进程这类耗时的外部操作；先把意图写库（Run 入队 +
//! `pending_effect`），再由调度线程取出执行，结果写回。
use crate::agent::{adapter, AgentEvent, AgentKind, PermissionMode, RunSpec};
use crate::db::{lock, Db};
use crate::git::{self, FileStat};
use crate::runner::{run_agent, RunOptions, RunnerEvent};
use crate::secrets::SecretStore;
use crate::store::projects::{self, Project};
use crate::store::providers;
use crate::store::runs::{self, Run, RunEventRow};
use crate::store::settings::{self, Settings};
use crate::store::tasks::{self, Task};
use crate::store::tickets::{self, Ticket};
use crate::store::workflows;
use crate::store::{add_timeline, not_found};
use crate::{workflow, Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// 引擎推给界面的消息。
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum EngineEvent {
    /// 运行日志新增一条。
    RunEvent { event: RunEventRow },
    /// 有数据变了，界面该刷新。
    Changed {
        ticket_id: Option<String>,
        task_id: Option<String>,
        run_id: Option<String>,
    },
    /// 一轮运行结束（用于系统通知）。
    RunFinished {
        run_id: String,
        task_id: String,
        ticket_key: String,
        task_title: String,
        status: String,
        failed_checks: Vec<String>,
    },
    /// AI 预审结束（用于系统通知）。
    ReviewFinished {
        ticket_id: String,
        ticket_key: String,
        ticket_title: String,
        issues: usize,
        blockers: usize,
        error: Option<String>,
    },
    /// AI 交叉审查结束（用于系统通知）。
    CodeReviewFinished {
        ticket_id: String,
        task_id: String,
        ticket_key: String,
        task_title: String,
        findings: usize,
        blockers: usize,
        error: Option<String>,
    },
    /// 拆分建议 / 变更影响分析出来了，等人确认。
    ProposalReady {
        ticket_id: String,
        ticket_key: String,
        ticket_title: String,
        /// split | impact
        kind: String,
        error: Option<String>,
    },
    /// 护栏提醒：Agent 长时间没输出、超出预算、执行了危险命令。
    Guard {
        run_id: String,
        ticket_key: String,
        /// stuck | budget | danger
        kind: String,
        message: String,
        /// 这一轮是否因此被停掉。
        stopped: bool,
    },
    /// Agent 提了一个要人回答的问题。
    QuestionAsked {
        question_id: String,
        ticket_id: String,
        ticket_key: String,
        body: String,
    },
    /// 后端子任务产出了接口文档，等它的子任务解锁。
    ApiDocReady {
        ticket_key: String,
        task_title: String,
        path: String,
        unblocked: usize,
    },
}

pub type Listener = Arc<dyn Fn(EngineEvent) + Send + Sync>;

pub struct EngineConfig {
    /// 应用数据目录：运行日志在 `logs/`，默认工作区在 `workspace/`。
    pub data_dir: PathBuf,
    /// 演示 Agent 的可执行文件（桌面应用传自身路径）。
    pub mock_binary: Option<PathBuf>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StartOptions {
    /// 追加给 Agent 的说明（返工意见等）。
    pub instruction: Option<String>,
    /// 不续接上一轮会话，从头开始。
    pub fresh_session: bool,
    /// 用户在"预览 Prompt"里改过的全文。
    pub prompt_override: Option<String>,
    /// 幂等键：同一个键重复提交只入队一次。
    pub command_id: Option<String>,
    /// 引擎自己发起的"检查失败自动修复"：第几次。界面传不进来。
    #[serde(skip)]
    pub auto_fix: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskDiff {
    pub base: String,
    pub files: Vec<FileStat>,
    pub patch: String,
    /// 补丁太大被截断。
    pub truncated: bool,
}

/// 开 PR 时的标题和正文。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PrInput {
    pub title: String,
    pub body: String,
    pub draft: bool,
}

/// 子任务能不能推：要有分支和改动、不在运行中，项目要配了远程。返回（仓库目录，分支，远程名）。
fn pushable(task: &Task, project: &Project) -> Result<(PathBuf, String, String)> {
    if !matches!(task.status.as_str(), "review" | "accepted") {
        return Err(Error::invalid(format!(
            "子任务处于「{}」，待验收或已通过时才能推送。",
            workflow::task_status_label(&task.status)
        )));
    }
    let repo = PathBuf::from(&project.local_path);
    let branch = task
        .branch
        .clone()
        .filter(|b| git::branch_exists(&repo, b))
        .ok_or_else(|| Error::invalid("子任务还没有分支。"))?;
    let remote = git::default_remote(&repo)?.ok_or_else(|| {
        Error::invalid("项目的仓库没有配置远程（git remote），没有地方可以推送。")
    })?;
    Ok((repo, branch, remote))
}

/// 一轮运行的执行方案：谁来跑、用什么模型、发什么 Prompt。
#[derive(Debug, Clone, Serialize)]
pub struct RunPlan {
    pub agent: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub permission: String,
    /// 续接的会话：(来源 Run, 会话 id)。
    pub resume: Option<(String, String)>,
    pub prompt: String,
    /// 没法续接会话时（换了 Agent、要求新开会话、上一轮没留下会话），附在 Prompt 里的上一轮摘要。
    pub handoff: Option<Handoff>,
}

/// 新会话接手已有工作时的上下文交接。
#[derive(Debug, Clone, Serialize)]
pub struct Handoff {
    pub from_run_id: String,
    pub summary_md: String,
}

/// 打回时写在 Diff 某一行上的评论。
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InlineComment {
    pub file_path: String,
    pub line: Option<i64>,
    pub body: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct RejectInput {
    pub note: String,
    pub comments: Vec<InlineComment>,
    /// 打回后立刻入队返工。
    pub rerun: bool,
}

const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(30 * 60);

struct Inner {
    db: Db,
    secrets: Box<dyn SecretStore>,
    cfg: EngineConfig,
    listener: Listener,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// 运行中 Agent 的追加指令收件箱（只有支持流式输入的 Agent 才有）。
    steers: Mutex<HashMap<String, Arc<crate::runner::SteerInbox>>>,
    /// 排到这轮结束后、以续接会话方式开始下一轮的追加指令。
    followups: Mutex<HashMap<String, Vec<String>>>,
    /// 合并会动项目主检出的索引和 HEAD，同一时间只做一个。
    merging: Mutex<()>,
    /// 本地接口开着时的地址和签发运行令牌用的密钥。
    api: Mutex<Option<ApiEndpoint>>,
    wake: (Mutex<bool>, Condvar),
    shutdown: AtomicBool,
    /// 为真时不派发新的运行、不开始新的 AI 分析（安装更新期间）。
    hold: AtomicBool,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

/// [`Engine::hold`] 的守卫：丢弃时恢复派发。
pub struct Hold(Arc<Inner>);

impl Drop for Hold {
    fn drop(&mut self) {
        self.0.hold.store(false, Ordering::SeqCst);
        self.0.wake();
    }
}

pub struct Engine {
    inner: Arc<Inner>,
    scheduler: Mutex<Option<JoinHandle<()>>>,
    /// 数据目录的独占锁，引擎存在期间一直持有。
    _instance_lock: std::fs::File,
}

impl Engine {
    /// 启动引擎：先把上次没跑完的运行标为孤儿，再开调度线程。
    ///
    /// 一个数据目录同一时间只能有一个引擎：第二个实例会把第一个实例正在跑的运行当成孤儿收掉，
    /// 之后两个 Agent 就可能在同一个 worktree 里同时改文件。所以先拿独占锁，拿不到就不启动。
    pub fn start(
        db: Db,
        secrets: Box<dyn SecretStore>,
        cfg: EngineConfig,
        listener: Listener,
    ) -> Result<Engine> {
        let instance_lock = lock_data_dir(&cfg.data_dir)?;
        runs::recover_orphans(&mut lock(&db))?;
        ticket_flow::recover(&mut lock(&db))?;
        let inner = Arc::new(Inner {
            db,
            secrets,
            cfg,
            listener,
            cancels: Mutex::new(HashMap::new()),
            steers: Mutex::new(HashMap::new()),
            followups: Mutex::new(HashMap::new()),
            merging: Mutex::new(()),
            api: Mutex::new(None),
            wake: (Mutex::new(false), Condvar::new()),
            shutdown: AtomicBool::new(false),
            hold: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        });
        let sched = inner.clone();
        let handle = std::thread::Builder::new()
            .name("tw-scheduler".into())
            .spawn(move || sched.schedule_loop())?;
        Ok(Engine {
            inner,
            scheduler: Mutex::new(Some(handle)),
            _instance_lock: instance_lock,
        })
    }

    pub fn db(&self) -> &Db {
        &self.inner.db
    }

    pub fn secrets(&self) -> &dyn SecretStore {
        self.inner.secrets.as_ref()
    }

    /// 这轮运行会怎么跑（不入队），供"预览 Prompt"。
    pub fn plan(&self, task_id: &str, opts: &StartOptions) -> Result<RunPlan> {
        let conn = lock(&self.inner.db);
        let task = tasks::get(&conn, task_id)?;
        let ticket = tickets::get(&conn, &task.ticket_id)?;
        plan_run(&conn, &task, &ticket, opts)
    }

    /// 让子任务开始一轮运行（入队）。停止 / 失败 / 打回后再次调用即续接上一轮会话。
    pub fn start_task(&self, task_id: &str, opts: StartOptions) -> Result<Run> {
        self.inner.enqueue_task(task_id, opts)
    }

    /// 停止一轮运行：排队中的直接撤掉，运行中的结束整棵进程树后照常收尾。
    pub fn stop_run(&self, run_id: &str) -> Result<()> {
        let run = {
            let conn = lock(&self.inner.db);
            let run = runs::get(&conn, run_id)?;
            match run.status.as_str() {
                "queued" => {
                    runs::set_fields(
                        &conn,
                        run_id,
                        &[("status", &"stopped"), ("finished_at", &crate::util::now())],
                    )?;
                    if let Some(t) = &run.task_id {
                        tasks::set_status(&conn, t, "stopped")?;
                    }
                    set_effect(&conn, run_id, "done")?;
                    if run.task_id.is_none() {
                        drop(conn);
                        self.inner
                            .conclude_ticket_run(&run, Err("在开始前被停止。"), None);
                    }
                }
                "starting" | "running" | "finalizing" | "stopping" => {
                    if let Some(flag) = self.inner.cancels.lock().unwrap().get(run_id) {
                        flag.store(true, Ordering::SeqCst);
                    }
                    conn.execute(
                        "UPDATE run SET status = 'stopping' WHERE id = ?1 AND status IN ('starting', 'running')",
                        [run_id],
                    )?;
                }
                _ => return Err(Error::invalid("这轮运行已经结束。")),
            }
            run
        };
        self.inner.stop_delegated(run_id);
        self.inner.changed(run.ticket_id, run.task_id, Some(run.id));
        Ok(())
    }

    /// 验收通过。
    pub fn accept_task(&self, task_id: &str) -> Result<Task> {
        let conn = lock(&self.inner.db);
        let task = tasks::get(&conn, task_id)?;
        if task.status != "review" {
            return Err(Error::invalid(format!(
                "子任务处于「{}」，只有待验收的可以通过。",
                workflow::task_status_label(&task.status)
            )));
        }
        if tasks::comparing(&conn, &task)? {
            return Err(Error::invalid(
                "这个子任务还在和别的 Agent 对比：先「采用这个结果」，再验收通过。",
            ));
        }
        tasks::set_status(&conn, task_id, "accepted")?;
        add_timeline(
            &conn,
            &task.ticket_id,
            "task_accepted",
            json!({ "task_id": task_id }),
        )?;
        drop(conn);
        self.inner
            .changed(Some(task.ticket_id.clone()), Some(task_id.into()), None);
        tasks::get(&lock(&self.inner.db), task_id)
    }

    /// 打回：记下意见，子任务变为"已打回"，工单回到开发中。之后用 [`Engine::start_task`] 带意见返工。
    pub fn reject_task(&self, task_id: &str, note: &str) -> Result<Task> {
        self.reject_task_with(
            task_id,
            RejectInput {
                note: note.into(),
                ..Default::default()
            },
        )
    }

    /// 打回，可以带 Diff 上的行内评论；`rerun` 时立刻入队返工（优先续接原会话）。
    pub fn reject_task_with(&self, task_id: &str, input: RejectInput) -> Result<Task> {
        let mut conn = lock(&self.inner.db);
        let task = tasks::get(&conn, task_id)?;
        if !matches!(task.status.as_str(), "review" | "accepted") {
            return Err(Error::invalid(format!(
                "子任务处于「{}」，不能打回。",
                workflow::task_status_label(&task.status)
            )));
        }
        let note = input.note.trim();
        let comments: Vec<&InlineComment> = input
            .comments
            .iter()
            .filter(|c| !c.body.trim().is_empty())
            .collect();
        if note.is_empty() && comments.is_empty() {
            return Err(Error::invalid("请写明打回的原因，Agent 返工时会看到它。"));
        }
        // Agent 看到的返工意见：总体意见 + 每条行内评论
        let mut instruction = note.to_string();
        if !comments.is_empty() {
            if !instruction.is_empty() {
                instruction.push_str("\n\n");
            }
            instruction.push_str("行内评论：");
            for c in &comments {
                let at = match c.line {
                    Some(l) => format!("{}:{l}", c.file_path),
                    None => c.file_path.clone(),
                };
                instruction.push_str(&format!("\n- `{at}`：{}", c.body.trim()));
            }
        }
        let batch = crate::util::new_id();
        let last_run: Option<String> = runs::list_for_task(&conn, task_id)?
            .last()
            .map(|r| r.id.clone());
        let tx = conn.transaction()?;
        for c in &comments {
            tx.execute(
                "INSERT INTO comment (id, ticket_id, task_id, run_id, kind, file_path, line, body_md, created_at, batch_id)
                 VALUES (?1, ?2, ?3, ?4, 'inline', ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    crate::util::new_id(),
                    task.ticket_id,
                    task_id,
                    last_run,
                    c.file_path,
                    c.line,
                    c.body.trim(),
                    crate::util::now(),
                    batch
                ],
            )?;
        }
        tasks::set_status(&tx, task_id, "changes_requested")?;
        // 依赖它的子任务回到"等依赖"，返工完成前不能开始
        tasks::refresh_blocked(&tx, &task.ticket_id)?;
        add_timeline(
            &tx,
            &task.ticket_id,
            "task_rejected",
            json!({ "task_id": task_id, "note": note, "instruction": instruction, "batch_id": batch, "comments": comments.len() }),
        )?;
        tx.commit()?;
        if tickets::get(&conn, &task.ticket_id)?.status == "acceptance" {
            tickets::transition(&mut conn, &task.ticket_id, "developing")?;
        }
        drop(conn);
        self.inner
            .changed(Some(task.ticket_id.clone()), Some(task_id.into()), None);
        if input.rerun {
            self.inner.enqueue_task(task_id, StartOptions::default())?;
        }
        tasks::get(&lock(&self.inner.db), task_id)
    }

    /// 把已通过的子任务合并进项目的目标分支，然后清理 worktree。
    pub fn merge_task(&self, task_id: &str) -> Result<Task> {
        let _merging = self.inner.merging.lock().unwrap_or_else(|e| e.into_inner());
        let (task, ticket, project, st) = self.load(task_id)?;
        if task.status != "accepted" {
            return Err(Error::invalid(format!(
                "子任务处于「{}」，验收通过后才能合并。",
                workflow::task_status_label(&task.status)
            )));
        }
        if crate::store::code_reviews::running_for_task(&lock(&self.inner.db), task_id)? {
            return Err(Error::invalid(
                "交叉审查还在这个子任务的 worktree 里进行，等它结束或停掉后再合并。",
            ));
        }
        if tasks::comparing(&lock(&self.inner.db), &task)? {
            return Err(Error::invalid(
                "这个子任务还在和别的 Agent 对比：先「采用这个结果」，再合并。",
            ));
        }
        let branch = task
            .branch
            .clone()
            .ok_or_else(|| Error::invalid("子任务还没有分支。"))?;
        let repo = PathBuf::from(&project.local_path);
        let current = git::current_branch(&repo)?;
        if current != project.default_branch {
            return Err(Error::invalid(format!(
                "项目目录当前在分支 {current}，而合并目标是 {}。请先在项目目录切换到 {} 再合并。",
                project.default_branch, project.default_branch
            )));
        }
        let worktree = task
            .worktree_path
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| p.exists());
        if let Some(wt) = &worktree {
            // 验收期间在 worktree 里手动改的内容一并带上
            git::commit_all(wt, &format!("tw({}): 验收期间的手动修改", ticket.key))?;
        }
        let message = format!("{}: {}（{}）", ticket.key, ticket.title, task.title);
        let squash = st.merge_mode == "squash";
        let commit = git::merge(&repo, &branch, squash, &message)?;
        let mut cleanup_note = None;
        if let Some(wt) = &worktree {
            if let Err(e) = git::worktree_remove(&repo, wt, true) {
                cleanup_note = Some(format!("worktree 没能删除：{e}"));
            }
        }
        if st.delete_branch_after_merge && cleanup_note.is_none() {
            if let Err(e) = git::delete_branch(&repo, &branch, squash) {
                cleanup_note = Some(format!("分支没能删除：{e}"));
            }
        }
        {
            let mut conn = lock(&self.inner.db);
            tasks::set_merged(&conn, task_id, &commit)?;
            add_timeline(
                &conn,
                &ticket.id,
                "task_merged",
                json!({ "task_id": task_id, "commit": commit, "branch": branch, "note": cleanup_note }),
            )?;
            tasks::refresh_blocked(&conn, &ticket.id)?;
            let statuses: Vec<String> = tasks::list_for_ticket(&conn, &ticket.id)?
                .into_iter()
                .map(|t| t.status)
                .collect();
            if statuses
                .iter()
                .all(|s| matches!(s.as_str(), "merged" | "cancelled"))
            {
                let now = tickets::get(&conn, &ticket.id)?.status;
                if now == "developing" {
                    tickets::transition(&mut conn, &ticket.id, "acceptance")?;
                }
                // 自定义工作流还有环节没过、或者关掉了自动完成：停在验收，等人来
                let ticket = tickets::get(&conn, &ticket.id)?;
                if matches!(now.as_str(), "developing" | "acceptance")
                    && workflows::can_auto_done(&conn, &ticket)?
                {
                    tickets::transition(&mut conn, &ticket.id, "done")?;
                }
            }
        }
        self.inner
            .changed(Some(ticket.id), Some(task_id.into()), None);
        tasks::get(&lock(&self.inner.db), task_id)
    }

    /// 取消子任务：保留分支，删除 worktree（未提交的改动先提交到分支上）。
    pub fn cancel_task(&self, task_id: &str) -> Result<Task> {
        // 校验和改状态在同一次加锁里完成：之后再点"开始"会被拒绝，
        // 不会出现这边删 worktree、那边新一轮的 Agent 正在里面写文件的情况。
        let (task, ticket, project) = {
            let conn = lock(&self.inner.db);
            let task = tasks::get(&conn, task_id)?;
            if matches!(task.status.as_str(), "queued" | "running") {
                return Err(Error::invalid("子任务在排队或运行中，请先停止。"));
            }
            if matches!(task.status.as_str(), "merged" | "cancelled") {
                return Err(Error::invalid("子任务已经结束。"));
            }
            let ticket = tickets::get(&conn, &task.ticket_id)?;
            let project = projects::get(&conn, &task.project_id)?;
            tasks::set_status(&conn, task_id, "cancelled")?;
            (task, ticket, project)
        };
        let removed: Result<()> = (|| {
            if let Some(wt) = task
                .worktree_path
                .as_ref()
                .map(PathBuf::from)
                .filter(|p| p.exists())
            {
                git::commit_all(&wt, &format!("tw({}): 取消前的未提交改动", ticket.key))?;
                git::worktree_remove(Path::new(&project.local_path), &wt, true)?;
            }
            Ok(())
        })();
        if let Err(e) = removed {
            // worktree 没删成：子任务回到原来的状态
            tasks::set_status(&lock(&self.inner.db), task_id, &task.status)?;
            return Err(e);
        }
        {
            let conn = lock(&self.inner.db);
            conn.execute(
                "UPDATE task SET worktree_path = NULL WHERE id = ?1",
                [task_id],
            )?;
            add_timeline(
                &conn,
                &ticket.id,
                "task_cancelled",
                json!({ "task_id": task_id }),
            )?;
            tasks::refresh_blocked(&conn, &ticket.id)?;
        }
        self.inner
            .changed(Some(ticket.id), Some(task_id.into()), None);
        tasks::get(&lock(&self.inner.db), task_id)
    }

    /// 子任务相对起点的全部改动。worktree 还在时包含未提交的修改（不含未跟踪的新文件）。
    pub fn task_diff(&self, task_id: &str) -> Result<TaskDiff> {
        let (task, _, project, _) = self.load(task_id)?;
        let repo = PathBuf::from(&project.local_path);
        let worktree = task
            .worktree_path
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| p.exists());
        let (dir, base, head): (PathBuf, String, Option<String>) = match (
            &task.merged_commit,
            worktree,
            &task.base_commit,
            &task.branch,
        ) {
            // 合并提交相对它的第一父提交，就是这次合并带进来的内容
            (Some(m), _, _, _) => (repo, format!("{m}^1"), Some(m.clone())),
            (None, Some(wt), Some(base), _) => (wt, base.clone(), None),
            (None, None, Some(base), Some(branch)) if git::branch_exists(&repo, branch) => {
                (repo, base.clone(), Some(branch.clone()))
            }
            _ => {
                return Ok(TaskDiff {
                    base: String::new(),
                    files: vec![],
                    patch: String::new(),
                    truncated: false,
                })
            }
        };
        let files = git::diff_stat(&dir, &base, head.as_deref())?;
        let mut patch = git::diff(&dir, &base, head.as_deref())?;
        let truncated = patch.len() > MAX_PATCH_BYTES;
        if truncated {
            let mut cut = MAX_PATCH_BYTES;
            while !patch.is_char_boundary(cut) {
                cut -= 1;
            }
            patch.truncate(cut);
        }
        Ok(TaskDiff {
            base,
            files,
            patch,
            truncated,
        })
    }

    /// 把子任务的分支推到远程。只在用户点了之后做，观澜自己从不推送。
    pub fn push_task(&self, task_id: &str) -> Result<Task> {
        let (task, _, project, _) = self.load(task_id)?;
        let (repo, branch, remote) = pushable(&task, &project)?;
        let head = git::rev_parse(&repo, &format!("refs/heads/{branch}"))?;
        git::push_branch(&repo, &remote, &branch)?;
        {
            let conn = lock(&self.inner.db);
            tasks::set_pushed(&conn, task_id, &head)?;
            add_timeline(
                &conn,
                &task.ticket_id,
                "task_pushed",
                json!({ "task_id": task_id, "remote": remote, "branch": branch, "commit": head }),
            )?;
        }
        self.inner
            .changed(Some(task.ticket_id.clone()), Some(task_id.into()), None);
        tasks::get(&lock(&self.inner.db), task_id)
    }

    /// 用 `gh pr create` 给子任务的分支开 PR；分支还没推（或推过之后又有新提交）就先推。
    pub fn create_pr(&self, task_id: &str, input: PrInput) -> Result<Task> {
        let (task, ticket, project, _) = self.load(task_id)?;
        let (repo, branch, _) = pushable(&task, &project)?;
        if git::gh_binary().is_none() {
            return Err(Error::invalid(
                "没有找到 GitHub CLI（gh）。安装并运行 gh auth login 之后再开 PR。",
            ));
        }
        let head = git::rev_parse(&repo, &format!("refs/heads/{branch}"))?;
        let task = if task.pushed_commit.as_deref() == Some(head.as_str()) {
            task
        } else {
            self.push_task(task_id)?
        };
        let title = Some(input.title.trim())
            .filter(|t| !t.is_empty())
            .map(String::from)
            .unwrap_or_else(|| format!("{} {}", ticket.key, task.title));
        let url = git::gh_pr_create(
            &repo,
            &branch,
            &project.default_branch,
            &title,
            input.body.trim(),
            input.draft,
        )?;
        {
            let conn = lock(&self.inner.db);
            tasks::set_pr_url(&conn, task_id, &url)?;
            add_timeline(
                &conn,
                &task.ticket_id,
                "pr_created",
                json!({ "task_id": task_id, "url": url }),
            )?;
        }
        self.inner
            .changed(Some(task.ticket_id.clone()), Some(task_id.into()), None);
        tasks::get(&lock(&self.inner.db), task_id)
    }

    /// 开 PR 对话框里预填的标题和正文。
    pub fn pr_draft(&self, task_id: &str) -> Result<PrInput> {
        let (task, ticket, _, _) = self.load(task_id)?;
        let conn = lock(&self.inner.db);
        let last = runs::list_for_task(&conn, task_id)?.pop();
        let criteria = if task.acceptance_criteria.is_empty() {
            &ticket.acceptance_criteria
        } else {
            &task.acceptance_criteria
        };
        let mut body = format!("## {} {}\n\n", ticket.key, ticket.title);
        if let Some(s) = last
            .as_ref()
            .and_then(|r| r.summary.as_deref())
            .filter(|s| !s.trim().is_empty())
        {
            body.push_str(&format!("### 改动说明\n{}\n\n", s.trim()));
        }
        if !criteria.is_empty() {
            body.push_str("### 验收标准\n");
            for c in criteria {
                body.push_str(&format!("- [x] {c}\n"));
            }
            body.push('\n');
        }
        if let Some(r) = &last {
            let reports = runs::test_reports(&conn, &r.id)?;
            if !reports.is_empty() {
                body.push_str("### 自动检查\n");
                for t in reports {
                    body.push_str(&format!(
                        "- `{}`：{}\n",
                        t.command,
                        if t.exit_code == Some(0) {
                            "通过"
                        } else {
                            "没有通过"
                        }
                    ));
                }
            }
        }
        Ok(PrInput {
            title: format!("{} {}", ticket.key, task.title),
            body,
            draft: false,
        })
    }

    fn load(&self, task_id: &str) -> Result<(Task, Ticket, Project, Settings)> {
        let conn = lock(&self.inner.db);
        let task = tasks::get(&conn, task_id)?;
        let ticket = tickets::get(&conn, &task.ticket_id)?;
        let project = projects::get(&conn, &task.project_id)?;
        Ok((task, ticket, project, settings::get(&conn)?))
    }

    /// 暂停派发：安装更新前调用，免得下载期间又有运行开始、随后被退出的进程留成孤儿。
    /// 有运行或 AI 分析在进行时拒绝。排队中的运行留在队列里；守卫丢弃后恢复派发。
    pub fn hold(&self) -> Result<Hold> {
        let conn = lock(&self.inner.db);
        let busy: i64 = conn.query_row(
            "SELECT COUNT(*) FROM run WHERE status IN ('starting', 'running', 'finalizing', 'stopping')",
            [],
            |r| r.get(0),
        )?;
        if busy > 0 {
            return Err(Error::invalid(format!(
                "还有 {busy} 个运行在进行。请等它们结束或先停止，再安装更新。"
            )));
        }
        // 和 dispatch、smart_analyze 一样在这两把锁里改标记，它们就不会在检查之后又开始
        let cancels = self.inner.cancels.lock().unwrap();
        if !cancels.is_empty() {
            return Err(Error::invalid(
                "还有 Agent 调用（运行或 AI 分析）没有结束。请等它结束或先取消，再安装更新。",
            ));
        }
        if self.inner.hold.swap(true, Ordering::SeqCst) {
            return Err(Error::invalid("更新已经在进行中。"));
        }
        Ok(Hold(self.inner.clone()))
    }

    /// 停掉所有运行并等线程退出。
    pub fn shutdown(&self) {
        self.inner.shutdown.store(true, Ordering::SeqCst);
        for flag in self.inner.cancels.lock().unwrap().values() {
            flag.store(true, Ordering::SeqCst);
        }
        self.inner.wake();
        if let Some(h) = self.scheduler.lock().unwrap().take() {
            let _ = h.join();
        }
        let workers: Vec<_> = std::mem::take(&mut *self.inner.workers.lock().unwrap());
        for h in workers {
            let _ = h.join();
        }
    }
}

impl Drop for Engine {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn set_effect(conn: &rusqlite::Connection, run_id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE pending_effect SET status = ?2, attempts = attempts + (?2 = 'running'), updated_at = ?3 WHERE id = ?1",
        rusqlite::params![run_id, status, crate::util::now()],
    )?;
    Ok(())
}

/// 按 子任务 > 工单 > 项目 > 全局 的顺序取第一个有值的。
fn first<'a>(candidates: impl IntoIterator<Item = &'a Option<String>>) -> Option<String> {
    candidates
        .into_iter()
        .find_map(|c| c.clone().filter(|s| !s.trim().is_empty()))
}

fn last_rejection_note(
    conn: &rusqlite::Connection,
    ticket_id: &str,
    task_id: &str,
) -> Result<Option<String>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT COALESCE(json_extract(payload_json, '$.instruction'), json_extract(payload_json, '$.note'))
                 FROM timeline_event
             WHERE ticket_id = ?1 AND type = 'task_rejected'
               AND json_extract(payload_json, '$.task_id') = ?2
             ORDER BY ts DESC, rowid DESC LIMIT 1",
            [ticket_id, task_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

fn plan_run(
    conn: &rusqlite::Connection,
    task: &Task,
    ticket: &Ticket,
    opts: &StartOptions,
) -> Result<RunPlan> {
    let project = projects::get(conn, &task.project_id)?;
    let st = settings::get(conn)?;
    let agent = first([&task.agent, &ticket.default_agent, &project.default_agent])
        .unwrap_or(st.default_agent.clone());
    let kind = AgentKind::parse(&agent)?;
    let provider_id = first([
        &task.provider_id,
        &ticket.default_provider_id,
        &project.default_provider_id,
        &st.default_provider_id,
    ]);
    let provider = provider_id
        .as_deref()
        .map(|id| providers::get(conn, id))
        .transpose()?;
    if let Some(p) = &provider {
        // 协议不匹配现在就报，不等排到了才失败
        crate::provider::inject(kind, &p.config()?, None)?;
    }
    let model =
        first([&task.model]).or_else(|| provider.as_ref().and_then(|p| p.default_model.clone()));
    let permission = first([&task.permission_profile]).unwrap_or(st.default_permission.clone());
    PermissionMode::parse(&permission)?;

    let resume = if opts.fresh_session {
        None
    } else {
        runs::last_session(conn, &task.id, kind.as_str())?
    };
    // 被打回的子任务：没有另给指令时，把最近一次打回意见交给 Agent
    let rejection = if task.status == "changes_requested" {
        last_rejection_note(conn, &task.ticket_id, &task.id)?
    } else {
        None
    };
    let instruction = opts
        .instruction
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or(rejection.as_deref());
    let ov = crate::prompt::load_overrides(conn)?;
    // 没有会话可续接、但这个子任务已经跑过：新会话要知道上一轮做了什么
    let handoff = if resume.is_none() {
        handoff_from_last_run(conn, task)?
    } else {
        None
    };
    let prompt = match (&opts.prompt_override, &resume, instruction) {
        (Some(p), _, _) if !p.trim().is_empty() => p.clone(),
        (_, Some(_), Some(i)) if opts.auto_fix.is_some() => {
            crate::prompt::test_fix_prompt(&ov, &project, ticket, opts.auto_fix.unwrap_or(1), i)
        }
        (_, Some(_), Some(i)) => crate::prompt::rework_prompt(&ov, &project, ticket, i),
        (_, Some(_), None) => {
            "请从上次停下的地方继续，把任务完成；最后用简短中文总结做了什么。".to_string()
        }
        (_, None, i) => {
            let prd = tickets::current_prd(conn, &ticket.id)?
                .map(|v| v.content_md)
                .unwrap_or_default();
            let api_doc = api_docs_for(conn, task)?;
            let mut p =
                crate::prompt::dev_prompt(&ov, &project, ticket, task, &prd, api_doc.as_deref());
            if let Some(h) = &handoff {
                p.push_str(&format!("\n## 上一轮的情况\n{}\n", h.summary_md));
            }
            if let Some(i) = i {
                p.push_str(&format!("\n## 补充说明\n{i}\n"));
            }
            p
        }
    };
    // 续接的会话只见过它开始时的正文。Bug 工单在开发 / 验收中还能改正文（比如换了截图），
    // 这之后改过的话把现行版本附上，否则 Agent 会照着旧的做
    let custom = opts
        .prompt_override
        .as_deref()
        .is_some_and(|p| !p.trim().is_empty());
    let prompt = match (&resume, prd_changed_since_last_run(conn, task)?) {
        (Some(_), Some(prd)) if !custom => format!(
            "{prompt}\n\n## 工单正文已更新\n上一轮之后工单正文有改动，下面是现行版本（v{}），以它为准：\n\n{}\n",
            prd.version, prd.content_md
        ),
        _ => prompt,
    };
    Ok(RunPlan {
        agent: kind.as_str().into(),
        provider_id,
        model,
        permission,
        resume,
        prompt,
        handoff,
    })
}

/// 这个子任务上一轮开始之后，工单正文有没有出新版本；有就返回现行版本。
fn prd_changed_since_last_run(
    conn: &rusqlite::Connection,
    task: &Task,
) -> Result<Option<tickets::PrdVersion>> {
    let last: Option<String> = conn.query_row(
        "SELECT MAX(started_at) FROM run WHERE task_id = ?1",
        [&task.id],
        |r| r.get(0),
    )?;
    let Some(last) = last else {
        return Ok(None);
    };
    Ok(tickets::current_prd(conn, &task.ticket_id)?.filter(|v| v.created_at > last))
}

/// 上一轮运行的摘要，给接手的新会话看。
fn handoff_from_last_run(conn: &rusqlite::Connection, task: &Task) -> Result<Option<Handoff>> {
    let Some(last) = runs::list_for_task(conn, &task.id)?
        .into_iter()
        .rfind(|r| r.agent_finished_at.is_some())
    else {
        return Ok(None);
    };
    let mut md = format!(
        "这个子任务之前已经运行过（第 {} 轮，Agent：{}，结果：{}），当前 worktree 里保留着之前已提交的改动。\n请先用 `git log` 和 `git status` 了解现状，在此基础上继续，不要从头重做。\n",
        last.number, last.agent, last.status
    );
    if let Some(summary) = last.summary.as_deref().filter(|s| !s.trim().is_empty()) {
        md.push_str("\n上一轮结束时的总结：\n");
        for line in summary.trim().lines() {
            md.push_str(&format!("> {line}\n"));
        }
    }
    if !last.diff_stat.is_empty() {
        let files: Vec<&str> = last.diff_stat.iter().map(|f| f.path.as_str()).collect();
        md.push_str(&format!("\n已经改动的文件：{}\n", files.join("、")));
    }
    Ok(Some(Handoff {
        from_run_id: last.id,
        summary_md: md,
    }))
}

/// 这个子任务按现在的配置会以 full_auto 运行：每次开始都要用户确认，不能由系统自动入队。
pub(crate) fn needs_confirmation(task: &Task, default_permission: &str) -> bool {
    first([&task.permission_profile])
        .as_deref()
        .unwrap_or(default_permission)
        == "full_auto"
}

const MAX_API_DOC_BYTES: usize = 60 * 1024;

/// 依赖的子任务产出的接口文档全文（前端子任务的 Prompt 用）。文档还在对方的 worktree 里就读文件，
/// 否则从含有它的提交里取。读的都是本地小文件，所以允许在持有数据库锁时做。
fn api_docs_for(conn: &rusqlite::Connection, task: &Task) -> Result<Option<String>> {
    let mut out = String::new();
    for dep_id in &task.depends_on {
        let Some(a) = tasks::artifact(conn, dep_id, "api_doc")? else {
            continue;
        };
        let dep = tasks::get(conn, dep_id)?;
        let from_worktree = dep
            .worktree_path
            .as_deref()
            .and_then(|w| std::fs::read_to_string(Path::new(w).join(&a.path)).ok());
        let text = match (from_worktree, &a.commit_sha) {
            (Some(t), _) => Some(t),
            (None, Some(commit)) => {
                let repo = projects::get(conn, &dep.project_id)?.local_path;
                git::git(Path::new(&repo), &["show", &format!("{commit}:{}", a.path)]).ok()
            }
            (None, None) => None,
        };
        let Some(mut text) = text.filter(|t| !t.trim().is_empty()) else {
            continue;
        };
        if text.len() > MAX_API_DOC_BYTES {
            let mut cut = MAX_API_DOC_BYTES;
            while !text.is_char_boundary(cut) {
                cut -= 1;
            }
            text.truncate(cut);
            text.push_str("\n\n（文档太长，后面的部分省略了）\n");
        }
        out.push_str(&format!(
            "来自后端子任务「{}」（`{}`）：\n\n{}\n",
            dep.title,
            a.path,
            text.trim()
        ));
    }
    Ok(Some(out).filter(|o| !o.is_empty()))
}

/// worktree 与分支的名字：`<工单编号>-<类型>`，同一工单下同类型的第二个起加序号。
fn workspace_name(ticket_key: &str, task: &Task, siblings: &[Task]) -> String {
    let index = siblings
        .iter()
        .filter(|t| t.kind == task.kind)
        .position(|t| t.id == task.id)
        .unwrap_or(0);
    if index == 0 {
        format!("{ticket_key}-{}", task.kind)
    } else {
        format!("{ticket_key}-{}-{}", task.kind, index + 1)
    }
}

/// 目录名里只留 ASCII 字母数字和 `-_`，避免路径问题；全被替换掉时退回 id 尾部。
fn slug(name: &str, id: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    let s = s.trim_matches('-').to_string();
    if s.is_empty() {
        id.chars()
            .rev()
            .take(8)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>()
            .to_lowercase()
    } else {
        s
    }
}

#[path = "engine_ticket.rs"]
mod ticket_flow;
pub use ticket_flow::{CodeReviewOptions, Finalized};
#[path = "engine_api.rs"]
mod api_flow;
#[path = "engine_compare.rs"]
mod compare_flow;
#[path = "engine_followup.rs"]
mod followup_flow;
#[path = "engine_smart.rs"]
mod smart_flow;
pub use api_flow::{ApiEndpoint, DelegateInput, RUN_TOKEN_ENV};
pub use followup_flow::{FollowupMode, FollowupResult};
pub use smart_flow::{apply_analysis, SmartAnalysis};

struct Finished {
    status: &'static str,
    error: Option<String>,
    failed_checks: Vec<String>,
    /// 没过的检查的说明（命令、退出码、输出末尾），自动修复时交给 Agent。
    failures_md: String,
}

impl Inner {
    /// 子任务入队一轮运行。
    pub(crate) fn enqueue_task(&self, task_id: &str, opts: StartOptions) -> Result<Run> {
        let run = {
            let mut conn = lock(&self.db);
            if let Some(cid) = &opts.command_id {
                let prior: Option<String> = conn
                    .query_row(
                        "SELECT result_json FROM command_receipt WHERE command_id = ?1",
                        [cid],
                        |r| r.get(0),
                    )
                    .ok();
                if let Some(run_id) = prior {
                    return runs::get(&conn, &run_id);
                }
            }
            let task = tasks::get(&conn, task_id)?;
            workflow::check_task_can_start(&task.status)?;
            if let Some(dep) = tasks::unmet_dependency(&conn, &task)? {
                return Err(Error::invalid(format!(
                    "依赖的子任务「{}」还没完成（现在是「{}」），等它完成后再开始。",
                    dep.title,
                    workflow::task_status_label(&dep.status)
                )));
            }
            let ticket = tickets::get(&conn, &task.ticket_id)?;
            let plan = plan_run(&conn, &task, &ticket, &opts)?;
            if ticket.status == "finalized" {
                ticket_flow::check_no_open_proposal(&conn, &ticket.id)?;
            }
            match ticket.status.as_str() {
                "developing" => {}
                "acceptance" | "finalized" => {
                    tickets::transition(&mut conn, &ticket.id, "developing")?;
                }
                "draft" if ticket.kind == "bug" => {
                    tickets::transition(&mut conn, &ticket.id, "developing")?;
                }
                other => {
                    return Err(Error::invalid(format!(
                        "工单处于「{}」，还不能开始开发。",
                        workflow::ticket_status_label(other)
                    )))
                }
            }
            let tx = conn.transaction()?;
            let run = runs::enqueue(
                &tx,
                runs::NewRun {
                    task_id: Some(&task.id),
                    ticket_id: &ticket.id,
                    purpose: if opts.auto_fix.is_some() {
                        "test_fix"
                    } else if plan.resume.is_some()
                        || opts.instruction.is_some()
                        || task.status == "changes_requested"
                    {
                        "rework"
                    } else {
                        "dev"
                    },
                    parent_run_id: plan.resume.as_ref().map(|(r, _)| r.as_str()),
                    agent: &plan.agent,
                    provider_id: plan.provider_id.as_deref(),
                    model: plan.model.as_deref(),
                    permission: &plan.permission,
                    resume_session_id: plan.resume.as_ref().map(|(_, s)| s.as_str()),
                    prompt: &plan.prompt,
                },
            )?;
            tasks::set_status(&tx, &task.id, "queued")?;
            tasks::set_start_when_ready(&tx, &task.id, false)?;
            tasks::refresh_blocked(&tx, &ticket.id)?;
            if let Some(h) = &plan.handoff {
                tx.execute(
                    "INSERT INTO context_handoff (id, task_id, target_run_id, from_run_ids_json, strategy, summary_md,
                     created_by, status, created_at)
                     VALUES (?1, ?2, ?3, ?4, 'full_summary', ?5, 'system', 'ready', ?6)",
                    rusqlite::params![
                        crate::util::new_id(),
                        task.id,
                        run.id,
                        json!([h.from_run_id]).to_string(),
                        h.summary_md,
                        crate::util::now()
                    ],
                )?;
            }
            let now = crate::util::now();
            tx.execute(
                "INSERT INTO pending_effect (id, kind, payload_json, status, attempts, created_at, updated_at)
                 VALUES (?1, 'run', ?2, 'pending', 0, ?3, ?3)",
                rusqlite::params![run.id, json!({ "run_id": run.id }).to_string(), now],
            )?;
            if let Some(cid) = &opts.command_id {
                tx.execute(
                    "INSERT INTO command_receipt (command_id, result_json, created_at) VALUES (?1, ?2, ?3)",
                    rusqlite::params![cid, run.id, now],
                )?;
            }
            add_timeline(
                &tx,
                &ticket.id,
                "run_queued",
                json!({ "task_id": task.id, "run_id": run.id, "number": run.number, "agent": run.agent }),
            )?;
            tx.commit()?;
            run
        };
        self.changed(
            run.ticket_id.clone(),
            run.task_id.clone(),
            Some(run.id.clone()),
        );
        self.wake();
        Ok(run)
    }

    fn emit(&self, ev: EngineEvent) {
        (self.listener)(ev);
    }

    fn changed(&self, ticket_id: Option<String>, task_id: Option<String>, run_id: Option<String>) {
        self.emit(EngineEvent::Changed {
            ticket_id,
            task_id,
            run_id,
        });
    }

    fn wake(&self) {
        *self.wake.0.lock().unwrap() = true;
        self.wake.1.notify_all();
    }

    fn schedule_loop(self: Arc<Self>) {
        while !self.shutdown.load(Ordering::SeqCst) {
            if let Err(e) = self.dispatch() {
                eprintln!("调度失败：{e}");
            }
            let guard = self.wake.0.lock().unwrap();
            let (mut guard, _) = self
                .wake
                .1
                .wait_timeout_while(guard, Duration::from_millis(500), |woken| !*woken)
                .unwrap();
            *guard = false;
        }
    }

    /// 在并发上限内，把排队的 Run 逐个交给工作线程。
    fn dispatch(self: &Arc<Self>) -> Result<()> {
        loop {
            let (run, cancel) = {
                let conn = lock(&self.db);
                if self.hold.load(Ordering::SeqCst) {
                    return Ok(());
                }
                let max = settings::get(&conn)?.max_concurrency;
                // 委派出去的运行不占并发名额：调用方正占着名额等它，否则并发上限为 1 时会互相等死
                let next = if runs::count_running(&conn)? >= max {
                    runs::next_queued_delegate(&conn)?
                } else {
                    runs::next_queued(&conn)?
                };
                let Some(run) = next else {
                    return Ok(());
                };
                // 取消标记要在持有数据库锁时登记：stop_run 也是先拿这把锁再查标记，
                // 否则"已是 starting、标记还没登记"的空档里点停止会被漏掉。
                let cancel = Arc::new(AtomicBool::new(false));
                self.cancels
                    .lock()
                    .unwrap()
                    .insert(run.id.clone(), cancel.clone());
                runs::set_fields(
                    &conn,
                    &run.id,
                    &[("status", &"starting"), ("started_at", &crate::util::now())],
                )?;
                if let Some(t) = &run.task_id {
                    tasks::set_status(&conn, t, "running")?;
                }
                set_effect(&conn, &run.id, "running")?;
                (run, cancel)
            };
            self.changed(
                run.ticket_id.clone(),
                run.task_id.clone(),
                Some(run.id.clone()),
            );
            let me = self.clone();
            let handle = std::thread::Builder::new()
                .name(format!("tw-run-{}", run.id))
                .spawn(move || {
                    me.execute(&run, &cancel);
                    me.start_followups(&run);
                    // 调用方结束了，它委派出去还没跑完的也不用再跑
                    me.stop_delegated(&run.id);
                    me.cancels.lock().unwrap().remove(&run.id);
                    me.wake();
                })?;
            let mut workers = self.workers.lock().unwrap();
            workers.retain(|h| !h.is_finished());
            workers.push(handle);
        }
    }

    fn log(&self, run_id: &str, kind: &str, payload: serde_json::Value) {
        let row = runs::add_event(&lock(&self.db), run_id, kind, &payload);
        if let Ok(event) = row {
            self.emit(EngineEvent::RunEvent { event });
        }
    }

    /// 跑完一轮并把最终状态写回。任何一步出错都记为失败，不让 Run 卡在中间状态。
    fn execute(&self, run: &Run, cancel: &AtomicBool) {
        if run.task_id.is_none() {
            return self.execute_ticket_run(run, cancel);
        }
        let task_id = run.task_id.clone().unwrap_or_default();
        let fin = match self.execute_steps(run, &task_id, cancel) {
            Ok(f) => f,
            Err(e) => Finished {
                status: if cancel.load(Ordering::SeqCst) {
                    "stopped"
                } else {
                    "failed"
                },
                error: Some(e.to_string()),
                failed_checks: vec![],
                failures_md: String::new(),
            },
        };
        // 检查没过：还有自动修复的次数就不交给人，让 Agent 接着修
        let auto_fix = if fin.status == "succeeded"
            && !self.has_followups(&run.id)
            && !fin.failed_checks.is_empty()
            && !cancel.load(Ordering::SeqCst)
        {
            let conn = lock(&self.db);
            let limit = settings::get(&conn)
                .map(|s| s.auto_fix_attempts)
                .unwrap_or(0);
            let done = runs::list_for_task(&conn, &task_id)
                .map(|all| {
                    all.iter()
                        .rev()
                        .take_while(|r| r.purpose == "test_fix")
                        .count() as u32
                })
                .unwrap_or(u32::MAX);
            (done < limit).then_some(done + 1)
        } else {
            None
        };
        if let Some(e) = &fin.error {
            self.log(&run.id, "error", json!({ "message": e }));
        }
        let task_status = match fin.status {
            "succeeded" if auto_fix.is_some() => "changes_requested",
            "succeeded" => "review",
            "stopped" => "stopped",
            _ => "failed",
        };
        let mut ticket_key = String::new();
        let mut task_title = String::new();
        let mut unblocked = vec![];
        let res: Result<()> = (|| {
            let mut conn = lock(&self.db);
            runs::set_fields(
                &conn,
                &run.id,
                &[
                    ("status", &fin.status),
                    ("finished_at", &crate::util::now()),
                    ("error", &fin.error),
                ],
            )?;
            set_effect(
                &conn,
                &run.id,
                if fin.status == "failed" {
                    "failed"
                } else {
                    "done"
                },
            )?;
            let task = tasks::get(&conn, &task_id)?;
            task_title = task.title.clone();
            tasks::set_status(&conn, &task_id, task_status)?;
            let ticket = tickets::get(&conn, &task.ticket_id)?;
            ticket_key = ticket.key.clone();
            add_timeline(
                &conn,
                &ticket.id,
                "run_finished",
                json!({ "task_id": task_id, "run_id": run.id, "number": run.number, "status": fin.status, "failed_checks": fin.failed_checks }),
            )?;
            unblocked = tasks::refresh_blocked(&conn, &ticket.id)?;
            let all = tasks::list_for_ticket(&conn, &ticket.id)?;
            if ticket.status == "developing"
                && workflow::all_tasks_ready_for_acceptance(all.iter().map(|t| t.status.as_str()))
            {
                tickets::transition(&mut conn, &ticket.id, "acceptance")?;
            }
            Ok(())
        })();
        if let Err(e) = res {
            eprintln!("写回运行结果失败：{e}");
        }
        self.log(&run.id, "status", json!({ "status": fin.status }));
        self.changed(
            run.ticket_id.clone(),
            run.task_id.clone(),
            Some(run.id.clone()),
        );
        let mut fixing = false;
        if let Some(attempt) = auto_fix {
            let started = self.enqueue_task(
                &task_id,
                StartOptions {
                    instruction: Some(fin.failures_md.clone()),
                    auto_fix: Some(attempt),
                    ..Default::default()
                },
            );
            match started {
                Ok(next) => {
                    fixing = true;
                    if let Some(t) = &run.ticket_id {
                        let _ = add_timeline(
                            &lock(&self.db),
                            t,
                            "auto_fix",
                            json!({ "task_id": task_id, "run_id": next.id, "attempt": attempt, "failed_checks": fin.failed_checks }),
                        );
                    }
                    self.log(
                        &run.id,
                        "auto_fix",
                        json!({ "attempt": attempt, "run_id": next.id, "failed_checks": fin.failed_checks }),
                    )
                }
                Err(e) => {
                    // 起不来就照常交给人验收
                    self.log(
                        &run.id,
                        "error",
                        json!({ "message": format!("没能发起自动修复：{e}") }),
                    );
                    let conn = lock(&self.db);
                    let _ = tasks::set_status(&conn, &task_id, "review");
                    if let Some(t) = &run.ticket_id {
                        unblocked = tasks::refresh_blocked(&conn, t).unwrap_or_default();
                    }
                    drop(conn);
                    self.changed(
                        run.ticket_id.clone(),
                        run.task_id.clone(),
                        Some(run.id.clone()),
                    );
                }
            }
        }
        if (task_status == "review" || (auto_fix.is_some() && !fixing))
            && !self.has_followups(&run.id)
            && settings::get(&lock(&self.db)).is_ok_and(|s| s.auto_cross_review)
        {
            if let Err(e) = self.start_code_review(&task_id, &Default::default()) {
                self.log(
                    &run.id,
                    "error",
                    json!({ "message": format!("没能发起交叉审查：{e}") }),
                );
            }
        }
        self.auto_start(&unblocked);
        if let Some(ticket_id) = &run.ticket_id {
            self.start_deferred(ticket_id);
        }
        // 已经交回 Agent 自动修复：还没到要人看的时候，不发"等你验收"的通知
        if fixing {
            return;
        }
        self.emit(EngineEvent::RunFinished {
            run_id: run.id.clone(),
            task_id,
            ticket_key,
            task_title,
            status: fin.status.into(),
            failed_checks: fin.failed_checks,
        });
    }

    fn execute_steps(&self, run: &Run, task_id: &str, cancel: &AtomicBool) -> Result<Finished> {
        let (task, ticket, project, st, provider, siblings) = {
            let conn = lock(&self.db);
            let task = tasks::get(&conn, task_id).map_err(|_| not_found("子任务", task_id))?;
            let ticket = tickets::get(&conn, &task.ticket_id)?;
            let project = projects::get(&conn, &task.project_id)?;
            let provider = run
                .provider_id
                .as_deref()
                .map(|id| providers::get(&conn, id))
                .transpose()?;
            let siblings = tasks::list_for_ticket(&conn, &ticket.id)?;
            (
                task,
                ticket,
                project,
                settings::get(&conn)?,
                provider,
                siblings,
            )
        };
        let kind = AgentKind::parse(&run.agent)?;
        let repo = PathBuf::from(&project.local_path);

        // 1. worktree
        let (worktree, base) =
            self.ensure_worktree(run, &task, &ticket, &project, &st, &siblings, cancel)?;
        let start_commit = git::rev_parse(&worktree, "HEAD")?;
        git::save_checkpoint(
            &repo,
            &format!("{}/{}-start", task.id, run.id),
            &start_commit,
        )?;
        runs::set_fields(&lock(&self.db), &run.id, &[("start_commit", &start_commit)])?;

        // 2. Agent
        let logs = self.cfg.data_dir.join("logs");
        // 后端子任务：Agent 一写出接口文档就解锁等它的子任务，不用等整轮跑完
        let doc_rel = format!("docs/api/{}.md", ticket.key);
        let doc_file = worktree.join(&doc_rel);
        let mut doc_known = task.kind != "backend"
            || tasks::artifact(&lock(&self.db), &task.id, "api_doc")?.is_some();
        // 返工时 worktree 里可能留着上一版文档（需求变更后已作废）：内容变了才算新文档
        let doc_before = if doc_known {
            None
        } else {
            std::fs::read(&doc_file).ok()
        };
        let mut last_check = Instant::now();
        let mut last_size = 0u64;
        let mut watch_doc = || {
            if doc_known || last_check.elapsed() < Duration::from_millis(500) {
                return;
            }
            last_check = Instant::now();
            let size = std::fs::metadata(&doc_file).map(|m| m.len()).unwrap_or(0);
            // 连续两次大小不变才算写完
            if size > 0
                && size == last_size
                && (doc_before.is_none() || std::fs::read(&doc_file).ok() != doc_before)
            {
                doc_known = true;
                self.api_doc_ready(&task, &ticket, &doc_rel, None);
            }
            last_size = size;
        };
        let outcome = self.agent_phase(
            run,
            kind,
            &worktree,
            &project,
            &st,
            &provider,
            cancel,
            &mut watch_doc,
        )?;
        let agent_ok = outcome.succeeded();

        let message = format!("tw({}): {} · 第 {} 轮", ticket.key, task.title, run.number);
        if let Some(commit) = git::commit_all(&worktree, &message)? {
            self.log(
                &run.id,
                "commit",
                json!({ "commit": commit, "message": message }),
            );
        }
        let end_commit = git::rev_parse(&worktree, "HEAD")?;
        git::save_checkpoint(&repo, &format!("{}/{}-end", task.id, run.id), &end_commit)?;
        // 返工没成功、文档也没动过：留着的还是作废的那一版，不能当成新文档放行依赖方
        let doc_current = agent_ok || std::fs::read(&doc_file).ok() != doc_before;
        if task.kind == "backend" && doc_file.is_file() && doc_current {
            // 提交之后记下含有文档的提交：worktree 清理后还能从提交里取到
            self.api_doc_ready(&task, &ticket, &doc_rel, Some(&end_commit));
        }
        let stat = git::diff_stat(&worktree, &base, Some("HEAD"))?;
        runs::set_fields(
            &lock(&self.db),
            &run.id,
            &[
                ("end_commit", &end_commit),
                ("diff_stat_json", &serde_json::to_string(&stat)?),
            ],
        )?;

        let mut failed_checks = vec![];
        let mut failures_md = String::new();
        if agent_ok && st.auto_test {
            for (name, command) in project.commands.checks() {
                if cancel.load(Ordering::SeqCst) {
                    break;
                }
                self.log(
                    &run.id,
                    "check_started",
                    json!({ "name": name, "command": command }),
                );
                let out = logs.join(format!("{}-{name}.log", run.id));
                let res = run_shell(&command, &worktree, &out, cancel, CHECK_TIMEOUT)?;
                runs::add_test_report(
                    &lock(&self.db),
                    &run.id,
                    &task.id,
                    &command,
                    res.exit_code.map(i64::from),
                    Some(&out.display().to_string()),
                    res.duration_ms as i64,
                )?;
                let ok = res.exit_code == Some(0);
                if !ok && !res.cancelled {
                    failed_checks.push(name.to_string());
                    failures_md.push_str(&format!(
                        "### {name}：`{command}`（{}）\n```\n{}\n```\n\n",
                        match (res.timed_out, res.exit_code) {
                            (true, _) => "超时".to_string(),
                            (_, Some(c)) => format!("退出码 {c}"),
                            _ => "没有正常退出".to_string(),
                        },
                        res.tail.trim()
                    ));
                }
                self.log(
                    &run.id,
                    "check_finished",
                    json!({ "name": name, "command": command, "exit_code": res.exit_code, "duration_ms": res.duration_ms, "timed_out": res.timed_out, "tail": res.tail }),
                );
            }
        }

        let (status, error) = if outcome.stopped || cancel.load(Ordering::SeqCst) {
            ("stopped", None)
        } else if outcome.timed_out {
            (
                "failed",
                Some(format!(
                    "超过单次运行时限（{} 分钟），已停止。",
                    st.run_timeout_minutes
                )),
            )
        } else if outcome.aborted {
            (
                "failed",
                Some(
                    outcome
                        .abort_reason
                        .clone()
                        .unwrap_or_else(|| "被护栏停止。".into()),
                ),
            )
        } else if !agent_ok {
            let detail = [outcome.summary.trim(), outcome.stderr_tail.trim()]
                .into_iter()
                .find(|s| !s.is_empty())
                .unwrap_or("没有输出");
            (
                "failed",
                Some(format!(
                    "Agent 没有正常完成（退出码 {:?}）：{detail}",
                    outcome.exit_code
                )),
            )
        } else {
            ("succeeded", None)
        };
        Ok(Finished {
            status,
            error,
            failed_checks,
            failures_md,
        })
    }

    /// 接口文档出现（或已提交）：记为产物，解锁只等文档的子任务。
    fn api_doc_ready(&self, task: &Task, ticket: &Ticket, path: &str, commit: Option<&str>) {
        let res: Result<Option<Vec<String>>> = (|| {
            let conn = lock(&self.db);
            let first = tasks::artifact(&conn, &task.id, "api_doc")?.is_none();
            tasks::record_artifact(&conn, &task.id, "api_doc", path, commit)?;
            if !first {
                return Ok(None);
            }
            let unblocked = tasks::refresh_blocked(&conn, &ticket.id)?;
            add_timeline(
                &conn,
                &ticket.id,
                "api_doc_ready",
                json!({ "task_id": task.id, "path": path, "unblocked": unblocked }),
            )?;
            Ok(Some(unblocked))
        })();
        match res {
            Ok(Some(unblocked)) => {
                self.changed(Some(ticket.id.clone()), Some(task.id.clone()), None);
                self.emit(EngineEvent::ApiDocReady {
                    ticket_key: ticket.key.clone(),
                    task_title: task.title.clone(),
                    path: path.into(),
                    unblocked: unblocked.len(),
                });
                self.auto_start(&unblocked);
                self.start_deferred(&ticket.id);
            }
            Ok(None) => {}
            Err(e) => eprintln!("记录接口文档失败：{e}"),
        }
    }

    /// 刚解锁的子任务自动入队（设置里可以关）。
    pub(crate) fn auto_start(&self, task_ids: &[String]) {
        if task_ids.is_empty() {
            return;
        }
        let on = settings::get(&lock(&self.db))
            .map(|s| s.auto_start_unblocked)
            .unwrap_or(false);
        if !on {
            return;
        }
        for id in task_ids {
            let startable = (|| -> Result<bool> {
                let conn = lock(&self.db);
                let task = tasks::get(&conn, id)?;
                // full_auto 每次开始都要用户确认，不自动入队
                let confirm = needs_confirmation(&task, &settings::get(&conn)?.default_permission);
                Ok(!confirm && tickets::get(&conn, &task.ticket_id)?.status == "developing")
            })()
            .unwrap_or(false);
            if !startable {
                continue;
            }
            if let Err(e) = self.enqueue_task(id, StartOptions::default()) {
                eprintln!("自动开始子任务 {id} 失败：{e}");
            }
        }
    }

    /// "开始开发"时依赖还没满足的返工子任务：依赖满足了就入队。
    /// 这是用户点过"开始开发"的延续，不受"依赖满足后自动开始"开关影响。
    pub(crate) fn start_deferred(&self, ticket_id: &str) {
        let ready = (|| -> Result<Vec<String>> {
            let conn = lock(&self.db);
            if tickets::get(&conn, ticket_id)?.status != "developing" {
                return Ok(vec![]);
            }
            // 等的时候权限被改成 full_auto 的不再自动入队，要用户自己点开始并确认
            let default_permission = settings::get(&conn)?.default_permission;
            let mut ready = vec![];
            for id in tasks::ready_deferred(&conn, ticket_id)? {
                if !needs_confirmation(&tasks::get(&conn, &id)?, &default_permission) {
                    ready.push(id);
                }
            }
            Ok(ready)
        })()
        .unwrap_or_default();
        for id in ready {
            if let Err(e) = self.enqueue_task(&id, StartOptions::default()) {
                eprintln!("自动开始子任务 {id} 失败：{e}");
            }
        }
    }

    /// 在 `worktree` 里启动 Agent 并等它结束：事件写库并推给界面，用量、费用、摘要记到 Run 上，
    /// Run 进入 finalizing。子任务运行和工单级运行（预审 / 拆分 / 影响分析）共用。
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn agent_phase(
        &self,
        run: &Run,
        kind: AgentKind,
        worktree: &Path,
        project: &Project,
        st: &Settings,
        provider: &Option<providers::Provider>,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(),
    ) -> Result<crate::runner::RunOutcome> {
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::invalid("在 Agent 启动前被停止。"));
        }
        let logs = self.cfg.data_dir.join("logs");
        std::fs::create_dir_all(&logs)?;
        let raw_log = logs.join(format!("{}.jsonl", run.id));
        // PRD 里的图片：复制到 worktree 之外，Prompt 里换成路径并加图片清单
        let images = {
            let conn = lock(&self.db);
            let workspace = st.workspace(&self.cfg.data_dir);
            // Agent 之前提的问题有了回答：附在这一轮 Prompt 的末尾
            let answered = match (&run.task_id, &run.ticket_id) {
                (Some(task), Some(ticket)) => {
                    crate::store::questions::take_answers(&conn, ticket, Some(task), &run.id)?
                }
                _ => vec![],
            };
            let prompt = format!(
                "{}{}",
                run.prompt,
                crate::store::questions::answers_md(&answered)
            );
            crate::store::attachments::prepare_prompt(
                &conn,
                &workspace,
                &prompt,
                &crate::store::attachments::PrepareOptions {
                    dir: &workspace.join("tmp").join(&run.id).join("images"),
                    text_only: provider.as_ref().is_some_and(|p| p.text_only),
                    skip_superseded: run.task_id.is_some() && !st.send_superseded_images,
                    review: run.purpose == "review",
                },
            )?
        };
        if images.prompt != run.prompt {
            // 记下 Agent 实际看到的 Prompt 和图片，便于复盘
            runs::set_fields(&lock(&self.db), &run.id, &[("prompt", &images.prompt)])?;
            self.log(
                &run.id,
                "images",
                json!({
                    "files": images.files.iter().map(|f| f.display().to_string()).collect::<Vec<_>>(),
                    "missing": images.missing,
                }),
            );
        }
        let mut spec = RunSpec {
            prompt: images.prompt,
            images: images.files,
            cwd: worktree.to_path_buf(),
            model: run.model.clone(),
            permission: PermissionMode::parse(
                run.permission.as_deref().unwrap_or("workspace_write"),
            )?,
            resume_session: run.resume_session_id.clone(),
            allowed_tools: project
                .commands
                .checks()
                .into_iter()
                .map(|(_, c)| format!("Bash({c})"))
                .collect(),
            binary: if kind == AgentKind::Mock {
                self.cfg.mock_binary.clone()
            } else {
                None
            },
            ..Default::default()
        };
        spec.interactive = adapter(kind).steer_message("").is_some();
        if let Some(p) = &provider {
            let secret = match &p.secret_ref {
                Some(name) => self.secrets.get(name)?,
                None => None,
            };
            let inj = crate::provider::inject(kind, &p.config()?, secret.as_deref())?;
            spec.env = inj.env;
            spec.extra_args = inj.args;
            if spec.model.is_none() {
                spec.model = inj.model;
            }
        }
        if let Some((mcp, token)) = self.mcp_for(run, kind) {
            spec.env.push((RUN_TOKEN_ENV.into(), token));
            spec.mcp = Some(mcp);
        }
        let attempt = {
            let conn = lock(&self.db);
            let reason = if run.resume_session_id.is_some() {
                "retry"
            } else {
                "initial"
            };
            runs::set_fields(
                &conn,
                &run.id,
                &[("raw_log_path", &raw_log.display().to_string())],
            )?;
            runs::start_attempt(&conn, &run.id, reason, Some(&raw_log.display().to_string()))?
        };
        let opts = RunOptions {
            raw_log: Some(raw_log),
            cancel: self
                .cancels
                .lock()
                .unwrap()
                .get(&run.id)
                .cloned()
                .unwrap_or_default(),
            timeout: (st.run_timeout_minutes > 0)
                .then(|| Duration::from_secs(st.run_timeout_minutes as u64 * 60)),
            abort: Arc::new(AtomicBool::new(false)),
            steer: spec.interactive.then(|| {
                let inbox = Arc::new(crate::runner::SteerInbox::default());
                self.steers
                    .lock()
                    .unwrap()
                    .insert(run.id.clone(), inbox.clone());
                inbox
            }),
        };
        let steer_inbox = opts.steer.clone();
        let abort = opts.abort.clone();
        // 护栏：预算、卡住、危险命令。停下时的原因留给收尾写进 Run
        let pricing = provider.as_ref().and_then(|p| p.pricing.clone());
        let mut spent = crate::agent::Usage::default();
        let mut aborted: Option<String> = None;
        let mut stuck_reported = false;
        let ticket_key = run
            .ticket_id
            .as_deref()
            .and_then(|id| tickets::get(&lock(&self.db), id).ok())
            .map(|t| t.key)
            .unwrap_or_default();
        let guard = |kind: &str, message: String, stop: bool, aborted: &mut Option<String>| {
            self.log(
                &run.id,
                "guard",
                json!({ "kind": kind, "message": message, "stopped": stop }),
            );
            self.emit(EngineEvent::Guard {
                run_id: run.id.clone(),
                ticket_key: ticket_key.clone(),
                kind: kind.into(),
                message: message.clone(),
                stopped: stop,
            });
            if stop && aborted.is_none() {
                *aborted = Some(message);
                abort.store(true, Ordering::SeqCst);
            }
        };
        let outcome = run_agent(adapter(kind).as_ref(), &spec, &opts, &mut |ev| match ev {
            RunnerEvent::Spawned { pid, command } => {
                {
                    let conn = lock(&self.db);
                    let _ = runs::set_fields(&conn, &run.id, &[("command_json", &command)]);
                    let _ = conn.execute(
                        "UPDATE run SET status = 'running' WHERE id = ?1 AND status = 'starting'",
                        [&run.id],
                    );
                }
                self.log(
                    &run.id,
                    "spawned",
                    json!({ "pid": pid, "command": command }),
                );
                self.changed(
                    run.ticket_id.clone(),
                    run.task_id.clone(),
                    Some(run.id.clone()),
                );
            }
            RunnerEvent::Idle(idle) => {
                let limit = Duration::from_secs(st.stuck_minutes as u64 * 60);
                if st.stuck_minutes > 0 && idle >= limit && !stuck_reported {
                    stuck_reported = true;
                    let stop = st.stuck_action == "stop";
                    guard(
                        "stuck",
                        format!(
                            "Agent 已经 {} 分钟没有任何输出，可能卡住了{}。",
                            st.stuck_minutes,
                            if stop { "，已停止这一轮" } else { "" }
                        ),
                        stop,
                        &mut aborted,
                    );
                }
            }
            RunnerEvent::Agent(ev) => {
                on_event();
                stuck_reported = false;
                match &ev {
                    AgentEvent::Usage { usage } => spent.add(usage),
                    AgentEvent::Finished { usage: Some(u), .. } => spent = u.clone(),
                    _ => {}
                }
                if aborted.is_none() {
                    let tokens = spent.input_tokens + spent.output_tokens;
                    let cost = spent
                        .cost_usd
                        .filter(|c| *c > 0.0)
                        .or_else(|| pricing.as_ref().map(|p| p.estimate(&spent)));
                    if st.budget_tokens_per_run > 0 && tokens > st.budget_tokens_per_run {
                        guard(
                            "budget",
                            format!(
                                "这一轮已用 {tokens} Token，超过上限 {}，已停止。",
                                st.budget_tokens_per_run
                            ),
                            true,
                            &mut aborted,
                        );
                    } else if let Some(c) =
                        cost.filter(|c| st.budget_usd_per_run > 0.0 && *c > st.budget_usd_per_run)
                    {
                        guard(
                            "budget",
                            format!(
                                "这一轮已花费约 ${c:.2}，超过上限 ${:.2}，已停止。",
                                st.budget_usd_per_run
                            ),
                            true,
                            &mut aborted,
                        );
                    }
                }
                let command = match &ev {
                    AgentEvent::ToolCall { name, input, .. } => {
                        crate::guard::command_of(name, input)
                    }
                    // 只在命令开始时报一次
                    AgentEvent::Command { command, done, .. } if !*done => Some(command.clone()),
                    _ => None,
                };
                let danger = command.as_deref().and_then(crate::guard::danger);
                if let AgentEvent::SessionStarted { session_id, .. } = &ev {
                    // 立刻记下会话 id：即使观澜中途退出，下一轮也能续接
                    let _ = runs::set_fields(
                        &lock(&self.db),
                        &run.id,
                        &[("agent_session_id", session_id)],
                    );
                }
                let mut payload = serde_json::to_value(&ev).unwrap_or_default();
                if let (Some(reason), Some(obj)) = (danger, payload.as_object_mut()) {
                    obj.insert("danger".into(), json!(reason));
                }
                let kind = payload
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("raw")
                    .to_string();
                self.log(&run.id, &kind, payload);
                if let Some(reason) = danger {
                    let stop = st.stop_on_danger;
                    guard(
                        "danger",
                        format!(
                            "危险命令：{reason}{}\n{}",
                            if stop { "，已停止这一轮" } else { "" },
                            command.unwrap_or_default()
                        ),
                        stop,
                        &mut aborted,
                    );
                }
            }
            RunnerEvent::Stderr(line) => self.log(&run.id, "stderr", json!({ "text": line })),
            RunnerEvent::Steered(text) => self.log(&run.id, "steer", json!({ "text": text })),
        });
        if let Some(inbox) = steer_inbox {
            self.steers.lock().unwrap().remove(&run.id);
            for text in inbox.take_left() {
                self.queue_followup(&run.id, &text, "queue");
            }
        }
        // 这一轮用的图片副本不再需要；原图在 attachments/ 里
        let _ = std::fs::remove_dir_all(st.workspace(&self.cfg.data_dir).join("tmp").join(&run.id));
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => {
                let _ = runs::finish_attempt(&lock(&self.db), &attempt, "failed", None);
                return Err(e);
            }
        };

        // 3. 收尾：Agent 进程结束 ≠ Run 结束
        let mut outcome = outcome;
        if outcome.aborted {
            outcome.abort_reason = aborted;
        }
        let agent_ok = outcome.succeeded();
        let cost = match (
            outcome.usage.cost_usd,
            provider.as_ref().and_then(|p| p.pricing.clone()),
        ) {
            (Some(c), _) if c > 0.0 || provider.is_none() => Some((c, "reported")),
            (_, Some(pricing)) => Some((pricing.estimate(&outcome.usage), "estimated")),
            (Some(c), None) => Some((c, "reported")),
            (None, None) => None,
        };
        {
            let conn = lock(&self.db);
            let attempt_status = if outcome.stopped {
                "stopped"
            } else if agent_ok {
                "succeeded"
            } else {
                "failed"
            };
            runs::finish_attempt(
                &conn,
                &attempt,
                attempt_status,
                outcome.session_id.as_deref(),
            )?;
            let session = outcome.session_id.clone().or(run.resume_session_id.clone());
            runs::set_fields(
                &conn,
                &run.id,
                &[
                    ("status", &"finalizing"),
                    ("agent_finished_at", &crate::util::now()),
                    ("exit_code", &outcome.exit_code),
                    ("agent_session_id", &session),
                    ("input_tokens", &(outcome.usage.input_tokens as i64)),
                    ("output_tokens", &(outcome.usage.output_tokens as i64)),
                    ("cache_tokens", &(outcome.usage.cached_tokens as i64)),
                    ("cost_usd", &cost.map(|c| c.0)),
                    ("cost_source", &cost.map(|c| c.1)),
                    ("summary", &outcome.summary),
                ],
            )?;
        }
        self.log(&run.id, "status", json!({ "status": "finalizing" }));
        self.changed(
            run.ticket_id.clone(),
            run.task_id.clone(),
            Some(run.id.clone()),
        );
        if let Some(e) = &outcome.raw_log_error {
            self.log(
                &run.id,
                "error",
                json!({ "message": format!("原始日志没有写全：{e}") }),
            );
        }
        Ok(outcome)
    }

    /// 取得子任务的 worktree；第一次运行时创建分支与 worktree，并执行项目的准备命令。
    /// 返回 (worktree 路径, 起点提交)。
    #[allow(clippy::too_many_arguments)]
    fn ensure_worktree(
        &self,
        run: &Run,
        task: &Task,
        ticket: &Ticket,
        project: &Project,
        st: &Settings,
        siblings: &[Task],
        cancel: &AtomicBool,
    ) -> Result<(PathBuf, String)> {
        let repo = PathBuf::from(&project.local_path);
        if let (Some(path), Some(base)) = (&task.worktree_path, &task.base_commit) {
            let path = PathBuf::from(path);
            if path.exists() {
                return Ok((path, base.clone()));
            }
        }
        let name = workspace_name(&ticket.key, task, siblings);
        let branch = task.branch.clone().unwrap_or_else(|| format!("tw/{name}"));
        let path = st
            .workspace(&self.cfg.data_dir)
            .join("worktrees")
            .join(slug(&project.name, &project.id))
            .join(&name);
        let base = match &task.base_commit {
            Some(b) => b.clone(),
            None => git::rev_parse(&repo, &project.default_branch)?,
        };
        // 目录被手动删掉过的话，先让 git 忘掉旧登记
        let _ = git::git(&repo, &["worktree", "prune"]);
        git::worktree_add(&repo, &path, &branch, &base)?;
        let path = crate::util::canonical(&path)?;
        tasks::set_workspace(
            &lock(&self.db),
            &task.id,
            &branch,
            &path.display().to_string(),
            &base,
        )?;
        self.log(
            &run.id,
            "worktree",
            json!({ "path": path.display().to_string(), "branch": branch, "base": base }),
        );
        if let Some(setup) = &project.commands.setup {
            if let Err(e) = self.run_setup(run, setup, &path, cancel) {
                // 准备没做完的 worktree 不留：重试时重新创建并再跑一遍准备命令。
                // 此时 Agent 还没运行过，目录里没有要保留的改动。
                let _ = git::worktree_remove(&repo, &path, true);
                let _ = lock(&self.db).execute(
                    "UPDATE task SET worktree_path = NULL WHERE id = ?1",
                    [&task.id],
                );
                return Err(e);
            }
        }
        Ok((path, base))
    }

    fn run_setup(&self, run: &Run, setup: &str, path: &Path, cancel: &AtomicBool) -> Result<()> {
        self.log(
            &run.id,
            "check_started",
            json!({ "name": "setup", "command": setup }),
        );
        let out = self
            .cfg
            .data_dir
            .join("logs")
            .join(format!("{}-setup.log", run.id));
        let res = run_shell(setup, path, &out, cancel, CHECK_TIMEOUT)?;
        self.log(
            &run.id,
            "check_finished",
            json!({ "name": "setup", "command": setup, "exit_code": res.exit_code, "duration_ms": res.duration_ms, "timed_out": res.timed_out, "tail": res.tail }),
        );
        if res.exit_code != Some(0) {
            return Err(Error::invalid(format!(
                "准备命令失败（退出码 {:?}）：{setup}\n{}",
                res.exit_code, res.tail
            )));
        }
        Ok(())
    }
}

pub struct ShellResult {
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub timed_out: bool,
    pub cancelled: bool,
    /// 输出的最后一段，方便直接看到失败原因。
    pub tail: String,
}

/// 对数据目录加独占锁（锁文件 `engine.lock`）。进程退出或引擎销毁时由系统释放。
fn lock_data_dir(data_dir: &Path) -> Result<std::fs::File> {
    std::fs::create_dir_all(data_dir)?;
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(data_dir.join("engine.lock"))?;
    match file.try_lock() {
        Ok(()) => Ok(file),
        Err(std::fs::TryLockError::WouldBlock) => Err(Error::invalid(format!(
            "观澜已经在运行：数据目录 {} 正被另一个实例使用。请切换到已打开的窗口，或先退出它。",
            data_dir.display()
        ))),
        Err(std::fs::TryLockError::Error(e)) => Err(e.into()),
    }
}

/// 在 `cwd` 里用系统 shell 执行一条命令，输出（stdout + stderr）写到 `log`。
/// 只继承系统变量：启动观澜的 shell 里的其它变量（各种密钥）不传给项目脚本。
pub fn run_shell(
    command: &str,
    cwd: &Path,
    log: &Path,
    cancel: &AtomicBool,
    timeout: Duration,
) -> Result<ShellResult> {
    use std::io::{Read, Write};
    if let Some(dir) = log.parent() {
        std::fs::create_dir_all(dir)?;
    }
    #[cfg(windows)]
    let mut cmd = {
        use std::os::windows::process::CommandExt;
        let mut c = std::process::Command::new("cmd");
        c.arg("/S").arg("/C").raw_arg(format!("\"{command}\""));
        c
    };
    #[cfg(not(windows))]
    let mut cmd = {
        let mut c = std::process::Command::new("sh");
        c.arg("-c").arg(command);
        c
    };
    cmd.current_dir(cwd).env_clear();
    for (k, v) in std::env::vars_os() {
        if k.to_str().is_some_and(crate::runner::is_system_env) {
            cmd.env(k, v);
        }
    }
    let started = Instant::now();
    let mut tree = crate::process::spawn(cmd)?;
    drop(tree.take_stdin());
    let file = Arc::new(Mutex::new(std::fs::File::create(log)?));
    let mut readers = vec![];
    let streams: [Option<Box<dyn Read + Send>>; 2] = [
        tree.take_stdout()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
        tree.take_stderr()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    ];
    for mut stream in streams.into_iter().flatten() {
        let file = file.clone();
        readers.push(std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            while let Ok(n) = stream.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let _ = file.lock().unwrap().write_all(&buf[..n]);
            }
        }));
    }
    let mut timed_out = false;
    let mut cancelled = false;
    let status = loop {
        if let Some(s) = tree.try_wait()? {
            break Some(s);
        }
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
        } else if started.elapsed() > timeout {
            timed_out = true;
        }
        if cancelled || timed_out {
            tree.kill_tree(Duration::from_millis(500))?;
            break None;
        }
        std::thread::sleep(Duration::from_millis(30));
    };
    // 命令留下的后台进程还占着管道，不结束它们读线程退不出来
    tree.kill_leftovers();
    for r in readers {
        let _ = r.join();
    }
    drop(file);
    let bytes = std::fs::read(log).unwrap_or_default();
    let text = String::from_utf8_lossy(&bytes[bytes.len().saturating_sub(2000)..]).to_string();
    Ok(ShellResult {
        exit_code: status.and_then(|s| s.code()),
        duration_ms: started.elapsed().as_millis() as u64,
        timed_out,
        cancelled,
        tail: text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs_are_path_safe() {
        assert_eq!(slug("shop-api", "01ABC"), "shop-api");
        assert_eq!(slug("商城 api", "01ABC"), "api");
        assert_eq!(slug("商城", "01HZX3ABCDEFGH"), "abcdefgh");
    }

    #[test]
    fn shell_captures_output_and_exit_code() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("out.log");
        let cancel = AtomicBool::new(false);
        let ok = run_shell(
            "echo hello",
            dir.path(),
            &log,
            &cancel,
            Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(ok.exit_code, Some(0));
        assert!(ok.tail.contains("hello"));
        let bad = run_shell("exit 3", dir.path(), &log, &cancel, Duration::from_secs(20)).unwrap();
        assert_eq!(bad.exit_code, Some(3));
    }

    #[test]
    fn shell_does_not_inherit_unrelated_variables() {
        std::env::set_var("TW_TEST_SHELL_SECRET", "leaked-value");
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("out.log");
        let command = if cfg!(windows) {
            "echo [%TW_TEST_SHELL_SECRET%] && where cmd"
        } else {
            "echo \"[$TW_TEST_SHELL_SECRET]\" && command -v sh"
        };
        let res = run_shell(
            command,
            dir.path(),
            &log,
            &AtomicBool::new(false),
            Duration::from_secs(20),
        )
        .unwrap();
        assert_eq!(
            res.exit_code,
            Some(0),
            "PATH 还在，系统命令找得到：{}",
            res.tail
        );
        assert!(!res.tail.contains("leaked-value"), "{}", res.tail);
    }

    #[test]
    fn shell_stops_on_cancel_and_timeout() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("out.log");
        let sleep = if cfg!(windows) {
            "ping -n 30 127.0.0.1"
        } else {
            "sleep 30"
        };
        let t = run_shell(
            sleep,
            dir.path(),
            &log,
            &AtomicBool::new(false),
            Duration::from_millis(200),
        )
        .unwrap();
        assert!(t.timed_out && t.exit_code.is_none());
        assert!(t.duration_ms < 10_000);
        let c = run_shell(
            sleep,
            dir.path(),
            &log,
            &AtomicBool::new(true),
            Duration::from_secs(60),
        )
        .unwrap();
        assert!(c.cancelled);
    }
}
