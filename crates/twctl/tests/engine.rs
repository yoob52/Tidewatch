//! 执行引擎端到端：用 twctl 自带的演示 Agent 走完"建工单 → 子任务 → 运行 → 停止 → 续接 →
//! 检查 → 验收 → 合并"，以及排队、幂等、失败、打回返工和孤儿恢复。Linux 与 Windows 都跑。
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tidewatch_core::db::{self, lock, Db};
use tidewatch_core::engine::{Engine, EngineConfig, EngineEvent, StartOptions};
use tidewatch_core::git;
use tidewatch_core::secrets::MemoryStore;
use tidewatch_core::store::{projects, runs, settings, stats, tasks, tickets};

struct Fixture {
    engine: Engine,
    events: Arc<Mutex<Vec<EngineEvent>>>,
    repo: PathBuf,
    project_id: String,
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

/// `test_command`：项目的测试命令。`git --version` 两个平台都返回 0。
fn fixture(max_concurrency: u32, test_command: Option<&str>) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    let repo = root.join("repo");
    init_repo(&repo);
    let conn = db::open(&root.join("data/tidewatch.db")).unwrap();
    let st = settings::Settings {
        max_concurrency,
        default_agent: "mock".into(),
        ..Default::default()
    };
    settings::save(&conn, &st).unwrap();
    let project = projects::create(
        &conn,
        projects::ProjectInput {
            name: "演示 demo".into(),
            kind: "backend".into(),
            local_path: repo.display().to_string(),
            commands: projects::Commands {
                test: test_command.map(String::from),
                ..Default::default()
            },
            ..Default::default()
        },
    )
    .unwrap();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let engine = Engine::start(
        db::shared(conn),
        Box::new(MemoryStore::default()),
        EngineConfig {
            data_dir: root.join("data"),
            mock_binary: Some(PathBuf::from(env!("CARGO_BIN_EXE_twctl"))),
        },
        Arc::new(move |e| sink.lock().unwrap().push(e)),
    )
    .unwrap();
    Fixture {
        engine,
        events,
        repo,
        project_id: project.id,
        _dir: dir,
    }
}

impl Fixture {
    fn db(&self) -> &Db {
        self.engine.db()
    }

    /// 建一个 Bug 工单和它的一个子任务；`body` 里可以带演示 Agent 的标记。
    fn bug(&self, title: &str, body: &str, priority: &str) -> (String, String) {
        let mut conn = lock(self.db());
        let ticket = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: title.into(),
                kind: "bug".into(),
                priority: Some(priority.into()),
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
        runs::get(&lock(self.db()), id).unwrap()
    }

    fn task(&self, id: &str) -> tasks::Task {
        tasks::get(&lock(self.db()), id).unwrap()
    }

