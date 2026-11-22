//! 工单与 PRD 版本。
use super::*;
use crate::workflow;
use serde::{Deserialize, Serialize};

pub const TYPES: &[&str] = &[
    "feature_backend",
    "feature_frontend",
    "feature_mixed",
    "bug",
    "chore",
];
pub const PRIORITIES: &[&str] = &["urgent", "high", "medium", "low"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TicketProject {
    pub project_id: String,
    /// backend | frontend
    pub role: String,
    pub subdir: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ticket {
    pub id: String,
    pub key: String,
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub status: String,
    pub priority: String,
    pub labels: Vec<String>,
    pub due_date: Option<String>,
    pub source_kind: String,
    pub current_version_id: Option<String>,
    pub finalized_version_id: Option<String>,
    pub acceptance_criteria: Vec<String>,
    pub bug_info: Option<serde_json::Value>,
    pub default_agent: Option<String>,
    pub default_provider_id: Option<String>,
    pub cancelled_from: Option<String>,
    /// 变更进行中：发起变更时的定稿版本。再次定稿并处理完影响分析后清空。
    pub change_from_version_id: Option<String>,
    pub change_reason: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub projects: Vec<TicketProject>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TicketInput {
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub priority: Option<String>,
    pub labels: Vec<String>,
    pub due_date: Option<String>,
    /// 需求 / Bug 描述正文（Markdown），存为 PRD 版本。
    pub content_md: String,
    pub projects: Vec<TicketProject>,
    pub acceptance_criteria: Vec<String>,
    pub bug_info: Option<serde_json::Value>,
    /// manual | file
    pub source_kind: Option<String>,
    pub default_agent: Option<String>,
    pub default_provider_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PrdVersion {
    pub id: String,
    pub ticket_id: String,
    pub version: i64,
    pub content_md: String,
    pub author: String,
    pub origin: Option<String>,
    pub note: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TimelineEvent {
    pub id: String,
    pub ts: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub payload: serde_json::Value,
}

const COLS: &str = "id, key, title, type, status, priority, labels_json, due_date, source_kind, current_version_id, finalized_version_id, acceptance_criteria_json, bug_info_json, default_agent, default_provider_id, cancelled_from, created_at, updated_at, change_from_version_id, change_reason";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Ticket> {
    Ok(Ticket {
        id: r.get(0)?,
        key: r.get(1)?,
        title: r.get(2)?,
        kind: r.get(3)?,
        status: r.get(4)?,
        priority: r
            .get::<_, Option<String>>(5)?
            .unwrap_or_else(|| "medium".into()),
        labels: from_json(r.get(6)?),
        due_date: r.get(7)?,
        source_kind: r
            .get::<_, Option<String>>(8)?
            .unwrap_or_else(|| "manual".into()),
        current_version_id: r.get(9)?,
        finalized_version_id: r.get(10)?,
        acceptance_criteria: from_json(r.get(11)?),
        bug_info: r
            .get::<_, Option<String>>(12)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        default_agent: r.get(13)?,
        default_provider_id: r.get(14)?,
        cancelled_from: r.get(15)?,
        created_at: r.get::<_, Option<String>>(16)?.unwrap_or_default(),
        updated_at: r.get::<_, Option<String>>(17)?.unwrap_or_default(),
        change_from_version_id: r.get(18)?,
        change_reason: r.get(19)?,
        projects: vec![],
    })
}

fn with_projects(conn: &Connection, mut t: Ticket) -> Result<Ticket> {
    let mut st = conn.prepare(
        "SELECT project_id, role, subdir FROM ticket_project WHERE ticket_id = ?1 ORDER BY role, project_id",
    )?;
    t.projects = st
        .query_map([&t.id], |r| {
            Ok(TicketProject {
                project_id: r.get(0)?,
                role: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                subdir: r.get(2)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(t)
}

fn clean_list(v: Vec<String>) -> Vec<String> {
    v.into_iter()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

struct Checked {
    title: String,
    kind: String,
    priority: String,
    default_agent: Option<String>,
}

fn check(conn: &Connection, input: &TicketInput) -> Result<Checked> {
    let title = required(&input.title, "标题")?;
    let kind = input.kind.trim().to_string();
    if !TYPES.contains(&kind.as_str()) {
        return Err(Error::invalid(format!("未知工单类型：{kind}")));
    }
    let priority = clean(input.priority.clone()).unwrap_or_else(|| "medium".into());
    if !PRIORITIES.contains(&priority.as_str()) {
        return Err(Error::invalid(format!("未知优先级：{priority}")));
    }
    for p in &input.projects {
        super::projects::get(conn, &p.project_id)?;
    }
    let default_agent = clean(input.default_agent.clone());
    if let Some(a) = &default_agent {
        crate::agent::AgentKind::parse(a)?;
    }
    Ok(Checked {
        title,
        kind,
        priority,
        default_agent,
    })
}

fn set_projects(conn: &Connection, ticket_id: &str, projects: &[TicketProject]) -> Result<()> {
    conn.execute(
        "DELETE FROM ticket_project WHERE ticket_id = ?1",
        [ticket_id],
    )?;
    for p in projects {
        let role = if p.role.trim().is_empty() {
            "backend"
        } else {
            p.role.trim()
        };
        conn.execute(
            "INSERT OR IGNORE INTO ticket_project (ticket_id, project_id, role, subdir) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![ticket_id, p.project_id, role, clean(p.subdir.clone())],
        )?;
    }
    Ok(())
}

fn index(conn: &Connection, ticket_id: &str, title: &str, content: &str) -> Result<()> {
    conn.execute("DELETE FROM ticket_fts WHERE ticket_id = ?1", [ticket_id])?;
    conn.execute(
        "INSERT INTO ticket_fts (title, content, ticket_id) VALUES (?1, ?2, ?3)",
        rusqlite::params![title, content, ticket_id],
    )?;
    Ok(())
}

fn insert_version(
    conn: &Connection,
    ticket_id: &str,
    content: &str,
    origin: &str,
    note: Option<&str>,
) -> Result<PrdVersion> {
    insert_version_by(conn, ticket_id, content, "human", origin, note)
}

fn insert_version_by(
    conn: &Connection,
    ticket_id: &str,
    content: &str,
    author: &str,
    origin: &str,
    note: Option<&str>,
) -> Result<PrdVersion> {
    let version: i64 = conn.query_row(
        "SELECT COALESCE(MAX(version), 0) + 1 FROM prd_version WHERE ticket_id = ?1",
        [ticket_id],
        |r| r.get(0),
    )?;
    let v = PrdVersion {
        id: crate::util::new_id(),
        ticket_id: ticket_id.into(),
        version,
        content_md: content.into(),
        author: author.into(),
        origin: Some(origin.into()),
        note: note.map(String::from),
        created_at: crate::util::now(),
    };
    conn.execute(
        "INSERT INTO prd_version (id, ticket_id, version, content_md, author, origin, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![v.id, v.ticket_id, v.version, v.content_md, v.author, v.origin, v.note, v.created_at],
    )?;
    conn.execute(
        "UPDATE ticket SET current_version_id = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![ticket_id, v.id, v.created_at],
    )?;
    Ok(v)
}

/// 创建工单（草稿），正文存为 PRD v1。
pub fn create(conn: &mut Connection, input: TicketInput) -> Result<Ticket> {
    let c = check(conn, &input)?;
    let tx = conn.transaction()?;
    let id = crate::util::new_id();
    let key = format!("TW-{}", super::settings::next_seq(&tx, "ticket")?);
    let now = crate::util::now();
    let source = clean(input.source_kind.clone()).unwrap_or_else(|| "manual".into());
    tx.execute(
        "INSERT INTO ticket (id, key, title, type, status, priority, labels_json, due_date, source_kind,
         acceptance_criteria_json, bug_info_json, default_agent, default_provider_id, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, 'draft', ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        rusqlite::params![
            id,
            key,
            c.title,
            c.kind,
            c.priority,
            to_json(&clean_list(input.labels.clone())),
            clean(input.due_date.clone()),
            source,
            to_json(&clean_list(input.acceptance_criteria.clone())),
            input.bug_info.as_ref().map(|v| v.to_string()),
            c.default_agent,
            clean(input.default_provider_id.clone()),
            now
        ],
    )?;
    set_projects(&tx, &id, &input.projects)?;
    let origin = if source == "manual" { "edit" } else { "import" };
    insert_version(&tx, &id, &input.content_md, origin, None)?;
    index(&tx, &id, &c.title, &input.content_md)?;
    add_timeline(&tx, &id, "created", serde_json::json!({ "key": key }))?;
    tx.commit()?;
    get(conn, &id)
}

/// 改标题、类型、优先级、标签、项目等管理字段；正文走 [`save_prd`]。
pub fn update(conn: &mut Connection, id: &str, input: TicketInput) -> Result<Ticket> {
    let old = get(conn, id)?;
    // 调用方可以传编号（TW-1）；下面一律用内部 id
    let id = old.id.as_str();
    let c = check(conn, &input)?;
    if c.kind != old.kind && old.status != "draft" {
        return Err(Error::invalid("只有草稿状态可以改工单类型。"));
    }
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE ticket SET title = ?2, type = ?3, priority = ?4, labels_json = ?5, due_date = ?6,
         acceptance_criteria_json = ?7, bug_info_json = ?8, default_agent = ?9, default_provider_id = ?10, updated_at = ?11
         WHERE id = ?1",
        rusqlite::params![
            id,
            c.title,
            c.kind,
            c.priority,
            to_json(&clean_list(input.labels.clone())),
            clean(input.due_date.clone()),
            to_json(&clean_list(input.acceptance_criteria.clone())),
            input.bug_info.as_ref().map(|v| v.to_string()),
            c.default_agent,
            clean(input.default_provider_id.clone()),
            crate::util::now()
        ],
    )?;
    set_projects(&tx, id, &input.projects)?;
    let content = current_prd(&tx, id)?
        .map(|v| v.content_md)
        .unwrap_or_default();
    index(&tx, id, &c.title, &content)?;
    tx.commit()?;
    get(conn, id)
}

/// 保存正文为新版本（版本不可变）。定稿后不能直接改。
pub fn save_prd(
    conn: &mut Connection,
    ticket_id: &str,
    content: &str,
    note: Option<&str>,
) -> Result<PrdVersion> {
    save_prd_as(conn, ticket_id, content, "edit", note)
}

/// 同 [`save_prd`]，并标明这一版的来源：`edit`（手动）、`ai_suggestion`（采纳预审建议）。
pub fn save_prd_as(
    conn: &mut Connection,
    ticket_id: &str,
    content: &str,
    origin: &str,
    note: Option<&str>,
) -> Result<PrdVersion> {
    let t = get(conn, ticket_id)?;
    let ticket_id = t.id.as_str();
    let editable = matches!(t.status.as_str(), "draft" | "human_review")
        || (t.kind == "bug" && matches!(t.status.as_str(), "developing" | "acceptance"));
    if !editable {
        return Err(Error::invalid(format!(
            "工单处于「{}」，正文已锁定，不能直接修改。",
            workflow::ticket_status_label(&t.status)
        )));
    }
    if current_prd(conn, ticket_id)?.is_some_and(|v| v.content_md == content) {
        return Err(Error::invalid("正文没有变化，不需要保存新版本。"));
    }
    let tx = conn.transaction()?;
    let author = if origin == "ai_suggestion" {
        "ai"
    } else {
        "human"
    };
    let v = insert_version_by(&tx, ticket_id, content, author, origin, note)?;
    index(&tx, ticket_id, &t.title, content)?;
    add_timeline(
        &tx,
        ticket_id,
        "prd_version",
        serde_json::json!({ "version": v.version, "note": note, "origin": origin }),
    )?;
    tx.commit()?;
    Ok(v)
}

pub fn current_prd(conn: &Connection, ticket_id: &str) -> Result<Option<PrdVersion>> {
    Ok(prd_versions(conn, ticket_id)?.into_iter().next_back())
}

pub fn prd_versions(conn: &Connection, ticket_id: &str) -> Result<Vec<PrdVersion>> {
    prd_versions_where(conn, "ticket_id = ?1", ticket_id)
}

fn prd_versions_where(conn: &Connection, cond: &str, arg: &str) -> Result<Vec<PrdVersion>> {
    let mut st = conn.prepare(&format!(
        "SELECT id, ticket_id, version, content_md, author, origin, note, created_at
         FROM prd_version WHERE {cond} ORDER BY version"
    ))?;
    let rows = st
        .query_map([arg], |r| {
            Ok(PrdVersion {
                id: r.get(0)?,
                ticket_id: r.get(1)?,
                version: r.get(2)?,
                content_md: r.get(3)?,
                author: r.get(4)?,
                origin: r.get(5)?,
                note: r.get(6)?,
                created_at: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 状态流转；不合法时返回原因。
pub fn transition(conn: &mut Connection, id: &str, to: &str) -> Result<Ticket> {
    let t = get(conn, id)?;
    if t.status == to {
        return Ok(t);
    }
    let id = t.id.as_str();
    workflow::check_ticket_transition(&t.kind, &t.status, to, t.cancelled_from.as_deref())?;
    if to == "acceptance" || to == "done" {
        let statuses = task_statuses(conn, id)?;
        let refs = statuses.iter().map(String::as_str);
        if to == "acceptance" && !workflow::all_tasks_ready_for_acceptance(refs) {
            return Err(Error::invalid("还有子任务没有完成，工单不能进入验收。"));
        }
        if to == "done"
            && statuses
                .iter()
                .any(|s| !matches!(s.as_str(), "merged" | "cancelled"))
        {
            return Err(Error::invalid("还有子任务没有合并，工单不能完成。"));
        }
    }
    if to == "finalized" {
        check_can_finalize(conn, &t)?;
    }
    if to == "human_review" && matches!(t.status.as_str(), "finalized" | "developing") {
        return Err(Error::invalid(
            "定稿后要改需求，请用「发起变更」并填写变更原因。",
        ));
    }
    let tx = conn.transaction()?;
    let cancelled_from = (to == "cancelled").then(|| t.status.clone());
    tx.execute(
        "UPDATE ticket SET status = ?2, cancelled_from = ?3, updated_at = ?4 WHERE id = ?1",
        rusqlite::params![id, to, cancelled_from, crate::util::now()],
    )?;
    if to == "finalized" {
        // 定稿：锁定当前版本
        tx.execute(
            "UPDATE ticket SET finalized_version_id = current_version_id WHERE id = ?1",
            [id],
        )?;
    }
    add_timeline(
        &tx,
        id,
        "status",
        serde_json::json!({ "from": t.status, "to": to }),
    )?;
    tx.commit()?;
    get(conn, id)
}

/// 定稿前置条件：没有未处理的阻塞级问题，验收标准不为空。
pub fn check_can_finalize(conn: &Connection, t: &Ticket) -> Result<()> {
    let blockers: i64 = conn.query_row(
        "SELECT COUNT(*) FROM review_issue WHERE ticket_id = ?1 AND severity = 'blocker'
         AND COALESCE(resolution, 'open') IN ('open', 'pending_confirm')",
        [&t.id],
        |r| r.get(0),
    )?;
    if blockers > 0 {
        return Err(Error::invalid(format!(
            "还有 {blockers} 个阻塞级问题没有处理（采纳、手动修改，或忽略并写明理由），不能定稿。"
        )));
    }
    if t.acceptance_criteria.is_empty() {
        return Err(Error::invalid("验收标准为空，不能定稿。"));
    }
    Ok(())
}

/// 只改验收标准。定稿后锁定（Bug 工单没有定稿环节，开发中也能改）。
pub fn set_criteria(conn: &Connection, id: &str, criteria: Vec<String>) -> Result<Ticket> {
    let t = get(conn, id)?;
    let editable = matches!(t.status.as_str(), "draft" | "ai_review" | "human_review")
        || (t.kind == "bug" && matches!(t.status.as_str(), "developing" | "acceptance"));
    if !editable {
        return Err(Error::invalid(format!(
            "工单处于「{}」，验收标准已锁定；要改请发起变更。",
            workflow::ticket_status_label(&t.status)
        )));
    }
    conn.execute(
        "UPDATE ticket SET acceptance_criteria_json = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![t.id, to_json(&clean_list(criteria)), crate::util::now()],
    )?;
    get(conn, &t.id)
}

/// 发起变更：定稿后的工单回到人工审核，正文重新可编辑。
/// 记下此刻的定稿版本，再次定稿时用它和新版本做影响分析。
pub fn request_change(conn: &mut Connection, id: &str, reason: &str) -> Result<Ticket> {
    let t = get(conn, id)?;
    if !matches!(t.status.as_str(), "finalized" | "developing") {
        return Err(Error::invalid(format!(
            "工单处于「{}」，不需要发起变更。",
            workflow::ticket_status_label(&t.status)
        )));
    }
    let reason = super::required(reason, "变更原因")?;
    let tx = conn.transaction()?;
    // 连续两次变更之间没有再定稿时，保留最早那个定稿版本作为对比基准
    tx.execute(
        "UPDATE ticket SET status = 'human_review',
           change_from_criteria_json = CASE WHEN change_from_version_id IS NULL
             THEN acceptance_criteria_json ELSE change_from_criteria_json END,
           change_from_version_id = COALESCE(change_from_version_id, finalized_version_id),
           change_reason = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![t.id, reason, crate::util::now()],
    )?;
    add_timeline(
        &tx,
        &t.id,
        "status",
        serde_json::json!({ "from": t.status, "to": "human_review" }),
    )?;
    add_timeline(
        &tx,
        &t.id,
        "change_requested",
        serde_json::json!({ "reason": reason }),
    )?;
    tx.commit()?;
    get(conn, &t.id)
}

/// 变更的影响已经处理完。
pub fn clear_change(conn: &Connection, id: &str) -> Result<()> {
    conn.execute(
        "UPDATE ticket SET change_from_version_id = NULL, change_reason = NULL,
           change_from_criteria_json = NULL WHERE id = ?1",
        [id],
    )?;
    Ok(())
}

/// 发起变更那一刻的验收标准（和变更前的定稿版本一起，作为影响分析的对比基准）。
pub fn change_from_criteria(conn: &Connection, id: &str) -> Result<Vec<String>> {
    let json: Option<String> = conn.query_row(
        "SELECT change_from_criteria_json FROM ticket WHERE id = ?1",
        [id],
        |r| r.get(0),
    )?;
    Ok(json
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default())
}

pub fn prd_version(conn: &Connection, version_id: &str) -> Result<PrdVersion> {
    prd_versions_where(conn, "id = ?1", version_id)?
        .into_iter()
        .next()
        .ok_or_else(|| not_found("PRD 版本", version_id))
}

fn task_statuses(conn: &Connection, ticket_id: &str) -> Result<Vec<String>> {
    let mut st = conn.prepare("SELECT status FROM task WHERE ticket_id = ?1")?;
    let rows = st
        .query_map([ticket_id], |r| r.get(0))?
        .collect::<rusqlite::Result<Vec<String>>>()?;
    Ok(rows)
}

pub fn get(conn: &Connection, id: &str) -> Result<Ticket> {
    let t = conn
        .query_row(
            &format!("SELECT {COLS} FROM ticket WHERE id = ?1 OR key = ?1"),
            [id],
            row,
        )
        .map_err(|_| not_found("工单", id))?;
    with_projects(conn, t)
}

/// 列表；`query` 非空时按标题 / 正文 / 编号搜索。
pub fn list(conn: &Connection, query: Option<&str>) -> Result<Vec<Ticket>> {
    let q = query.map(str::trim).filter(|q| !q.is_empty());
    let tickets = match q {
        None => {
            let mut st = conn.prepare(&format!(
                "SELECT {COLS} FROM ticket ORDER BY created_at DESC"
            ))?;
            let rows = st
                .query_map([], row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        }
        Some(q) => {
            // 反斜杠是转义符，本身也要转义：否则搜 `C:\code` 这样的路径匹配不到
            let like = format!(
                "%{}%",
                q.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            let mut st = conn.prepare(&format!(
                "SELECT {COLS} FROM ticket WHERE key LIKE ?1 ESCAPE '\\' OR title LIKE ?1 ESCAPE '\\'
                 OR id IN (SELECT ticket_id FROM ticket_fts WHERE content LIKE ?1 ESCAPE '\\')
                 ORDER BY created_at DESC"
            ))?;
            let rows = st
                .query_map([like], row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            rows
        }
    };
    tickets
        .into_iter()
        .map(|t| with_projects(conn, t))
        .collect()
}

pub fn timeline(conn: &Connection, ticket_id: &str) -> Result<Vec<TimelineEvent>> {
    let mut st = conn.prepare(
        "SELECT id, ts, type, payload_json FROM timeline_event WHERE ticket_id = ?1 ORDER BY ts, rowid",
    )?;
    let rows = st
        .query_map([ticket_id], |r| {
            Ok(TimelineEvent {
                id: r.get(0)?,
                ts: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                kind: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                payload: from_json(r.get(3)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 从粘贴 / 导入的 Markdown 预填工单：标题取第一个标题行或首行，按关键词猜是不是 Bug。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Draft {
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub content_md: String,
}

pub fn draft_from_markdown(md: &str) -> Draft {
    let md = md.trim_start_matches('\u{feff}').trim();
    let first = md
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let title: String = first
        .trim_start_matches('#')
        .trim()
        .chars()
        .take(80)
        .collect();
    let lower = md.to_lowercase();
    let bug_words = [
        "bug", "报错", "异常", "崩溃", "复现", "修复", "错误", "失败", "闪退",
    ];
    let kind = if bug_words.iter().any(|w| lower.contains(w)) {
        "bug"
    } else {
        let fe = ["前端", "页面", "界面", "样式", "组件"]
            .iter()
            .any(|w| lower.contains(w));
        let be = ["后端", "接口", "数据库", "api", "服务端"]
            .iter()
            .any(|w| lower.contains(w));
        match (fe, be) {
            (true, true) => "feature_mixed",
            (true, false) => "feature_frontend",
            _ => "feature_backend",
        }
    };
    Draft {
        title,
        kind: kind.into(),
        content_md: md.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn search_matches_backslashes_and_wildcards_literally() {
        let mut conn = crate::db::open_in_memory().unwrap();
        for (title, body) in [
            ("打不开 C:\\code\\app", "正文"),
            ("另一个", "日志在 D:\\logs\\a_b.txt，进度 100%"),
            ("无关", "C:code"),
        ] {
            create(
                &mut conn,
                TicketInput {
                    title: title.into(),
                    kind: "bug".into(),
                    content_md: body.into(),
                    ..Default::default()
                },
            )
            .unwrap();
        }
        let titles = |q: &str| -> Vec<String> {
            list(&conn, Some(q))
                .unwrap()
                .into_iter()
                .map(|t| t.title)
                .collect()
        };
        assert_eq!(titles("C:\\code"), ["打不开 C:\\code\\app"]);
        assert_eq!(titles("D:\\logs\\a_b"), ["另一个"]);
        assert_eq!(titles("100%"), ["另一个"]);
        assert!(titles("a%b").is_empty());
    }

    fn bug(title: &str) -> TicketInput {
        TicketInput {
            title: title.into(),
            kind: "bug".into(),
            content_md: "结算页优惠券叠加后金额为负".into(),
            labels: vec![" 支付 ".into(), "".into()],
            ..Default::default()
        }
    }

    #[test]
    fn create_assigns_keys_and_first_version() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let a = create(&mut conn, bug("金额为负")).unwrap();
        let b = create(&mut conn, bug("另一个")).unwrap();
        assert_eq!((a.key.as_str(), b.key.as_str()), ("TW-1", "TW-2"));
        assert_eq!(a.status, "draft");
        assert_eq!(a.labels, vec!["支付"]);
        let v = current_prd(&conn, &a.id).unwrap().unwrap();
        assert_eq!(
            (v.version, v.content_md.as_str()),
            (1, "结算页优惠券叠加后金额为负")
        );
        assert_eq!(get(&conn, "TW-2").unwrap().id, b.id);
        assert_eq!(list(&conn, Some("优惠券")).unwrap().len(), 2);
        assert_eq!(list(&conn, Some("另一个")).unwrap().len(), 1);
        assert_eq!(list(&conn, Some("TW-1")).unwrap().len(), 1);
    }

    #[test]
    fn key_works_wherever_an_id_does() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let t = create(&mut conn, bug("按编号操作")).unwrap();
        let v = save_prd(&mut conn, &t.key, "新的正文", None).unwrap();
        assert_eq!((v.version, v.ticket_id.as_str()), (2, t.id.as_str()));
        let mut input = bug("改了标题");
        input.priority = Some("urgent".into());
        let u = update(&mut conn, &t.key, input).unwrap();
        assert_eq!(
            (u.title.as_str(), u.priority.as_str()),
            ("改了标题", "urgent")
        );
        assert_eq!(
            transition(&mut conn, &t.key, "developing").unwrap().status,
            "developing"
        );
        assert_eq!(get(&conn, &t.id).unwrap().status, "developing");
        assert_eq!(timeline(&conn, &t.id).unwrap().len(), 3);
    }

    #[test]
    fn prd_versions_are_immutable_and_increment() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let t = create(&mut conn, bug("x")).unwrap();
        assert!(save_prd(&mut conn, &t.id, "结算页优惠券叠加后金额为负", None).is_err());
        let v2 = save_prd(
            &mut conn,
            &t.id,
            "补充：只在叠加两张时出现",
            Some("补充复现条件"),
        )
        .unwrap();
        assert_eq!(v2.version, 2);
        let all = prd_versions(&conn, &t.id).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].content_md, "结算页优惠券叠加后金额为负");
        assert_eq!(get(&conn, &t.id).unwrap().current_version_id, Some(v2.id));
    }

    #[test]
    fn transitions_are_checked_and_recorded() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let t = create(&mut conn, bug("x")).unwrap();
        assert!(transition(&mut conn, &t.id, "done").is_err());
        let t2 = transition(&mut conn, &t.id, "developing").unwrap();
        assert_eq!(t2.status, "developing");
        // 没有子任务时不能进入验收
        assert!(transition(&mut conn, &t.id, "acceptance").is_err());
        let c = transition(&mut conn, &t.id, "cancelled").unwrap();
        assert_eq!(c.cancelled_from.as_deref(), Some("developing"));
        assert!(transition(&mut conn, &t.id, "draft").is_err());
        assert_eq!(
            transition(&mut conn, &t.id, "developing")
                .unwrap()
                .cancelled_from,
            None
        );
        let kinds: Vec<_> = timeline(&conn, &t.id)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect();
        assert_eq!(kinds, vec!["created", "status", "status", "status"]);

        let mut f = bug("需求");
        f.kind = "feature_backend".into();
        let f = create(&mut conn, f).unwrap();
        assert!(transition(&mut conn, &f.id, "developing").is_err());
    }

    #[test]
    fn markdown_draft_guesses_title_and_type() {
        let d = draft_from_markdown("# 登录页点击按钮报错\n\n复现步骤：...");
        assert_eq!(
            (d.title.as_str(), d.kind.as_str()),
            ("登录页点击按钮报错", "bug")
        );
        let d = draft_from_markdown("优惠券叠加规则\n后端接口需要支持……前端页面展示……");
        assert_eq!(d.kind, "feature_mixed");
    }
}
