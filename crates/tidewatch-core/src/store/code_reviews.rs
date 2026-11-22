//! AI 交叉审查的结果：另一个 Agent 只读看完子任务的 Diff 后给出的问题列表。
use super::reviews::extract_json;
use crate::Result;
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// blocker | major | minor
    pub severity: String,
    pub file: Option<String>,
    pub line: Option<i64>,
    pub title: String,
    pub detail: String,
    pub suggestion: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CodeReview {
    pub id: String,
    pub task_id: String,
    pub ticket_id: String,
    pub run_id: String,
    pub head_commit: Option<String>,
    pub agent: String,
    pub model: Option<String>,
    /// running | done | failed
    pub status: String,
    pub summary: Option<String>,
    pub findings: Vec<Finding>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const COLS: &str = "id, task_id, ticket_id, run_id, head_commit, agent, model, status, summary, findings_json, error, created_at, updated_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<CodeReview> {
    Ok(CodeReview {
        id: r.get(0)?,
        task_id: r.get(1)?,
        ticket_id: r.get(2)?,
        run_id: r.get(3)?,
        head_commit: r.get(4)?,
        agent: r.get(5)?,
        model: r.get(6)?,
        status: r.get(7)?,
        summary: r.get(8)?,
        findings: super::from_json(r.get(9)?),
        error: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
    })
}

pub struct NewCodeReview<'a> {
    pub task_id: &'a str,
    pub ticket_id: &'a str,
    pub run_id: &'a str,
    pub head_commit: Option<&'a str>,
    pub agent: &'a str,
    pub model: Option<&'a str>,
}

pub fn create(conn: &Connection, new: NewCodeReview) -> Result<CodeReview> {
    let id = crate::util::new_id();
    let now = crate::util::now();
    conn.execute(
        "INSERT INTO code_review (id, task_id, ticket_id, run_id, head_commit, agent, model, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'running', ?8, ?8)",
        rusqlite::params![id, new.task_id, new.ticket_id, new.run_id, new.head_commit, new.agent, new.model, now],
    )?;
    Ok(by_run(conn, new.run_id)?.expect("刚写入"))
}

pub fn by_run(conn: &Connection, run_id: &str) -> Result<Option<CodeReview>> {
    Ok(conn
        .query_row(
            &format!("SELECT {COLS} FROM code_review WHERE run_id = ?1"),
            [run_id],
            row,
        )
        .optional()?)
}

/// 一个子任务的全部审查，最新的在前。
pub fn list_for_task(conn: &Connection, task_id: &str) -> Result<Vec<CodeReview>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM code_review WHERE task_id = ?1 ORDER BY created_at DESC, rowid DESC"
    ))?;
    let rows = st
        .query_map([task_id], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn list_for_ticket(conn: &Connection, ticket_id: &str) -> Result<Vec<CodeReview>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM code_review WHERE ticket_id = ?1 ORDER BY created_at DESC, rowid DESC"
    ))?;
    let rows = st
        .query_map([ticket_id], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn running_for_task(conn: &Connection, task_id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM code_review WHERE task_id = ?1 AND status = 'running')",
        [task_id],
        |r| r.get(0),
    )?)
}

fn text(v: &Value, key: &str) -> String {
    v[key].as_str().unwrap_or_default().trim().to_string()
}

/// 把 Agent 的回答解析成（总评，问题列表）。严重程度不认识的按 minor 算，没有标题的丢掉。
pub fn parse(output: &str) -> Result<(String, Vec<Finding>)> {
    let v = extract_json(output)?;
    let findings = v["findings"]
        .as_array()
        .map(|a| a.as_slice())
        .unwrap_or_default()
        .iter()
        .filter_map(|f| {
            let title = text(f, "title");
            let detail = text(f, "detail");
            if title.is_empty() && detail.is_empty() {
                return None;
            }
            let severity = match text(f, "severity").to_lowercase().as_str() {
                "blocker" | "critical" => "blocker",
                "major" | "high" => "major",
                _ => "minor",
            };
            Some(Finding {
                severity: severity.into(),
                file: Some(text(f, "file")).filter(|s| !s.is_empty()),
                line: f["line"].as_i64().filter(|n| *n > 0),
                title: if title.is_empty() {
                    detail.chars().take(40).collect()
                } else {
                    title
                },
                detail,
                suggestion: text(f, "suggestion"),
            })
        })
        .collect();
    Ok((text(&v, "summary"), findings))
}