    fn ticket_status(&self, id: &str) -> String {
        tickets::get(&lock(self.db()), id).unwrap().status
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

    fn event_kinds(&self, run_id: &str) -> Vec<String> {
        runs::events(&lock(self.db()), run_id, 0)
            .unwrap()
            .into_iter()
            .map(|e| e.kind)
            .collect()
    }
}

#[test]
fn bug_ticket_from_start_to_merge_with_stop_and_resume() {
    let f = fixture(2, Some("git --version"));
    let (ticket, task) = f.bug(
        "金额为负",
        "优惠券叠加后金额为负 [mock:steps=8] [mock:delay=250]",
        "high",
    );

    // 第 1 轮：跑到一半停掉
    let r1 = f.engine.start_task(&task, StartOptions::default()).unwrap();
    assert_eq!(
        (r1.status.as_str(), r1.number, r1.agent.as_str()),
        ("queued", 1, "mock")
    );
    assert!(r1.prompt.contains("优惠券叠加后金额为负") && r1.prompt.contains("TW-1"));
    assert_eq!(f.ticket_status(&ticket), "developing", "Bug 工单走快速通道");
    f.wait_run(&r1.id, "Agent 开始写文件", |_| {
        f.event_kinds(&r1.id)
            .iter()
            .filter(|k| *k == "tool_call")
            .count()
            >= 2
    });
    assert_eq!(f.task(&task).status, "running");
    assert!(
        f.engine.start_task(&task, StartOptions::default()).is_err(),
        "运行中不能再开一轮"
    );
    f.engine.stop_run(&r1.id).unwrap();
    let r1 = f.wait_done(&r1.id);
    assert_eq!(r1.status, "stopped");
    let session = r1
        .agent_session_id
        .clone()
        .expect("停止的运行也要留下会话 id");
    assert!(r1.end_commit.is_some() && r1.start_commit.is_some());
    assert!(r1
        .raw_log_path
        .as_ref()
        .is_some_and(|p| Path::new(p).exists()));
    let t = f.task(&task);
    assert_eq!(t.status, "stopped");
    assert_eq!(t.branch.as_deref(), Some("tw/TW-1-bug"));
    let worktree = PathBuf::from(t.worktree_path.clone().unwrap());
    assert!(
        worktree.ends_with("worktrees/demo/TW-1-bug"),
        "{worktree:?}"
    );
    assert!(worktree.join("TIDEWATCH_DEMO.md").exists());
    assert!(
        !f.repo.join("TIDEWATCH_DEMO.md").exists(),
        "改动只在 worktree 里"
    );
    assert_eq!(f.ticket_status(&ticket), "developing");

    // 第 2 轮：续接同一个会话，跑完后执行项目的测试命令
    let r2 = f.engine.start_task(&task, StartOptions::default()).unwrap();
    assert_eq!(r2.number, 2);
    assert_eq!(r2.resume_session_id.as_deref(), Some(session.as_str()));
    assert_eq!(r2.parent_run_id.as_deref(), Some(r1.id.as_str()));
    let r2 = f.wait_done(&r2.id);
    assert_eq!(r2.status, "succeeded", "{:?}", r2.error);
    assert_eq!(r2.agent_session_id.as_deref(), Some(session.as_str()));
    assert!(r2.input_tokens > 0 && r2.output_tokens > 0);
    assert!(r2.agent_finished_at.is_some() && r2.finished_at.is_some());
    assert_eq!(r2.diff_stat.len(), 1);
    assert_eq!(r2.diff_stat[0].path, "TIDEWATCH_DEMO.md");
    let kinds = f.event_kinds(&r2.id);
    for k in [
        "spawned",
        "session_started",
        "tool_call",
        "finished",
        "check_started",
        "check_finished",
        "status",
    ] {
        assert!(kinds.iter().any(|x| x == k), "缺少事件 {k}：{kinds:?}");
    }
    let reports = runs::test_reports(&lock(f.db()), &r2.id).unwrap();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        (reports[0].command.as_str(), reports[0].exit_code),
        ("git --version", Some(0))
    );
    assert!(
        std::fs::read_to_string(reports[0].output_path.as_ref().unwrap())
            .unwrap()
            .contains("git version")
    );
    assert_eq!(f.task(&task).status, "review");
    assert_eq!(
        f.ticket_status(&ticket),
        "acceptance",
        "子任务都待验收后工单进入验收"
    );

    // Diff、验收、合并、清理
    let diff = f.engine.task_diff(&task).unwrap();
    assert_eq!(diff.files.len(), 1);
    assert!(diff.patch.contains("+++ b/TIDEWATCH_DEMO.md") && !diff.truncated);
    assert!(f.engine.merge_task(&task).is_err(), "没验收不能合并");
    assert_eq!(f.engine.accept_task(&task).unwrap().status, "accepted");
    let merged = f.engine.merge_task(&task).unwrap();
    assert_eq!(merged.status, "merged");
    let commit = merged.merged_commit.clone().unwrap();
    assert_eq!(git::rev_parse(&f.repo, "HEAD").unwrap(), commit);
    assert!(f.repo.join("TIDEWATCH_DEMO.md").exists());
    assert!(!worktree.exists(), "合并后删除 worktree");
    assert!(
        !git::branch_exists(&f.repo, "tw/TW-1-bug"),
        "默认合并后删除分支"
    );
    assert_eq!(f.ticket_status(&ticket), "done");
    assert_eq!(
        f.engine.task_diff(&task).unwrap().files.len(),
        1,
        "合并后仍能看 Diff"
    );
    assert!(git::rev_parse(
        &f.repo,
        &format!("refs/tidewatch/checkpoints/{task}/{}-end", r2.id)
    )
    .is_ok());

