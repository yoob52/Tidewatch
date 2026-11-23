//! AI 预审的结果与问题清单，以及拆分建议 / 变更影响分析（等人确认后才生效）。
use super::*;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

pub const SEVERITIES: &[&str] = &["blocker", "major", "minor", "suggestion"];
/// 问题的处理状态。`open`、`pending_confirm` 算没处理完。
pub const RESOLUTIONS: &[&str] = &["open", "accepted", "manual", "ignored", "pending_confirm"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Review {
    pub id: String,
    pub ticket_id: String,
    pub prd_version_id: Option<String>,
    pub run_id: Option<String>,
    pub summary: String,
    pub scope: Option<Value>,
    pub criteria_draft: Vec<String>,
    /// 预审没能产出可用结果的原因（运行失败、输出不是约定的 JSON）。
    pub error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    pub review_id: String,
    pub ticket_id: String,
    pub code: String,
    pub severity: String,
    pub category: String,
    pub location: String,
    pub problem: String,
    pub question: Option<String>,
    pub suggestion: Option<String>,
    pub resolution: String,
    pub resolution_note: Option<String>,
    pub resolved_in_version_id: Option<String>,
    pub updated_at: String,
}

/// 从 Agent 的最后一条消息里取出 JSON 对象：允许包在 ```json 代码块里，或前后带说明文字。
pub fn extract_json(text: &str) -> Result<Value> {
    let text = text.trim();
    if let Some(v) = parse_object(text) {
        return Ok(v);
    }
    // 代码块：取最后一个能解析的
    let mut rest = text;
    let mut found = None;
    while let Some(i) = rest.find("```") {
        let after = &rest[i + 3..];
        let Some(j) = after.find("```") else { break };
        let block = &after[..j];
        let body = block.split_once('\n').map(|(_, b)| b).unwrap_or(block);
        for candidate in [body, block] {
            if let Some(v) = parse_object(candidate.trim()) {
                found = Some(v);
                break;
            }
        }
        rest = &after[j + 3..];
    }
    if let Some(v) = found {
        return Ok(v);
    }
    // 第一个 { 到最后一个 }
    if let (Some(i), Some(j)) = (text.find('{'), text.rfind('}')) {
        if i < j {
            if let Some(v) = parse_object(&text[i..=j]) {
                return Ok(v);
            }
        }
    }
    Err(Error::invalid(
        "Agent 的输出里没有找到约定格式的 JSON。可以重试，或换一个模型。",
    ))
}

/// 解析成 JSON 对象；失败时再试一次把字符串里没转义的引号补上转义。
fn parse_object(s: &str) -> Option<Value> {
    let parse = |t: &str| {
        serde_json::from_str::<Value>(t)
            .ok()
            .filter(Value::is_object)
    };
    parse(s).or_else(|| parse(&escape_stray_quotes(s)))
}

/// 模型常在中文字符串里直接用半角引号（`可明确写"仅支持 number"。`）。
/// 字符串里的 `"` 后面（跳过空白）不是 `,` `"` `:` `}` `]` 或结尾时，当作内容转义掉。
fn escape_stray_quotes(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len() + 8);
    let mut in_str = false;
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if in_str && c == '\\' {
            out.push(c);
            if let Some(&n) = chars.get(i + 1) {
                out.push(n);
            }
            i += 2;
            continue;
        }
        if c == '"' {
            if !in_str {
                in_str = true;
            } else {
                let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
                // 紧挨着的 `""` 是引用短语收尾，中间隔了空白的是漏了逗号
                let quoted_tail = chars.get(i + 1) == Some(&'"');
                if !quoted_tail && matches!(next, None | Some(',' | '"' | ':' | '}' | ']')) {
                    in_str = false;
                } else {
                    out.push('\\');
                }
            }
        }
        out.push(c);
        i += 1;
    }
    out
}

fn str_of(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string()
}

fn opt_str(v: &Value, key: &str) -> Option<String> {
    Some(str_of(v, key)).filter(|s| !s.is_empty())
}

