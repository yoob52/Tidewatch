//! Tauri 壳：把 tidewatch-core 暴露成命令，并把引擎事件转给界面。业务逻辑都在 core 里。
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tidewatch_core::agent::{adapter, mock_process, AgentKind, Detection};
use tidewatch_core::db::{self, lock};
use tidewatch_core::engine::{Engine, EngineConfig, EngineEvent, RunPlan, StartOptions, TaskDiff};
use tidewatch_core::git;
use tidewatch_core::secrets;
use tidewatch_core::store::{projects, providers, runs, settings, stats, tasks, tickets};

/// 推给界面的事件名。
pub const ENGINE_EVENT: &str = "tw://engine";

pub struct AppState {
    pub engine: Arc<Engine>,
    pub data_dir: PathBuf,
}

type CmdResult<T> = Result<T, String>;
type State<'a> = tauri::State<'a, AppState>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// 会碰 git / 文件的命令放到阻塞线程池，不卡界面。
async fn blocking<T: Send + 'static>(
    state: State<'_>,
    f: impl FnOnce(&Engine) -> tidewatch_core::Result<T> + Send + 'static,
) -> CmdResult<T> {
    let engine = state.engine.clone();
    tauri::async_runtime::spawn_blocking(move || f(&engine).map_err(err))
        .await
        .map_err(err)?
}

#[derive(Serialize)]
struct SystemInfo {
    version: &'static str,
    data_dir: String,
    db_path: String,
    schema_version: u32,
    sqlite_version: String,
    git_version: Option<String>,
    long_paths_hint: Option<&'static str>,
    os: &'static str,
    secret_store: String,
}

#[tauri::command]
fn system_info(state: State) -> CmdResult<SystemInfo> {
    let conn = lock(state.engine.db());
    Ok(SystemInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.data_dir.display().to_string(),
        db_path: state.data_dir.join("tidewatch.db").display().to_string(),
        schema_version: db::schema_version(&conn).map_err(err)?,
        sqlite_version: db::sqlite_version(&conn).map_err(err)?,
        git_version: git::version(),
        long_paths_hint: git::long_paths_hint(),
        os: std::env::consts::OS,
        secret_store: state.engine.secrets().describe(),
    })
}

#[tauri::command]
async fn detect_agents() -> CmdResult<Vec<Detection>> {
    tauri::async_runtime::spawn_blocking(|| {
        [AgentKind::ClaudeCode, AgentKind::Codex, AgentKind::Mock]
            .into_iter()
            .map(|k| {
                let mut d = adapter(k).detect();
                if k == AgentKind::Mock {
                    // 桌面应用自己就能充当演示 Agent
                    d.installed = true;
                    d.path = std::env::current_exe()
                        .ok()
                        .map(|p| p.display().to_string());
                }
                d
            })
            .collect()
    })
    .await
    .map_err(err)
}

// ---- 设置 ----

#[tauri::command]
fn settings_get(state: State) -> CmdResult<settings::Settings> {
    settings::get(&lock(state.engine.db())).map_err(err)
}

#[tauri::command]
fn settings_save(state: State, settings: settings::Settings) -> CmdResult<settings::Settings> {
    settings::save(&lock(state.engine.db()), &settings).map_err(err)
}

// ---- 项目 ----

#[tauri::command]
fn project_list(state: State) -> CmdResult<Vec<projects::Project>> {
    projects::list(&lock(state.engine.db())).map_err(err)
}

#[tauri::command]
async fn project_save(
    state: State<'_>,
    id: Option<String>,
    input: projects::ProjectInput,
) -> CmdResult<projects::Project> {
    blocking(state, move |e| {
        let conn = lock(e.db());
        match id {
            Some(id) => projects::update(&conn, &id, input),
            None => projects::create(&conn, input),
        }
    })
    .await
}

#[tauri::command]
fn project_delete(state: State, id: String) -> CmdResult<()> {
    projects::delete(&lock(state.engine.db()), &id).map_err(err)
}

/// 仓库根目录的 AGENTS.md / CLAUDE.md，供导入为上下文卡片。
#[tauri::command]
fn project_agent_docs(path: String) -> Vec<(String, String)> {
    projects::read_agent_docs(&path)
}

// ---- Provider ----

#[tauri::command]
fn provider_list(state: State) -> CmdResult<Vec<providers::Provider>> {
    providers::list(&lock(state.engine.db())).map_err(err)
}