    let s = stats::compute(&lock(f.db())).unwrap();
    assert_eq!((s.tickets_done, s.runs_total), (1, 2));
    let mock = &s.agents[0];
    assert_eq!(
        (
            mock.agent.as_str(),
            mock.succeeded,
            mock.stopped,
            mock.failed
        ),
        ("mock", 1, 1, 0)
    );

    let events = f.events.lock().unwrap();
    assert!(events
        .iter()
        .any(|e| matches!(e, EngineEvent::RunEvent { .. })));
    let finished: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            EngineEvent::RunFinished {
                status, ticket_key, ..
            } => Some((status.clone(), ticket_key.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(
        finished,
        vec![
            ("stopped".to_string(), "TW-1".to_string()),
            ("succeeded".to_string(), "TW-1".to_string())
        ]
    );
}

#[test]
fn queue_respects_concurrency_and_ticket_priority() {
    let f = fixture(1, None);
    let body = "x [mock:steps=2] [mock:delay=200]";
    let (_, a) = f.bug("A", body, "low");
    let (_, b) = f.bug("B", body, "low");
    let (_, c) = f.bug("C", body, "urgent");
    let ra = f.engine.start_task(&a, StartOptions::default()).unwrap();
    f.wait_run(&ra.id, "A 开始运行", |r| r.status != "queued");
    let rb = f.engine.start_task(&b, StartOptions::default()).unwrap();
    let rc = f.engine.start_task(&c, StartOptions::default()).unwrap();
    let mut peak = 0;
    let deadline = Instant::now() + Duration::from_secs(90);
    loop {
        let (running, left): (u32, u32) = {
            let conn = lock(f.db());
            (
                runs::count_running(&conn).unwrap(),
                conn.query_row(
                    "SELECT COUNT(*) FROM run WHERE status IN ('queued', 'starting', 'running', 'finalizing', 'stopping')",
                    [],
                    |r| r.get(0),
                )
                .unwrap(),
            )
        };
        peak = peak.max(running);
        if left == 0 {
            break;
        }
        assert!(Instant::now() < deadline, "队列没有跑完");
        std::thread::sleep(Duration::from_millis(15));
    }
    assert_eq!(peak, 1, "并发上限是 1");
    let (ra, rb, rc) = (f.run(&ra.id), f.run(&rb.id), f.run(&rc.id));
    for r in [&ra, &rb, &rc] {
        assert_eq!(r.status, "succeeded", "{:?}", r.error);
    }
    assert!(
        rc.started_at < rb.started_at,
        "紧急工单后入队也先跑：C {:?} B {:?}",
        rc.started_at,
        rb.started_at
    );
    assert!(ra.finished_at <= rc.started_at, "A 结束后才轮到下一个");
}

#[test]
fn queued_run_can_be_withdrawn_and_command_id_is_idempotent() {
    let f = fixture(1, None);
    let (_, a) = f.bug("A", "x [mock:steps=3] [mock:delay=300]", "medium");
    let (_, b) = f.bug("B", "x", "medium");
    let ra = f.engine.start_task(&a, StartOptions::default()).unwrap();
    let opts = || StartOptions {
        command_id: Some("cmd-1".into()),
        ..Default::default()
    };
    let rb = f.engine.start_task(&b, opts()).unwrap();
    let again = f.engine.start_task(&b, opts()).unwrap();
    assert_eq!(rb.id, again.id, "同一个幂等键只入队一次");
    assert_eq!(runs::list_for_task(&lock(f.db()), &b).unwrap().len(), 1);

    f.engine.stop_run(&rb.id).unwrap();
    assert_eq!(f.run(&rb.id).status, "stopped");
    assert_eq!(f.task(&b).status, "stopped");
    assert!(f.engine.stop_run(&rb.id).is_err(), "已结束的不能再停");
    assert_eq!(f.wait_done(&ra.id).status, "succeeded");
    assert!(
        f.run(&rb.id).started_at.is_none(),
        "撤掉的排队运行不会被执行"
    );
}

#[test]
fn failures_and_failed_checks_are_reported() {
    let f = fixture(2, Some("git no-such-subcommand"));
    let (ticket, bad) = f.bug(
        "会失败",
        "x [mock:fail] [mock:steps=1] [mock:delay=50]",
        "medium",
    );
    let r = f.engine.start_task(&bad, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "failed");
    assert!(
        r.error.as_ref().is_some_and(|e| e.contains("[mock:fail]")),
        "{:?}",
        r.error
    );
    assert_eq!(f.task(&bad).status, "failed");
    assert_eq!(f.ticket_status(&ticket), "developing");
    assert!(
        runs::test_reports(&lock(f.db()), &r.id).unwrap().is_empty(),
        "Agent 失败就不跑检查"
    );

    let (ticket2, ok) = f.bug("测试不过", "x [mock:steps=1] [mock:delay=50]", "medium");
    let r = f.engine.start_task(&ok, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "succeeded");
    let reports = runs::test_reports(&lock(f.db()), &r.id).unwrap();
    assert_eq!(reports.len(), 1);
    assert_ne!(reports[0].exit_code, Some(0));
    assert_eq!(
        f.task(&ok).status,
        "review",
        "检查没过也交给人验收，由人决定"
    );
    assert_eq!(f.ticket_status(&ticket2), "acceptance");
    let events = f.events.lock().unwrap();
    assert!(events.iter().any(|e| matches!(e,
        EngineEvent::RunFinished { run_id, failed_checks, .. } if *run_id == r.id && failed_checks == &["test".to_string()])));
}

#[test]
fn failed_setup_is_run_again_on_retry() {
    let f = fixture(2, None);
    let set_setup = |command: &str| {
        lock(f.db())
            .execute(
                "UPDATE project SET commands_json = ?1 WHERE id = ?2",
                [
                    serde_json::json!({ "setup": command }).to_string(),
                    f.project_id.clone(),
                ],
            )
            .unwrap();
    };
    set_setup("git no-such-subcommand");
    let (_, task) = f.bug("准备失败", "x [mock:steps=1] [mock:delay=50]", "medium");
    let r = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "failed");
    assert!(r.error.as_ref().is_some_and(|e| e.contains("准备命令失败")));
    let t = f.task(&task);
    assert_eq!(t.worktree_path, None, "准备没做完的 worktree 不留");
    assert!(t.branch.is_some());

    set_setup("git --version");
    let r = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "succeeded", "{:?}", r.error);
    let kinds = f.event_kinds(&r.id);
    assert!(
        kinds.iter().any(|k| k == "check_started"),
        "重试时要重新跑准备命令：{kinds:?}"
    );
}

