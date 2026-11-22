//! 自定义工作流：在内置状态机上加自己的环节。
//!
//! 内置的八个状态和它们之间的转换不变（引擎依赖它们）。一个工作流给某几类工单加上
//! 若干个环节，每个环节卡在「定稿」或「完成」之前，要有人逐个点通过，工单才能往下走。
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use super::tickets::{self, Ticket};
use super::{add_timeline, from_json, required, to_json};
use crate::{Error, Result};

/// 环节能卡在哪个状态之前。
pub const GATES: &[&str] = &["finalized", "done"];
const MAX_STAGES: usize = 12;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stage {
    /// 留空时保存会生成；之后改名不影响已经通过的记录。
    pub id: String,
    pub name: String,
    /// `finalized`：人工审核时要通过，之后才能定稿；`done`：验收时要通过，之后才能完成。
    pub gate: String,
    /// 给通过的人看的说明（要检查什么）。
    pub hint: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorkflowConfig {
    /// 适用的工单类型。一种类型同一时间只能有一个启用的工作流。
    pub ticket_types: Vec<String>,
    pub stages: Vec<Stage>,
    /// 子任务全部合并后自动完成工单。关掉后要有人手动点完成。
    pub auto_done: bool,
}

impl Default for WorkflowConfig {
    fn default() -> Self {
        WorkflowConfig {
            ticket_types: vec![],
            stages: vec![],
            auto_done: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub config: WorkflowConfig,
    pub enabled: bool,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct WorkflowInput {
    pub name: String,
    pub config: WorkflowConfig,
    pub enabled: bool,
}

/// 一个环节在某张工单上的情况。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct StageState {
    pub id: String,
    pub name: String,
    pub gate: String,
    pub hint: String,
    pub passed_at: Option<String>,
    pub actor: Option<String>,
    pub note: Option<String>,
    /// 现在轮到它：工单在对应的状态，前面的环节都通过了。
    pub current: bool,
}

fn row(r: &rusqlite::Row) -> rusqlite::Result<Workflow> {
    Ok(Workflow {
        id: r.get(0)?,
        name: r.get(1)?,
        config: from_json(r.get(2)?),
        enabled: r.get::<_, i64>(3)? != 0,
        created_at: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
    })
}

const COLS: &str = "id, name, config_json, enabled, created_at";

pub fn list(conn: &Connection) -> Result<Vec<Workflow>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM workflow ORDER BY created_at, id"
    ))?;
    let v = st.query_map([], row)?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

pub fn get(conn: &Connection, id: &str) -> Result<Workflow> {
    conn.query_row(
        &format!("SELECT {COLS} FROM workflow WHERE id = ?1"),
        [id],
        row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("工作流不存在：{id}")))
}

/// 这类工单现在用的工作流。
pub fn for_kind(conn: &Connection, kind: &str) -> Result<Option<Workflow>> {
    Ok(list(conn)?
        .into_iter()
        .find(|w| w.enabled && w.config.ticket_types.iter().any(|t| t == kind)))
}

fn check(
    conn: &Connection,
    own_id: Option<&str>,
    input: &WorkflowInput,
) -> Result<(String, WorkflowConfig)> {
    let name = required(&input.name, "名称")?;
    let mut c = input.config.clone();
    c.ticket_types.sort();
    c.ticket_types.dedup();
    if c.ticket_types.is_empty() {
        return Err(Error::invalid("至少选一种工单类型。"));
    }
    if let Some(bad) = c
        .ticket_types
        .iter()
        .find(|t| !tickets::TYPES.contains(&t.as_str()))
    {
        return Err(Error::invalid(format!("未知工单类型：{bad}")));
    }
    if input.enabled {
        for other in list(conn)? {
            if !other.enabled || Some(other.id.as_str()) == own_id {
                continue;
            }
            if let Some(t) = c
                .ticket_types
                .iter()
                .find(|t| other.config.ticket_types.contains(t))
            {
                return Err(Error::invalid(format!(
                    "「{}」已经在用工作流「{}」。一种工单类型同一时间只能有一个启用的工作流。",
                    tickets::type_label(t),
                    other.name
                )));
            }
        }
    }
    if c.stages.len() > MAX_STAGES {
        return Err(Error::invalid(format!("环节最多 {MAX_STAGES} 个。")));
    }
    let mut seen: Vec<String> = vec![];
    for s in &mut c.stages {
        s.name = required(&s.name, "环节名称")?;
        if seen.contains(&s.name) {
            return Err(Error::invalid(format!("环节名称重复：{}", s.name)));
        }
        seen.push(s.name.clone());
        if !GATES.contains(&s.gate.as_str()) {
            return Err(Error::invalid(format!(
                "环节「{}」要选卡在哪里：定稿之前，或者完成之前。",
                s.name
            )));
        }
        s.hint = s.hint.trim().to_string();
        if s.id.trim().is_empty() {
            s.id = crate::util::new_id();
        }
    }
    Ok((name, c))
}

