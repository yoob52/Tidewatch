//! 用真实的 Claude Code / Codex 走一遍预审和拆分，检查它们的输出能被解析入库。
//! 需要账号和网络，默认不跑：
//!
//! ```text
//! TW_REAL_AGENT=claude_code TW_REAL_BASE_URL=... TW_REAL_KEY=... TW_REAL_MODEL=... \
//!   cargo test -p twctl --test real_agents -- --ignored --nocapture
//! ```
//!
//! `TW_REAL_BASE_URL` / `TW_REAL_KEY` 不设时用本机登录态。密钥只放进内存里的凭据存储。

use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use tidewatch_core::db::{self, lock};
use tidewatch_core::engine::{Engine, EngineConfig};
use tidewatch_core::git;
use tidewatch_core::secrets::MemoryStore;
use tidewatch_core::store::{projects, providers, reviews, runs, settings, tickets};

const PRD: &str = "## 背景\n用户想在个人中心看到自己的优惠券。\n\n## 需求\n- 新增「我的优惠券」页面，展示优惠券列表\n- 列表按过期时间排序\n- 已使用和已过期的置灰\n- 后端提供查询接口\n";

fn init_repo(dir: &Path, files: &[(&str, &str)]) {
    std::fs::create_dir_all(dir).unwrap();
    git::git(dir, &["init", "-q", "-b", "main"]).unwrap();
    for (name, body) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
    }
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

fn wait_run(engine: &Engine, id: &str) -> runs::Run {
    let deadline = Instant::now() + Duration::from_secs(900);
    loop {
        let run = runs::get(&lock(engine.db()), id).unwrap();
        if !runs::ACTIVE.contains(&run.status.as_str()) {
            return run;
        }
        assert!(Instant::now() < deadline, "运行 15 分钟还没结束");
        std::thread::sleep(Duration::from_millis(500));
    }
}