#[test]
fn rejection_sends_the_note_back_to_the_same_session() {
    let f = fixture(2, None);
    let (ticket, task) = f.bug("打回", "x [mock:steps=1] [mock:delay=50]", "medium");
    let r1 = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let r1 = f.wait_done(&r1.id);
    assert_eq!(f.ticket_status(&ticket), "acceptance");
    assert!(f.engine.reject_task(&task, "  ").is_err(), "打回要写原因");
    assert_eq!(
        f.engine.reject_task(&task, "边界值没处理").unwrap().status,
        "changes_requested"
    );
    assert_eq!(f.ticket_status(&ticket), "developing");

    // 不另给指令时，打回意见本身就是返工指令
    let plan = f.engine.plan(&task, &StartOptions::default()).unwrap();
    assert!(plan.prompt.contains("边界值没处理") && plan.resume.is_some());

    let opts = StartOptions {
        instruction: Some("边界值没处理 [mock:steps=1] [mock:delay=50]".into()),
        ..Default::default()
    };
    let plan = f.engine.plan(&task, &opts).unwrap();
    assert!(plan.prompt.contains("边界值没处理") && plan.resume.is_some());
    let r2 = f.engine.start_task(&task, opts).unwrap();
    assert_eq!(r2.purpose, "rework");
    assert_eq!(r2.resume_session_id, r1.agent_session_id);
    assert_eq!(f.wait_done(&r2.id).status, "succeeded");
    assert_eq!(f.ticket_status(&ticket), "acceptance");

    // 不续接：换新会话、用完整的首轮 Prompt
    f.engine.reject_task(&task, "重来").unwrap();
    let fresh = f
        .engine
        .plan(
            &task,
            &StartOptions {
                fresh_session: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(fresh.resume.is_none() && fresh.prompt.contains("修复下面这个 Bug"));
    assert!(fresh.prompt.contains("重来"));

    // 取消：worktree 删掉，分支留着
    let t = f.engine.cancel_task(&task).unwrap();
    assert_eq!((t.status.as_str(), t.worktree_path), ("cancelled", None));
    assert!(git::branch_exists(&f.repo, "tw/TW-1-bug"));
}

#[test]
fn rejecting_a_task_blocks_its_dependents_again() {
    let f = fixture(2, None);
    let (ticket, first) = f.bug("依赖", "x [mock:steps=1] [mock:delay=50]", "medium");
    let second = tasks::create(
        &mut lock(f.db()),
        tasks::TaskInput {
            ticket_id: ticket.clone(),
            kind: "bug".into(),
            project_id: f.project_id.clone(),
            title: "后续".into(),
            depends_on: vec![first.clone()],
            ..Default::default()
        },
    )
    .unwrap()
    .id;
    assert_eq!(f.task(&second).status, "blocked");
    let run = f
        .engine
        .start_task(&first, StartOptions::default())
        .unwrap();
    f.wait_done(&run.id);
    assert_eq!(f.task(&second).status, "pending");

    f.engine.reject_task(&first, "还差一步").unwrap();
    assert_eq!(f.task(&second).status, "blocked");
    assert!(f
        .engine
        .start_task(&second, StartOptions::default())
        .is_err());

    let run = f
        .engine
        .start_task(&first, StartOptions::default())
        .unwrap();
    f.wait_done(&run.id);
    assert_eq!(f.task(&second).status, "pending");
}

#[test]
fn feature_tickets_cannot_skip_review() {
    let f = fixture(2, None);
    let (ticket, task) = {
        let mut conn = lock(f.db());
        let ticket = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: "新需求".into(),
                kind: "feature_backend".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = tasks::create(
            &mut conn,
            tasks::TaskInput {
                ticket_id: ticket.id.clone(),
                kind: "backend".into(),
                project_id: f.project_id.clone(),
                title: "实现".into(),
                ..Default::default()
            },
        )
        .unwrap();
        (ticket.id, task.id)
    };
    let e = f
        .engine
        .start_task(&task, StartOptions::default())
        .unwrap_err();
    assert!(e.to_string().contains("草稿"), "{e}");
    assert_eq!(f.ticket_status(&ticket), "draft");
    assert_eq!(f.task(&task).status, "pending");
}

#[test]
fn unfinished_runs_become_orphans_on_restart() {
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    let repo = root.join("repo");
    init_repo(&repo);
    let mut conn = db::open(&root.join("tidewatch.db")).unwrap();
    let project = projects::create(
        &conn,
        projects::ProjectInput {
            name: "demo".into(),
            local_path: repo.display().to_string(),
            ..Default::default()
        },
    )
    .unwrap();
    let ticket = tickets::create(
        &mut conn,
        tickets::TicketInput {
            title: "x".into(),
            kind: "bug".into(),
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
            title: "x".into(),
            ..Default::default()
        },
    )
    .unwrap();
    // 模拟上次退出时还在跑的一轮
    let run = runs::enqueue(
        &conn,
        runs::NewRun {
            task_id: &task.id,
            ticket_id: &ticket.id,
            purpose: "dev",
            parent_run_id: None,
            agent: "mock",
            provider_id: None,
            model: None,
            permission: "workspace_write",
            resume_session_id: None,
            prompt: "x",
        },
    )
    .unwrap();
    runs::set_status(&conn, &run.id, "running").unwrap();
    tasks::set_status(&conn, &task.id, "running").unwrap();

    let engine = Engine::start(
        db::shared(conn),
        Box::new(MemoryStore::default()),
        EngineConfig {
            data_dir: root.join("data"),
            mock_binary: None,
        },
        Arc::new(|_| {}),
    )
    .unwrap();
    // 同一个数据目录不能再起第二个引擎：否则它会把这边正在跑的运行当成孤儿收掉
    let second = Engine::start(
        db::shared(db::open(&root.join("data/tidewatch.db")).unwrap()),
        Box::new(MemoryStore::default()),
        EngineConfig {
            data_dir: root.join("data"),
            mock_binary: None,
        },
        Arc::new(|_| {}),
    );
    assert!(second.err().unwrap().to_string().contains("已经在运行"));

    let conn = lock(engine.db());
    let run = runs::get(&conn, &run.id).unwrap();
    assert_eq!(run.status, "orphaned");
    assert!(run.error.is_some() && run.finished_at.is_some());
    assert_eq!(
        tasks::get(&conn, &task.id).unwrap().status,
        "stopped",
        "可以再点开始来续接"
    );
}

#[test]
fn dependents_that_already_ran_wait_for_a_rejected_prerequisite() {
    let f = fixture(1, None);
    let (ticket, first) = f.bug("依赖返工", "x [mock:steps=2] [mock:delay=100]", "medium");
    let second = tasks::create(
        &mut lock(f.db()),
        tasks::TaskInput {
            ticket_id: ticket.clone(),
            kind: "bug".into(),
            project_id: f.project_id.clone(),
            title: "后续".into(),
            depends_on: vec![first.clone()],
            ..Default::default()
        },
    )
    .unwrap()
    .id;
    let run = f
        .engine
        .start_task(&first, StartOptions::default())
        .unwrap();
    f.wait_done(&run.id);

    // 后续子任务跑过一轮后停下（已停止，不是"等依赖"），这时前置被打回
    let r2 = f
        .engine
        .start_task(&second, StartOptions::default())
        .unwrap();
    f.wait_run(&r2.id, "后续开始运行", |r| r.status == "running");
    f.engine.stop_run(&r2.id).unwrap();
    f.wait_done(&r2.id);
    assert_eq!(f.task(&second).status, "stopped");
    f.engine.reject_task(&first, "还差一步").unwrap();
    let err = f
        .engine
        .start_task(&second, StartOptions::default())
        .unwrap_err()
        .to_string();
    assert!(err.contains("还没完成"), "{err}");

    // 前置返工完成后，后续可以续接；它在排队时前置又被打回，这一轮就留在队列里
    let rework = f
        .engine
        .start_task(&first, StartOptions::default())
        .unwrap();
    f.wait_done(&rework.id);
    let busy = f
        .bug("占住并发", "x [mock:steps=6] [mock:delay=150]", "urgent")
        .1;
    let busy = f.engine.start_task(&busy, StartOptions::default()).unwrap();
    let r3 = f
        .engine
        .start_task(&second, StartOptions::default())
        .unwrap();
    assert_eq!(f.run(&r3.id).status, "queued");
    f.engine.reject_task(&first, "又差一步").unwrap();
    f.wait_done(&busy.id);
    std::thread::sleep(Duration::from_millis(400));
    assert_eq!(f.run(&r3.id).status, "queued", "前置没完成，不调度");
    let rework = f
        .engine
        .start_task(&first, StartOptions::default())
        .unwrap();
    f.wait_done(&rework.id);
    assert_eq!(f.wait_done(&r3.id).status, "succeeded");
}
