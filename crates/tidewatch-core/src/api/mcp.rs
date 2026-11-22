//! MCP（Streamable HTTP，无会话、无推送）：一次 POST 一个 JSON-RPC 请求或一批。
//! 运行令牌的调用方只能碰它所在的工单；成员令牌按角色。
use super::{ApiError, ApiResult, Caller};
use crate::db::lock;
use crate::engine::{DelegateInput, Engine};
use crate::store::runs::Run;
use crate::store::{projects, questions, tasks, tickets};
use serde_json::{json, Value};
use std::sync::Arc;
use std::time::{Duration, Instant};

const PROTOCOL: &str = "2025-03-26";
/// `tw_task_wait` 单次最多等这么久：多数 MCP 客户端的工具超时是 60 秒。
const MAX_WAIT_SECS: u64 = 50;

pub(super) fn handle(
    engine: &Arc<Engine>,
    caller: &Caller,
    body: Value,
) -> ApiResult<(u16, Value)> {
    match body {
        Value::Array(batch) => {
            let out: Vec<Value> = batch
                .into_iter()
                .filter_map(|m| message(engine, caller, m))
                .collect();
            Ok(if out.is_empty() {
                (202, Value::Null)
            } else {
                (200, Value::Array(out))
            })
        }
        m => Ok(match message(engine, caller, m) {
            Some(resp) => (200, resp),
            None => (202, Value::Null),
        }),
    }
}

/// 处理一条消息；通知（没有 id）不回。
fn message(engine: &Arc<Engine>, caller: &Caller, msg: Value) -> Option<Value> {
    let id = msg.get("id").cloned().filter(|v| !v.is_null())?;
    let method = msg
        .get("method")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let params = msg.get("params").cloned().unwrap_or(json!({}));
    let result: std::result::Result<Value, (i64, String)> = match method {
        "initialize" => Ok(json!({
            "protocolVersion": params.get("protocolVersion").and_then(Value::as_str).unwrap_or(PROTOCOL),
            "capabilities": { "tools": { "listChanged": false } },
            "serverInfo": { "name": "tidewatch", "version": env!("CARGO_PKG_VERSION") },
            "instructions": "观澜的工单接口。运行中的 Agent 可以读当前工单、把只读任务委派给另一个 Agent、把拿不准的问题挂给人。",
        })),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(json!({ "tools": tools(caller) })),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            // 工具自己的失败按 MCP 约定放在结果里（isError），模型能看到原因
            Ok(match call(engine, caller, name, &args) {
                Ok(v) => json!({
                    "content": [{ "type": "text", "text": serde_json::to_string_pretty(&v).unwrap_or_default() }],
                    "isError": false,
                }),
                Err(e) => json!({
                    "content": [{ "type": "text", "text": e.message }],
                    "isError": true,
                }),
            })
        }
        other => Err((-32601, format!("不支持的方法：{other}"))),
    };
    Some(match result {
        Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
        Err((code, message)) => {
            json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
        }
    })
}

fn tool(name: &str, description: &str, properties: Value, required: &[&str]) -> Value {
    json!({
        "name": name,
        "description": description,
        "inputSchema": { "type": "object", "properties": properties, "required": required },
    })
}