#[test]
#[ignore = "需要真实 Agent 的账号和网络"]
fn real_agent_review_and_split_are_parsed() {
    let agent = std::env::var("TW_REAL_AGENT").expect("设置 TW_REAL_AGENT=claude_code 或 codex");
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    init_repo(
        &root.join("api"),
        &[
            ("README.md", "# shop-api\nExpress + SQLite 的订单服务。\n"),
            (
                "src/routes/orders.js",
                "// GET /api/orders -> { code, data: Order[] }\nmodule.exports = (app, db) => {\n  app.get('/api/orders', (req, res) => res.json({ code: 0, data: db.orders(req.user.id) }));\n};\n",
            ),
        ],
    );
    init_repo(
        &root.join("web"),
        &[("README.md", "# shop-web\nReact 前端，页面在 src/pages。\n")],
    );

    let conn = db::open(&root.join("data/tidewatch.db")).unwrap();
    let secrets = MemoryStore::default();
    let mut st = settings::Settings {
        default_agent: agent.clone(),
        ..Default::default()
    };
    if let Ok(base_url) = std::env::var("TW_REAL_BASE_URL") {
        let model = std::env::var("TW_REAL_MODEL").ok();
        let provider = providers::create(
            &conn,
            providers::ProviderInput {
                name: "real".into(),
                protocol: std::env::var("TW_REAL_PROTOCOL").unwrap_or_else(|_| {
                    if agent == "codex" {
                        "openai_responses".into()
                    } else {
                        "anthropic".into()
                    }
                }),
                base_url: Some(base_url),
                models: model.clone().into_iter().collect(),
                default_model: model,
                ..Default::default()
            },
        )
        .unwrap();
        let key = std::env::var("TW_REAL_KEY").expect("设置了 TW_REAL_BASE_URL 就要给 TW_REAL_KEY");
        providers::store_secret(&conn, &secrets, &provider.id, Some(&key)).unwrap();
        st.default_provider_id = Some(provider.id);
    }
    settings::save(&conn, &st).unwrap();
    let project = |name: &str, kind: &str| {
        projects::create(
            &conn,
            projects::ProjectInput {
                name: name.into(),
                kind: kind.into(),
                local_path: root
                    .join(name.trim_start_matches("shop-"))
                    .display()
                    .to_string(),
                ..Default::default()
            },
        )
        .unwrap()
        .id
    };
    let (api, web) = (
        project("shop-api", "backend"),
        project("shop-web", "frontend"),
    );
    let engine = Engine::start(
        db::shared(conn),
        Box::new(secrets),
        EngineConfig {
            data_dir: root.join("data"),
            mock_binary: None,
        },
        Arc::new(|_| {}),
    )
    .unwrap();
    let ticket = tickets::create(
        &mut lock(engine.db()),
        tickets::TicketInput {
            title: "我的优惠券".into(),
            kind: "feature_mixed".into(),
            content_md: PRD.into(),
            projects: vec![
                tickets::TicketProject {
                    project_id: api,
                    role: "backend".into(),
                    subdir: None,
                },
                tickets::TicketProject {
                    project_id: web,
                    role: "frontend".into(),
                    subdir: None,
                },
            ],
            ..Default::default()
        },
    )
    .unwrap()
    .id;

    // 预审
    let run = wait_run(&engine, &engine.start_review(&ticket).unwrap().id);
    let (review, issues) = {
        let conn = lock(engine.db());
        (
            reviews::list(&conn, &ticket).unwrap().pop().unwrap(),
            reviews::issues(&conn, &ticket).unwrap(),
        )
    };
    println!(
        "预审：运行 {}，{} 个问题，{} 条验收标准草稿，总评：{}",
        run.status,
        issues.len(),
        review.criteria_draft.len(),
        review.summary
    );
    for i in &issues {
        println!(
            "  [{}] {} {}：{}",
            i.severity, i.code, i.category, i.problem
        );
    }
    assert_eq!(run.status, "succeeded", "{:?}", run.error);
    assert_eq!(review.error, None, "预审输出没能解析");
    assert!(!review.summary.trim().is_empty());
    assert!(!issues.is_empty(), "这份 PRD 很粗，应该能提出问题");
    for i in &issues {
        assert!(
            ["blocker", "major", "minor", "suggestion"].contains(&i.severity.as_str()),
            "严重程度不在约定范围内：{}",
            i.severity
        );
        assert!(!i.problem.trim().is_empty());
    }
    assert_eq!(
        tickets::get(&lock(engine.db()), &ticket).unwrap().status,
        "human_review"
    );
    // 只读：预审不应该改动项目目录
    assert_eq!(
        git::git(&root.join("api"), &["status", "--porcelain"])
            .unwrap()
            .trim(),
        "",
        "预审改动了项目文件"
    );

    // 处理掉阻塞问题后定稿，让 AI 拆分
    {
        let mut conn = lock(engine.db());
        for i in issues.iter().filter(|i| i.severity == "blocker") {
            reviews::resolve(&mut conn, &i.id, "ignore", Some("测试里先跳过"), None).unwrap();
        }
        let mut criteria = review.criteria_draft.clone();
        if criteria.is_empty() {
            criteria.push("能看到自己的优惠券列表".into());
        }
        tickets::set_criteria(&conn, &ticket, criteria).unwrap();
    }
    let split_run = engine.finalize(&ticket, true).unwrap().run.unwrap();
    let split_run = wait_run(&engine, &split_run.id);
    let proposal = reviews::latest_proposal(&lock(engine.db()), &ticket, "split")
        .unwrap()
        .unwrap();
    println!("拆分：运行 {}，建议 {}", split_run.status, proposal.status);
    assert_eq!(split_run.status, "succeeded", "{:?}", split_run.error);
    assert_eq!(proposal.error, None, "拆分输出没能解析");
    assert_eq!(proposal.status, "pending");
    let drafts: Vec<reviews::TaskDraft> =
        serde_json::from_value(proposal.content["tasks"].clone()).unwrap();
    for d in &drafts {
        println!(
            "  [{}] {} @ {}，依赖 {:?}（{:?}）",
            d.kind, d.title, d.project, d.depends_on, d.dependency_kind
        );
    }
    // 能按建议原样建出子任务（项目名、依赖、类型都合法）
    let created = engine.apply_split(&ticket, drafts).unwrap();
    assert!(created.iter().any(|t| t.kind == "backend"));
    assert!(created.iter().any(|t| t.kind == "frontend"));
    engine.shutdown();
}

