//! Tauri 壳：把 tidewatch-core 暴露成命令，并把引擎事件转给界面。业务逻辑都在 core 里。
use serde::Serialize;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{Emitter, Manager};
use tidewatch_core::agent::{adapter, mock_process, AgentKind, Detection};
use tidewatch_core::backup;
use tidewatch_core::db::{self, lock};
use tidewatch_core::engine::{
    CodeReviewOptions, Engine, EngineConfig, EngineEvent, Finalized, PrInput, RejectInput, RunPlan,
    StartOptions, TaskDiff,
};
use tidewatch_core::git;
use tidewatch_core::prompt;
use tidewatch_core::secrets;
use tidewatch_core::store::{
    attachments, code_reviews, projects, providers, reviews, runs, settings, stats, tasks, tickets,
};

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
    /// 预审提出、还没处理的问题数。
    issues_open: usize,
}

#[tauri::command]
fn ticket_list(state: State, query: Option<String>) -> CmdResult<Vec<TicketSummary>> {
    let conn = lock(state.engine.db());
    let all_tasks = tasks::list_all(&conn).map_err(err)?;
    let open_issues = reviews::open_counts(&conn).map_err(err)?;
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
                issues_open: open_issues.get(&ticket.id).copied().unwrap_or(0),
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
    /// 每次 AI 预审的结果，旧的在前。
    reviews: Vec<reviews::Review>,
    issues: Vec<reviews::Issue>,
    /// 最近一份拆分建议 / 变更影响分析。
    split: Option<reviews::Proposal>,
    impact: Option<reviews::Proposal>,
    artifacts: Vec<tasks::Artifact>,
    /// AI 交叉审查的结果，新的在前。
    code_reviews: Vec<code_reviews::CodeReview>,
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
        reviews: reviews::list(&conn, &ticket.id).map_err(err)?,
        issues: reviews::issues(&conn, &ticket.id).map_err(err)?,
        split: reviews::latest_proposal(&conn, &ticket.id, "split").map_err(err)?,
        impact: reviews::latest_proposal(&conn, &ticket.id, "impact").map_err(err)?,
        artifacts: tasks::artifacts_for_ticket(&conn, &ticket.id).map_err(err)?,
        code_reviews: code_reviews::list_for_ticket(&conn, &ticket.id).map_err(err)?,
        ticket,
    })
}

#[tauri::command]
fn ticket_create(state: State, input: tickets::TicketInput) -> CmdResult<tickets::Ticket> {
    let mut conn = lock(state.engine.db());
    let t = tickets::create(&mut conn, input).map_err(err)?;
    // 导入 / 粘贴时先放在收件箱里的图，挪到这张工单的目录
    let ws = workspace(&state, &conn);
    let _ = attachments::relocate(&conn, &ws, &t.id, &t.key);
    Ok(t)
}

fn workspace(state: &State, conn: &tidewatch_core::rusqlite::Connection) -> PathBuf {
    settings::get(conn)
        .unwrap_or_default()
        .workspace(&state.data_dir)
}

// ---- 图片附件 ----

/// 存一张图（粘贴 / 上传）。`ticket_id` 为空表示工单还没建，建单时认领。
#[tauri::command]
fn attachment_add(
    state: State,
    ticket_id: Option<String>,
    file_name: String,
    data_base64: String,
    origin: String,
) -> CmdResult<attachments::Attachment> {
    let bytes = attachments::decode_base64(&data_base64).map_err(err)?;
    let conn = lock(state.engine.db());
    let ticket = ticket_id
        .map(|id| tickets::get(&conn, &id))
        .transpose()
        .map_err(err)?;
    attachments::add(
        &conn,
        &workspace(&state, &conn),
        ticket.as_ref().map(|t| (t.id.as_str(), t.key.as_str())),
        &file_name,
        &bytes,
        &origin,
    )
    .map_err(err)
}

#[tauri::command]
fn attachment_list(state: State, ticket_id: String) -> CmdResult<Vec<attachments::Attachment>> {
    attachments::list_for_ticket(&lock(state.engine.db()), &ticket_id).map_err(err)
}

