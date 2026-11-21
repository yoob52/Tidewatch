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
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskDiff {
    pub base: String,
    pub files: Vec<FileStat>,
    pub patch: String,
    /// 补丁太大被截断。
    pub truncated: bool,
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
}

const MAX_PATCH_BYTES: usize = 2 * 1024 * 1024;
const CHECK_TIMEOUT: Duration = Duration::from_secs(30 * 60);

struct Inner {
    db: Db,
    secrets: Box<dyn SecretStore>,
    cfg: EngineConfig,
    listener: Listener,
    cancels: Mutex<HashMap<String, Arc<AtomicBool>>>,
    /// 合并会动项目主检出的索引和 HEAD，同一时间只做一个。
    merging: Mutex<()>,
    wake: (Mutex<bool>, Condvar),
    shutdown: AtomicBool,
    workers: Mutex<Vec<JoinHandle<()>>>,
}

pub struct Engine {
    inner: Arc<Inner>,
    scheduler: Mutex<Option<JoinHandle<()>>>,
}

impl Engine {
    /// 启动引擎：先把上次没跑完的运行标为孤儿，再开调度线程。
    pub fn start(
        db: Db,
        secrets: Box<dyn SecretStore>,
        cfg: EngineConfig,
        listener: Listener,
    ) -> Result<Engine> {
        runs::recover_orphans(&mut lock(&db))?;
        let inner = Arc::new(Inner {
            db,
            secrets,
            cfg,
            listener,
            cancels: Mutex::new(HashMap::new()),
            merging: Mutex::new(()),
            wake: (Mutex::new(false), Condvar::new()),
            shutdown: AtomicBool::new(false),
            workers: Mutex::new(Vec::new()),
        });
        let sched = inner.clone();
        let handle = std::thread::Builder::new()
            .name("tw-scheduler".into())
            .spawn(move || sched.schedule_loop())?;
        Ok(Engine {
            inner,
            scheduler: Mutex::new(Some(handle)),
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
        let run = {
            let mut conn = lock(&self.inner.db);
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
            let ticket = tickets::get(&conn, &task.ticket_id)?;
            let plan = plan_run(&conn, &task, &ticket, &opts)?;
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
                    task_id: &task.id,
                    ticket_id: &ticket.id,
                    purpose: if plan.resume.is_some()
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
            tasks::refresh_blocked(&tx, &ticket.id)?;
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
        self.inner.changed(
            run.ticket_id.clone(),
            run.task_id.clone(),
            Some(run.id.clone()),
        );
        self.inner.wake();
        Ok(run)
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
        let mut conn = lock(&self.inner.db);
        let task = tasks::get(&conn, task_id)?;
        if !matches!(task.status.as_str(), "review" | "accepted") {
            return Err(Error::invalid(format!(
                "子任务处于「{}」，不能打回。",
                workflow::task_status_label(&task.status)
            )));
        }
        let note = note.trim();
        if note.is_empty() {
            return Err(Error::invalid("请写明打回的原因，Agent 返工时会看到它。"));
        }
        tasks::set_status(&conn, task_id, "changes_requested")?;
        // 依赖它的子任务回到"等依赖"，返工完成前不能开始
        tasks::refresh_blocked(&conn, &task.ticket_id)?;
        add_timeline(
            &conn,
            &task.ticket_id,
            "task_rejected",
            json!({ "task_id": task_id, "note": note }),
        )?;
        if tickets::get(&conn, &task.ticket_id)?.status == "acceptance" {
            tickets::transition(&mut conn, &task.ticket_id, "developing")?;
        }
        drop(conn);
        self.inner
            .changed(Some(task.ticket_id.clone()), Some(task_id.into()), None);
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
                if matches!(now.as_str(), "developing" | "acceptance") {
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
        let (task, ticket, project, _) = self.load(task_id)?;
        if matches!(task.status.as_str(), "queued" | "running") {
            return Err(Error::invalid("子任务在排队或运行中，请先停止。"));
        }
        if matches!(task.status.as_str(), "merged" | "cancelled") {
            return Err(Error::invalid("子任务已经结束。"));
        }
        if let Some(wt) = task
            .worktree_path
            .as_ref()
            .map(PathBuf::from)
            .filter(|p| p.exists())
        {
            git::commit_all(&wt, &format!("tw({}): 取消前的未提交改动", ticket.key))?;
            git::worktree_remove(Path::new(&project.local_path), &wt, true)?;
        }
        {
            let conn = lock(&self.inner.db);
            tasks::set_status(&conn, task_id, "cancelled")?;
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

    fn load(&self, task_id: &str) -> Result<(Task, Ticket, Project, Settings)> {
        let conn = lock(&self.inner.db);
        let task = tasks::get(&conn, task_id)?;
        let ticket = tickets::get(&conn, &task.ticket_id)?;
        let project = projects::get(&conn, &task.project_id)?;
        Ok((task, ticket, project, settings::get(&conn)?))
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
            "SELECT json_extract(payload_json, '$.note') FROM timeline_event
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
    let prompt = match (&opts.prompt_override, &resume, instruction) {
        (Some(p), _, _) if !p.trim().is_empty() => p.clone(),
        (_, Some(_), Some(i)) => crate::prompt::rework_prompt(&project, ticket, i),
        (_, Some(_), None) => {
            "请从上次停下的地方继续，把任务完成；最后用简短中文总结做了什么。".to_string()
        }
        (_, None, i) => {
            let prd = tickets::current_prd(conn, &ticket.id)?
                .map(|v| v.content_md)
                .unwrap_or_default();
            let mut p = crate::prompt::dev_prompt(&project, ticket, task, &prd);
            if let Some(i) = i {
                p.push_str(&format!("\n## 补充说明\n{i}\n"));
            }
            p
        }
    };
    Ok(RunPlan {
        agent: kind.as_str().into(),
        provider_id,
        model,
        permission,
        resume,
        prompt,
    })
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

struct Finished {
    status: &'static str,
    error: Option<String>,
    failed_checks: Vec<String>,
}

impl Inner {
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
                let max = settings::get(&conn)?.max_concurrency;
                if runs::count_running(&conn)? >= max {
                    return Ok(());
                }
                let Some(run) = runs::next_queued(&conn)? else {
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
            },
        };
        if let Some(e) = &fin.error {
            self.log(&run.id, "error", json!({ "message": e }));
        }
        let task_status = match fin.status {
            "succeeded" => "review",
            "stopped" => "stopped",
            _ => "failed",
        };
        let mut ticket_key = String::new();
        let mut task_title = String::new();
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
            tasks::refresh_blocked(&conn, &ticket.id)?;
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
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::invalid("在 Agent 启动前被停止。"));
        }
        let logs = self.cfg.data_dir.join("logs");
        std::fs::create_dir_all(&logs)?;
        let raw_log = logs.join(format!("{}.jsonl", run.id));
        let mut spec = RunSpec {
            prompt: run.prompt.clone(),
            cwd: worktree.clone(),
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
            RunnerEvent::Agent(ev) => {
                if let AgentEvent::SessionStarted { session_id, .. } = &ev {
                    // 立刻记下会话 id：即使观澜中途退出，下一轮也能续接
                    let _ = runs::set_fields(
                        &lock(&self.db),
                        &run.id,
                        &[("agent_session_id", session_id)],
                    );
                }
                let payload = serde_json::to_value(&ev).unwrap_or_default();
                let kind = payload
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("raw")
                    .to_string();
                self.log(&run.id, &kind, payload);
            }
            RunnerEvent::Stderr(line) => self.log(&run.id, "stderr", json!({ "text": line })),
        });
        let outcome = match outcome {
            Ok(o) => o,
            Err(e) => {
                let _ = runs::finish_attempt(&lock(&self.db), &attempt, "failed", None);
                return Err(e);
            }
        };

        // 3. 收尾：Agent 进程结束 ≠ Run 结束
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
        })
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
        let workspace = match &st.workspace_dir {
            Some(d) => PathBuf::from(d),
            None => self.cfg.data_dir.join("workspace"),
        };
        let path = workspace
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