/// 审查运行结束：成功就解析入库，失败（或回答不是约定的 JSON）记下原因。
pub fn finish(
    conn: &Connection,
    run_id: &str,
    output: std::result::Result<&str, &str>,
) -> Result<Option<CodeReview>> {
    if by_run(conn, run_id)?.is_none() {
        return Ok(None);
    }
    let now = crate::util::now();
    let parsed = match output {
        Ok(text) => parse(text).map_err(|e| format!("审查者的回答不是约定的格式，没法解析：{e}")),
        Err(e) => Err(e.to_string()),
    };
    match parsed {
        Ok((summary, findings)) => conn.execute(
            "UPDATE code_review SET status = 'done', summary = ?2, findings_json = ?3, error = NULL, updated_at = ?4
             WHERE run_id = ?1",
            rusqlite::params![run_id, summary, super::to_json(&findings), now],
        )?,
        Err(e) => conn.execute(
            "UPDATE code_review SET status = 'failed', error = ?2, updated_at = ?3 WHERE run_id = ?1",
            rusqlite::params![run_id, e, now],
        )?,
    };
    by_run(conn, run_id)
}

/// 把问题列表写成打回意见，方便一键交回开发的 Agent。
pub fn as_rejection_note(findings: &[Finding]) -> String {
    findings
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let at = match (&f.file, f.line) {
                (Some(p), Some(l)) => format!("（{p}:{l}）"),
                (Some(p), None) => format!("（{p}）"),
                _ => String::new(),
            };
            let mut s = format!("{}. [{}] {}{at}", i + 1, f.severity, f.title);
            if !f.detail.is_empty() && f.detail != f.title {
                s.push_str(&format!("\n   {}", f.detail));
            }
            if !f.suggestion.is_empty() {
                s.push_str(&format!("\n   建议：{}", f.suggestion));
            }
            s
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_findings_and_tolerates_sloppy_output() {
        let (summary, f) = parse(
            "看完了。\n```json\n{\"summary\":\"有一处边界没处理\",\"findings\":[\
             {\"severity\":\"Major\",\"file\":\"src/a.rs\",\"line\":12,\"title\":\"金额可能为负\",\"detail\":\"没有下限\",\"suggestion\":\"加 max(0)\"},\
             {\"severity\":\"nit\",\"file\":\"\",\"line\":null,\"title\":\"\",\"detail\":\"命名不一致\"},\
             {\"title\":\"\",\"detail\":\"\"}]}\n```",
        )
        .unwrap();
        assert_eq!(summary, "有一处边界没处理");
        assert_eq!(f.len(), 2);
        assert_eq!(
            (f[0].severity.as_str(), f[0].file.as_deref(), f[0].line),
            ("major", Some("src/a.rs"), Some(12))
        );
        assert_eq!(
            (f[1].severity.as_str(), f[1].file.as_deref()),
            ("minor", None)
        );
        assert_eq!(f[1].title, "命名不一致");
        assert_eq!(parse("{\"summary\":\"没问题\"}").unwrap().1, vec![]);
        assert!(parse("我觉得挺好").is_err());

        let note = as_rejection_note(&f);
        assert!(note.starts_with("1. [major] 金额可能为负（src/a.rs:12）\n   没有下限\n   建议：加 max(0)\n2. [minor] 命名不一致"), "{note}");
    }
}
