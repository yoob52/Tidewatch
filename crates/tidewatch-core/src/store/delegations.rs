//! Agent 通过 MCP 委派出去的运行：谁派的、派给了哪一轮。
use super::*;
use rusqlite::OptionalExtension;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Delegation {
    pub id: String,
    pub parent_run_id: String,
    pub run_id: String,
    pub task_id: Option<String>,
    pub created_at: String,
}

const COLS: &str = "id, parent_run_id, run_id, task_id, created_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Delegation> {
    Ok(Delegation {
        id: r.get(0)?,
        parent_run_id: r.get(1)?,
        run_id: r.get(2)?,
        task_id: r.get(3)?,
        created_at: r.get(4)?,
    })
}

pub fn create(
    conn: &Connection,
    parent_run_id: &str,
    run_id: &str,
    task_id: Option<&str>,
    client_request_id: Option<&str>,
) -> Result<Delegation> {
    let id = crate::util::new_id();
    conn.execute(
        "INSERT INTO delegation (id, parent_run_id, run_id, task_id, client_request_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            id,
            parent_run_id,
            run_id,
            task_id,
            client_request_id,
            crate::util::now()
        ],
    )?;
    by_run(conn, run_id)?.ok_or_else(|| not_found("委派", run_id))
}

pub fn by_run(conn: &Connection, run_id: &str) -> Result<Option<Delegation>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM delegation WHERE run_id = ?1"),
            [run_id],
            row,
        )
        .optional()?)
}

/// 同一个调用方用同一个请求 id 重试：返回之前那次委派。
pub fn by_request(
    conn: &Connection,
    parent_run_id: &str,
    client_request_id: &str,
) -> Result<Option<Delegation>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {COLS} FROM delegation WHERE parent_run_id = ?1 AND client_request_id = ?2"
            ),
            [parent_run_id, client_request_id],
            row,
        )
        .optional()?)
}

pub fn children(conn: &Connection, parent_run_id: &str) -> Result<Vec<Delegation>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLS} FROM delegation WHERE parent_run_id = ?1 ORDER BY id"
    ))?;
    let rows = stmt.query_map([parent_run_id], row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 委派链的深度：直接由人发起的运行是 0。
pub fn depth(conn: &Connection, run_id: &str) -> Result<u32> {
    let mut depth = 0;
    let mut cur = run_id.to_string();
    while let Some(d) = by_run(conn, &cur)? {
        depth += 1;
        cur = d.parent_run_id;
        if depth > 16 {
            break;
        }
    }
    Ok(depth)
}
