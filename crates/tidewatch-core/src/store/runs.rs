//! 运行（Run）、尝试（Attempt）、事件、测试报告。
use super::*;
use crate::git::FileStat;
use serde::{Deserialize, Serialize};

/// 还没结束的状态。
pub const ACTIVE: &[&str] = &["queued", "starting", "running", "finalizing", "stopping"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Run {
    pub id: String,
    pub task_id: Option<String>,
    pub ticket_id: Option<String>,
    pub number: i64,
    pub purpose: String,
    pub parent_run_id: Option<String>,
    pub agent: String,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub permission: Option<String>,
    pub agent_session_id: Option<String>,
    pub resume_session_id: Option<String>,
    pub prompt: String,
    pub command: Option<String>,
    pub status: String,
    pub exit_code: Option<i64>,
    pub queued_at: Option<String>,
    pub started_at: Option<String>,
    pub agent_finished_at: Option<String>,
    pub finished_at: Option<String>,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub cache_tokens: i64,
    pub cost_usd: Option<f64>,
    pub cost_source: Option<String>,
    pub start_commit: Option<String>,
    pub end_commit: Option<String>,
    pub summary: Option<String>,
    pub raw_log_path: Option<String>,
    pub error: Option<String>,
    pub diff_stat: Vec<FileStat>,
}

const COLS: &str = "id, task_id, ticket_id, number, purpose, parent_run_id, agent, provider_id, model, permission, agent_session_id, resume_session_id, prompt, command_json, status, exit_code, queued_at, started_at, agent_finished_at, finished_at, input_tokens, output_tokens, cache_tokens, cost_usd, cost_source, start_commit, end_commit, summary, raw_log_path, error, diff_stat_json";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: r.get(0)?,
        task_id: r.get(1)?,
        ticket_id: r.get(2)?,
        number: r.get::<_, Option<i64>>(3)?.unwrap_or(1),
        purpose: r.get::<_, Option<String>>(4)?.unwrap_or_else(|| "dev".into()),
        parent_run_id: r.get(5)?,
        agent: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        provider_id: r.get(7)?,
        model: r.get(8)?,
        permission: r.get(9)?,
        agent_session_id: r.get(10)?,
        resume_session_id: r.get(11)?,
        prompt: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
        command: r.get(13)?,
        status: r.get::<_, Option<String>>(14)?.unwrap_or_default(),
        exit_code: r.get(15)?,
        queued_at: r.get(16)?,
        started_at: r.get(17)?,
        agent_finished_at: r.get(18)?,
        finished_at: r.get(19)?,
        input_tokens: r.get::<_, Option<i64>>(20)?.unwrap_or(0),
        output_tokens: r.get::<_, Option<i64>>(21)?.unwrap_or(0),
        cache_tokens: r.get::<_, Option<i64>>(22)?.unwrap_or(0),
        cost_usd: r.get(23)?,
        cost_source: r.get(24)?,
        start_commit: r.get(25)?,
        end_commit: r.get(26)?,
        summary: r.get(27)?,
        raw_log_path: r.get(28)?,
        error: r.get(29)?,
        diff_stat: from_json(r.get(30)?),
    })
}

pub struct NewRun<'a> {
    pub task_id: &'a str,
    pub ticket_id: &'a str,
    pub purpose: &'a str,
    pub parent_run_id: Option<&'a str>,
    pub agent: &'a str,
    pub provider_id: Option<&'a str>,
    pub model: Option<&'a str>,
    pub permission: &'a str,
    pub resume_session_id: Option<&'a str>,
    pub prompt: &'a str,
}

