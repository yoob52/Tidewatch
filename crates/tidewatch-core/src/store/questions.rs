//! Agent 提给人的问题：挂在工单的"需要你"里，回答写进下一轮的 Prompt。
use super::*;
use rusqlite::OptionalExtension;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Question {
    pub id: String,
    pub ticket_id: String,
    pub task_id: Option<String>,
    pub run_id: Option<String>,
    pub body: String,
    pub options: Vec<String>,
    /// open | answered | dismissed
    pub status: String,
    pub answer: Option<String>,
    pub answered_by: Option<String>,
    pub consumed_run_id: Option<String>,
    pub created_at: String,
    pub answered_at: Option<String>,
}

const COLS: &str = "id, ticket_id, task_id, run_id, body, options_json, status, answer, answered_by, consumed_run_id, created_at, answered_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Question> {
    Ok(Question {
        id: r.get(0)?,
        ticket_id: r.get(1)?,
        task_id: r.get(2)?,
        run_id: r.get(3)?,
        body: r.get(4)?,
        options: from_json(r.get(5)?),
        status: r.get(6)?,
        answer: r.get(7)?,
        answered_by: r.get(8)?,
        consumed_run_id: r.get(9)?,
        created_at: r.get(10)?,
        answered_at: r.get(11)?,
    })
}

fn query(conn: &Connection, tail: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Question>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM question {tail}"))?;
    let rows = stmt.query_map(params, row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

pub struct NewQuestion<'a> {
    pub ticket_id: &'a str,
    pub task_id: Option<&'a str>,
    pub run_id: Option<&'a str>,
    pub body: &'a str,
    pub options: &'a [String],
    /// 同一轮运行里重试同一个请求，返回同一个问题。
    pub client_request_id: Option<&'a str>,
}

pub fn ask(conn: &Connection, n: NewQuestion) -> Result<Question> {
    let body = required(n.body, "问题")?;
    if let (Some(run), Some(req)) = (n.run_id, n.client_request_id) {
        let existing = query(
            conn,
            "WHERE run_id = ?1 AND client_request_id = ?2",
            &[&run, &req],
        )?;
        if let Some(q) = existing.into_iter().next() {
            return Ok(q);
        }
    }
    let id = crate::util::new_id();
    conn.execute(
        "INSERT INTO question (id, ticket_id, task_id, run_id, body, options_json, client_request_id, status, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'open', ?8)",
        rusqlite::params![id, n.ticket_id, n.task_id, n.run_id, body, to_json(&n.options), n.client_request_id, crate::util::now()],
    )?;
    add_timeline(
        conn,
        n.ticket_id,
        "question_asked",
        serde_json::json!({ "question_id": id, "run_id": n.run_id, "task_id": n.task_id, "body": body }),
    )?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Question> {
    conn.query_row(
        &format!("SELECT {COLS} FROM question WHERE id = ?1"),
        [id],
        row,
    )
    .optional()?
    .ok_or_else(|| not_found("问题", id))
}

pub fn list_for_ticket(conn: &Connection, ticket_id: &str) -> Result<Vec<Question>> {
    query(conn, "WHERE ticket_id = ?1 ORDER BY id", &[&ticket_id])
}

/// 所有还没回答的问题（"需要你"）。
pub fn list_open(conn: &Connection) -> Result<Vec<Question>> {
    query(conn, "WHERE status = 'open' ORDER BY id", &[])
}

pub fn answer(conn: &Connection, id: &str, answer: &str, by: Option<&str>) -> Result<Question> {
    let q = get(conn, id)?;
    if q.status != "open" {
        return Err(Error::invalid("这个问题已经处理过了。"));
    }
    let answer = required(answer, "回答")?;
    conn.execute(
        "UPDATE question SET status = 'answered', answer = ?2, answered_by = ?3, answered_at = ?4 WHERE id = ?1",
        rusqlite::params![id, answer, by, crate::util::now()],
    )?;
    add_timeline_as(
        conn,
        &q.ticket_id,
        "question_answered",
        serde_json::json!({ "question_id": id, "task_id": q.task_id, "body": q.body, "answer": answer }),
        by,
    )?;
    get(conn, id)
}

pub fn dismiss(conn: &Connection, id: &str) -> Result<Question> {
    conn.execute(
        "UPDATE question SET status = 'dismissed', answered_at = ?2 WHERE id = ?1 AND status = 'open'",
        rusqlite::params![id, crate::util::now()],
    )?;
    get(conn, id)
}

/// 已回答、还没交给 Agent 的问题：写进这一轮的 Prompt，并记下是哪一轮带走的。
/// 子任务上的问题只给该子任务；工单级的问题给这张工单的任何一轮。
pub fn take_answers(
    conn: &Connection,
    ticket_id: &str,
    task_id: Option<&str>,
    run_id: &str,
) -> Result<Vec<Question>> {
    let qs = query(
        conn,
        "WHERE ticket_id = ?1 AND status = 'answered' AND consumed_run_id IS NULL
           AND (task_id IS NULL OR task_id = ?2) ORDER BY id",
        &[&ticket_id, &task_id],
    )?;
    for q in &qs {
        conn.execute(
            "UPDATE question SET consumed_run_id = ?2 WHERE id = ?1",
            [&q.id, run_id],
        )?;
    }
    Ok(qs)
}

/// 附在 Prompt 末尾的"你之前的提问和回答"。
pub fn answers_md(qs: &[Question]) -> String {
    if qs.is_empty() {
        return String::new();
    }
    let mut out = String::from("\n\n## 你之前提的问题，现在有了回答\n");
    for q in qs {
        out.push_str(&format!(
            "\n- 问：{}\n  答：{}\n",
            q.body.trim().replace('\n', " "),
            q.answer
                .as_deref()
                .unwrap_or_default()
                .trim()
                .replace('\n', " ")
        ));
    }
    out
}
