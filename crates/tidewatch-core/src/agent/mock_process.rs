//! 演示 Agent 的进程体：按 Claude Code 的 stream-json 格式输出，在当前目录做一个小改动。
//! 不联网、不需要账号；用于走通流程和自动化测试。`twctl mock-agent` 和桌面应用自身都能充当它。
//!
//! 提示词里的标记可以改变行为（测试用）：`[mock:steps=5]`、`[mock:delay=800]`、`[mock:fail]`。
use serde_json::{json, Value};
use std::io::{BufRead, Read, Write};
use std::time::Duration;

pub struct MockArgs {
    pub model: String,
    pub permission: String,
    pub resume: Option<String>,
    pub steps: u32,
    pub delay_ms: u64,
}

fn emit(v: Value) {
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{v}");
    let _ = out.flush();
}

fn assistant(session: &str, content: Value) {
    emit(
        json!({"type":"assistant","session_id":session,"message":{"role":"assistant","content":[content]}}),
    );
}

/// 提示词中 `[mock:name=值]` 的值。
fn marker(prompt: &str, name: &str) -> Option<u64> {
    let tag = format!("[mock:{name}=");
    let rest = &prompt[prompt.find(&tag)? + tag.len()..];
    rest[..rest.find(']')?].trim().parse().ok()
}

/// 提示词里 `<!-- tw:purpose=xxx -->` 的值：预审 / 拆分 / 影响分析要求按约定的 JSON 作答。
fn purpose(prompt: &str) -> Option<&str> {
    let tag = "<!-- tw:purpose=";
    let rest = &prompt[prompt.find(tag)? + tag.len()..];
    Some(rest[..rest.find(' ')?].trim())
}

/// 提示词里 `- a | b | c` 形式的列表行，按 `|` 切开。
fn table_lines(prompt: &str) -> Vec<Vec<String>> {
    prompt
        .lines()
        .filter_map(|l| l.trim().strip_prefix("- "))
        .filter(|l| l.contains(" | "))
        .map(|l| l.split(" | ").map(|c| c.trim().to_string()).collect())
        .collect()
}

/// 演示用的预审结果：固定的两个问题；正文里已经写了对应的补充（采纳过），
/// 或者问题已经列在"未处理 / 不做范围"里，就不再提。`[mock:issues=0]` 一个都不提。
fn review_json(prompt: &str) -> Value {
    let body = prompt.split("<!-- tw:purpose=").next().unwrap_or(prompt);
    let candidates = [
        json!({"id":"I-1","severity":"blocker","category":"flow","location":"",
            "problem":"没有说明列表为空时界面显示什么",
            "question":"列表为空时显示什么？",
            "suggestion":"演示补充：列表为空时显示「暂无数据」，并提供「新建」入口。"}),
        json!({"id":"I-2","severity":"suggestion","category":"nonfunctional","location":"",
            "problem":"没有约定列表的分页大小",
            "suggestion":"演示建议：列表每页 20 条，按创建时间倒序。"}),
    ];
    let limit = marker(prompt, "issues").unwrap_or(2) as usize;
    let issues: Vec<Value> = candidates
        .into_iter()
        .filter(|i| {
            let said = |k: &str| i[k].as_str().is_some_and(|t| body.contains(t));
            !said("suggestion") && !said("problem")
        })
        .take(limit)
        .collect();
    let backend = !prompt.contains("类型：feature_frontend");
    let frontend = !prompt.contains("类型：feature_backend");
    json!({
        "summary": if issues.is_empty() { "需求描述清楚，可以定稿。" } else { "主要流程清楚，有几处需要补充。" },
        "scope": { "backend": backend, "frontend": frontend },
        "issues": issues,
        "acceptance_criteria_draft": ["列表为空时显示「暂无数据」", "接口返回的数据与界面一致"],
    })
}