/// 真实 Agent 要能看到 PRD 里的图：让它把图里的券码抄进文件。
#[test]
#[ignore = "需要真实 Agent 的账号和网络"]
fn real_agent_reads_the_prd_image() {
    use tidewatch_core::engine::StartOptions;
    use tidewatch_core::store::{attachments, tasks};

    let agent = std::env::var("TW_REAL_AGENT").expect("设置 TW_REAL_AGENT=claude_code 或 codex");
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    init_repo(&root.join("api"), &[("README.md", "# shop-api\n")]);
    let conn = db::open(&root.join("data/tidewatch.db")).unwrap();
    let secrets = MemoryStore::default();
    let mut st = settings::Settings {
        default_agent: agent.clone(),
        ..Default::default()
    };
    if let Ok(base_url) = std::env::var("TW_REAL_BASE_URL") {
        let model = std::env::var("TW_REAL_MODEL").ok();
        let provider = providers::create(
            &conn,
            providers::ProviderInput {
                name: "real".into(),
                protocol: std::env::var("TW_REAL_PROTOCOL").unwrap_or_else(|_| {
                    if agent == "codex" {
                        "openai_responses".into()
                    } else {
                        "anthropic".into()
                    }
                }),
                base_url: Some(base_url),
                models: model.clone().into_iter().collect(),
                default_model: model,
                ..Default::default()
            },
        )
        .unwrap();
        let key = std::env::var("TW_REAL_KEY").expect("设置了 TW_REAL_BASE_URL 就要给 TW_REAL_KEY");
        providers::store_secret(&conn, &secrets, &provider.id, Some(&key)).unwrap();
        st.default_provider_id = Some(provider.id);
    }
    settings::save(&conn, &st).unwrap();
    let project = projects::create(
        &conn,
        projects::ProjectInput {
            name: "shop-api".into(),
            kind: "backend".into(),
            local_path: root.join("api").display().to_string(),
            ..Default::default()
        },
    )
    .unwrap()
    .id;
    let workspace = st.workspace(&root.join("data"));
    let image = attachments::add(
        &conn,
        &workspace,
        None,
        "coupon.png",
        include_bytes!("fixtures/coupon.png"),
        "user",
    )
    .unwrap();
    let engine = Engine::start(
        db::shared(conn),
        Box::new(secrets),
        EngineConfig {
            data_dir: root.join("data"),
            mock_binary: None,
        },
        Arc::new(|_| {}),
    )
    .unwrap();
    let task = {
        let mut conn = lock(engine.db());
        let ticket = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: "把券码写进文件".into(),
                kind: "bug".into(),
                content_md: format!(
                    "下面这张截图里有一个券码。在仓库根目录新建 code.txt，内容只有这个券码，一字不差。\n\n![券码截图](attachment:{} \"authoritative\")\n",
                    image.id
                ),
                ..Default::default()
            },
        )
        .unwrap();
        tasks::create(
            &mut conn,
            tasks::TaskInput {
                ticket_id: ticket.id,
                kind: "bug".into(),
                project_id: project,
                title: "把券码写进文件".into(),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let run = wait_run(
        &engine,
        &engine
            .start_task(&task.id, StartOptions::default())
            .unwrap()
            .id,
    );
    println!(
        "run: {} {:?}\ncommand: {:?}",
        run.status, run.error, run.command
    );
    assert_eq!(run.status, "succeeded", "{:?}", run.error);
    let worktree = tasks::get(&lock(engine.db()), &task.id)
        .unwrap()
        .worktree_path
        .unwrap();
    let code = std::fs::read_to_string(Path::new(&worktree).join("code.txt")).unwrap();
    println!("code.txt: {code:?}");
    assert_eq!(code.trim(), "TW-7391-KQ");
}

/// 真实 Agent 经观澜注入的 MCP 服务读工单并提问（要求 Agent 支持 MCP：Claude Code、Codex、OpenCode）。
#[test]
#[ignore = "需要真实 Agent 的账号和网络"]
fn real_agent_calls_tidewatch_mcp_tools() {
    use tidewatch_core::engine::StartOptions;
    use tidewatch_core::store::{questions, tasks};
    let agent = std::env::var("TW_REAL_AGENT").expect("设置 TW_REAL_AGENT");
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    init_repo(&root.join("api"), &[("README.md", "# shop-api\n")]);
    let conn = db::open(&root.join("data/tidewatch.db")).unwrap();
    let secrets = MemoryStore::default();
    let mut st = settings::Settings {
        default_agent: agent.clone(),
        ..Default::default()
    };
    if let Ok(base_url) = std::env::var("TW_REAL_BASE_URL") {
        let model = std::env::var("TW_REAL_MODEL").ok();
        let provider = providers::create(
            &conn,
            providers::ProviderInput {
                name: "real".into(),
                protocol: std::env::var("TW_REAL_PROTOCOL")
                    .unwrap_or_else(|_| "openai_responses".into()),
                base_url: Some(base_url),
                models: model.clone().into_iter().collect(),
                default_model: model,
                ..Default::default()
            },
        )
        .unwrap();
        let key = std::env::var("TW_REAL_KEY").expect("设置了 TW_REAL_BASE_URL 就要给 TW_REAL_KEY");
        providers::store_secret(&conn, &secrets, &provider.id, Some(&key)).unwrap();
        st.default_provider_id = Some(provider.id);
    }
    settings::save(&conn, &st).unwrap();
    let project = projects::create(
        &conn,
        projects::ProjectInput {
            name: "shop-api".into(),
            kind: "backend".into(),
            local_path: root.join("api").display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let engine = Arc::new(
        Engine::start(
            db::shared(conn),
            Box::new(secrets),
            EngineConfig {
                data_dir: root.join("data"),
                mock_binary: None,
            },
            Arc::new(|_| {}),
        )
        .unwrap(),
    );
    let _server = tidewatch_core::api::ApiServer::start(engine.clone(), 0, false).unwrap();
    let (ticket, task) = {
        let mut conn = lock(engine.db());
        let ticket = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: "订单列表为空时崩溃".into(),
                kind: "bug".into(),
                content_md: "暗号是 TIDE-4711。\n\n请按顺序做三件事：\n1. 调用 tidewatch 这个 MCP 服务的 tw_ticket_read 工具读当前工单。\n2. 调用它的 tw_ask_user 工具，question 填「空列表时返回 [] 还是 null？」。\n3. 在 README.md 末尾追加一行 `mcp ok`。\n\n如果找不到这些 MCP 工具，在 README.md 末尾追加一行 `mcp missing`。".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = tasks::create(
            &mut conn,
            tasks::TaskInput {
                ticket_id: ticket.id.clone(),
                kind: "bug".into(),
                project_id: project.id,
                title: "修复".into(),
                ..Default::default()
            },
        )
        .unwrap();
        (ticket.id, task.id)
    };
    let run = engine.start_task(&task, StartOptions::default()).unwrap();
    let run = wait_run(&engine, &run.id);
    let events: Vec<String> = runs::events(&lock(engine.db()), &run.id, 0)
        .unwrap()
        .into_iter()
        .filter(|e| e.kind.contains("tool"))
        .map(|e| e.payload.to_string().chars().take(300).collect())
        .collect();
    println!(
        "状态 {} {:?}\n工具事件：\n{}",
        run.status,
        run.error,
        events.join("\n")
    );
    if let Some(raw) = run
        .raw_log_path
        .as_deref()
        .and_then(|p| std::fs::read_to_string(p).ok())
    {
        for line in raw.lines().filter(|l| l.contains("mcp")) {
            println!("原始：{}", line.chars().take(900).collect::<String>());
        }
    }
    assert_eq!(run.status, "succeeded", "{:?}", run.error);
    let asked = questions::list_for_ticket(&lock(engine.db()), &ticket).unwrap();
    assert_eq!(asked.len(), 1, "Agent 没有经 MCP 提问");
    assert!(asked[0].body.contains("空列表"), "{}", asked[0].body);
    assert_eq!(asked[0].task_id.as_deref(), Some(task.as_str()));
}

/// 智能创建的 AI 分析：真实 Agent 在空目录里只看正文和项目清单，回答能合进草稿。
#[test]
#[ignore = "需要真实 Agent 的账号和网络"]
fn real_agent_smart_analysis_is_parsed() {
    let agent = std::env::var("TW_REAL_AGENT").expect("设置 TW_REAL_AGENT");
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    let conn = db::open(&root.join("data/tidewatch.db")).unwrap();
    let secrets = MemoryStore::default();
    let mut st = settings::Settings {
        default_agent: agent.clone(),
        ..Default::default()
    };
    if let Ok(base_url) = std::env::var("TW_REAL_BASE_URL") {
        let model = std::env::var("TW_REAL_MODEL").ok();
        let provider = providers::create(
            &conn,
            providers::ProviderInput {
                name: "real".into(),
                protocol: std::env::var("TW_REAL_PROTOCOL").unwrap_or_else(|_| {
                    if agent == "claude_code" {
                        "anthropic".into()
                    } else {
                        "openai_responses".into()
                    }
                }),
                base_url: Some(base_url),
                models: model.clone().into_iter().collect(),
                default_model: model,
                ..Default::default()
            },
        )
        .unwrap();
        let key = std::env::var("TW_REAL_KEY").expect("设置了 TW_REAL_BASE_URL 就要给 TW_REAL_KEY");
        providers::store_secret(&conn, &secrets, &provider.id, Some(&key)).unwrap();
        st.default_provider_id = Some(provider.id);
    }
    settings::save(&conn, &st).unwrap();
    let project = |name: &str, kind: &str, card: &str| {
        projects::create(
            &conn,
            projects::ProjectInput {
                name: name.into(),
                kind: kind.into(),
                local_path: root.join(name).display().to_string(),
                context_md: card.into(),
                ..Default::default()
            },
        )
        .unwrap()
        .id
    };
    for name in ["shop-api", "shop-web", "ops-scripts"] {
        init_repo(&root.join(name), &[("README.md", "x\n")]);
    }
    let _api = project(
        "shop-api",
        "backend",
        "Express + SQLite 的订单与优惠券服务。",
    );
    let web = project(
        "shop-web",
        "frontend",
        "React 前端：个人中心、订单页、优惠券页。",
    );
    let _ops = project("ops-scripts", "other", "备份和部署脚本。");
    let engine = Engine::start(
        db::shared(conn),
        Box::new(secrets),
        EngineConfig {
            data_dir: root.join("data"),
            mock_binary: None,
        },
        Arc::new(|_| {}),
    )
    .unwrap();

    let text = "王姐：线上有用户反馈，个人中心的优惠券页面一打开就白屏，控制台里是 TypeError: Cannot read properties of undefined (reading 'map')。\n小李：我试了下，没领过券的新账号必现，有券的老账号正常。接口返回是 200，data 是 null。\n王姐：今天能修吗？影响所有新用户，挺急的。应该显示「暂无优惠券」才对。";
    let a = engine.smart_analyze(text).unwrap();
    eprintln!("{}", serde_json::to_string_pretty(&a).unwrap());
    assert_eq!(a.agent, agent);
    assert_eq!(a.draft.kind, "bug");
    assert!(["urgent", "high"].contains(&a.draft.priority.as_str()));
    assert!(a.draft.project_ids.contains(&web));
    assert!(!a.draft.title.is_empty() && a.draft.title.chars().count() <= 80);
    let bug = a.draft.bug_info.expect("Bug 三要素");
    assert!(bug["actual"].as_str().is_some_and(|s| !s.is_empty()));
    assert!(!a.draft.reasons.is_empty());
    assert_eq!(a.draft.content_md, text);
}
