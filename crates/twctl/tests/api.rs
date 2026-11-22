//! 本地 HTTP / MCP 接口端到端：令牌与角色、REST、MCP 工具，以及演示 Agent 在运行中
//! 经 MCP 提问和委派。Linux 与 Windows 都跑。
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tidewatch_core::api::ApiServer;
use tidewatch_core::db::{self, lock};
use tidewatch_core::engine::{DelegateInput, Engine, EngineConfig, EngineEvent, StartOptions};
use tidewatch_core::git;
use tidewatch_core::secrets::MemoryStore;
use tidewatch_core::store::{
    delegations, members, projects, questions, runs, settings, tasks, tickets, workflows,
};

struct Fixture {
    engine: Arc<Engine>,
    server: ApiServer,
    events: Arc<Mutex<Vec<EngineEvent>>>,
    project_id: String,
    owner: String,
    member: String,
    viewer: String,
    _dir: tempfile::TempDir,
}

fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).unwrap();
    git::git(dir, &["init", "-q", "-b", "main"]).unwrap();
    std::fs::write(dir.join("README.md"), "# demo\n").unwrap();
    git::git(dir, &["add", "-A"]).unwrap();
    git::git(
        dir,
        &[
            "-c",
            "user.name=t",
            "-c",
            "user.email=t@t",
            "commit",
            "-q",
            "-m",
            "init",
        ],
    )
    .unwrap();
}