fn strings(v: Option<&Value>) -> Vec<String> {
    v.and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

const REVIEW_COLS: &str =
    "id, ticket_id, prd_version_id, run_id, summary, scope_json, criteria_draft_json, error, created_at";

fn review_row(r: &rusqlite::Row) -> rusqlite::Result<Review> {
    Ok(Review {
        id: r.get(0)?,
        ticket_id: r.get(1)?,
        prd_version_id: r.get(2)?,
        run_id: r.get(3)?,
        summary: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        scope: r
            .get::<_, Option<String>>(5)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        criteria_draft: from_json(r.get(6)?),
        error: r.get(7)?,
        created_at: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
    })
}

const ISSUE_COLS: &str = "id, review_id, ticket_id, code, severity, category, location, problem, question, suggestion, resolution, resolution_note, resolved_in_version_id, updated_at";

fn issue_row(r: &rusqlite::Row) -> rusqlite::Result<Issue> {
    Ok(Issue {
        id: r.get(0)?,
        review_id: r.get(1)?,
        ticket_id: r.get(2)?,
        code: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        severity: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        category: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        location: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        problem: r.get::<_, Option<String>>(7)?.unwrap_or_default(),
        question: r.get(8)?,
        suggestion: r.get(9)?,
        resolution: r
            .get::<_, Option<String>>(10)?
            .unwrap_or_else(|| "open".into()),
        resolution_note: r.get(11)?,
        resolved_in_version_id: r.get(12)?,
        updated_at: r.get::<_, Option<String>>(13)?.unwrap_or_default(),
    })
}

/// 记下一次预审的结果。`output` 是 Agent 的最后一条消息；解析不了时仍然留一条记录，带上原因和原文。
pub fn save_result(
    conn: &mut Connection,
    ticket_id: &str,
    prd_version_id: Option<&str>,
    run_id: &str,
    output: std::result::Result<&str, &str>,
) -> Result<Review> {
    let id = crate::util::new_id();
    let now = crate::util::now();
    let parsed = match output {
        // 能解析成对象还不够：没有 issues 数组的输出（比如 `{}`）不能当成"没有问题"
        Ok(text) => {
            extract_json(text)
                .map_err(|e| e.to_string())
                .and_then(|v| match v.get("issues") {
                    Some(Value::Array(_)) => Ok(v),
                    _ => Err("预审输出里没有 issues 数组，不是约定的格式。".to_string()),
                })
        }
        Err(e) => Err(e.to_string()),
    };
    let raw = output.unwrap_or_default();
    // 没有对应的运行（比如启动时补记的失败）存 NULL
    let run_id = Some(run_id).filter(|r| !r.is_empty());
    let tx = conn.transaction()?;
    match parsed {
        Err(e) => {
            tx.execute(
                "INSERT INTO review (id, ticket_id, prd_version_id, run_id, summary, raw_json, error, created_at)
                 VALUES (?1, ?2, ?3, ?4, '', ?5, ?6, ?7)",
                rusqlite::params![id, ticket_id, prd_version_id, run_id, raw, e, now],
            )?;
        }
        Ok(v) => {
            tx.execute(
                "INSERT INTO review (id, ticket_id, prd_version_id, run_id, summary, raw_json, scope_json, criteria_draft_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                rusqlite::params![
                    id,
                    ticket_id,
                    prd_version_id,
                    run_id,
                    str_of(&v, "summary"),
                    raw,
                    v.get("scope").filter(|s| s.is_object()).map(Value::to_string),
                    to_json(&strings(v.get("acceptance_criteria_draft"))),
                    now
                ],
            )?;
            // 编号接着这张工单已有的问题往下排，重新预审后不会撞号
            let mut n: i64 = tx.query_row(
                "SELECT COUNT(*) FROM review_issue WHERE ticket_id = ?1",
                [ticket_id],
                |r| r.get(0),
            )?;
            for issue in v
                .get("issues")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                let problem = str_of(issue, "problem");
                if problem.is_empty() {
                    continue;
                }
                n += 1;
                let severity = str_of(issue, "severity").to_lowercase();
                let severity = if SEVERITIES.contains(&severity.as_str()) {
                    severity
                } else {
                    "minor".to_string()
                };
                tx.execute(
                    "INSERT INTO review_issue (id, review_id, ticket_id, code, severity, category, location, problem,
                     question, suggestion, resolution, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 'open', ?11)",
                    rusqlite::params![
                        crate::util::new_id(),
                        id,
                        ticket_id,
                        format!("I-{n}"),
                        severity,
                        str_of(issue, "category"),
                        str_of(issue, "location"),
                        problem,
                        opt_str(issue, "question"),
                        opt_str(issue, "suggestion"),
                        now
                    ],
                )?;
            }
        }
    }
    tx.commit()?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Review> {
    conn.query_row(
        &format!("SELECT {REVIEW_COLS} FROM review WHERE id = ?1"),
        [id],
        review_row,
    )
    .map_err(|_| not_found("预审记录", id))
}