/// 入队一轮新的 Run。
pub fn enqueue(conn: &Connection, n: NewRun) -> Result<Run> {
    let id = crate::util::new_id();
    let number: i64 = conn.query_row(
        "SELECT COALESCE(MAX(number), 0) + 1 FROM run WHERE task_id = ?1",
        [n.task_id],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO run (id, task_id, ticket_id, number, purpose, parent_run_id, agent, provider_id, model,
         permission, resume_session_id, prompt, status, queued_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, 'queued', ?13)",
        rusqlite::params![
            id,
            n.task_id,
            n.ticket_id,
            number,
            n.purpose,
            n.parent_run_id,
            n.agent,
            n.provider_id,
            n.model,
            n.permission,
            n.resume_session_id,
            n.prompt,
            crate::util::now()
        ],
    )?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Run> {
    conn.query_row(&format!("SELECT {COLS} FROM run WHERE id = ?1"), [id], row)
        .map_err(|_| not_found("运行", id))
}

fn query(conn: &Connection, tail: &str, params: &[&dyn rusqlite::ToSql]) -> Result<Vec<Run>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM run {tail}"))?;
    let rows = st.query_map(params, row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_for_task(conn: &Connection, task_id: &str) -> Result<Vec<Run>> {
    query(conn, "WHERE task_id = ?1 ORDER BY number", &[&task_id])
}

pub fn list_for_ticket(conn: &Connection, ticket_id: &str) -> Result<Vec<Run>> {
    query(conn, "WHERE ticket_id = ?1 ORDER BY queued_at, id", &[&ticket_id])
}

/// 最近的运行，未结束的排前面。
pub fn list_recent(conn: &Connection, limit: u32) -> Result<Vec<Run>> {
    query(
        conn,
        "ORDER BY (status IN ('queued', 'starting', 'running', 'finalizing', 'stopping')) DESC, queued_at DESC, id DESC LIMIT ?1",
        &[&limit],
    )
}

pub fn count_running(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM run WHERE status IN ('starting', 'running', 'finalizing', 'stopping')",
        [],
        |r| r.get(0),
    )?)
}

/// 队首：工单优先级高的先，其次先入队的先；同一子任务已有 Run 在跑则跳过。
pub fn next_queued(conn: &Connection) -> Result<Option<Run>> {
    let runs = query(
        conn,
        "WHERE status = 'queued' AND task_id NOT IN
           (SELECT task_id FROM run WHERE status IN ('starting', 'running', 'finalizing', 'stopping') AND task_id IS NOT NULL)
         ORDER BY (SELECT CASE t.priority WHEN 'urgent' THEN 0 WHEN 'high' THEN 1 WHEN 'medium' THEN 2 ELSE 3 END
                   FROM ticket t WHERE t.id = run.ticket_id), queued_at, id LIMIT 1",
        &[],
    )?;
    Ok(runs.into_iter().next())
}