/// 演示用的拆分：混合需求拆成后端 + 前端（前端等后端的接口文档），其它类型一个子任务。
fn split_json(prompt: &str) -> Value {
    let projects: Vec<(String, String)> = table_lines(prompt)
        .into_iter()
        .filter(|c| c.len() == 3 && c[2].starts_with("role="))
        .map(|c| (c[0].clone(), c[2]["role=".len()..].to_string()))
        .collect();
    let pick = |role: &str| {
        projects
            .iter()
            .find(|(_, r)| r == role)
            .or(projects.first())
            .map(|(n, _)| n.clone())
            .unwrap_or_default()
    };
    let be = json!({"ref":"T1","kind":"backend","project":pick("backend"),"title":"后端：数据与接口",
        "spec":"实现需求里的数据模型和接口；第一步先写接口文档。",
        "acceptance_criteria":["接口文档已提交","接口有单元测试"],"depends_on":[]});
    let fe = |deps: Value| {
        json!({"ref":"T2","kind":"frontend","project":pick("frontend"),"title":"前端：页面与对接",
        "spec":"实现需求里的页面，按接口文档对接。",
        "acceptance_criteria":["页面可以正常使用"],"depends_on":deps,"dependency_kind":"api_doc"})
    };
    let tasks = if prompt.contains("类型：feature_mixed") {
        vec![be, fe(json!(["T1"]))]
    } else if prompt.contains("类型：feature_frontend") {
        vec![fe(json!([]))]
    } else {
        vec![be]
    };
    json!({ "tasks": tasks })
}

/// 演示用的智能创建分析：标题取正文第一行，提到报错 / 崩溃就当 Bug，项目选清单里和类型对得上的第一个。
fn smart_json(prompt: &str) -> Value {
    let body = prompt
        .split("## 正文")
        .nth(1)
        .and_then(|b| b.split("<!-- tw:purpose=").next())
        .unwrap_or("");
    let title: String = body
        .lines()
        .map(|l| l.trim().trim_start_matches('#').trim())
        .find(|l| !l.is_empty())
        .unwrap_or("未命名")
        .chars()
        .take(40)
        .collect();
    let bug = ["报错", "崩溃", "bug", "Bug", "500"]
        .iter()
        .any(|k| body.contains(k));
    let frontend = ["页面", "按钮", "界面"].iter().any(|k| body.contains(k));
    let projects: Vec<(String, String)> = table_lines(prompt)
        .into_iter()
        .filter(|c| c.len() >= 3 && c[2].starts_with("kind="))
        .map(|c| (c[0].clone(), c[2]["kind=".len()..].to_string()))
        .collect();
    let want = if frontend { "frontend" } else { "backend" };
    let project = projects
        .iter()
        .find(|(_, k)| k == want)
        .or(projects.first())
        .map(|(id, _)| id.clone());
    json!({
        "title": format!("{title}（AI 整理）"),
        "type": if bug { "bug" } else if frontend { "feature_frontend" } else { "feature_backend" },
        "project_ids": project.into_iter().collect::<Vec<_>>(),
        "priority": if bug { "high" } else { "medium" },
        "labels": ["演示分析"],
        "bug": if bug { json!({"steps":"按正文里的操作再做一遍","expected":"不报错","actual":"报错"}) } else { json!({}) },
        "reasons": [
            if bug { "类型：正文在描述一个报错，按 Bug 处理" } else { "类型：正文在描述一个新功能" },
            "项目：演示 Agent 选了清单里类型对得上的第一个项目",
        ],
    })
}

/// 演示用的影响分析：后端子任务需要追加工作，其它不受影响。
fn impact_json(prompt: &str) -> Value {
    let tasks: Vec<Value> = table_lines(prompt)
        .into_iter()
        .filter(|c| c.len() == 4)
        .map(|c| {
            if c[1] == "backend" {
                json!({"task_id":c[0],"impact":"rework","instruction":"按变更后的需求调整接口，并更新接口文档。"})
            } else {
                json!({"task_id":c[0],"impact":"none","instruction":""})
            }
        })
        .collect();
    json!({ "summary": "变更影响后端接口，前端不受影响。", "tasks": tasks, "new_tasks": [] })
}

/// 演示用的交叉审查：固定提一个问题；`[mock:findings=0]` 一个都不提。
fn code_review_json(prompt: &str, cwd: &std::path::Path) -> Value {
    // 审查要在子任务的 worktree 里进行：看不到演示改动就说明目录不对
    if !cwd.join("TIDEWATCH_DEMO.md").exists() {
        return json!({
            "summary": "当前目录里没有这个子任务的改动。",
            "findings": [{"severity":"blocker","file":"","line":null,
                "title":"找不到要审查的改动",
                "detail":"当前目录里没有 TIDEWATCH_DEMO.md，审查没有在子任务的 worktree 里进行。",
                "suggestion":""}],
        });
    }
    let n = marker(prompt, "findings").unwrap_or(1) as usize;
    let all = [
        json!({"severity":"major","file":"TIDEWATCH_DEMO.md","line":3,
            "title":"演示改动没有对应的测试",
            "detail":"这次只改了说明文件，没有任何测试覆盖验收标准。",
            "suggestion":"补一个覆盖主流程的测试。"}),
        json!({"severity":"minor","file":"TIDEWATCH_DEMO.md","line":1,
            "title":"标题没有写明是哪张工单",
            "detail":"多张工单都会改这个文件，标题分不清来源。",
            "suggestion":"标题里带上工单号。"}),
    ];
    let findings: Vec<Value> = all.into_iter().take(n).collect();
    json!({
        "summary": if findings.is_empty() { "改动符合需求，没有发现问题。" } else { "改动基本符合需求，有需要注意的地方。" },
        "findings": findings,
    })
}