/// 这张工单的预审记录，按时间先后，最近一次在最后。
pub fn list(conn: &Connection, ticket_id: &str) -> Result<Vec<Review>> {
    let mut st = conn.prepare(&format!(
        "SELECT {REVIEW_COLS} FROM review WHERE ticket_id = ?1 ORDER BY created_at, rowid"
    ))?;
    let rows = st
        .query_map([ticket_id], review_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 这张工单的全部问题（跨多次预审），按提出顺序。
pub fn issues(conn: &Connection, ticket_id: &str) -> Result<Vec<Issue>> {
    let mut st = conn.prepare(&format!(
        "SELECT {ISSUE_COLS} FROM review_issue WHERE ticket_id = ?1 ORDER BY rowid"
    ))?;
    let rows = st
        .query_map([ticket_id], issue_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn issue(conn: &Connection, id: &str) -> Result<Issue> {
    conn.query_row(
        &format!("SELECT {ISSUE_COLS} FROM review_issue WHERE id = ?1"),
        [id],
        issue_row,
    )
    .map_err(|_| not_found("预审问题", id))
}

/// 每张工单还没处理的问题数（看板和列表用）。
pub fn open_counts(conn: &Connection) -> Result<std::collections::HashMap<String, usize>> {
    let mut st = conn.prepare(
        "SELECT ticket_id, COUNT(*) FROM review_issue
         WHERE COALESCE(resolution, 'open') IN ('open', 'pending_confirm') GROUP BY ticket_id",
    )?;
    let rows = st
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

pub fn is_unresolved(resolution: &str) -> bool {
    matches!(resolution, "open" | "pending_confirm")
}

/// 在 `heading` 这一节的末尾加一条；没有这一节就在文末新建。
pub fn append_to_section(prd: &str, heading: &str, bullet: &str) -> String {
    let lines: Vec<&str> = prd.lines().collect();
    let level = |l: &str| l.chars().take_while(|c| *c == '#').count();
    let start = lines.iter().position(|l| {
        let n = level(l);
        n > 0 && l[n..].trim().contains(heading)
    });
    let Some(start) = start else {
        let mut out = prd.trim_end().to_string();
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(&format!("## {heading}\n- {bullet}\n"));
        return out;
    };
    let depth = level(lines[start]);
    let mut end = lines[start + 1..]
        .iter()
        .position(|l| {
            let n = level(l);
            n > 0 && n <= depth && l[n..].starts_with(' ')
        })
        .map(|i| start + 1 + i)
        .unwrap_or(lines.len());
    while end > start + 1 && lines[end - 1].trim().is_empty() {
        end -= 1;
    }
    let mut out: Vec<String> = lines[..end].iter().map(|l| l.to_string()).collect();
    out.push(format!("- {bullet}"));
    out.extend(lines[end..].iter().map(|l| l.to_string()));
    out.join("\n") + "\n"
}

fn one_line(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// 采纳 / 忽略一个问题后正文会变成什么样（给用户预览，可以再改）。
/// 采纳：`location` 是正文里唯一出现的一段原文时，建议插在那一段后面；否则记到"澄清说明"。
/// 忽略：理由记到"不做范围"。
pub fn proposed_prd(prd: &str, issue: &Issue, action: &str, note: &str) -> Result<String> {
    match action {
        "accept" => {
            let suggestion = issue
                .suggestion
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| Error::invalid("这个问题没有给出可采纳的建议，请手动修改正文。"))?;
            let loc = issue.location.trim();
            if !loc.is_empty() && prd.matches(loc).count() == 1 {
                let at = prd.find(loc).unwrap() + loc.len();
                // 插到这一段的末尾（下一个空行之前）
                let end = prd[at..].find("\n\n").map(|i| at + i).unwrap_or(prd.len());
                let mut out = prd[..end].trim_end().to_string();
                out.push_str("\n\n");
                out.push_str(suggestion);
                out.push_str(if end < prd.len() { "" } else { "\n" });
                out.push_str(&prd[end..]);
                return Ok(out);
            }
            let bullet = if loc.is_empty() {
                one_line(suggestion)
            } else {
                format!("（关于「{}」）{}", one_line(loc), one_line(suggestion))
            };
            Ok(append_to_section(prd, "澄清说明", &bullet))
        }
        "ignore" => {
            let note = required(note, "忽略的理由")?;
            Ok(append_to_section(
                prd,
                "不做范围",
                &format!("{}（{}）", one_line(&issue.problem), one_line(&note)),
            ))
        }
        other => Err(Error::invalid(format!("这个操作不会修改正文：{other}"))),
    }
}

/// 处理一个问题。
/// - `accept`：采纳建议。`content` 是采纳后的正文（不传则用 [`proposed_prd`]），存为新版本（来源：AI 采纳）。
/// - `manual`：手动修改。传了 `content` 就存为新版本，否则认为正文已经改过。
/// - `ignore`：忽略，必须写理由；理由写进正文的"不做范围"。
/// - `pending`：待向需求方确认。
/// - `reopen`：重新打开。
pub fn resolve(
    conn: &mut Connection,
    issue_id: &str,
    action: &str,
    note: Option<&str>,
    content: Option<&str>,
) -> Result<Issue> {
    let it = issue(conn, issue_id)?;
    let ticket = super::tickets::get(conn, &it.ticket_id)?;
    let note = note.map(str::trim).filter(|n| !n.is_empty());
    let current = super::tickets::current_prd(conn, &ticket.id)?
        .map(|v| v.content_md)
        .unwrap_or_default();
    let (resolution, new_prd, origin) = match action {
        "accept" => {
            let text = match content {
                Some(c) => c.to_string(),
                None => proposed_prd(&current, &it, "accept", "")?,
            };
            ("accepted", Some(text), "ai_suggestion")
        }
        "manual" => ("manual", content.map(String::from), "edit"),
        "ignore" => {
            let reason = note.ok_or_else(|| Error::invalid("忽略问题需要写明理由。"))?;
            let text = match content {
                Some(c) => c.to_string(),
                None => proposed_prd(&current, &it, "ignore", reason)?,
            };
            ("ignored", Some(text), "edit")
        }
        "pending" => ("pending_confirm", None, ""),
        "reopen" => ("open", None, ""),
        other => return Err(Error::invalid(format!("未知的处理方式：{other}"))),
    };
    let mut version_id = None;
    if let Some(text) = new_prd.filter(|t| *t != current) {
        let label = match resolution {
            "accepted" => "采纳",
            "ignored" => "忽略",
            _ => "处理",
        };
        let v = super::tickets::save_prd_as(
            conn,
            &ticket.id,
            &text,
            origin,
            Some(&format!("{label} {}：{}", it.code, one_line(&it.problem))),
        )?;
        version_id = Some(v.id);
    }
    conn.execute(
        "UPDATE review_issue SET resolution = ?2, resolution_note = ?3, resolved_in_version_id = ?4, updated_at = ?5
         WHERE id = ?1",
        rusqlite::params![it.id, resolution, note, version_id, crate::util::now()],
    )?;
    add_timeline(
        conn,
        &ticket.id,
        "issue_resolved",
        json!({ "issue_id": it.id, "code": it.code, "resolution": resolution, "note": note }),
    )?;
    issue(conn, &it.id)
}

/// 把"待确认"的问题整理成一段可以直接发给需求方的文字。
pub fn export_questions(conn: &Connection, ticket_id: &str) -> Result<String> {
    let ticket = super::tickets::get(conn, ticket_id)?;
    let pending: Vec<Issue> = issues(conn, &ticket.id)?
        .into_iter()
        .filter(|i| i.resolution == "pending_confirm")
        .collect();
    if pending.is_empty() {
        return Ok(String::new());
    }
    let mut out = format!(
        "关于「{}」（{}），有 {} 个问题需要确认：\n",
        ticket.title,
        ticket.key,
        pending.len()
    );
    for (n, i) in pending.iter().enumerate() {
        let q = i.question.as_deref().unwrap_or(&i.problem);
        out.push_str(&format!("\n{}. {}", n + 1, one_line(q)));
        if !i.location.trim().is_empty() {
            out.push_str(&format!("\n   相关原文：{}", one_line(&i.location)));
        }
        if i.question.is_some() {
            out.push_str(&format!("\n   背景：{}", one_line(&i.problem)));
        }
        out.push('\n');
    }
    Ok(out)
}

// ---------- 拆分建议 / 影响分析 ----------

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Proposal {
    pub id: String,
    pub ticket_id: String,
    /// split | impact
    pub kind: String,
    pub prd_version_id: Option<String>,
    pub run_id: Option<String>,
    /// running | pending | applied | discarded | failed
    pub status: String,
    pub content: Value,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

const PROPOSAL_COLS: &str =
    "id, ticket_id, kind, prd_version_id, run_id, status, content_json, error, created_at, updated_at";

fn proposal_row(r: &rusqlite::Row) -> rusqlite::Result<Proposal> {
    Ok(Proposal {
        id: r.get(0)?,
        ticket_id: r.get(1)?,
        kind: r.get(2)?,
        prd_version_id: r.get(3)?,
        run_id: r.get(4)?,
        status: r.get(5)?,
        content: r
            .get::<_, Option<String>>(6)?
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null),
        error: r.get(7)?,
        created_at: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
        updated_at: r.get::<_, Option<String>>(9)?.unwrap_or_default(),
    })
}

/// 开始生成一份建议（Run 已入队）。同一工单同一类里还没处理的旧建议作废。
pub fn begin_proposal(
    conn: &Connection,
    ticket_id: &str,
    kind: &str,
    prd_version_id: Option<&str>,
    run_id: &str,
) -> Result<Proposal> {
    let now = crate::util::now();
    conn.execute(
        "UPDATE proposal SET status = 'discarded', updated_at = ?3
         WHERE ticket_id = ?1 AND kind = ?2 AND status IN ('running', 'pending', 'failed')",
        rusqlite::params![ticket_id, kind, now],
    )?;
    let id = crate::util::new_id();
    conn.execute(
        "INSERT INTO proposal (id, ticket_id, kind, prd_version_id, run_id, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?6)",
        rusqlite::params![id, ticket_id, kind, prd_version_id, run_id, now],
    )?;
    proposal(conn, &id)
}

/// Run 结束：写入解析后的建议，或失败原因。
pub fn finish_proposal(
    conn: &Connection,
    run_id: &str,
    output: std::result::Result<&str, &str>,
) -> Result<Option<Proposal>> {
    let id: Option<String> = conn
        .query_row(
            "SELECT id FROM proposal WHERE run_id = ?1 AND status = 'running'",
            [run_id],
            |r| r.get(0),
        )
        .ok();
    let Some(id) = id else { return Ok(None) };
    let parsed = match output {
        Ok(text) => extract_json(text).map_err(|e| e.to_string()),
        Err(e) => Err(e.to_string()),
    };
    let raw = output.unwrap_or_default();
    let now = crate::util::now();
    match parsed {
        Ok(v) => conn.execute(
            "UPDATE proposal SET status = 'pending', content_json = ?2, raw = ?3, updated_at = ?4 WHERE id = ?1",
            rusqlite::params![id, v.to_string(), raw, now],
        )?,
        Err(e) => conn.execute(
            "UPDATE proposal SET status = 'failed', error = ?2, raw = ?3, updated_at = ?4 WHERE id = ?1",
            rusqlite::params![id, e, raw, now],
        )?,
    };
    proposal(conn, &id).map(Some)
}

pub fn proposal(conn: &Connection, id: &str) -> Result<Proposal> {
    conn.query_row(
        &format!("SELECT {PROPOSAL_COLS} FROM proposal WHERE id = ?1"),
        [id],
        proposal_row,
    )
    .map_err(|_| not_found("建议", id))
}

/// 这张工单最近的一份建议（作废的不算）。
pub fn latest_proposal(conn: &Connection, ticket_id: &str, kind: &str) -> Result<Option<Proposal>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            &format!(
                "SELECT {PROPOSAL_COLS} FROM proposal WHERE ticket_id = ?1 AND kind = ?2 AND status != 'discarded'
                 ORDER BY created_at DESC, rowid DESC LIMIT 1"
            ),
            [ticket_id, kind],
            proposal_row,
        )
        .optional()?)
}

pub fn set_proposal_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    conn.execute(
        "UPDATE proposal SET status = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![id, status, crate::util::now()],
    )?;
    Ok(())
}

/// 子任务建议里的一项（拆分建议的 `tasks`、影响分析的 `new_tasks`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskDraft {
    #[serde(rename = "ref")]
    pub r#ref: String,
    pub kind: String,
    /// 项目名或项目 id。
    pub project: String,
    pub title: String,
    pub spec: String,
    pub acceptance_criteria: Vec<String>,
    pub depends_on: Vec<String>,
    /// api_doc：依赖的子任务产出接口文档就解锁；done：等它完成。
    pub dependency_kind: Option<String>,
    pub agent: Option<String>,
}