/// 图片内容（data URL），界面渲染用。
#[tauri::command]
fn attachment_data(state: State, id: String) -> CmdResult<String> {
    let conn = lock(state.engine.db());
    let a = attachments::get(&conn, &id).map_err(err)?;
    attachments::data_url(&workspace(&state, &conn), &a).map_err(err)
}

#[tauri::command]
fn attachment_describe(
    state: State,
    id: String,
    description: String,
) -> CmdResult<attachments::Attachment> {
    attachments::set_description(&lock(state.engine.db()), &id, &description).map_err(err)
}

/// 导入用户在文件对话框里选中的 md / txt / docx / pdf：返回 Markdown，图片存成附件。
#[tauri::command]
fn import_document(state: State, path: String) -> CmdResult<tidewatch_core::import::Imported> {
    let p = std::path::Path::new(&path);
    let meta = std::fs::metadata(p).map_err(err)?;
    if meta.len() > tidewatch_core::import::MAX_FILE_BYTES as u64 {
        return Err("文件超过 50 MB，没有导入。".into());
    }
    let bytes = std::fs::read(p).map_err(err)?;
    let name = p
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("导入的文件")
        .to_string();
    let conn = lock(state.engine.db());
    tidewatch_core::import::import(&conn, &workspace(&state, &conn), &name, &bytes, p.parent())
        .map_err(err)
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
    origin: Option<String>,
) -> CmdResult<tickets::PrdVersion> {
    // 界面只能存两种来源：手动编辑、图片变更（贴图、替换、改效力、移除）
    let origin = match origin.as_deref() {
        Some("image") => "image",
        _ => "edit",
    };
    tickets::save_prd_as(
        &mut lock(state.engine.db()),
        &id,
        &content,
        origin,
        note.as_deref(),
    )
    .map_err(err)
}

/// 手动流转。提交预审和定稿各有自己的命令（要起 AI 运行、做校验），这里转过去，
/// 免得从看板拖动时绕开它们。
#[tauri::command]
fn ticket_transition(state: State, id: String, to: String) -> CmdResult<tickets::Ticket> {
    match to.as_str() {
        "ai_review" => {
            let run = state.engine.start_review(&id).map_err(err)?;
            let ticket_id = run.ticket_id.unwrap_or(id);
            tickets::get(&lock(state.engine.db()), &ticket_id).map_err(err)
        }
        "finalized" => Ok(state.engine.finalize(&id, false).map_err(err)?.ticket),
        _ => {
            let current = tickets::get(&lock(state.engine.db()), &id).map_err(err)?;
            match (current.status.as_str(), to.as_str()) {
                // 定稿后开始开发：能开始的子任务一起入队
                ("finalized", "developing") => {
                    state.engine.start_development(&current.id).map_err(err)?;
                }
                // 预审还在跑：停掉它，引擎收尾时把工单送回人工审核
                ("ai_review", "human_review") => {
                    let active: Vec<String> =
                        runs::list_for_ticket(&lock(state.engine.db()), &current.id)
                            .map_err(err)?
                            .into_iter()
                            .filter(|r| {
                                r.task_id.is_none() && runs::ACTIVE.contains(&r.status.as_str())
                            })
                            .map(|r| r.id)
                            .collect();
                    if active.is_empty() {
                        tickets::transition(&mut lock(state.engine.db()), &current.id, &to)
                            .map_err(err)?;
                    }
                    for run_id in active {
                        state.engine.stop_run(&run_id).map_err(err)?;
                    }
                }
                _ => {
                    tickets::transition(&mut lock(state.engine.db()), &current.id, &to)
                        .map_err(err)?;
                }
            }
            tickets::get(&lock(state.engine.db()), &current.id).map_err(err)
        }
    }
}

// ---- 预审与定稿 ----

/// 提交 AI 预审。
#[tauri::command]
fn review_start(state: State, id: String) -> CmdResult<runs::Run> {
    state.engine.start_review(&id).map_err(err)
}