fn fixture(max_concurrency: u32) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    let repo = root.join("repo");
    init_repo(&repo);
    let conn = db::open(&root.join("data/tidewatch.db")).unwrap();
    settings::save(
        &conn,
        &settings::Settings {
            max_concurrency,
            default_agent: "mock".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let project = projects::create(
        &conn,
        projects::ProjectInput {
            name: "演示 demo".into(),
            kind: "backend".into(),
            local_path: repo.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let (_, owner) = members::create(&conn, "刘", "owner").unwrap();
    let (_, member) = members::create(&conn, "小王", "member").unwrap();
    let (_, viewer) = members::create(&conn, "访客", "viewer").unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let engine = Arc::new(
        Engine::start(
            db::shared(conn),
            Box::new(MemoryStore::default()),
            EngineConfig {
                data_dir: root.join("data"),
                mock_binary: Some(PathBuf::from(env!("CARGO_BIN_EXE_twctl"))),
            },
            Arc::new(move |e| sink.lock().unwrap().push(e)),
        )
        .unwrap(),
    );
    let server = ApiServer::start(engine.clone(), 0, false).unwrap();
    Fixture {
        engine,
        server,
        events,
        project_id: project.id,
        owner,
        member,
        viewer,
        _dir: dir,
    }
}

impl Fixture {
    fn http(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
    ) -> (u16, Value) {
        self.http_with(method, path, token, body, &[])
    }

    fn http_with(
        &self,
        method: &str,
        path: &str,
        token: Option<&str>,
        body: Option<Value>,
        headers: &[(&str, &str)],
    ) -> (u16, Value) {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(70)))
            .http_status_as_error(false)
            .build()
            .into();
        let url = format!("{}{path}", self.server.base_url());
        let auth = token.map(|t| format!("Bearer {t}"));
        let mut resp = if method == "GET" {
            let mut req = agent.get(&url);
            if let Some(a) = &auth {
                req = req.header("Authorization", a);
            }
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            req.call().unwrap()
        } else {
            let mut req = agent.post(&url).header("Content-Type", "application/json");
            if let Some(a) = &auth {
                req = req.header("Authorization", a);
            }
            for (k, v) in headers {
                req = req.header(*k, *v);
            }
            req.send(body.unwrap_or(json!({})).to_string()).unwrap()
        };
        let status = resp.status().as_u16();
        let text = resp.body_mut().read_to_string().unwrap_or_default();
        (
            status,
            serde_json::from_str(&text).unwrap_or(Value::String(text)),
        )
    }

    /// 调一个 MCP 工具，返回（是否出错，结果）。
    fn tool(&self, token: &str, name: &str, args: Value) -> (bool, Value) {
        let (status, resp) = self.http(
            "POST",
            "/mcp",
            Some(token),
            Some(json!({"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":name,"arguments":args}})),
        );
        assert_eq!(status, 200, "{resp}");
        let text = resp["result"]["content"][0]["text"]
            .as_str()
            .unwrap_or_default();
        (
            resp["result"]["isError"] == true,
            serde_json::from_str(text).unwrap_or(Value::String(text.into())),
        )
    }

    fn bug(&self, title: &str, body: &str) -> (String, String) {
        let mut conn = lock(self.engine.db());
        let ticket = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: title.into(),
                kind: "bug".into(),
                content_md: body.into(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = tasks::create(
            &mut conn,
            tasks::TaskInput {
                ticket_id: ticket.id.clone(),
                kind: "bug".into(),
                project_id: self.project_id.clone(),
                title: format!("修复：{title}"),
                ..Default::default()
            },
        )
        .unwrap();
        (ticket.id, task.id)
    }

    fn run(&self, id: &str) -> runs::Run {
        runs::get(&lock(self.engine.db()), id).unwrap()
    }

    fn wait_run(&self, id: &str, what: &str, pred: impl Fn(&runs::Run) -> bool) -> runs::Run {
        let deadline = Instant::now() + Duration::from_secs(90);
        loop {
            let run = self.run(id);
            if pred(&run) {
                return run;
            }
            assert!(
                Instant::now() < deadline,
                "等待「{what}」超时，Run 状态：{} {:?}",
                run.status,
                run.error
            );
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    fn wait_done(&self, id: &str) -> runs::Run {
        self.wait_run(id, "运行结束", |r| {
            !runs::ACTIVE.contains(&r.status.as_str())
        })
    }
}

#[test]
fn tokens_and_roles_gate_every_route() {
    let f = fixture(2);
    // 看板页面和健康检查不需要令牌，其余都要
    let (status, page) = f.http("GET", "/", None, None);
    assert_eq!(status, 200);
    assert!(page.as_str().unwrap().contains("观澜"));
    assert_eq!(f.http("GET", "/api/v1/health", None, None).0, 200);
    assert_eq!(f.http("GET", "/api/v1/tickets", None, None).0, 401);
    assert_eq!(
        f.http("GET", "/api/v1/tickets", Some("tw_wrong"), None).0,
        401
    );
    assert_eq!(f.http("POST", "/mcp", None, Some(json!({}))).0, 401);

    // viewer 只读
    let (status, me) = f.http("GET", "/api/v1/me", Some(&f.viewer), None);
    assert_eq!((status, me["role"].as_str()), (200, Some("viewer")));
    assert_eq!(
        f.http("GET", "/api/v1/tickets", Some(&f.viewer), None).0,
        200
    );
    let new = json!({"title":"接口建的单","type":"bug","content_md":"列表为空时崩溃","project_id":f.project_id});
    assert_eq!(
        f.http(
            "POST",
            "/api/v1/tickets",
            Some(&f.viewer),
            Some(new.clone())
        )
        .0,
        403
    );

    // member 能建单、评论、指派；时间线记下是谁做的
    let (status, ticket) = f.http("POST", "/api/v1/tickets", Some(&f.member), Some(new));
    assert_eq!(status, 201, "{ticket}");
    let key = ticket["key"].as_str().unwrap().to_string();
    let path = format!("/api/v1/tickets/{key}");
    assert_eq!(
        f.http(
            "POST",
            &format!("{path}/comments"),
            Some(&f.member),
            Some(json!({"body":"先看日志"}))
        )
        .0,
        201
    );
    let (_, list) = f.http("GET", "/api/v1/members", Some(&f.member), None);
    let wang = list
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["name"] == "小王")
        .unwrap();
    assert!(
        wang.get("token_hint").is_none(),
        "普通成员看不到别人的令牌信息"
    );
    let assign = json!({"member_id": wang["id"]});
    assert_eq!(
        f.http(
            "POST",
            &format!("{path}/assign"),
            Some(&f.member),
            Some(assign)
        )
        .0,
        200
    );
    let (_, detail) = f.http("GET", &path, Some(&f.viewer), None);
    // 建单、评论、指派各通知一次，桌面界面才会跟着刷新
    let id = detail["id"].as_str().unwrap();
    let changed = f
        .events
        .lock()
        .unwrap()
        .iter()
        .filter(|e| matches!(e, EngineEvent::Changed { ticket_id: Some(t), .. } if t == id))
        .count();
    assert_eq!(changed, 3);
    assert_eq!(detail["assignee"], "小王");
    assert_eq!(detail["prd_md"], "列表为空时崩溃");
    let timeline = detail["timeline"].as_array().unwrap();
    let comment = timeline.iter().find(|e| e["type"] == "comment").unwrap();
    assert_eq!(
        (
            comment["actor"].as_str(),
            comment["payload"]["body"].as_str()
        ),
        (Some("小王"), Some("先看日志"))
    );
    assert!(
        timeline.iter().all(|e| e["actor"] == "小王"),
        "{timeline:?}"
    );
    let (_, listed) = f.http(
        "GET",
        "/api/v1/tickets?q=%E6%8E%A5%E5%8F%A3",
        Some(&f.viewer),
        None,
    );
    assert_eq!(listed.as_array().unwrap().len(), 1);

    // 合并只有 owner 能做；这里子任务不存在，owner 得到 404 而不是 403
    assert_eq!(
        f.http("POST", "/api/v1/tasks/nope/merge", Some(&f.member), None)
            .0,
        403
    );
    assert_eq!(
        f.http("POST", "/api/v1/tasks/nope/merge", Some(&f.owner), None)
            .0,
        404
    );

    // 别的网页发来的请求（带别的 Origin）不处理
    let (status, _) = f.http_with(
        "GET",
        "/api/v1/tickets",
        Some(&f.owner),
        None,
        &[("Origin", "http://evil.example")],
    );
    assert_eq!(status, 403);
    // 请求体不是 JSON
    let (status, err) = f.http(
        "POST",
        "/api/v1/tickets",
        Some(&f.member),
        Some(Value::String("x".into())),
    );
    assert_eq!(status, 400, "{err}");

    // 自定义工作流的环节可以经接口通过，记下是谁
    workflows::create(
        &lock(f.engine.db()),
        workflows::WorkflowInput {
            name: "带 QA".into(),
            enabled: true,
            config: workflows::WorkflowConfig {
                ticket_types: vec!["bug".into()],
                stages: vec![workflows::Stage {
                    name: "QA 验证".into(),
                    gate: "done".into(),
                    ..Default::default()
                }],
                auto_done: true,
            },
        },
    )
    .unwrap();
    let (_, detail) = f.http("GET", &path, Some(&f.viewer), None);
    let stage = detail["stages"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(detail["stages"][0]["current"], false, "工单还没到验收");
    let pass = format!("{path}/stages/{stage}/pass");
    assert_eq!(f.http("POST", &pass, Some(&f.viewer), None).0, 403);
    let (status, err) = f.http("POST", &pass, Some(&f.member), None);
    assert_eq!(status, 400, "还没轮到这个环节：{err}");

    // 停用后令牌立刻失效
    let id = wang["id"].as_str().unwrap().to_string();
    members::update(&lock(f.engine.db()), &id, "小王", "member", true).unwrap();
    assert_eq!(f.http("GET", "/api/v1/me", Some(&f.member), None).0, 401);
}

#[test]
fn full_auto_tasks_need_the_owner_to_confirm_over_the_api() {
    let f = fixture(2);
    let (_, task) = f.bug("全自动", "[mock:steps=1] [mock:delay=20]");
    lock(f.engine.db())
        .execute(
            "UPDATE task SET permission_profile = 'full_auto' WHERE id = ?1",
            [&task],
        )
        .unwrap();
    let path = format!("/api/v1/tasks/{task}/start");
    assert_eq!(
        f.http(
            "POST",
            &path,
            Some(&f.member),
            Some(json!({"confirm_full_auto":true}))
        )
        .0,
        403
    );
    assert_eq!(f.http("POST", &path, Some(&f.owner), None).0, 403);
    let (status, run) = f.http(
        "POST",
        &path,
        Some(&f.owner),
        Some(json!({"confirm_full_auto":true})),
    );
    assert_eq!(status, 200, "{run}");
    f.wait_done(run["id"].as_str().unwrap());

    // 打回并立刻重跑也算开始：member 不行，owner 也要确认
    let path = format!("/api/v1/tasks/{task}/reject");
    let rerun = json!({"note":"再改一下","rerun":true});
    assert_eq!(
        f.http("POST", &path, Some(&f.member), Some(rerun.clone()))
            .0,
        403
    );
    assert_eq!(f.http("POST", &path, Some(&f.owner), Some(rerun)).0, 403);
    assert_eq!(
        tasks::get(&lock(f.engine.db()), &task).unwrap().status,
        "review",
        "被拒绝的请求不应该改动子任务"
    );
    let (status, t) = f.http(
        "POST",
        &path,
        Some(&f.owner),
        Some(json!({"note":"再改一下","rerun":true,"confirm_full_auto":true})),
    );
    assert_eq!(status, 200, "{t}");
}

#[test]
fn mcp_lists_tools_by_caller_and_members_can_create_and_read_tickets() {
    let f = fixture(2);
    let rpc = |method: &str| json!({"jsonrpc":"2.0","id":1,"method":method,"params":{}});
    let (status, init) = f.http("POST", "/mcp", Some(&f.member), Some(rpc("initialize")));
    assert_eq!(status, 200);
    assert_eq!(init["result"]["serverInfo"]["name"], "tidewatch");
    // 通知没有响应体
    let note = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
    assert_eq!(f.http("POST", "/mcp", Some(&f.member), Some(note)).0, 202);
    let (_, list) = f.http("POST", "/mcp", Some(&f.member), Some(rpc("tools/list")));
    let names: Vec<&str> = list["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert!(
        names.contains(&"tw_ticket_create") && !names.contains(&"tw_task_delegate"),
        "{names:?}"
    );
    let (_, unknown) = f.http("POST", "/mcp", Some(&f.member), Some(rpc("resources/list")));
    assert_eq!(unknown["error"]["code"], -32601);

    let (err, made) = f.tool(
        &f.member,
        "tw_ticket_create",
        json!({"title":"MCP 建的单","content_md":"正文"}),
    );
    assert!(!err, "{made}");
    let (err, read) = f.tool(&f.viewer, "tw_ticket_read", json!({"ticket": made["key"]}));
    assert!(!err);
    assert_eq!(
        (read["title"].as_str(), read["prd_md"].as_str()),
        (Some("MCP 建的单"), Some("正文"))
    );
    // viewer 不能写；委派只给运行中的 Agent
    assert!(
        f.tool(&f.viewer, "tw_ticket_create", json!({"title":"x"}))
            .0
    );
    assert!(
        f.tool(
            &f.viewer,
            "tw_ask_user",
            json!({"ticket": made["key"], "question":"?"})
        )
        .0
    );
    assert!(
        f.tool(&f.owner, "tw_task_delegate", json!({"prompt":"x"}))
            .0
    );
}

#[test]
fn agent_asks_and_delegates_over_mcp_while_holding_the_only_slot() {
    // 并发上限 1：调用方占着唯一的名额等委派结束，委派不能因此排不上
    let f = fixture(1);
    let (ticket, task) = f.bug(
        "列表为空时崩溃",
        "[mock:ask] [mock:delegate] [mock:steps=1] [mock:delay=30]",
    );
    let run = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let done = f.wait_done(&run.id);
    assert_eq!(done.status, "succeeded", "{:?}", done.error);
    let summary = done.summary.unwrap_or_default();
    assert!(summary.contains("有一个问题挂给了你"), "{summary}");
    assert!(summary.contains("委派的检查结束（succeeded）"), "{summary}");

    // 委派：只读、挂在同一个子任务下、在调用方的 worktree 里跑
    let children = delegations::children(&lock(f.engine.db()), &run.id).unwrap();
    assert_eq!(children.len(), 1);
    assert_eq!(children[0].task_id.as_deref(), Some(task.as_str()));
    let child = f.wait_done(&children[0].run_id);
    assert_eq!(
        (child.purpose.as_str(), child.permission.as_deref()),
        ("delegate", Some("read_only"))
    );
    assert!(child.task_id.is_none(), "委派不算子任务自己的运行");
    let worktree = tasks::get(&lock(f.engine.db()), &task)
        .unwrap()
        .worktree_path
        .unwrap();
    let logged = std::fs::read_to_string(child.raw_log_path.as_deref().unwrap()).unwrap();
    let cwd = serde_json::to_string(&worktree).unwrap();
    assert!(
        logged.contains(cwd.trim_matches('"')),
        "委派应该在调用方的 worktree（{worktree}）里跑"
    );

    // 提问：挂在工单上，发了通知事件；运行结束后令牌失效
    let open = questions::list_open(&lock(f.engine.db())).unwrap();
    assert_eq!(open.len(), 1);
    assert_eq!(
        (open[0].ticket_id.as_str(), open[0].task_id.as_deref()),
        (ticket.as_str(), Some(task.as_str()))
    );
    assert_eq!(open[0].options, vec!["[]", "null"]);
    assert!(f
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, EngineEvent::QuestionAsked { .. })));
    let token = f.engine.run_token(&run.id).unwrap();
    assert_eq!(
        f.http(
            "POST",
            "/mcp",
            Some(&token),
            Some(json!({"jsonrpc":"2.0","id":1,"method":"ping"}))
        )
        .0,
        401
    );

    // 人经 REST 回答；回答写进下一轮的 Prompt，而且只写一次
    let (_, listed) = f.http("GET", "/api/v1/questions", Some(&f.viewer), None);
    let qid = listed[0]["id"].as_str().unwrap().to_string();
    let answer = format!("/api/v1/questions/{qid}/answer");
    assert_eq!(
        f.http(
            "POST",
            &answer,
            Some(&f.viewer),
            Some(json!({"answer":"[]"}))
        )
        .0,
        403
    );
    let (status, answered) = f.http(
        "POST",
        &answer,
        Some(&f.owner),
        Some(json!({"answer":"返回 []"})),
    );
    assert_eq!(
        (status, answered["answered_by"].as_str()),
        (200, Some("刘"))
    );
    assert_eq!(
        f.http(
            "POST",
            &answer,
            Some(&f.owner),
            Some(json!({"answer":"再答一次"}))
        )
        .0,
        400
    );
    lock(f.engine.db())
        .execute(
            "UPDATE ticket SET title = '不再提问' WHERE id = ?1",
            [&ticket],
        )
        .unwrap();
    let second = f
        .engine
        .start_task(
            &task,
            StartOptions {
                prompt_override: Some("继续 [mock:steps=1] [mock:delay=20]".into()),
                ..Default::default()
            },
        )
        .unwrap();
    let second = f.wait_done(&second.id);
    assert!(
        second.prompt.contains("你之前提的问题，现在有了回答"),
        "{}",
        second.prompt
    );
    assert!(second.prompt.contains("答：返回 []"), "{}", second.prompt);
    let third = f
        .engine
        .start_task(
            &task,
            StartOptions {
                prompt_override: Some("再继续 [mock:steps=1] [mock:delay=20]".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(!f.wait_done(&third.id).prompt.contains("答：返回 []"));
}

#[test]
fn run_tokens_are_scoped_to_their_ticket_and_stopping_the_caller_stops_its_delegates() {
    let f = fixture(1);
    let (ticket, task) = f.bug("慢任务", "[mock:steps=400] [mock:delay=100]");
    let (other, _) = f.bug("别的工单", "不相干");
    let run = f.engine.start_task(&task, StartOptions::default()).unwrap();
    f.wait_run(&run.id, "开始运行", |r| r.status == "running");
    let token = f.engine.run_token(&run.id).unwrap();

    // 运行令牌：只能用 MCP，只看得到自己的工单，工具列表里有委派
    assert_eq!(f.http("GET", "/api/v1/tickets", Some(&token), None).0, 403);
    // 换掉最后一个字符（原来正好是 0 的话换成 1，不然伪造出来的就是原令牌）
    let last = if token.ends_with('0') { '1' } else { '0' };
    let forged = format!("{}{last}", &token[..token.len() - 1]);
    assert_eq!(
        f.http("POST", "/mcp", Some(&forged), Some(json!({}))).0,
        401
    );
    let (err, read) = f.tool(&token, "tw_ticket_read", json!({"ticket": other}));
    assert!(!err);
    assert_eq!(
        read["id"].as_str(),
        Some(ticket.as_str()),
        "传别的工单号也只读自己的"
    );
    assert!(f.tool(&token, "tw_ticket_create", json!({"title":"x"})).0);

    // 委派：同一个请求 id 只派一次
    let args =
        json!({"prompt":"慢慢看 [mock:steps=400] [mock:delay=100]","client_request_id":"r1"});
    let (err, child) = f.tool(&token, "tw_task_delegate", args.clone());
    assert!(!err, "{child}");
    let (_, again) = f.tool(&token, "tw_task_delegate", args);
    assert_eq!(child["run_id"], again["run_id"]);
    let child_id = child["run_id"].as_str().unwrap().to_string();
    f.wait_run(&child_id, "委派开始运行", |r| r.status == "running");
    let (_, status) = f.tool(&token, "tw_task_status", json!({"run_id": child_id}));
    assert_eq!(
        (status["status"].as_str(), status["finished"].as_bool()),
        (Some("running"), Some(false))
    );
    // 等不到就按时返回，不会一直挂着
    let started = Instant::now();
    let (_, waited) = f.tool(
        &token,
        "tw_task_wait",
        json!({"run_id": child_id, "timeout_seconds": 1}),
    );
    assert_eq!(waited["finished"], false);
    assert!(started.elapsed() < Duration::from_secs(20));
    // 别人的运行查不到
    assert!(
        f.tool(&token, "tw_task_status", json!({"run_id": run.id}))
            .0
    );

    // 委派套委派有层数上限；被委派的运行拿自己的令牌也只能看这张工单
    let child_token = f.engine.run_token(&child_id).unwrap();
    let nested = f
        .engine
        .delegate(
            &child_id,
            DelegateInput {
                prompt: "再看 [mock:steps=400] [mock:delay=100]".into(),
                ..Default::default()
            },
        )
        .unwrap();
    f.wait_run(&nested.id, "第二层委派开始运行", |r| {
        r.status == "running"
    });
    let too_deep = f.engine.delegate(
        &nested.id,
        DelegateInput {
            prompt: "x".into(),
            ..Default::default()
        },
    );
    assert!(too_deep.unwrap_err().to_string().contains("最多套"));
    let (_, read) = f.tool(&child_token, "tw_ticket_read", json!({}));
    assert_eq!(read["id"].as_str(), Some(ticket.as_str()));
    // 调用方能看到孙辈
    assert!(
        !f.tool(&token, "tw_task_status", json!({"run_id": nested.id}))
            .0
    );

    // 停掉调用方：两层委派都跟着停
    assert_eq!(
        f.http(
            "POST",
            &format!("/api/v1/runs/{}/stop", run.id),
            Some(&f.member),
            None
        )
        .0,
        200
    );
    assert_eq!(f.wait_done(&run.id).status, "stopped");
    for id in [&child_id, &nested.id] {
        let r = f.wait_done(id);
        assert!(
            matches!(r.status.as_str(), "stopped" | "failed"),
            "{} {:?}",
            r.status,
            r.error
        );
    }
    assert_eq!(f.http("POST", "/mcp", Some(&token), Some(json!({}))).0, 401);
    // 结束后不能再委派
    assert!(f
        .engine
        .delegate(
            &run.id,
            DelegateInput {
                prompt: "x".into(),
                ..Default::default()
            }
        )
        .is_err());

    // 关掉接口后端口不再应答，之后的运行不带 MCP
    let base = f.server.base_url();
    let Fixture { engine, server, .. } = f;
    server.stop();
    assert!(engine.run_token(&run.id).is_none());
    assert!(ureq::get(&format!("{base}/api/v1/health")).call().is_err());
}