/// `secret`：不传 = 不动现有密钥；空串 = 清除；其它 = 存入凭据管理器。
#[tauri::command]
fn provider_save(
    state: State,
    id: Option<String>,
    input: providers::ProviderInput,
    secret: Option<String>,
) -> CmdResult<providers::Provider> {
    let conn = lock(state.engine.db());
    let p = match id {
        Some(id) => providers::update(&conn, &id, input),
        None => providers::create(&conn, input),
    }
    .map_err(err)?;
    match secret {
        Some(s) => {
            providers::store_secret(&conn, state.engine.secrets(), &p.id, Some(&s)).map_err(err)
        }
        None => Ok(p),
    }
}

#[tauri::command]
fn provider_delete(state: State, id: String) -> CmdResult<()> {
    let conn = lock(state.engine.db());
    providers::delete(&conn, &id).map_err(err)?;
    state
        .engine
        .secrets()
        .delete(&providers::secret_name(&id))
        .map_err(err)
}

// ---- 工单 ----

#[derive(Serialize)]
struct TicketSummary {
    #[serde(flatten)]
    ticket: tickets::Ticket,
    tasks_total: usize,
    /// 待验收 / 已通过 / 已合并
    tasks_done: usize,
    tasks_running: usize,
}

#[tauri::command]
fn ticket_list(state: State, query: Option<String>) -> CmdResult<Vec<TicketSummary>> {
    let conn = lock(state.engine.db());
    let all_tasks = tasks::list_all(&conn).map_err(err)?;
    Ok(tickets::list(&conn, query.as_deref())
        .map_err(err)?
        .into_iter()
        .map(|ticket| {
            let mine = all_tasks
                .iter()
                .filter(|t| t.ticket_id == ticket.id && t.status != "cancelled");
            let (mut total, mut done, mut running) = (0, 0, 0);
            for t in mine {
                total += 1;
                done += matches!(t.status.as_str(), "review" | "accepted" | "merged") as usize;
                running += matches!(t.status.as_str(), "queued" | "running") as usize;
            }
            TicketSummary {
                ticket,
                tasks_total: total,
                tasks_done: done,
                tasks_running: running,
            }
        })
        .collect())
}

#[derive(Serialize)]
struct TicketDetail {
    ticket: tickets::Ticket,
    versions: Vec<tickets::PrdVersion>,
    tasks: Vec<tasks::Task>,
    runs: Vec<runs::Run>,
    timeline: Vec<tickets::TimelineEvent>,
}

#[tauri::command]
fn ticket_get(state: State, id: String) -> CmdResult<TicketDetail> {
    let conn = lock(state.engine.db());
    let ticket = tickets::get(&conn, &id).map_err(err)?;
    Ok(TicketDetail {
        versions: tickets::prd_versions(&conn, &ticket.id).map_err(err)?,
        tasks: tasks::list_for_ticket(&conn, &ticket.id).map_err(err)?,
        runs: runs::list_for_ticket(&conn, &ticket.id).map_err(err)?,
        timeline: tickets::timeline(&conn, &ticket.id).map_err(err)?,
        ticket,
    })
}

#[tauri::command]
fn ticket_create(state: State, input: tickets::TicketInput) -> CmdResult<tickets::Ticket> {
    tickets::create(&mut lock(state.engine.db()), input).map_err(err)
}

#[tauri::command]
fn ticket_update(
    state: State,
    id: String,
    input: tickets::TicketInput,
) -> CmdResult<tickets::Ticket> {
    tickets::update(&mut lock(state.engine.db()), &id, input).map_err(err)
}

#[tauri::command]
fn ticket_save_prd(
    state: State,
    id: String,
    content: String,
    note: Option<String>,
) -> CmdResult<tickets::PrdVersion> {
    tickets::save_prd(&mut lock(state.engine.db()), &id, &content, note.as_deref()).map_err(err)
}

#[tauri::command]
fn ticket_transition(state: State, id: String, to: String) -> CmdResult<tickets::Ticket> {
    tickets::transition(&mut lock(state.engine.db()), &id, &to).map_err(err)
}

/// 从粘贴的 Markdown 预填标题和类型。
#[tauri::command]
fn ticket_draft(markdown: String) -> tickets::Draft {
    tickets::draft_from_markdown(&markdown)
}