fn tools(caller: &Caller) -> Vec<Value> {
    let in_run = matches!(caller, Caller::Run(_));
    let ticket_arg = if in_run {
        json!({})
    } else {
        json!({ "ticket": { "type": "string", "description": "工单编号（如 TW-12）或 id" } })
    };
    let need_ticket: &[&str] = if in_run { &[] } else { &["ticket"] };
    let mut ask_props = json!({
        "question": { "type": "string", "description": "要问的问题，一次只问一件事" },
        "options": { "type": "array", "items": { "type": "string" }, "description": "可选：备选答案" },
        "client_request_id": { "type": "string", "description": "可选：重试时带同一个值，不会重复提问" },
    });
    if !in_run {
        ask_props["ticket"] = ticket_arg["ticket"].clone();
    }
    let mut list = vec![
        tool(
            "tw_ticket_read",
            "读取工单：定稿（或当前）PRD、验收标准、子任务及状态、已产出的接口文档路径。",
            ticket_arg,
            need_ticket,
        ),
        tool(
            "tw_ask_user",
            "把问题挂到工单的「需要你」。不会等待回答：能继续的先继续，卡住就说明卡在哪里后结束本轮；回答会出现在下一轮的 Prompt 里。",
            ask_props,
            &[&["question"], need_ticket].concat(),
        ),
    ];
    if in_run {
        list.extend([
            tool(
                "tw_task_delegate",
                "把一件只读的事（例如交叉审查、查一段代码怎么工作）委派给另一个 Agent，在你当前的工作目录里只读运行。立即返回 run_id，用 tw_task_wait 等结果。",
                json!({
                    "prompt": { "type": "string", "description": "要它做什么，写清楚要看哪些文件、要什么样的结论" },
                    "agent": { "type": "string", "description": "可选：claude_code | codex | opencode | gemini | aider；不填用和你一样的" },
                    "model": { "type": "string", "description": "可选：模型名" },
                    "client_request_id": { "type": "string", "description": "可选：重试时带同一个值，返回同一轮运行" },
                }),
                &["prompt"],
            ),
            tool(
                "tw_task_status",
                "查询你委派出去的运行的状态和结论。",
                json!({ "run_id": { "type": "string" } }),
                &["run_id"],
            ),
            tool(
                "tw_task_wait",
                "等你委派出去的运行结束（单次最多 50 秒；没结束就再调一次）。",
                json!({
                    "run_id": { "type": "string" },
                    "timeout_seconds": { "type": "integer", "description": "最多等多少秒，默认 50" },
                }),
                &["run_id"],
            ),
        ]);
    } else {
        list.extend([
            tool(
                "tw_ticket_list",
                "列出工单，可按编号或标题搜索。",
                json!({ "query": { "type": "string" } }),
                &[],
            ),
            tool(
                "tw_ticket_create",
                "新建一张草稿工单。",
                json!({
                    "title": { "type": "string" },
                    "type": { "type": "string", "description": "feature_backend | feature_frontend | feature_mixed | bug | chore，默认 chore" },
                    "content_md": { "type": "string", "description": "PRD 正文（Markdown）" },
                    "project_id": { "type": "string", "description": "可选：绑定的项目 id" },
                }),
                &["title"],
            ),
        ]);
    }
    list
}

fn arg<'a>(args: &'a Value, name: &str) -> Option<&'a str> {
    args.get(name)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn need<'a>(args: &'a Value, name: &str) -> ApiResult<&'a str> {
    arg(args, name).ok_or_else(|| ApiError::new(400, format!("缺少参数 {name}。")))
}

/// 调用方能碰的工单：运行令牌只能是自己那张。
fn ticket_id(caller: &Caller, args: &Value) -> ApiResult<String> {
    match caller {
        Caller::Run(run) => run
            .ticket_id
            .clone()
            .ok_or_else(|| ApiError::new(400, "这一轮运行不属于任何工单。")),
        Caller::Member(_) => Ok(need(args, "ticket")?.to_string()),
    }
}

fn run_json(run: &Run) -> Value {
    let done = run.finished_at.is_some();
    json!({
        "run_id": run.id,
        "agent": run.agent,
        "status": run.status,
        "finished": done,
        "result": if done { run.summary.clone() } else { None },
        "error": run.error,
    })
}