/// 不跑 AI，直接进入人工审核（没有可用的 Agent，或者这份需求不需要预审）。
#[tauri::command]
fn review_skip(state: State, id: String) -> CmdResult<tickets::Ticket> {
    let mut conn = lock(state.engine.db());
    let t = tickets::get(&conn, &id).map_err(err)?;
    if t.status == "draft" {
        tickets::transition(&mut conn, &t.id, "ai_review").map_err(err)?;
    }
    tickets::transition(&mut conn, &t.id, "human_review").map_err(err)
}

/// 采纳 / 忽略这个问题后正文会变成什么样（确认前预览，可以再改）。
#[tauri::command]
fn issue_preview(
    state: State,
    id: String,
    action: String,
    note: Option<String>,
) -> CmdResult<String> {
    let conn = lock(state.engine.db());
    let issue = reviews::issue(&conn, &id).map_err(err)?;
    let prd = tickets::current_prd(&conn, &issue.ticket_id)
        .map_err(err)?
        .map(|v| v.content_md)
        .unwrap_or_default();
    reviews::proposed_prd(&prd, &issue, &action, note.as_deref().unwrap_or("")).map_err(err)
}

#[tauri::command]
fn issue_resolve(
    state: State,
    id: String,
    action: String,
    note: Option<String>,
    content: Option<String>,
) -> CmdResult<reviews::Issue> {
    reviews::resolve(
        &mut lock(state.engine.db()),
        &id,
        &action,
        note.as_deref(),
        content.as_deref(),
    )
    .map_err(err)
}

/// 把待确认的问题整理成一段可以发给需求方的文字。
#[tauri::command]
fn questions_export(state: State, id: String) -> CmdResult<String> {
    reviews::export_questions(&lock(state.engine.db()), &id).map_err(err)
}

#[tauri::command]
fn criteria_set(state: State, id: String, criteria: Vec<String>) -> CmdResult<tickets::Ticket> {
    tickets::set_criteria(&lock(state.engine.db()), &id, criteria).map_err(err)
}

/// 定稿；`split` 为真时让 AI 给出拆分建议（变更后再次定稿且已有子任务时，改为影响分析）。
#[tauri::command]
fn ticket_finalize(state: State, id: String, split: bool) -> CmdResult<Finalized> {
    state.engine.finalize(&id, split).map_err(err)
}

#[tauri::command]
fn proposal_regenerate(state: State, id: String) -> CmdResult<runs::Run> {
    state.engine.regenerate_proposal(&id).map_err(err)
}

#[tauri::command]
fn proposal_discard(state: State, id: String) -> CmdResult<()> {
    state.engine.discard_proposal(&id).map_err(err)
}

#[tauri::command]
fn split_apply(
    state: State,
    id: String,
    tasks: Vec<reviews::TaskDraft>,
) -> CmdResult<Vec<tasks::Task>> {
    state.engine.apply_split(&id, tasks).map_err(err)
}

#[tauri::command]
fn impact_apply(
    state: State,
    id: String,
    impacts: Vec<reviews::TaskImpact>,
    new_tasks: Vec<reviews::TaskDraft>,
) -> CmdResult<Vec<tasks::Task>> {
    state
        .engine
        .apply_impact(&id, impacts, new_tasks)
        .map_err(err)
}

/// 开始开发：能开始的子任务全部入队。
#[tauri::command]
fn development_start(state: State, id: String) -> CmdResult<Vec<runs::Run>> {
    state.engine.start_development(&id).map_err(err)
}

/// 发起变更：定稿后的工单回到人工审核。
#[tauri::command]
async fn change_request(
    state: State<'_>,
    id: String,
    reason: String,
    stop_running: bool,
) -> CmdResult<tickets::Ticket> {
    blocking(state, move |e| e.request_change(&id, &reason, stop_running)).await
}

// ---- Prompt 模板 ----

#[tauri::command]
fn prompt_templates(state: State) -> CmdResult<Vec<prompt::TemplateInfo>> {
    let ov = prompt::load_overrides(&lock(state.engine.db())).map_err(err)?;
    Ok(prompt::template_infos(&ov))
}