/// 读取用户在文件对话框里选中的 Markdown / 文本文件。
#[tauri::command]
fn read_text_file(path: String) -> CmdResult<String> {
    let p = std::path::Path::new(&path);
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if !matches!(ext.as_str(), "md" | "markdown" | "txt") {
        return Err("只支持导入 .md / .markdown / .txt 文件。".into());
    }
    let meta = std::fs::metadata(p).map_err(err)?;
    if meta.len() > 2 * 1024 * 1024 {
        return Err("文件超过 2 MB，请先精简。".into());
    }
    let bytes = std::fs::read(p).map_err(err)?;
    String::from_utf8(bytes).map_err(|_| "文件不是 UTF-8 编码。".to_string())
}

// ---- 子任务 ----

#[tauri::command]
fn task_create(state: State, input: tasks::TaskInput) -> CmdResult<tasks::Task> {
    tasks::create(&mut lock(state.engine.db()), input).map_err(err)
}

#[tauri::command]
fn task_update(state: State, id: String, input: tasks::TaskInput) -> CmdResult<tasks::Task> {
    tasks::update(&lock(state.engine.db()), &id, input).map_err(err)
}

#[tauri::command]
fn task_plan(state: State, id: String, options: StartOptions) -> CmdResult<RunPlan> {
    state.engine.plan(&id, &options).map_err(err)
}

#[tauri::command]
fn task_start(state: State, id: String, options: StartOptions) -> CmdResult<runs::Run> {
    state.engine.start_task(&id, options).map_err(err)
}

#[tauri::command]
fn task_accept(state: State, id: String) -> CmdResult<tasks::Task> {
    state.engine.accept_task(&id).map_err(err)
}

#[tauri::command]
fn task_reject(state: State, id: String, note: String) -> CmdResult<tasks::Task> {
    state.engine.reject_task(&id, &note).map_err(err)
}

#[tauri::command]
async fn task_merge(state: State<'_>, id: String) -> CmdResult<tasks::Task> {
    blocking(state, move |e| e.merge_task(&id)).await
}

#[tauri::command]
async fn task_cancel(state: State<'_>, id: String) -> CmdResult<tasks::Task> {
    blocking(state, move |e| e.cancel_task(&id)).await
}

#[tauri::command]
async fn task_diff(state: State<'_>, id: String) -> CmdResult<TaskDiff> {
    blocking(state, move |e| e.task_diff(&id)).await
}

// ---- 运行 ----

#[derive(Serialize)]
struct RunRow {
    #[serde(flatten)]
    run: runs::Run,
    ticket_key: Option<String>,
    ticket_title: Option<String>,
    task_title: Option<String>,
}

#[tauri::command]
fn run_list(state: State, limit: Option<u32>) -> CmdResult<Vec<RunRow>> {
    let conn = lock(state.engine.db());
    Ok(runs::list_recent(&conn, limit.unwrap_or(200))
        .map_err(err)?
        .into_iter()
        .map(|mut run| {
            // 列表不需要 Prompt 全文
            run.prompt = String::new();
            let ticket = run
                .ticket_id
                .as_deref()
                .and_then(|id| tickets::get(&conn, id).ok());
            let task = run
                .task_id
                .as_deref()
                .and_then(|id| tasks::get(&conn, id).ok());
            RunRow {
                ticket_key: ticket.as_ref().map(|t| t.key.clone()),
                ticket_title: ticket.map(|t| t.title),
                task_title: task.map(|t| t.title),
                run,
            }
        })
        .collect())
}

#[derive(Serialize)]
struct RunDetail {
    run: runs::Run,
    events: Vec<runs::RunEventRow>,
    reports: Vec<runs::TestReport>,
}

/// `after`：只取序号大于它的事件（增量刷新）。
#[tauri::command]
fn run_get(state: State, id: String, after: Option<i64>) -> CmdResult<RunDetail> {
    let conn = lock(state.engine.db());
    Ok(RunDetail {
        run: runs::get(&conn, &id).map_err(err)?,
        events: runs::events(&conn, &id, after.unwrap_or(0)).map_err(err)?,
        reports: runs::test_reports(&conn, &id).map_err(err)?,
    })
}

#[tauri::command]
fn run_stop(state: State, id: String) -> CmdResult<()> {
    state.engine.stop_run(&id).map_err(err)
}