/// 提示词要求先写的接口文档路径（`docs/api/XXX.md`）。
fn api_doc_path(prompt: &str) -> Option<&str> {
    let start = prompt.find("`docs/api/")? + 1;
    let rest = &prompt[start..];
    Some(&rest[..rest.find('`')?]).filter(|p| p.ends_with(".md") && !p.contains(".."))
}

/// 演示 Agent 调观澜 MCP 工具（地址和运行令牌由引擎放在环境变量里）；没开接口时返回 None。
fn mcp_call(tool: &str, arguments: Value) -> Option<Value> {
    let url = std::env::var(super::mock::MCP_URL_ENV).ok()?;
    let token = std::env::var(crate::engine::RUN_TOKEN_ENV).ok()?;
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(70)))
        .http_status_as_error(false)
        .build()
        .into();
    let body = json!({"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":tool,"arguments":arguments}});
    let text = agent
        .post(&url)
        .header("Authorization", &format!("Bearer {token}"))
        .header("Content-Type", "application/json")
        .send(body.to_string())
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let resp: Value = serde_json::from_str(&text).ok()?;
    let result = resp.get("result")?;
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    Some(json!({
        "is_error": result["isError"].as_bool().unwrap_or(false),
        "value": serde_json::from_str::<Value>(text).unwrap_or(Value::String(text.into())),
    }))
}

/// `[mock:ask]` / `[mock:delegate]`：演示经 MCP 提问和委派。返回要写进结论的一句话。
fn mcp_demo(session: &str, prompt: &str) -> Option<String> {
    let step = |id: &str, tool: &str, args: Value| -> Option<Value> {
        assistant(
            session,
            json!({"type":"tool_use","id":id,"name":format!("mcp__tidewatch__{tool}"),"input":args}),
        );
        let out = mcp_call(tool, args);
        emit(
            json!({"type":"user","session_id":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":id,
            "content":out.as_ref().map(|v| v["value"].to_string()).unwrap_or_else(|| "观澜的接口没有开启。".into()),
            "is_error":out.as_ref().is_none_or(|v| v["is_error"] == true)}]}}),
        );
        out.filter(|v| v["is_error"] != true)
            .map(|v| v["value"].clone())
    };
    let mut notes = Vec::new();
    if prompt.contains("[mock:ask]") {
        let asked = step(
            "toolu_ask",
            "tw_ask_user",
            json!({"question":"空列表时返回 [] 还是 null？","options":["[]","null"],"client_request_id":"mock-ask"}),
        );
        notes.push(match asked {
            Some(_) => "有一个问题挂给了你。".to_string(),
            None => "提问失败。".to_string(),
        });
    }
    if prompt.contains("[mock:delegate]") {
        let task = "只读看一遍 TIDEWATCH_DEMO.md，说说有没有问题。[mock:steps=1] [mock:delay=50]";
        let note = match step(
            "toolu_delegate",
            "tw_task_delegate",
            json!({"prompt":task,"client_request_id":"mock-delegate"}),
        ) {
            Some(child) => {
                let run_id = child["run_id"].as_str().unwrap_or_default().to_string();
                let mut last = child;
                for i in 0..6 {
                    if last["finished"] == true {
                        break;
                    }
                    match step(
                        &format!("toolu_wait_{i}"),
                        "tw_task_wait",
                        json!({"run_id":run_id,"timeout_seconds":20}),
                    ) {
                        Some(v) => last = v,
                        None => break,
                    }
                }
                format!(
                    "委派的检查结束（{}）：{}",
                    last["status"].as_str().unwrap_or("未知"),
                    last["result"].as_str().unwrap_or("没有结论")
                )
            }
            None => "委派失败。".to_string(),
        };
        notes.push(note);
    }
    (!notes.is_empty()).then(|| notes.join(" "))
}