pub fn create(conn: &Connection, input: WorkflowInput) -> Result<Workflow> {
    let (name, config) = check(conn, None, &input)?;
    let id = crate::util::new_id();
    conn.execute(
        "INSERT INTO workflow (id, name, config_json, enabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, name, to_json(&config), input.enabled, crate::util::now()],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: WorkflowInput) -> Result<Workflow> {
    let before = get(conn, id)?;
    let (name, config) = check(conn, Some(id), &input)?;
    conn.execute(
        "UPDATE workflow SET name = ?2, config_json = ?3, enabled = ?4 WHERE id = ?1",
        rusqlite::params![id, name, to_json(&config), input.enabled],
    )?;
    // 删掉的环节：通过记录一起清掉
    for gone in before
        .config
        .stages
        .iter()
        .filter(|s| !config.stages.iter().any(|n| n.id == s.id))
    {
        conn.execute("DELETE FROM ticket_stage WHERE stage_id = ?1", [&gone.id])?;
    }
    get(conn, id)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let w = get(conn, id)?;
    for s in &w.config.stages {
        conn.execute("DELETE FROM ticket_stage WHERE stage_id = ?1", [&s.id])?;
    }
    conn.execute("DELETE FROM workflow WHERE id = ?1", [id])?;
    Ok(())
}

/// 环节在哪个状态下轮到。
fn active_status(gate: &str) -> &'static str {
    if gate == "finalized" {
        "human_review"
    } else {
        "acceptance"
    }
}