/// 影响分析里对一个已有子任务的判断。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TaskImpact {
    pub task_id: String,
    /// none | rework
    pub impact: String,
    pub instruction: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::tickets;

    fn ticket(conn: &mut Connection) -> String {
        tickets::create(
            conn,
            tickets::TicketInput {
                title: "优惠券叠加".into(),
                kind: "feature_mixed".into(),
                content_md: "# 优惠券叠加\n\n## 功能需求\n用户可以叠加使用优惠券。\n\n## 验收标准\n- 金额不为负\n".into(),
                ..Default::default()
            },
        )
        .unwrap()
        .id
    }

    const OUTPUT: &str = r#"我看完了。
```json
{
  "summary": "缺少叠加上限",
  "scope": { "backend": true, "frontend": true },
  "issues": [
    { "id": "I-1", "severity": "blocker", "category": "data", "location": "用户可以叠加使用优惠券。",
      "problem": "没有说最多叠加几张", "question": "最多可以叠加几张？", "suggestion": "最多叠加 2 张优惠券。" },
    { "id": "I-2", "severity": "SUGGESTION", "category": "flow", "location": "不存在的原文",
      "problem": "没写退款时怎么退券", "suggestion": "退款时按比例退回优惠券。" },
    { "id": "I-3", "severity": "weird", "category": "x", "location": "", "problem": "" }
  ],
  "acceptance_criteria_draft": ["叠加后金额不为负", " "]
}
```"#;

    #[test]
    fn repairs_unescaped_quotes_inside_strings() {
        let raw = "```json
{\"issues\": [{\"question\": \"可明确写\"仅支持 number\"。\", \"n\": 1}]}
```";
        let v = extract_json(raw).unwrap();
        assert_eq!(v["issues"][0]["question"], "可明确写\"仅支持 number\"。");
        assert_eq!(v["issues"][0]["n"], 1);
        assert!(extract_json("没有 JSON").is_err());
        let v = extract_json(r#"{"s": "写成"xxx"", "n": 1}"#).unwrap();
        assert_eq!(v["s"], "写成\"xxx\"");
        // 漏了逗号不能把两条合成一条
        assert!(extract_json(r#"{"c": ["a" "b"]}"#).is_err());
    }

    #[test]
    fn extracts_json_from_fenced_or_bare_output() {
        assert!(extract_json(r#"{"a":1}"#).unwrap()["a"] == 1);
        assert!(extract_json("结果如下：\n```json\n{\"a\":2}\n```\n完。").unwrap()["a"] == 2);
        assert!(extract_json("前言 {\"a\": {\"b\": 3}} 后记").unwrap()["a"]["b"] == 3);
        assert!(extract_json("没有 JSON").is_err());
        assert!(extract_json("[1,2]").is_err());
    }

    #[test]
    fn saves_review_issues_and_keeps_raw_output_on_parse_failure() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let t = ticket(&mut conn);
        let r = save_result(&mut conn, &t, None, "run1", Ok(OUTPUT)).unwrap();
        assert_eq!(r.summary, "缺少叠加上限");
        assert_eq!(r.criteria_draft, vec!["叠加后金额不为负"]);
        let all = issues(&conn, &t).unwrap();
        assert_eq!(all.len(), 2, "没有问题描述的条目不收");
        assert_eq!(all[0].code, "I-1");
        assert_eq!(all[1].severity, "suggestion");
        assert!(all.iter().all(|i| i.resolution == "open"));

        let bad = save_result(&mut conn, &t, None, "run2", Ok("抱歉，我无法完成")).unwrap();
        assert!(bad.error.is_some());
        let failed = save_result(&mut conn, &t, None, "run3", Err("Agent 退出码 1")).unwrap();
        assert_eq!(failed.error.as_deref(), Some("Agent 退出码 1"));
        assert_eq!(issues(&conn, &t).unwrap().len(), 2);
        // 再审一次，编号接着排
        let again = save_result(&mut conn, &t, None, "run4", Ok(OUTPUT)).unwrap();
        assert_eq!(issues(&conn, &t).unwrap()[2].code, "I-3");
        // 记录按时间先后排，界面取最后一条当作最近一次
        let all = list(&conn, &t).unwrap();
        assert_eq!(all.len(), 4);
        assert_eq!(all[0].id, r.id);
        assert_eq!(all[3].id, again.id);
    }

    #[test]
    fn output_without_an_issue_list_is_a_failed_review() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let t = ticket(&mut conn);
        for raw in ["{}", r#"{"summary":"不错","issues":"无"}"#] {
            let r = save_result(&mut conn, &t, None, "", Ok(raw)).unwrap();
            assert!(r.error.is_some(), "{raw} 不应该算预审成功");
            assert_eq!(r.summary, "");
        }
        assert!(issues(&conn, &t).unwrap().is_empty());
        // 明确给出空数组才是"没有问题"
        let ok = save_result(
            &mut conn,
            &t,
            None,
            "",
            Ok(r#"{"summary":"清楚","issues":[]}"#),
        )
        .unwrap();
        assert_eq!(ok.error, None);
    }

    #[test]
    fn resolving_issues_writes_prd_versions() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let t = ticket(&mut conn);
        save_result(&mut conn, &t, None, "run1", Ok(OUTPUT)).unwrap();
        let all = issues(&conn, &t).unwrap();

        // 采纳：原文唯一，建议插在那一段后面
        let done = resolve(&mut conn, &all[0].id, "accept", None, None).unwrap();
        assert_eq!(done.resolution, "accepted");
        let v = tickets::current_prd(&conn, &t).unwrap().unwrap();
        assert_eq!(done.resolved_in_version_id.as_deref(), Some(v.id.as_str()));
        assert_eq!(v.origin.as_deref(), Some("ai_suggestion"));
        assert_eq!(v.author, "ai");
        assert!(v
            .content_md
            .contains("用户可以叠加使用优惠券。\n\n最多叠加 2 张优惠券。\n\n## 验收标准"));

        // 忽略必须写理由，理由进"不做范围"
        assert!(resolve(&mut conn, &all[1].id, "ignore", Some("  "), None).is_err());
        let ig = resolve(&mut conn, &all[1].id, "ignore", Some("本期不做退款"), None).unwrap();
        assert_eq!(ig.resolution, "ignored");
        let v = tickets::current_prd(&conn, &t).unwrap().unwrap();
        assert!(v
            .content_md
            .ends_with("## 不做范围\n- 没写退款时怎么退券（本期不做退款）\n"));
        assert_eq!(v.version, 3);

        // 待确认：导出问题
        resolve(&mut conn, &all[0].id, "pending", None, None).unwrap();
        let text = export_questions(&conn, &t).unwrap();
        assert!(
            text.contains("1. 最多可以叠加几张？") && text.contains("背景：没有说最多叠加几张")
        );
        assert_eq!(tickets::prd_versions(&conn, &t).unwrap().len(), 3);
    }

    #[test]
    fn accept_without_matching_location_goes_to_clarifications() {
        let issue = Issue {
            id: "i".into(),
            review_id: "r".into(),
            ticket_id: "t".into(),
            code: "I-1".into(),
            severity: "major".into(),
            category: "".into(),
            location: "找不到的原文".into(),
            problem: "p".into(),
            question: None,
            suggestion: Some("补充一句".into()),
            resolution: "open".into(),
            resolution_note: None,
            resolved_in_version_id: None,
            updated_at: "".into(),
        };
        let prd = "# 标题\n\n## 澄清说明\n- 已有一条\n\n## 其它\n内容\n";
        let out = proposed_prd(prd, &issue, "accept", "").unwrap();
        assert_eq!(
            out,
            "# 标题\n\n## 澄清说明\n- 已有一条\n- （关于「找不到的原文」）补充一句\n\n## 其它\n内容\n"
        );
    }

    #[test]
    fn finalize_needs_resolved_blockers_and_criteria() {
        let mut conn = crate::db::open_in_memory().unwrap();
        let t = ticket(&mut conn);
        tickets::transition(&mut conn, &t, "ai_review").unwrap();
        save_result(&mut conn, &t, None, "run1", Ok(OUTPUT)).unwrap();
        tickets::transition(&mut conn, &t, "human_review").unwrap();
        let err = tickets::transition(&mut conn, &t, "finalized").unwrap_err();
        assert!(err.to_string().contains("阻塞级问题"), "{err}");
        let all = issues(&conn, &t).unwrap();
        resolve(&mut conn, &all[0].id, "accept", None, None).unwrap();
        let err = tickets::transition(&mut conn, &t, "finalized").unwrap_err();
        assert!(err.to_string().contains("验收标准"), "{err}");
        tickets::set_criteria(&conn, &t, vec!["叠加后金额不为负".into()]).unwrap();
        let done = tickets::transition(&mut conn, &t, "finalized").unwrap();
        assert_eq!(done.finalized_version_id, done.current_version_id);
        assert!(tickets::save_prd(&mut conn, &t, "改一下", None).is_err());

        // 定稿后只能通过"发起变更"回到审核
        assert!(tickets::transition(&mut conn, &t, "human_review").is_err());
        assert!(tickets::request_change(&mut conn, &t, " ").is_err());
        let back = tickets::request_change(&mut conn, &t, "运营要加上限").unwrap();
        assert_eq!(back.status, "human_review");
        assert_eq!(back.change_from_version_id, done.finalized_version_id);
        tickets::save_prd(&mut conn, &t, "改一下", None).unwrap();
    }
}