#[derive(Serialize)]
struct PromptPreview {
    text: String,
    /// 模板里写了、但这个模板不认识的变量（多半是拼错了）。
    unknown_variables: Vec<String>,
}

#[tauri::command]
fn prompt_preview(name: String, content: String) -> CmdResult<PromptPreview> {
    Ok(PromptPreview {
        text: prompt::preview(&name, &content).map_err(err)?,
        unknown_variables: prompt::unknown_variables(&name, &content).map_err(err)?,
    })
}

/// 保存自定义模板；`content` 为空恢复默认。
#[tauri::command]
fn prompt_save(
    state: State,
    name: String,
    content: Option<String>,
) -> CmdResult<Vec<prompt::TemplateInfo>> {
    let conn = lock(state.engine.db());
    prompt::save_override(&conn, &name, content.as_deref()).map_err(err)?;
    Ok(prompt::template_infos(
        &prompt::load_overrides(&conn).map_err(err)?,
    ))
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

/// 打回，可以带 Diff 上的行内评论；`input.rerun` 时立刻入队返工。
#[tauri::command]
fn task_reject(state: State, id: String, input: RejectInput) -> CmdResult<tasks::Task> {
    state.engine.reject_task_with(&id, input).map_err(err)
}

/// AI 交叉审查：让另一个 Agent 只读审查这个子任务的 Diff。
#[tauri::command]
fn code_review_start(
    state: State,
    task_id: String,
    agent: Option<String>,
    provider_id: Option<String>,
) -> CmdResult<runs::Run> {
    state
        .engine
        .start_code_review(&task_id, CodeReviewOptions { agent, provider_id })
        .map_err(err)
}

#[tauri::command]
fn backup_list(state: State) -> Vec<backup::BackupFile> {
    backup::list(&state.data_dir)
}

/// 立刻备份一份数据库。
#[tauri::command]
async fn backup_now(state: State<'_>) -> CmdResult<Vec<backup::BackupFile>> {
    let dir = state.data_dir.clone();
    blocking(state, move |e| {
        backup::now(&lock(e.db()), &dir)?;
        Ok(backup::list(&dir))
    })
    .await
}

/// 把全部工单导出成 Markdown 归档（带附件和一份数据库快照），放到用户选的目录下。
#[tauri::command]
async fn export_tickets(state: State<'_>, dest: String) -> CmdResult<backup::Exported> {
    let data_dir = state.data_dir.clone();
    blocking(state, move |e| {
        let conn = lock(e.db());
        let ws = settings::get(&conn)?.workspace(&data_dir);
        backup::export(&conn, &ws, std::path::Path::new(&dest))
    })
    .await
}

/// 把子任务的分支推到远程。只有用户点了才会调用。
#[tauri::command]
async fn task_push(state: State<'_>, id: String) -> CmdResult<tasks::Task> {
    blocking(state, move |e| e.push_task(&id)).await
}

/// 开 PR 对话框里预填的标题和正文。
#[tauri::command]
fn task_pr_draft(state: State, id: String) -> CmdResult<PrInput> {
    state.engine.pr_draft(&id).map_err(err)
}

/// 推送分支并用 `gh pr create` 开 PR。
#[tauri::command]
async fn task_pr_create(state: State<'_>, id: String, input: PrInput) -> CmdResult<tasks::Task> {
    blocking(state, move |e| e.create_pr(&id, input)).await
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

fn run_row(conn: &db::Connection, run: runs::Run) -> RunRow {
    let ticket = run
        .ticket_id
        .as_deref()
        .and_then(|id| tickets::get(conn, id).ok());
    let task = run
        .task_id
        .as_deref()
        .and_then(|id| tasks::get(conn, id).ok());
    RunRow {
        ticket_key: ticket.as_ref().map(|t| t.key.clone()),
        ticket_title: ticket.map(|t| t.title),
        task_title: task.map(|t| t.title),
        run,
    }
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
            run_row(&conn, run)
        })
        .collect())
}