/// 该子任务最近一次拿到的 Agent 会话 id（续接用）。
pub fn last_session(conn: &Connection, task_id: &str) -> Result<Option<(String, String)>> {
    Ok(conn
        .query_row(
            "SELECT id, agent_session_id FROM run WHERE task_id = ?1 AND agent_session_id IS NOT NULL
             ORDER BY number DESC LIMIT 1",
            [task_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .ok())
}

pub fn set_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    conn.execute("UPDATE run SET status = ?2 WHERE id = ?1", rusqlite::params![id, status])?;
    Ok(())
}

/// 更新若干列；列名只来自代码里的常量。
pub fn set_fields(conn: &Connection, id: &str, fields: &[(&str, &dyn rusqlite::ToSql)]) -> Result<()> {
    if fields.is_empty() {
        return Ok(());
    }
    let sets: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(i, (k, _))| format!("{k} = ?{}", i + 2))
        .collect();
    let mut params: Vec<&dyn rusqlite::ToSql> = vec![&id];
    params.extend(fields.iter().map(|(_, v)| *v));
    conn.execute(&format!("UPDATE run SET {} WHERE id = ?1", sets.join(", ")), params.as_slice())?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RunEventRow {
    pub run_id: String,
    pub seq: i64,
    pub ts: String,
    #[serde(rename = "type")]
    pub kind: String,
    pub payload: serde_json::Value,
}

pub fn add_event(conn: &Connection, run_id: &str, kind: &str, payload: &serde_json::Value) -> Result<RunEventRow> {
    let seq: i64 = conn.query_row(
        "SELECT COALESCE(MAX(seq), 0) + 1 FROM run_event WHERE run_id = ?1",
        [run_id],
        |r| r.get(0),
    )?;
    let ev = RunEventRow {
        run_id: run_id.into(),
        seq,
        ts: crate::util::now(),
        kind: kind.into(),
        payload: payload.clone(),
    };
    conn.execute(
        "INSERT INTO run_event (run_id, seq, ts, type, payload_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![ev.run_id, ev.seq, ev.ts, ev.kind, payload.to_string()],
    )?;
    Ok(ev)
}

pub fn events(conn: &Connection, run_id: &str, after_seq: i64) -> Result<Vec<RunEventRow>> {
    let mut st = conn.prepare(
        "SELECT run_id, seq, ts, type, payload_json FROM run_event WHERE run_id = ?1 AND seq > ?2 ORDER BY seq",
    )?;
    let rows = st
        .query_map(rusqlite::params![run_id, after_seq], |r| {
            Ok(RunEventRow {
                run_id: r.get(0)?,
                seq: r.get(1)?,
                ts: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                kind: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                payload: from_json(r.get(4)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 一次 Agent 进程。返回 attempt id。
pub fn start_attempt(conn: &Connection, run_id: &str, reason: &str, raw_log: Option<&str>) -> Result<String> {
    let id = crate::util::new_id();
    conn.execute(
        "INSERT INTO run_attempt (id, run_id, ordinal, reason, transport, status, started_at, raw_log_path)
         VALUES (?1, ?2, (SELECT COALESCE(MAX(ordinal), 0) + 1 FROM run_attempt WHERE run_id = ?2), ?3,
                 'headless_cli', 'running', ?4, ?5)",
        rusqlite::params![id, run_id, reason, crate::util::now(), raw_log],
    )?;
    Ok(id)
}

pub fn finish_attempt(conn: &Connection, id: &str, status: &str, session: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE run_attempt SET status = ?2, agent_session_id = ?3, finished_at = ?4 WHERE id = ?1",
        rusqlite::params![id, status, session, crate::util::now()],
    )?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestReport {
    pub id: String,
    pub run_id: String,
    pub task_id: Option<String>,
    pub command: String,
    pub exit_code: Option<i64>,
    pub output_path: Option<String>,
    pub duration_ms: i64,
    pub created_at: String,
}

pub fn add_test_report(
    conn: &Connection,
    run_id: &str,
    task_id: &str,
    command: &str,
    exit_code: Option<i64>,
    output_path: Option<&str>,
    duration_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO test_report (id, run_id, task_id, command, exit_code, output_path, duration_ms, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        rusqlite::params![crate::util::new_id(), run_id, task_id, command, exit_code, output_path, duration_ms, crate::util::now()],
    )?;
    Ok(())
}

pub fn test_reports(conn: &Connection, run_id: &str) -> Result<Vec<TestReport>> {
    let mut st = conn.prepare(
        "SELECT id, run_id, task_id, command, exit_code, output_path, duration_ms, created_at
         FROM test_report WHERE run_id = ?1 ORDER BY created_at, id",
    )?;
    let rows = st
        .query_map([run_id], |r| {
            Ok(TestReport {
                id: r.get(0)?,
                run_id: r.get(1)?,
                task_id: r.get(2)?,
                command: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                exit_code: r.get(4)?,
                output_path: r.get(5)?,
                duration_ms: r.get::<_, Option<i64>>(6)?.unwrap_or(0),
                created_at: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 应用启动时：上次没跑完的 Run 进程已经不在了，标为孤儿，子任务退回"已停止"。排队中的保留。
/// 返回被标记的 Run 数。
pub fn recover_orphans(conn: &mut Connection) -> Result<usize> {
    let tx = conn.transaction()?;
    let now = crate::util::now();
    let ids: Vec<(String, Option<String>)> = {
        let mut st = tx.prepare(
            "SELECT id, task_id FROM run WHERE status IN ('starting', 'running', 'finalizing', 'stopping')",
        )?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (run_id, task_id) in &ids {
        tx.execute(
            "UPDATE run SET status = 'orphaned', finished_at = ?2, error = '观澜上次退出时这轮运行还没结束，进程已丢失。' WHERE id = ?1",
            rusqlite::params![run_id, now],
        )?;
        tx.execute(
            "UPDATE run_attempt SET status = 'orphaned', finished_at = ?2 WHERE run_id = ?1 AND status = 'running'",
            rusqlite::params![run_id, now],
        )?;
        if let Some(t) = task_id {
            tx.execute(
                "UPDATE task SET status = 'stopped', updated_at = ?2 WHERE id = ?1 AND status IN ('queued', 'running')",
                rusqlite::params![t, now],
            )?;
        }
    }
    tx.execute(
        "UPDATE pending_effect SET status = 'orphaned', updated_at = ?1 WHERE status = 'running'",
        [&now],
    )?;
    tx.commit()?;
    Ok(ids.len())
}