pub(super) fn ticket_json(engine: &Engine, id: &str) -> ApiResult<Value> {
    let conn = lock(engine.db());
    let t = tickets::get(&conn, id)?;
    let version = match &t.finalized_version_id {
        Some(v) => Some(tickets::prd_version(&conn, v)?),
        None => tickets::current_prd(&conn, &t.id)?,
    };
    let artifacts = tasks::artifacts_for_ticket(&conn, &t.id)?;
    let task_list: Vec<Value> = tasks::list_for_ticket(&conn, &t.id)?
        .into_iter()
        .map(|task| {
            let project = projects::get(&conn, &task.project_id).ok();
            json!({
                "id": task.id, "kind": task.kind, "title": task.title, "status": task.status,
                "project": project.map(|p| p.name), "spec_md": task.spec_md,
                "acceptance_criteria": task.acceptance_criteria, "depends_on": task.depends_on,
                "agent": task.agent, "branch": task.branch, "pr_url": task.pr_url,
                "api_doc": artifacts.iter().find(|a| a.task_id == task.id && a.kind == "api_doc").map(|a| a.path.clone()),
            })
        })
        .collect();
    let open: Vec<Value> = questions::list_for_ticket(&conn, &t.id)?
        .into_iter()
        .filter(|q| q.status == "open")
        .map(|q| json!({ "id": q.id, "body": q.body, "options": q.options, "task_id": q.task_id, "created_at": q.created_at }))
        .collect();
    Ok(json!({
        "id": t.id, "key": t.key, "title": t.title, "type": t.kind, "status": t.status,
        "priority": t.priority, "labels": t.labels,
        "prd_finalized": t.finalized_version_id.is_some(),
        "prd_version": version.as_ref().map(|v| v.version),
        "prd_md": version.map(|v| v.content_md),
        "acceptance_criteria": t.acceptance_criteria,
        "tasks": task_list,
        "open_questions": open,
        "updated_at": t.updated_at,
    }))
}

fn call(engine: &Arc<Engine>, caller: &Caller, name: &str, args: &Value) -> ApiResult<Value> {
    match (name, caller) {
        ("tw_ticket_read", _) => ticket_json(engine, &ticket_id(caller, args)?),
        ("tw_ask_user", _) => {
            if let Caller::Member(m) = caller {
                if !m.can_write() {
                    return Err(ApiError::forbidden());
                }
            }
            let ticket = ticket_id(caller, args)?;
            let options: Vec<String> = args
                .get("options")
                .and_then(Value::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            let run_id = match caller {
                Caller::Run(r) => Some(r.id.as_str()),
                Caller::Member(_) => None,
            };
            let q = engine.ask_user(
                &ticket,
                run_id,
                need(args, "question")?,
                &options,
                arg(args, "client_request_id"),
            )?;
            Ok(json!({
                "question_id": q.id,
                "status": q.status,
                "note": "问题已挂到工单。不会在这一轮里得到回答：能继续的先继续，否则说明卡在哪里后结束。",
            }))
        }
        ("tw_task_delegate", Caller::Run(run)) => {
            let child = engine.delegate(
                &run.id,
                DelegateInput {
                    prompt: need(args, "prompt")?.to_string(),
                    agent: arg(args, "agent").map(String::from),
                    provider_id: None,
                    model: arg(args, "model").map(String::from),
                    client_request_id: arg(args, "client_request_id").map(String::from),
                },
            )?;
            Ok(run_json(&child))
        }
        ("tw_task_status", Caller::Run(run)) => Ok(run_json(
            &engine.delegated_run(&run.id, need(args, "run_id")?)?,
        )),
        ("tw_task_wait", Caller::Run(run)) => {
            let child_id = need(args, "run_id")?;
            let secs = args
                .get("timeout_seconds")
                .and_then(Value::as_u64)
                .unwrap_or(MAX_WAIT_SECS)
                .min(MAX_WAIT_SECS);
            let deadline = Instant::now() + Duration::from_secs(secs);
            loop {
                let child = engine.delegated_run(&run.id, child_id)?;
                if child.finished_at.is_some() || Instant::now() >= deadline {
                    return Ok(run_json(&child));
                }
                std::thread::sleep(Duration::from_millis(300));
            }
        }
        ("tw_ticket_list", Caller::Member(_)) => {
            let list = tickets::list(&lock(engine.db()), arg(args, "query"))?;
            Ok(json!(list
                .iter()
                .map(|t| json!({ "id": t.id, "key": t.key, "title": t.title, "type": t.kind,
                                 "status": t.status, "priority": t.priority, "updated_at": t.updated_at }))
                .collect::<Vec<_>>()))
        }
        ("tw_ticket_create", Caller::Member(m)) => {
            if !m.can_write() {
                return Err(ApiError::forbidden());
            }
            let t = super::rest::create_ticket(engine, m, args)?;
            Ok(json!({ "id": t.id, "key": t.key, "status": t.status }))
        }
        _ => Err(ApiError::new(
            404,
            format!("没有这个工具，或者当前令牌不能用：{name}"),
        )),
    }
}