/// 读取检查命令的输出日志；只允许读数据目录下 `logs/` 里的文件。
#[tauri::command]
fn read_log(state: State, path: String) -> CmdResult<String> {
    let logs = tidewatch_core::util::canonical(&state.data_dir.join("logs")).map_err(err)?;
    let p = tidewatch_core::util::canonical(std::path::Path::new(&path)).map_err(err)?;
    if !p.starts_with(&logs) {
        return Err("只能查看观澜自己的运行日志。".into());
    }
    let bytes = std::fs::read(&p).map_err(err)?;
    let start = bytes.len().saturating_sub(200_000);
    Ok(String::from_utf8_lossy(&bytes[start..]).to_string())
}

#[tauri::command]
fn stats_get(state: State) -> CmdResult<stats::Stats> {
    stats::compute(&lock(state.engine.db())).map_err(err)
}

fn notify(app: &tauri::AppHandle, db: &db::Db, ev: &EngineEvent) {
    use tauri_plugin_notification::NotificationExt;
    let EngineEvent::RunFinished {
        ticket_key,
        task_title,
        status,
        failed_checks,
        ..
    } = ev
    else {
        return;
    };
    if !settings::get(&lock(db))
        .map(|s| s.notifications)
        .unwrap_or(false)
    {
        return;
    }
    let (title, body) = match status.as_str() {
        "succeeded" if failed_checks.is_empty() => {
            ("等你验收", format!("{ticket_key} {task_title} 已完成"))
        }
        "succeeded" => (
            "等你验收（检查未通过）",
            format!(
                "{ticket_key} {task_title}：{} 没通过",
                failed_checks.join("、")
            ),
        ),
        "failed" => ("运行失败", format!("{ticket_key} {task_title}")),
        _ => return,
    };
    let _ = app
        .notification()
        .builder()
        .title(format!("观澜 · {title}"))
        .body(body)
        .show();
}

/// `tidewatch mock-agent ...`：桌面应用充当演示 Agent 子进程。是的话返回退出码。
pub fn maybe_run_mock_agent() -> Option<i32> {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) != Some("mock-agent") {
        return None;
    }
    let value = |flag: &str| {
        args.iter()
            .position(|a| a == flag)
            .and_then(|i| args.get(i + 1))
            .cloned()
    };
    let num = |env: &str, default: u64| {
        std::env::var(env)
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(default)
    };
    Some(mock_process::run(mock_process::MockArgs {
        model: value("--model").unwrap_or_else(|| "mock-1".into()),
        permission: value("--permission").unwrap_or_else(|| "workspace_write".into()),
        resume: value("--resume"),
        steps: num("TIDEWATCH_MOCK_STEPS", 3) as u32,
        delay_ms: num("TIDEWATCH_MOCK_DELAY_MS", 400),
    }))
}

pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            let data_dir = match std::env::var_os("TIDEWATCH_DATA_DIR") {
                Some(p) => PathBuf::from(p),
                None => app.path().app_data_dir()?,
            };
            std::fs::create_dir_all(data_dir.join("logs"))?;
            let db = db::shared(db::open(&data_dir.join("tidewatch.db"))?);
            let handle = app.handle().clone();
            let events_db = db.clone();
            let engine = Engine::start(
                db,
                secrets::default_store(&data_dir),
                EngineConfig {
                    data_dir: data_dir.clone(),
                    mock_binary: std::env::current_exe().ok(),
                },
                Arc::new(move |ev| {
                    notify(&handle, &events_db, &ev);
                    let _ = handle.emit(ENGINE_EVENT, &ev);
                }),
            )?;
            app.manage(AppState {
                engine: Arc::new(engine),
                data_dir,
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            system_info,
            detect_agents,
            settings_get,
            settings_save,
            project_list,
            project_save,
            project_delete,
            project_agent_docs,
            provider_list,
            provider_save,
            provider_delete,
            ticket_list,
            ticket_get,
            ticket_create,
            ticket_update,
            ticket_save_prd,
            ticket_transition,
            ticket_draft,
            read_text_file,
            task_create,
            task_update,
            task_plan,
            task_start,
            task_accept,
            task_reject,
            task_merge,
            task_cancel,
            task_diff,
            run_list,
            run_get,
            run_stop,
            read_log,
            stats_get
        ])
        .run(tauri::generate_context!())
        .expect("启动观澜失败");
}