#[derive(Serialize)]
struct RunDetail {
    run: RunRow,
    events: Vec<runs::RunEventRow>,
    reports: Vec<runs::TestReport>,
}

/// `after`：只取序号大于它的事件（增量刷新）。
#[tauri::command]
fn run_get(state: State, id: String, after: Option<i64>) -> CmdResult<RunDetail> {
    let conn = lock(state.engine.db());
    Ok(RunDetail {
        run: run_row(&conn, runs::get(&conn, &id).map_err(err)?),
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
    let (title, body): (&str, String) = match ev {
        EngineEvent::RunFinished {
            ticket_key,
            task_title,
            status,
            failed_checks,
            ..
        } => match status.as_str() {
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
        },
        EngineEvent::ReviewFinished {
            ticket_key,
            ticket_title,
            issues,
            blockers,
            error,
            ..
        } => match error {
            Some(_) => ("预审没有完成", format!("{ticket_key} {ticket_title}")),
            None if *issues == 0 => (
                "预审完成",
                format!("{ticket_key} {ticket_title}：没有发现问题"),
            ),
            None => (
                "预审完成，等你处理",
                format!("{ticket_key} {ticket_title}：{issues} 个问题，其中 {blockers} 个阻塞"),
            ),
        },
        EngineEvent::CodeReviewFinished {
            ticket_key,
            task_title,
            findings,
            blockers,
            error,
            ..
        } => match error {
            Some(_) => ("交叉审查没有完成", format!("{ticket_key} {task_title}")),
            None if *findings == 0 => {
                ("交叉审查没有发现问题", format!("{ticket_key} {task_title}"))
            }
            None => (
                "交叉审查完成",
                format!("{ticket_key} {task_title}：{findings} 个问题，其中 {blockers} 个阻塞"),
            ),
        },
        EngineEvent::ProposalReady {
            ticket_key,
            ticket_title,
            kind,
            error,
            ..
        } => {
            let what = if kind == "impact" {
                "变更影响分析"
            } else {
                "拆分建议"
            };
            match error {
                Some(_) => (
                    "没有生成出来",
                    format!("{ticket_key} {ticket_title} 的{what}"),
                ),
                None => (
                    "等你确认",
                    format!("{ticket_key} {ticket_title} 的{what}出来了"),
                ),
            }
        }
        EngineEvent::ApiDocReady {
            ticket_key,
            task_title,
            unblocked,
            ..
        } if *unblocked > 0 => (
            "接口文档已产出",
            format!("{ticket_key} {task_title}：{unblocked} 个等文档的子任务可以开始了"),
        ),
        EngineEvent::Guard {
            ticket_key,
            kind,
            message,
            ..
        } => (
            match kind.as_str() {
                "stuck" => "Agent 可能卡住了",
                "budget" => "超出预算，已停止",
                _ => "危险命令",
            },
            format!("{ticket_key} {message}"),
        ),
        _ => return,
    };
    if !settings::get(&lock(db))
        .map(|s| s.notifications)
        .unwrap_or(false)
    {
        return;
    }
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
            // 每天备份一次数据库：启动时一次，之后每小时看一眼有没有跨天。失败不影响使用
            {
                let (db, dir) = (db.clone(), data_dir.clone());
                std::thread::spawn(move || loop {
                    if let Err(e) = backup::daily(&lock(&db), &dir) {
                        eprintln!("数据库自动备份失败：{e}");
                    }
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                });
            }
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
            review_start,
            review_skip,
            issue_preview,
            issue_resolve,
            questions_export,
            criteria_set,
            ticket_finalize,
            proposal_regenerate,
            proposal_discard,
            split_apply,
            impact_apply,
            development_start,
            change_request,
            prompt_templates,
            prompt_preview,
            prompt_save,
            ticket_draft,
            read_text_file,
            attachment_add,
            attachment_list,
            attachment_data,
            attachment_describe,
            import_document,
            task_create,
            task_update,
            task_plan,
            task_start,
            task_accept,
            task_reject,
            code_review_start,
            backup_list,
            backup_now,
            export_tickets,
            task_push,
            task_pr_draft,
            task_pr_create,
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