pub fn run(mut args: MockArgs) -> i32 {
    // 第一行是 stream-json 用户消息时按交互模式跑：答完一轮后继续读追加指令，直到 stdin 关闭
    let mut input = std::io::stdin().lock();
    let mut first = String::new();
    let _ = input.read_line(&mut first);
    let interactive = user_text(&first);
    let prompt = match &interactive {
        Some(text) => text.clone(),
        None => {
            let mut rest = String::new();
            let _ = input.read_to_string(&mut rest);
            first + &rest
        }
    };
    if let Some(n) = marker(&prompt, "steps") {
        args.steps = n as u32;
    }
    if let Some(n) = marker(&prompt, "delay") {
        args.delay_ms = n;
    }
    let fail = prompt.contains("[mock:fail]");
    let session = args
        .resume
        .clone()
        .unwrap_or_else(|| format!("mock-{}", crate::util::new_id().to_lowercase()));
    let cwd = std::env::current_dir().unwrap_or_default();
    let pause = || std::thread::sleep(Duration::from_millis(args.delay_ms));
    let first_line = prompt
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("（空提示词）")
        .trim()
        .to_string();

    emit(
        json!({"type":"system","subtype":"init","session_id":session,"model":args.model,"cwd":cwd,"permissionMode":args.permission}),
    );
    pause();
    let verb = if args.resume.is_some() {
        "继续上次的会话"
    } else {
        "开始处理"
    };
    assistant(
        &session,
        json!({"type":"text","text":format!("{verb}：{first_line}")}),
    );

    // 演示护栏：报一条危险命令（只是报出来，不执行）
    if prompt.contains("[mock:danger]") {
        assistant(
            &session,
            json!({"type":"tool_use","id":"toolu_danger","name":"Bash","input":{"command":"rm -rf ./build"}}),
        );
        emit(
            json!({"type":"user","session_id":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_danger","content":"（演示：没有真的执行）"}]}}),
        );
        pause();
    }
    let read_only = args.permission == "read_only";
    if !read_only && args.resume.is_none() {
        if let Some(rel) = api_doc_path(&prompt) {
            let doc = cwd.join(rel);
            let text = "# 接口文档（演示）\n\n## GET /api/items\n\n返回列表：`[{ \"id\": 1, \"name\": \"示例\" }]`；为空时返回 `[]`。\n";
            let res = doc
                .parent()
                .map(std::fs::create_dir_all)
                .unwrap_or(Ok(()))
                .and_then(|_| std::fs::write(&doc, text));
            assistant(
                &session,
                json!({"type":"tool_use","id":"toolu_doc","name":"Write","input":{"file_path":doc,"content":text}}),
            );
            emit(
                json!({"type":"user","session_id":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"toolu_doc","content":match &res {Ok(_)=>"已写入".to_string(),Err(e)=>e.to_string()},"is_error":res.is_err()}]}}),
            );
        }
    }
    let file = cwd.join("TIDEWATCH_DEMO.md");
    let mut body =
        std::fs::read_to_string(&file).unwrap_or_else(|_| "# 观澜演示改动\n".to_string());
    for step in 1..=args.steps {
        pause();
        if read_only {
            assistant(
                &session,
                json!({"type":"tool_use","id":format!("toolu_{step}"),"name":"Read","input":{"file_path":"README.md"}}),
            );
            emit(
                json!({"type":"user","session_id":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":format!("toolu_{step}"),"content":"（只读模式：不修改文件）"}]}}),
            );
            continue;
        }
        body.push_str(&format!(
            "\n- 第 {step} 步（会话 {session}）：{first_line}\n"
        ));
        assistant(
            &session,
            json!({"type":"tool_use","id":format!("toolu_{step}"),"name":"Write","input":{"file_path":file,"content":body}}),
        );
        let res = std::fs::write(&file, &body);
        emit(
            json!({"type":"user","session_id":session,"message":{"role":"user","content":[{"type":"tool_result","tool_use_id":format!("toolu_{step}"),"content":match &res {Ok(_)=>"已写入".to_string(),Err(e)=>e.to_string()},"is_error":res.is_err()}]}}),
        );
    }
    let mcp_note = mcp_demo(&session, &prompt);
    pause();
    if fail {
        let msg = "演示 Agent 按提示词中的 [mock:fail] 标记失败。";
        emit(
            json!({"type":"result","subtype":"error_during_execution","is_error":true,"session_id":session,
            "num_turns":args.steps+1,"result":msg,"total_cost_usd":0.0,
            "usage":{"input_tokens":120,"output_tokens":20,"cache_read_input_tokens":0}}),
        );
        return 1;
    }
    let structured = match purpose(&prompt) {
        Some("review") => Some(review_json(&prompt)),
        Some("split") => Some(split_json(&prompt)),
        Some("impact") => Some(impact_json(&prompt)),
        Some("smart") => Some(smart_json(&prompt)),
        Some("code_review") => Some(code_review_json(&prompt, &cwd)),
        _ => None,
    };
    let summary = if let Some(v) = structured {
        format!(
            "```json\n{}\n```",
            serde_json::to_string_pretty(&v).unwrap_or_default()
        )
    } else if read_only {
        "只读检查完成，没有修改文件。".to_string()
    } else {
        format!("已在 TIDEWATCH_DEMO.md 追加 {} 条记录。", args.steps)
    };
    let summary = match mcp_note {
        Some(note) => format!("{summary} {note}"),
        None => summary,
    };
    assistant(&session, json!({"type":"text","text":summary}));
    let chars = prompt.chars().count() as u64;
    emit(
        json!({"type":"result","subtype":"success","is_error":false,"session_id":session,"num_turns":args.steps+1,
        "duration_ms":args.delay_ms*(args.steps as u64+2),"result":summary,"total_cost_usd":0.0,
        "usage":{"input_tokens":chars/2+120,"output_tokens":80+40*args.steps as u64,"cache_read_input_tokens":0}}),
    );
    if interactive.is_some() {
        let mut line = String::new();
        while input.read_line(&mut line).unwrap_or(0) > 0 {
            if let Some(text) = user_text(&line) {
                pause();
                let reply = format!("收到追加指令：{}", text.trim());
                assistant(&session, json!({"type":"text","text":reply}));
                emit(
                    json!({"type":"result","subtype":"success","is_error":false,"session_id":session,
                    "num_turns":1,"duration_ms":args.delay_ms,"result":reply,"total_cost_usd":0.0,
                    "usage":{"input_tokens":text.chars().count() as u64/2+20,"output_tokens":20,"cache_read_input_tokens":0}}),
                );
            }
            line.clear();
        }
    }
    0
}

/// stream-json 输入里一条用户消息的文本；不是这种格式时返回 `None`。
fn user_text(line: &str) -> Option<String> {
    let v: Value = serde_json::from_str(line.trim()).ok()?;
    if v["type"] != "user" {
        return None;
    }
    let content = &v["message"]["content"];
    if let Some(s) = content.as_str() {
        return Some(s.to_string());
    }
    let parts: Vec<&str> = content
        .as_array()?
        .iter()
        .filter_map(|p| p["text"].as_str())
        .collect();
    Some(parts.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::marker;

    #[test]
    fn reads_prompt_markers() {
        assert_eq!(marker("修一下 [mock:steps=5] 谢谢", "steps"), Some(5));
        assert_eq!(marker("[mock:delay=800]", "delay"), Some(800));
        assert_eq!(marker("[mock:steps=x]", "steps"), None);
        assert_eq!(marker("没有标记", "steps"), None);
    }

    #[test]
    fn answers_structured_prompts_in_the_agreed_json() {
        use super::*;
        let p = "需求…类型：feature_mixed）\n- shop-api | kind=backend | role=backend\n- shop-web | kind=frontend | role=frontend\n<!-- tw:purpose=split -->";
        assert_eq!(purpose(p), Some("split"));
        let v = split_json(p);
        assert_eq!(v["tasks"][0]["project"], "shop-api");
        assert_eq!(v["tasks"][1]["project"], "shop-web");
        assert_eq!(v["tasks"][1]["depends_on"][0], "T1");

        let r = review_json("正文\n<!-- tw:purpose=review -->");
        assert_eq!(r["issues"].as_array().unwrap().len(), 2);
        // 采纳过的补充已经在正文里：不再提
        let r = review_json("正文\n演示补充：列表为空时显示「暂无数据」，并提供「新建」入口。\n<!-- tw:purpose=review -->");
        assert_eq!(r["issues"].as_array().unwrap().len(), 1);
        assert_eq!(r["issues"][0]["severity"], "suggestion");

        let i = impact_json("- T_A | backend | review | 后端\n- T_B | frontend | pending | 前端\n<!-- tw:purpose=impact -->");
        assert_eq!(i["tasks"][0]["impact"], "rework");
        assert_eq!(i["tasks"][1]["impact"], "none");

        assert_eq!(
            api_doc_path("先写 `docs/api/TW-3.md`"),
            Some("docs/api/TW-3.md")
        );
        assert_eq!(api_doc_path("没有"), None);
    }
}