/// 这张工单要过的环节（按顺序）。Bug 不经过定稿，卡在定稿前的环节对它不适用。
pub fn stages_of(conn: &Connection, t: &Ticket) -> Result<Vec<StageState>> {
    let Some(w) = for_kind(conn, &t.kind)? else {
        return Ok(vec![]);
    };
    let mut st = conn.prepare(
        "SELECT stage_id, passed_at, actor, note FROM ticket_stage WHERE ticket_id = ?1",
    )?;
    let passed: Vec<(String, String, Option<String>, Option<String>)> = st
        .query_map([&t.id], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut out: Vec<StageState> = vec![];
    for s in w.config.stages {
        if t.kind == "bug" && s.gate == "finalized" {
            continue;
        }
        let p = passed.iter().find(|p| p.0 == s.id);
        let earlier_pending = out
            .iter()
            .any(|o| o.gate == s.gate && o.passed_at.is_none());
        out.push(StageState {
            current: p.is_none() && !earlier_pending && t.status == active_status(&s.gate),
            passed_at: p.map(|p| p.1.clone()),
            actor: p.and_then(|p| p.2.clone()),
            note: p.and_then(|p| p.3.clone()),
            id: s.id,
            name: s.name,
            gate: s.gate,
            hint: s.hint,
        });
    }
    Ok(out)
}

/// 进入 `gate` 状态之前还没通过的环节。
pub fn pending(conn: &Connection, t: &Ticket, gate: &str) -> Result<Vec<StageState>> {
    Ok(stages_of(conn, t)?
        .into_iter()
        .filter(|s| s.gate == gate && s.passed_at.is_none())
        .collect())
}

/// 还有环节没通过时不让进 `gate` 状态。
pub fn check_gate(conn: &Connection, t: &Ticket, gate: &str) -> Result<()> {
    let waiting = pending(conn, t, gate)?;
    if waiting.is_empty() {
        return Ok(());
    }
    let names: Vec<String> = waiting.iter().map(|s| format!("「{}」", s.name)).collect();
    Err(Error::invalid(format!(
        "还有环节没有通过：{}。通过之后才能{}。",
        names.join("、"),
        if gate == "finalized" {
            "定稿"
        } else {
            "完成"
        }
    )))
}

/// 子任务全部合并后能不能自动完成工单。
pub fn can_auto_done(conn: &Connection, t: &Ticket) -> Result<bool> {
    let auto = for_kind(conn, &t.kind)?.is_none_or(|w| w.config.auto_done);
    Ok(auto && pending(conn, t, "done")?.is_empty())
}

/// 通过一个环节。要轮到它才行：工单在对应的状态，前面的环节都通过了。
pub fn pass(conn: &Connection, ticket_id: &str, stage_id: &str, note: &str) -> Result<StageState> {
    let t = tickets::get(conn, ticket_id)?;
    let stages = stages_of(conn, &t)?;
    let s = stages
        .iter()
        .find(|s| s.id == stage_id)
        .ok_or_else(|| Error::NotFound("这张工单没有这个环节。".into()))?;
    if s.passed_at.is_some() {
        return Ok(s.clone());
    }
    if !s.current {
        let wanted = active_status(&s.gate);
        return Err(Error::invalid(if t.status != wanted {
            format!(
                "环节「{}」要在工单处于「{}」时通过，现在是「{}」。",
                s.name,
                crate::workflow::ticket_status_label(wanted),
                crate::workflow::ticket_status_label(&t.status)
            )
        } else {
            format!("环节「{}」前面还有环节没有通过。", s.name)
        }));
    }
    let note = note.trim();
    conn.execute(
        "INSERT INTO ticket_stage (ticket_id, stage_id, passed_at, actor, note) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            t.id,
            s.id,
            crate::util::now(),
            super::current_actor(),
            (!note.is_empty()).then_some(note)
        ],
    )?;
    add_timeline(
        conn,
        &t.id,
        "stage_passed",
        serde_json::json!({ "stage": s.name, "gate": s.gate, "note": note }),
    )?;
    Ok(stages_of(conn, &tickets::get(conn, &t.id)?)?
        .into_iter()
        .find(|x| x.id == stage_id)
        .expect("刚通过的环节"))
}

/// 工单退回到 `to`：后面要重新过的环节清掉。
pub fn reset(conn: &Connection, t: &Ticket, to: &str) -> Result<()> {
    let gates: &[&str] = match to {
        "draft" | "ai_review" | "human_review" => GATES,
        "developing" => &["done"],
        _ => return Ok(()),
    };
    let Some(w) = for_kind(conn, &t.kind)? else {
        return Ok(());
    };
    for s in w
        .config
        .stages
        .iter()
        .filter(|s| gates.contains(&s.gate.as_str()))
    {
        conn.execute(
            "DELETE FROM ticket_stage WHERE ticket_id = ?1 AND stage_id = ?2",
            [&t.id, &s.id],
        )?;
    }
    Ok(())
}

/// 每张正在等某个环节的工单：`(工单 id, 环节名)`。给看板用。
pub fn waiting(conn: &Connection) -> Result<Vec<(String, String)>> {
    if !list(conn)?
        .iter()
        .any(|w| w.enabled && !w.config.stages.is_empty())
    {
        return Ok(vec![]);
    }
    let mut out = vec![];
    for t in tickets::list(conn, None)? {
        if !matches!(t.status.as_str(), "human_review" | "acceptance") {
            continue;
        }
        if let Some(s) = stages_of(conn, &t)?.into_iter().find(|s| s.current) {
            out.push((t.id, s.name));
        }
    }
    Ok(out)
}
