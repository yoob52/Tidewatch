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
    /// 负责人（成员 id）。
    #[serde(default)]
    pub assignee_id: Option<String>,
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
    /// 经接口操作的成员名；本机操作为空。
    pub actor: Option<String>,
}

const COLS: &str = "id, key, title, type, status, priority, labels_json, due_date, source_kind, current_version_id, finalized_version_id, acceptance_criteria_json, bug_info_json, default_agent, default_provider_id, cancelled_from, created_at, updated_at, change_from_version_id, change_reason, assignee_id";

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
        assignee_id: r.get(20)?,
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
    let previous: Option<String> = conn
        .query_row(
            "SELECT content_md FROM prd_version WHERE ticket_id = ?1 ORDER BY version DESC LIMIT 1",
            [ticket_id],
            |r| r.get(0),
        )
        .ok();
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
    attachments::index_version(conn, ticket_id, &v.id, content, author, previous.as_deref())?;
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

/// 只改标题（外部来源改了标题时用）。
pub fn rename(conn: &Connection, id: &str, title: &str) -> Result<()> {
    let title = required(title, "标题")?;
    conn.execute(
        "UPDATE ticket SET title = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![id, title, crate::util::now()],
    )?;
    let content = current_prd(conn, id)?
        .map(|v| v.content_md)
        .unwrap_or_default();
    index(conn, id, &title, &content)
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
        "SELECT id, ts, type, payload_json, actor FROM timeline_event WHERE ticket_id = ?1 ORDER BY ts, rowid",
    )?;
    let rows = st
        .query_map([ticket_id], |r| {
            Ok(TimelineEvent {
                id: r.get(0)?,
                ts: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                kind: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                payload: from_json(r.get(3)?),
                actor: r.get(4)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 在工单上留一条评论，同时进时间线。
pub fn add_comment(conn: &Connection, id: &str, body: &str) -> Result<()> {
    let t = get(conn, id)?;
    let body = required(body, "评论")?;
    conn.execute(
        "INSERT INTO comment (id, ticket_id, kind, body_md, created_at) VALUES (?1, ?2, 'note', ?3, ?4)",
        rusqlite::params![crate::util::new_id(), t.id, body, crate::util::now()],
    )?;
    add_timeline(conn, &t.id, "comment", serde_json::json!({ "body": body }))
}

/// 指派负责人；传空取消指派。
pub fn set_assignee(conn: &Connection, id: &str, member_id: Option<&str>) -> Result<Ticket> {
    let t = get(conn, id)?;
    let member = member_id
        .map(|m| super::members::get(conn, m))
        .transpose()?;
    conn.execute(
        "UPDATE ticket SET assignee_id = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![t.id, member.as_ref().map(|m| &m.id), crate::util::now()],
    )?;
    add_timeline(
        conn,
        &t.id,
        "assigned",
        serde_json::json!({ "assignee": member.map(|m| m.name) }),
    )?;
    get(conn, &t.id)
}

/// 从粘贴 / 导入的 Markdown 预填工单：标题取第一个标题行或首行，按关键词猜是不是 Bug。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Draft {
    pub title: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub content_md: String,
    pub priority: String,
    pub labels: Vec<String>,
    /// 建议绑定的项目。
    pub project_ids: Vec<String>,
    /// Bug：从正文里抽出的 `{steps, expected, actual}`。
    pub bug_info: Option<serde_json::Value>,
    /// 每一项是怎么识别出来的，给用户看。
    pub reasons: Vec<String>,
}

/// 智能创建：在 [`draft_from_markdown`] 之上再识别建议绑定的项目、优先级、标签和 Bug 三要素。
/// 全部是本机规则，不联网、不调 Agent；结果只用来预填表单，用户确认后才建单。
pub fn smart_draft(md: &str, projects: &[super::projects::Project]) -> Draft {
    let mut d = draft_from_markdown(md);
    let lower = d.content_md.to_lowercase();
    let type_label = match d.kind.as_str() {
        "bug" => "Bug",
        "feature_frontend" => "前端需求",
        "feature_backend" => "后端需求",
        _ => "前后端需求",
    };
    d.reasons
        .push(format!("类型：按正文里的关键词判断为{type_label}"));

    // 项目：先看正文有没有直接提到项目名，再看和项目上下文卡片里的英文关键词重合多少
    let mut scored: Vec<(usize, &super::projects::Project, String)> = projects
        .iter()
        .filter_map(|p| {
            let name = p.name.trim().to_lowercase();
            if !name.is_empty() && lower.contains(&name) {
                return Some((usize::MAX, p, format!("正文提到了项目「{}」", p.name)));
            }
            let mut words: Vec<String> = format!("{} {}", p.name, p.context_md)
                .to_lowercase()
                .split(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-'))
                .filter(|w| w.len() >= 4 && w.chars().any(|c| c.is_ascii_alphabetic()))
                .map(String::from)
                .collect();
            words.sort();
            words.dedup();
            let hits: Vec<&String> = words
                .iter()
                .filter(|w| lower.contains(w.as_str()))
                .collect();
            (hits.len() >= 2).then(|| {
                let shown: Vec<&str> = hits.iter().take(3).map(|w| w.as_str()).collect();
                (
                    hits.len(),
                    p,
                    format!(
                        "正文和项目「{}」的上下文都提到 {}",
                        p.name,
                        shown.join("、")
                    ),
                )
            })
        })
        .collect();
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    if let Some(best) = scored.first().map(|x| x.0) {
        for (n, p, why) in scored.iter().filter(|x| x.0 == best || x.0 == usize::MAX) {
            let _ = n;
            d.project_ids.push(p.id.clone());
            d.reasons.push(format!("项目：{why}"));
        }
    } else {
        // 没有线索时，只有在不会选错的情况下才预选：每种角色恰好一个项目
        let only = |kind: &str| {
            let mut it = projects.iter().filter(|p| p.kind == kind);
            match (it.next(), it.next()) {
                (Some(p), None) => Some(p),
                _ => None,
            }
        };
        let picks: Vec<&super::projects::Project> = if projects.len() == 1 {
            vec![&projects[0]]
        } else {
            match d.kind.as_str() {
                "feature_frontend" => only("frontend").into_iter().collect(),
                "feature_backend" => only("backend").into_iter().collect(),
                "feature_mixed" => match (only("backend"), only("frontend")) {
                    (Some(b), Some(f)) => vec![b, f],
                    _ => vec![],
                },
                _ => vec![],
            }
        };
        for p in picks {
            d.project_ids.push(p.id.clone());
            d.reasons
                .push(format!("项目：这类工单只有「{}」一个项目可选", p.name));
        }
    }

    for (level, label, words) in [
        (
            "urgent",
            "紧急",
            &["紧急", "urgent", "p0", "线上故障", "阻塞", "立即", "马上"][..],
        ),
        (
            "high",
            "高",
            &["尽快", "高优", "p1", "重要", "优先处理"][..],
        ),
        (
            "low",
            "低",
            &["不急", "低优", "p3", "有空再", "低优先级"][..],
        ),
    ] {
        if let Some(w) = words.iter().find(|w| lower.contains(**w)) {
            d.priority = level.into();
            d.reasons
                .push(format!("优先级：正文里有「{w}」，预填为{label}"));
            break;
        }
    }

    // 标签：`标签：a、b` 这样的一行
    for line in d.content_md.lines() {
        let l = line.trim().trim_start_matches(['-', '*', '#', ' ']);
        let low = l.to_lowercase();
        let Some(rest) = ["标签", "labels", "tags", "label", "tag"]
            .iter()
            .find_map(|k| low.strip_prefix(k).map(|_| &l[k.len()..]))
            .and_then(|r| r.trim_start().strip_prefix([':', '：']))
        else {
            continue;
        };
        d.labels = rest
            .split([',', '，', '、', ' ', ';', '；'])
            .map(|t| t.trim().trim_start_matches('#'))
            .filter(|t| !t.is_empty())
            .map(String::from)
            .collect();
        if !d.labels.is_empty() {
            d.reasons.push("标签：取自正文里的标签行".into());
            break;
        }
    }

    if d.kind == "bug" {
        d.bug_info = bug_sections(&d.content_md);
        if d.bug_info.is_some() {
            d.reasons
                .push("Bug：从正文里抽出了复现步骤 / 期望 / 实际".into());
        }
    }
    d
}

/// 找 `复现步骤：…` / `期望：…` / `实际：…` 这样的段落（冒号结尾的一行或小标题，内容到下一段为止）。
fn bug_sections(md: &str) -> Option<serde_json::Value> {
    const KEYS: [(&str, &[&str]); 3] = [
        (
            "steps",
            &[
                "复现步骤",
                "重现步骤",
                "操作步骤",
                "steps to reproduce",
                "复现",
            ],
        ),
        (
            "expected",
            &["期望结果", "预期结果", "期望", "预期", "expected"],
        ),
        ("actual", &["实际结果", "实际", "actual"]),
    ];
    let mut found: Vec<(&str, String)> = vec![];
    let mut current: Option<usize> = None;
    for line in md.lines() {
        let head = line
            .trim()
            .trim_start_matches(['#', '*', '-', ' ', '【'])
            .trim_end_matches(['*', '】', ' ']);
        let low = head.to_lowercase();
        let hit = KEYS.iter().find_map(|(field, names)| {
            names.iter().find_map(|n| {
                let rest = low.strip_prefix(n)?;
                let rest = rest.trim_start_matches(['*', '】', ' ']);
                // 关键词后面要么结束，要么是冒号：避免把"实际上……"这种句子当成小节
                (rest.is_empty() || rest.starts_with([':', '：'])).then(|| {
                    let body = head[head.len() - rest.len()..].trim_start_matches([':', '：', ' ']);
                    (*field, body.to_string())
                })
            })
        });
        match hit {
            Some((field, body)) if !found.iter().any(|(f, _)| *f == field) => {
                found.push((field, body));
                current = Some(found.len() - 1);
            }
            _ if line.trim_start().starts_with('#') => current = None,
            _ => {
                if let Some(i) = current {
                    if !found[i].1.is_empty() {
                        found[i].1.push('\n');
                    }
                    found[i].1.push_str(line.trim_end());
                }
            }
        }
    }
    let mut out = serde_json::Map::new();
    for (field, body) in found {
        let body = body.trim();
        if !body.is_empty() {
            out.insert(field.into(), body.into());
        }
    }
    (!out.is_empty()).then_some(serde_json::Value::Object(out))
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
        priority: "medium".into(),
        labels: vec![],
        project_ids: vec![],
        bug_info: None,
        reasons: vec![],
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

    #[test]
    fn smart_draft_prefills_project_priority_labels_and_bug_sections() {
        use crate::store::projects::Project;
        let project = |id: &str, name: &str, kind: &str, ctx: &str| Project {
            id: id.into(),
            name: name.into(),
            kind: kind.into(),
            local_path: String::new(),
            git_url: None,
            default_branch: "main".into(),
            context_md: ctx.into(),
            commands: Default::default(),
            default_agent: None,
            default_provider_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        };
        let all = [
            project(
                "p1",
                "mall-api",
                "backend",
                "Spring Boot，订单模块在 order-service，用 MyBatis",
            ),
            project(
                "p2",
                "mall-web",
                "frontend",
                "Vue3 + Pinia，结算页在 checkout 目录",
            ),
            project("p3", "admin-web", "frontend", "React 后台"),
        ];
        let md = "# 结算页金额显示为负数\n\n紧急，线上用户反馈。\n\n标签：支付、结算\n\n## 复现步骤\n1. 加两张优惠券\n2. 打开结算页\n\n**期望**：金额不小于 0\n实际：显示 -3.00\n\n## 备注\n实际上只有 checkout 用了 Pinia 的这个 store\n";
        let d = smart_draft(md, &all);
        assert_eq!(d.kind, "bug");
        assert_eq!(d.priority, "urgent");
        assert_eq!(d.labels, ["支付", "结算"]);
        assert_eq!(d.project_ids, ["p2"], "{:?}", d.reasons);
        let bug = d.bug_info.unwrap();
        assert_eq!(bug["steps"], "1. 加两张优惠券\n2. 打开结算页");
        assert_eq!(bug["expected"], "金额不小于 0");
        assert_eq!(bug["actual"], "显示 -3.00");
        assert!(
            d.reasons.iter().any(|r| r.contains("checkout")),
            "{:?}",
            d.reasons
        );

        // 直接提到项目名
        let d = smart_draft("给 mall-api 加一个导出订单的接口", &all);
        assert_eq!(
            (d.kind.as_str(), d.project_ids.as_slice()),
            ("feature_backend", &["p1".to_string()][..])
        );
        assert_eq!(d.priority, "medium");
        assert!(d.bug_info.is_none());

        // 没有线索：后端只有一个项目可以预选，前端有两个就不猜
        assert_eq!(smart_draft("新增数据库字段", &all).project_ids, ["p1"]);
        assert!(smart_draft("页面样式调整", &all).project_ids.is_empty());
        assert!(smart_draft("页面样式调整", &[]).project_ids.is_empty());
    }
}
