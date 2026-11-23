//! REST 接口（`/api/v1`）。成员令牌按角色：
//! viewer 只读；member 能建单、评论、指派、开始 / 停止、验收 / 打回、回答问题；
//! owner 还能合并、以 full_auto 开始、查看成员。
use super::{ApiError, ApiResult};
use crate::db::lock;
use crate::engine::{Engine, RejectInput, StartOptions};
use crate::store::members::{self, Member};
use crate::store::tickets::{self, Ticket, TicketInput, TicketProject};
use crate::store::{projects, questions, runs, tasks, with_actor, workflows};
use serde_json::{json, Value};
use std::sync::Arc;

fn text<'a>(body: &'a Value, name: &str) -> Option<&'a str> {
    body.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn query_param(query: &str, name: &str) -> Option<String> {
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
        (k == name).then(|| percent_decode(v))
    })
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 3 <= bytes.len()
                && s.is_char_boundary(i + 1)
                && s.is_char_boundary(i + 3) =>
            {
                match u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    Ok(b) => {
                        out.push(b);
                        i += 3;
                        continue;
                    }
                    Err(_) => out.push(b'%'),
                }
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn writer(m: &Member) -> ApiResult<()> {
    if m.can_write() {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

/// full_auto 的 Agent 不受沙箱限制：只有 owner 能经接口让它开始，并且要明确确认。
fn confirm_full_auto(
    engine: &Engine,
    m: &Member,
    task_id: &str,
    opts: &StartOptions,
    body: &Value,
) -> ApiResult<()> {
    if engine.plan(task_id, opts)?.permission == "full_auto"
        && !(m.is_owner() && body.get("confirm_full_auto") == Some(&json!(true)))
    {
        return Err(ApiError::new(
            403,
            "这个子任务会以 full_auto 运行：只有 owner 能开始，并且要带上 \"confirm_full_auto\": true。",
        ));
    }
    Ok(())
}

fn owner(m: &Member) -> ApiResult<()> {
    if m.is_owner() {
        Ok(())
    } else {
        Err(ApiError::forbidden())
    }
}

/// 新建草稿工单（REST 和 MCP 共用）。
pub(super) fn create_ticket(engine: &Engine, m: &Member, body: &Value) -> ApiResult<Ticket> {
    writer(m)?;
    let title = text(body, "title").ok_or_else(|| ApiError::new(400, "缺少 title。"))?;
    let mut conn = lock(engine.db());
    let project_ids: Vec<String> = match (body.get("project_ids"), text(body, "project_id")) {
        (Some(Value::Array(a)), _) => a
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        (_, Some(p)) => vec![p.to_string()],
        _ => vec![],
    };
    let mut bound = Vec::new();
    for id in project_ids {
        let p = projects::get(&conn, &id)?;
        bound.push(TicketProject {
            project_id: p.id,
            role: p.kind,
            subdir: None,
        });
    }
    let input = TicketInput {
        title: title.to_string(),
        kind: text(body, "type").unwrap_or("chore").to_string(),
        priority: text(body, "priority").map(String::from),
        labels: body
            .get("labels")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        content_md: text(body, "content_md").unwrap_or_default().to_string(),
        projects: bound,
        source_kind: Some("api".into()),
        ..Default::default()
    };
    let t = with_actor(&m.name, || tickets::create(&mut conn, input))?;
    drop(conn);
    engine.ticket_changed(&t.id);
    Ok(t)
}

fn ticket_summary(t: &Ticket, names: &[Member], open_questions: usize) -> Value {
    json!({
        "id": t.id, "key": t.key, "title": t.title, "type": t.kind, "status": t.status,
        "priority": t.priority, "labels": t.labels, "due_date": t.due_date,
        "assignee_id": t.assignee_id,
        "assignee": t.assignee_id.as_ref().and_then(|a| names.iter().find(|m| &m.id == a)).map(|m| m.name.clone()),
        "open_questions": open_questions,
        "updated_at": t.updated_at,
    })
}

fn run_json(r: &runs::Run) -> Value {
    json!({
        "id": r.id, "task_id": r.task_id, "ticket_id": r.ticket_id, "number": r.number,
        "purpose": r.purpose, "agent": r.agent, "model": r.model, "status": r.status,
        "queued_at": r.queued_at, "started_at": r.started_at, "finished_at": r.finished_at,
        "input_tokens": r.input_tokens, "output_tokens": r.output_tokens, "cost_usd": r.cost_usd,
        "summary": r.summary, "error": r.error,
    })
}

pub(super) fn handle(
    engine: &Arc<Engine>,
    m: &Member,
    method: &str,
    path: &str,
    query: &str,
    body: Value,
) -> ApiResult<(u16, Value)> {
    let seg: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).collect();
    let ok = |v: Value| Ok((200, v));
    // 这次请求里写下的时间线事件都记在这个成员名下
    with_actor(&m.name, || match (method, seg.as_slice()) {
        ("GET", ["me"]) => ok(json!({ "id": m.id, "name": m.name, "role": m.role })),
        ("GET", ["members"]) => {
            // 普通成员只看得到名字（指派要用）；令牌末位和最近访问时间只给 owner
            let list = members::list(&lock(engine.db()))?;
            ok(json!(list
                .iter()
                .filter(|x| !x.disabled || m.is_owner())
                .map(|x| if m.is_owner() {
                    json!(x)
                } else {
                    json!({ "id": x.id, "name": x.name, "role": x.role })
                })
                .collect::<Vec<_>>()))
        }
        ("GET", ["projects"]) => {
            let list = projects::list(&lock(engine.db()))?;
            ok(json!(list
                .iter()
                .map(|p| json!({ "id": p.id, "name": p.name, "kind": p.kind }))
                .collect::<Vec<_>>()))
        }
        ("GET", ["tickets"]) => {
            let conn = lock(engine.db());
            let q = query_param(query, "q");
            let names = members::list(&conn)?;
            let open = questions::list_open(&conn)?;
            ok(json!(tickets::list(&conn, q.as_deref())?
                .iter()
                .map(|t| ticket_summary(
                    t,
                    &names,
                    open.iter().filter(|q| q.ticket_id == t.id).count()
                ))
                .collect::<Vec<_>>()))
        }
        ("POST", ["tickets"]) => {
            let t = create_ticket(engine, m, &body)?;
            Ok((201, super::mcp::ticket_json(engine, &t.id)?))
        }
        ("GET", ["tickets", id]) => {
            let mut v = super::mcp::ticket_json(engine, id)?;
            let conn = lock(engine.db());
            let t = tickets::get(&conn, id)?;
            v["assignee_id"] = json!(t.assignee_id);
            v["assignee"] = json!(t
                .assignee_id
                .as_deref()
                .and_then(|a| members::get(&conn, a).ok())
                .map(|x| x.name));
            v["stages"] = json!(workflows::stages_of(&conn, &t)?);
            v["timeline"] = json!(tickets::timeline(&conn, &t.id)?);
            v["runs"] = json!(runs::list_for_ticket(&conn, &t.id)?
                .iter()
                .map(run_json)
                .collect::<Vec<_>>());
            ok(v)
        }
        ("POST", ["tickets", id, "comments"]) => {
            writer(m)?;
            let text = text(&body, "body").ok_or_else(|| ApiError::new(400, "缺少 body。"))?;
            let id = tickets::get(&lock(engine.db()), id)?.id;
            tickets::add_comment(&lock(engine.db()), &id, text)?;
            engine.ticket_changed(&id);
            Ok((201, json!({ "ok": true })))
        }
        ("POST", ["tickets", id, "assign"]) => {
            writer(m)?;
            let t = tickets::set_assignee(&lock(engine.db()), id, text(&body, "member_id"))?;
            engine.ticket_changed(&t.id);
            ok(json!({ "id": t.id, "assignee_id": t.assignee_id }))
        }
        // 自定义工作流的环节：通过的人记在环节上
        ("POST", ["tickets", id, "stages", stage, "pass"]) => {
            writer(m)?;
            let id = tickets::get(&lock(engine.db()), id)?.id;
            let note = text(&body, "note").unwrap_or_default();
            ok(json!(engine.pass_stage(&id, stage, note)?))
        }
        ("POST", ["tickets", id, "review"]) => {
            writer(m)?;
            let (id, full_auto) = {
                let conn = lock(engine.db());
                let default = crate::store::settings::get(&conn)?.default_permission;
                (tickets::get(&conn, id)?.id, default == "full_auto")
            };
            // 默认权限是 full_auto 时预审也以 full_auto 运行，和子任务一样要 owner 明确确认
            if full_auto && !(m.is_owner() && body.get("confirm_full_auto") == Some(&json!(true))) {
                return Err(ApiError::new(
                    403,
                    "默认权限是 full_auto，预审会以 full_auto 运行：只有 owner 能发起，并且要带上 \"confirm_full_auto\": true。",
                ));
            }
            ok(run_json(&engine.start_review(&id)?))
        }
        ("POST", ["tickets", id, "start"]) => {
            writer(m)?;
            let id = tickets::get(&lock(engine.db()), id)?.id;
            let started = engine.start_development(&id)?;
            ok(json!(started.iter().map(run_json).collect::<Vec<_>>()))
        }
        ("GET", ["tasks", id]) => {
            let conn = lock(engine.db());
            let task = tasks::get(&conn, id)?;
            let list = runs::list_for_task(&conn, &task.id)?;
            ok(json!({ "task": task, "runs": list.iter().map(run_json).collect::<Vec<_>>() }))
        }
        ("GET", ["tasks", id, "diff"]) => ok(json!(engine.task_diff(id)?)),
        ("POST", ["tasks", id, "start"]) => {
            writer(m)?;
            let opts = StartOptions {
                instruction: text(&body, "instruction").map(String::from),
                command_id: text(&body, "command_id").map(String::from),
                ..Default::default()
            };
            confirm_full_auto(engine, m, id, &opts, &body)?;
            ok(run_json(&engine.start_task(id, opts)?))
        }
        ("POST", ["tasks", id, "accept"]) => {
            writer(m)?;
            ok(json!(engine.accept_task(id)?))
        }
        ("POST", ["tasks", id, "reject"]) => {
            writer(m)?;
            let note = text(&body, "note").ok_or_else(|| ApiError::new(400, "缺少 note。"))?;
            let rerun = body.get("rerun") == Some(&json!(true));
            if rerun {
                // 打回后立刻重跑也是一次开始，打回意见就是给 Agent 的指令
                confirm_full_auto(engine, m, id, &StartOptions::default(), &body)?;
            }
            ok(json!(engine.reject_task_with(
                id,
                RejectInput {
                    note: note.to_string(),
                    comments: vec![],
                    rerun,
                }
            )?))
        }
        ("POST", ["tasks", id, "merge"]) => {
            owner(m)?;
            ok(json!(engine.merge_task(id)?))
        }
        ("GET", ["runs", id]) => ok(run_json(&runs::get(&lock(engine.db()), id)?)),
        ("GET", ["runs", id, "events"]) => {
            let after = query_param(query, "after")
                .and_then(|s| s.parse::<i64>().ok())
                .unwrap_or(0);
            let conn = lock(engine.db());
            let run = runs::get(&conn, id)?;
            ok(json!(runs::events(&conn, &run.id, after)?))
        }
        ("POST", ["runs", id, "stop"]) => {
            writer(m)?;
            engine.stop_run(id)?;
            ok(json!({ "ok": true }))
        }
        ("GET", ["questions"]) => ok(json!(questions::list_open(&lock(engine.db()))?)),
        ("POST", ["questions", id, "answer"]) => {
            writer(m)?;
            let answer =
                text(&body, "answer").ok_or_else(|| ApiError::new(400, "缺少 answer。"))?;
            ok(json!(engine.answer_question(id, answer, Some(&m.name))?))
        }
        _ => Err(ApiError::new(404, "没有这个接口。")),
    })
}
