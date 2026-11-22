//! 执行引擎端到端：用 twctl 自带的演示 Agent 走完"建工单 → 子任务 → 运行 → 停止 → 续接 →
//! 检查 → 验收 → 合并"，以及排队、幂等、失败、打回返工和孤儿恢复。Linux 与 Windows 都跑。
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tidewatch_core::db::{self, lock, Db};
use tidewatch_core::engine::{
    Engine, EngineConfig, EngineEvent, InlineComment, RejectInput, StartOptions,
};
use tidewatch_core::git;
use tidewatch_core::secrets::MemoryStore;
use tidewatch_core::store::{projects, reviews, runs, settings, stats, tasks, tickets};

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
    fixture_with(max_concurrency, test_command, |_| {})
}

/// `seed`：在引擎启动前往库里放数据（模拟上次退出时留下的状态）。
fn fixture_with(
    max_concurrency: u32,
    test_command: Option<&str>,
    seed: impl FnOnce(&mut tidewatch_core::rusqlite::Connection),
) -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let root = tidewatch_core::util::canonical(dir.path()).unwrap();
    let repo = root.join("repo");
    init_repo(&repo);
    let mut conn = db::open(&root.join("data/tidewatch.db")).unwrap();
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
    seed(&mut conn);
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

    /// 再建一个前端项目，以及绑了前后端两个项目的混合需求工单。返回 (工单 id, 前端项目 id)。
    fn mixed(&self, title: &str, body: &str) -> (String, String) {
        let web = self.repo.parent().unwrap().join("web");
        init_repo(&web);
        let mut conn = lock(self.db());
        let web = projects::create(
            &conn,
            projects::ProjectInput {
                name: "演示 web".into(),
                kind: "frontend".into(),
                local_path: web.display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let ticket = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: title.into(),
                kind: "feature_mixed".into(),
                content_md: body.into(),
                projects: vec![
                    tickets::TicketProject {
                        project_id: self.project_id.clone(),
                        role: "backend".into(),
                        subdir: None,
                    },
                    tickets::TicketProject {
                        project_id: web.id.clone(),
                        role: "frontend".into(),
                        subdir: None,
                    },
                ],
                ..Default::default()
            },
        )
        .unwrap();
        (ticket.id, web.id)
    }

    /// 后端 + 前端两个子任务的拆分草稿，前端只等后端的接口文档。
    fn be_fe_drafts(&self, web: &str, be_spec: &str) -> Vec<reviews::TaskDraft> {
        vec![
            reviews::TaskDraft {
                r#ref: "T1".into(),
                kind: "backend".into(),
                project: self.project_id.clone(),
                title: "后端".into(),
                spec: be_spec.into(),
                ..Default::default()
            },
            reviews::TaskDraft {
                r#ref: "T2".into(),
                kind: "frontend".into(),
                project: web.into(),
                title: "前端".into(),
                depends_on: vec!["T1".into()],
                dependency_kind: Some("api_doc".into()),
                ..Default::default()
            },
        ]
    }

    fn ticket(&self, id: &str) -> tickets::Ticket {
        tickets::get(&lock(self.db()), id).unwrap()
    }

    fn wait_for(&self, what: &str, pred: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(90);
        while !pred() {
            assert!(Instant::now() < deadline, "等待「{what}」超时");
            std::thread::sleep(Duration::from_millis(40));
        }
    }

    /// 等这张工单上所有运行结束。
    fn wait_idle(&self, ticket: &str) {
        self.wait_for("工单上的运行全部结束", || {
            runs::list_for_ticket(&lock(self.db()), ticket)
                .unwrap()
                .iter()
                .all(|r| !runs::ACTIVE.contains(&r.status.as_str()))
        });
    }

    fn last_run(&self, task: &str) -> runs::Run {
        runs::list_for_task(&lock(self.db()), task)
            .unwrap()
            .pop()
            .expect("子任务还没有运行")
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

    // 关掉自动修复：检查没过直接交给人
    {
        let conn = lock(f.db());
        let mut st = settings::get(&conn).unwrap();
        st.auto_fix_attempts = 0;
        settings::save(&conn, &st).unwrap();
    }
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
    // 事件在运行落库之后才发出，等一下
    f.wait_for("RunFinished 带上没过的检查", || {
        f.events.lock().unwrap().iter().any(|e| matches!(e,
            EngineEvent::RunFinished { run_id, failed_checks, .. } if *run_id == r.id && failed_checks == &["test".to_string()]))
    });
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
    // 这里要看"解锁"本身，不让解锁的子任务自动入队
    {
        let conn = lock(f.db());
        let mut st = settings::get(&conn).unwrap();
        st.auto_start_unblocked = false;
        settings::save(&conn, &st).unwrap();
    }
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
            task_id: Some(&task.id),
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

// ---------- M2：预审 → 定稿 → 拆分 → 接口文档 → 打回 → 完成 ----------

#[test]
fn mixed_prd_from_draft_through_review_split_api_doc_and_rework_to_done() {
    let f = fixture(2, None);
    let (ticket, _web) = f.mixed("优惠券列表", "## 背景\n在个人中心展示用户的优惠券。\n");
    assert_eq!(f.ticket_status(&ticket), "draft");

    // 预审：工单进入 AI 预审，结束后回到人工审核，带着问题清单
    let review_run = f.engine.start_review(&ticket).unwrap();
    assert_eq!(
        (
            review_run.purpose.as_str(),
            review_run.permission.as_deref().unwrap_or_default(),
            &review_run.task_id
        ),
        ("review", "read_only", &None)
    );
    assert_eq!(f.ticket_status(&ticket), "ai_review");
    assert!(f.engine.start_review(&ticket).is_err(), "预审中不能再提交");
    assert_eq!(f.wait_done(&review_run.id).status, "succeeded");
    assert_eq!(f.ticket_status(&ticket), "human_review");
    let (review, issues) = {
        let conn = lock(f.db());
        (
            reviews::list(&conn, &ticket).unwrap().pop().unwrap(),
            reviews::issues(&conn, &ticket).unwrap(),
        )
    };
    assert_eq!(review.error, None);
    assert_eq!(review.criteria_draft.len(), 2);
    assert_eq!(
        issues
            .iter()
            .map(|i| (i.code.as_str(), i.severity.as_str()))
            .collect::<Vec<_>>(),
        [("I-1", "blocker"), ("I-2", "suggestion")]
    );
    f.wait_for("ReviewFinished 事件", || {
        f.events.lock().unwrap().iter().any(|e| {
            matches!(
                e,
                EngineEvent::ReviewFinished {
                    issues: 2,
                    blockers: 1,
                    error: None,
                    ..
                }
            )
        })
    });
    assert!(
        git::git(&f.repo, &["status", "--porcelain"])
            .unwrap()
            .trim()
            .is_empty(),
        "预审只读，不改项目目录"
    );

    // 定稿被拦住：先是验收标准为空，再是阻塞级问题没处理
    let err = f.engine.finalize(&ticket, true).unwrap_err().to_string();
    assert!(err.contains("阻塞级"), "{err}");
    {
        let mut conn = lock(f.db());
        // 采纳阻塞级问题的建议：正文存为新版本
        let fixed = reviews::resolve(&mut conn, &issues[0].id, "accept", None, None).unwrap();
        assert_eq!(fixed.resolution, "accepted");
        let prd = tickets::current_prd(&conn, &ticket).unwrap().unwrap();
        assert_eq!(prd.origin.as_deref(), Some("ai_suggestion"));
        assert!(prd
            .content_md
            .contains("演示补充：列表为空时显示「暂无数据」"));
        assert_eq!(
            fixed.resolved_in_version_id.as_deref(),
            Some(prd.id.as_str())
        );
        // 忽略必须写理由，理由进"不做范围"
        assert!(reviews::resolve(&mut conn, &issues[1].id, "ignore", Some(" "), None).is_err());
        reviews::resolve(&mut conn, &issues[1].id, "ignore", Some("首版不分页"), None).unwrap();
        let prd = tickets::current_prd(&conn, &ticket).unwrap().unwrap();
        assert!(prd.content_md.contains("## 不做范围") && prd.content_md.contains("首版不分页"));
    }
    let err = f.engine.finalize(&ticket, true).unwrap_err().to_string();
    assert!(err.contains("验收标准"), "{err}");
    tickets::set_criteria(&lock(f.db()), &ticket, review.criteria_draft.clone()).unwrap();

    // 再预审一次：处理过的问题不再提
    let again = f.engine.start_review(&ticket).unwrap();
    f.wait_done(&again.id);
    assert_eq!(reviews::issues(&lock(f.db()), &ticket).unwrap().len(), 2);

    // 定稿并拆分：锁定版本，拆分建议出来之前不能开始开发
    let fin = f.engine.finalize(&ticket, true).unwrap();
    assert_eq!(fin.ticket.status, "finalized");
    assert_eq!(
        fin.ticket.finalized_version_id,
        fin.ticket.current_version_id
    );
    let split_run = fin.run.unwrap();
    assert_eq!(split_run.purpose, "split");
    assert!(f.engine.start_development(&ticket).is_err());
    assert_eq!(f.wait_done(&split_run.id).status, "succeeded");
    let proposal = reviews::latest_proposal(&lock(f.db()), &ticket, "split")
        .unwrap()
        .unwrap();
    assert_eq!(
        (proposal.status.as_str(), &proposal.error),
        ("pending", &None)
    );
    let mut drafts: Vec<reviews::TaskDraft> =
        serde_json::from_value(proposal.content["tasks"].clone()).unwrap();
    assert_eq!(drafts.len(), 2);
    // 人可以在确认前改建议：这里让后端跑久一点，好看到"文档一出、前端就开始"
    drafts[0].spec.push_str(" [mock:steps=12] [mock:delay=200]");
    let created = f.engine.apply_split(&ticket, drafts).unwrap();
    let (be, fe) = (created[0].id.clone(), created[1].id.clone());
    assert_eq!(
        (created[0].kind.as_str(), created[0].status.as_str()),
        ("backend", "pending")
    );
    assert_eq!(
        (created[1].kind.as_str(), created[1].status.as_str()),
        ("frontend", "blocked")
    );
    assert_eq!(created[1].api_doc_deps, vec![be.clone()]);
    assert_ne!(created[0].project_id, created[1].project_id);

    // 开始开发：只有后端入队；后端写出接口文档后前端自动开始，这时后端还在跑
    let started = f.engine.start_development(&ticket).unwrap();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].task_id.as_deref(), Some(be.as_str()));
    assert_eq!(f.ticket_status(&ticket), "developing");
    f.wait_for("前端被接口文档解锁", || {
        f.task(&fe).status != "blocked"
    });
    assert!(
        runs::ACTIVE.contains(&f.run(&started[0].id).status.as_str()),
        "接口文档一出就解锁，不用等后端做完"
    );
    f.wait_for("前端自动入队", || {
        !runs::list_for_task(&lock(f.db()), &fe).unwrap().is_empty()
    });
    let fe_run = f.last_run(&fe);
    assert!(
        fe_run.prompt.contains("GET /api/items"),
        "前端的 Prompt 带上接口文档"
    );
    f.wait_idle(&ticket);
    assert_eq!(
        (f.task(&be).status, f.task(&fe).status),
        ("review".into(), "review".into())
    );
    assert_eq!(f.ticket_status(&ticket), "acceptance");
    let doc = tasks::artifact(&lock(f.db()), &be, "api_doc")
        .unwrap()
        .unwrap();
    assert_eq!(doc.path, "docs/api/TW-1.md");
    assert!(doc.commit_sha.is_some(), "运行结束后记下含有文档的提交");
    assert!(f
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, EngineEvent::ApiDocReady { unblocked: 1, .. })));

    // 验收打回一次：行内评论进返工 Prompt，续接原会话
    f.engine
        .reject_task_with(
            &fe,
            RejectInput {
                note: "空状态没做".into(),
                comments: vec![InlineComment {
                    file_path: "TIDEWATCH_DEMO.md".into(),
                    line: Some(3),
                    body: "这里要显示「暂无数据」".into(),
                }],
                rerun: true,
            },
        )
        .unwrap();
    assert_eq!(f.ticket_status(&ticket), "developing");
    let rework = f.last_run(&fe);
    assert_eq!(rework.number, 2);
    assert_eq!(rework.resume_session_id, fe_run_session(&f, &fe_run.id));
    assert!(rework.prompt.contains("空状态没做"));
    assert!(rework
        .prompt
        .contains("`TIDEWATCH_DEMO.md:3`：这里要显示「暂无数据」"));
    f.wait_idle(&ticket);
    assert_eq!(f.ticket_status(&ticket), "acceptance");

    // 通过、合并，工单完成；接口文档进了后端项目的主分支
    for t in [&be, &fe] {
        f.engine.accept_task(t).unwrap();
        f.engine.merge_task(t).unwrap();
    }
    assert_eq!(f.ticket_status(&ticket), "done");
    assert!(f.repo.join("docs/api/TW-1.md").is_file());
}

fn fe_run_session(f: &Fixture, run_id: &str) -> Option<String> {
    f.run(run_id).agent_session_id
}

#[test]
fn change_after_finalize_runs_impact_analysis_and_reworks_affected_tasks() {
    let f = fixture(2, None);
    let (ticket, web) = f.mixed("导出报表", "## 背景\n导出订单报表。\n");
    {
        let mut conn = lock(f.db());
        tickets::set_criteria(&conn, &ticket, vec!["能导出 CSV".into()]).unwrap();
        tickets::transition(&mut conn, &ticket, "ai_review").unwrap();
        tickets::transition(&mut conn, &ticket, "human_review").unwrap();
    }
    // 不让 AI 拆，手动给子任务
    let fin = f.engine.finalize(&ticket, false).unwrap();
    assert!(fin.run.is_none());
    let created = f
        .engine
        .apply_split(&ticket, f.be_fe_drafts(&web, "导出接口"))
        .unwrap();
    let (be, fe) = (created[0].id.clone(), created[1].id.clone());
    f.engine.start_development(&ticket).unwrap();
    f.wait_for("前端自动入队", || {
        !runs::list_for_task(&lock(f.db()), &fe).unwrap().is_empty()
    });
    f.wait_idle(&ticket);
    assert_eq!(f.ticket_status(&ticket), "acceptance");
    let v1 = f.ticket(&ticket).finalized_version_id.unwrap();

    // 验收阶段不能发起变更的话先打回到开发中；这里直接在验收中打回一个再发起
    f.engine.reject_task(&fe, "先等变更").unwrap();
    assert!(
        f.engine.request_change(&ticket, " ", false).is_err(),
        "变更原因必填"
    );
    let t = f
        .engine
        .request_change(&ticket, "还要支持 Excel", false)
        .unwrap();
    assert_eq!(t.status, "human_review");
    assert_eq!(t.change_from_version_id.as_deref(), Some(v1.as_str()));
    assert!(
        f.engine.start_task(&fe, StartOptions::default()).is_err(),
        "变更没定稿前不能继续开发"
    );
    tickets::save_prd(
        &mut lock(f.db()),
        &ticket,
        "## 背景\n导出订单报表，支持 CSV 和 Excel。\n",
        None,
    )
    .unwrap();
    tickets::set_criteria(
        &lock(f.db()),
        &ticket,
        vec!["能导出 CSV".into(), "能导出 Excel".into()],
    )
    .unwrap();

    // 再次定稿：已有子任务，跑的是影响分析而不是拆分
    let fin = f.engine.finalize(&ticket, false).unwrap();
    let impact_run = fin.run.unwrap();
    assert_eq!(impact_run.purpose, "impact");
    assert!(
        impact_run.prompt.contains("还要支持 Excel")
            && impact_run.prompt.contains("支持 CSV 和 Excel")
    );
    // 验收标准不在 PRD 版本里，变更前后的两份都要给到 AI
    assert!(
        impact_run
            .prompt
            .contains("## 旧版验收标准\n- 能导出 CSV\n\n")
            && impact_run
                .prompt
                .contains("## 新版验收标准\n- 能导出 CSV\n- 能导出 Excel\n"),
        "{}",
        impact_run.prompt
    );
    assert_ne!(
        fin.ticket.finalized_version_id.as_deref(),
        Some(v1.as_str())
    );
    f.wait_done(&impact_run.id);
    let proposal = reviews::latest_proposal(&lock(f.db()), &ticket, "impact")
        .unwrap()
        .unwrap();
    assert_eq!(proposal.status, "pending");
    let impacts: Vec<reviews::TaskImpact> =
        serde_json::from_value(proposal.content["tasks"].clone()).unwrap();
    assert_eq!(
        impacts
            .iter()
            .map(|i| (i.task_id.as_str(), i.impact.as_str()))
            .collect::<Vec<_>>(),
        [(be.as_str(), "rework"), (fe.as_str(), "none")]
    );
    assert!(
        f.engine.start_development(&ticket).is_err(),
        "影响分析没确认前不能开始"
    );
    // 新增的子任务有问题时整个确认失败，已有子任务不动
    let bad = reviews::TaskDraft {
        kind: "backend".into(),
        project: "不存在的项目".into(),
        title: "多出来的".into(),
        ..Default::default()
    };
    assert!(f
        .engine
        .apply_impact(&ticket, impacts.clone(), vec![bad])
        .is_err());
    assert_eq!(f.task(&be).status, "review");
    assert!(tasks::artifact(&lock(f.db()), &be, "api_doc")
        .unwrap()
        .is_some());

    f.engine.apply_impact(&ticket, impacts, vec![]).unwrap();
    assert_eq!(f.task(&be).status, "changes_requested");
    assert!(
        tasks::artifact(&lock(f.db()), &be, "api_doc")
            .unwrap()
            .is_none(),
        "需求变了，旧的接口文档作废"
    );
    let t = f.ticket(&ticket);
    assert_eq!((t.change_from_version_id, t.change_reason), (None, None));

    // 继续开发：后端在原 worktree 里增量返工，Prompt 里是变更原因和要追加的工作
    let started = f.engine.start_development(&ticket).unwrap();
    assert_eq!(
        started.len(),
        1,
        "只有后端（变更）入队；前端（之前打回的）等后端重新给出接口文档"
    );
    let be_run = &started[0];
    assert_eq!(be_run.task_id.as_deref(), Some(be.as_str()));
    assert!(be_run.prompt.contains("还要支持 Excel") && be_run.prompt.contains("更新接口文档"));
    assert_eq!(be_run.number, 2);
    assert_eq!(f.task(&fe).status, "changes_requested");
    f.wait_for("后端返工完后前端自动入队", || {
        runs::list_for_task(&lock(f.db()), &fe).unwrap().len() == 2
    });
    assert!(
        !runs::ACTIVE.contains(&f.run(&be_run.id).status.as_str())
            || tasks::artifact(&lock(f.db()), &be, "api_doc")
                .unwrap()
                .is_some(),
        "前端入队时，后端要么已经返工完，要么已经写出新文档"
    );
    f.wait_idle(&ticket);
    assert_eq!(f.ticket_status(&ticket), "acceptance");
}

#[test]
fn failed_backend_rework_does_not_republish_the_stale_api_doc() {
    let f = fixture(2, None);
    let (ticket, web) = f.mixed("导出报表", "## 背景\n导出订单报表。\n");
    {
        let mut conn = lock(f.db());
        tickets::set_criteria(&conn, &ticket, vec!["能导出 CSV".into()]).unwrap();
        tickets::transition(&mut conn, &ticket, "ai_review").unwrap();
        tickets::transition(&mut conn, &ticket, "human_review").unwrap();
    }
    f.engine.finalize(&ticket, false).unwrap();
    let created = f
        .engine
        .apply_split(
            &ticket,
            f.be_fe_drafts(&web, "x [mock:steps=1] [mock:delay=50]"),
        )
        .unwrap();
    let (be, fe) = (created[0].id.clone(), created[1].id.clone());
    f.engine.start_development(&ticket).unwrap();
    f.wait_for("前端自动入队", || {
        !runs::list_for_task(&lock(f.db()), &fe).unwrap().is_empty()
    });
    f.wait_idle(&ticket);

    // 两个都返工，后端的旧文档作废（需求变更确认后的状态），这一轮后端没改文档就失败了
    f.engine.reject_task(&fe, "跟着接口改").unwrap();
    f.engine.reject_task(&be, "接口要改 [mock:fail]").unwrap();
    tasks::remove_artifact(&lock(f.db()), &be, "api_doc").unwrap();
    let started = f.engine.start_development(&ticket).unwrap();
    assert_eq!(started.len(), 1);
    assert_eq!(f.wait_done(&started[0].id).status, "failed");
    f.wait_idle(&ticket);
    assert!(
        tasks::artifact(&lock(f.db()), &be, "api_doc")
            .unwrap()
            .is_none(),
        "worktree 里留着的是作废的那一版"
    );
    assert_eq!(runs::list_for_task(&lock(f.db()), &fe).unwrap().len(), 1);
    assert_eq!(f.task(&fe).status, "changes_requested");
}

#[test]
fn split_with_a_dependency_cycle_creates_nothing() {
    let f = fixture(2, None);
    let (ticket, web) = f.mixed("环", "## 背景\n随便。\n");
    {
        let mut conn = lock(f.db());
        tickets::set_criteria(&conn, &ticket, vec!["能用".into()]).unwrap();
        tickets::transition(&mut conn, &ticket, "ai_review").unwrap();
        tickets::transition(&mut conn, &ticket, "human_review").unwrap();
    }
    f.engine.finalize(&ticket, false).unwrap();
    let mut drafts = f.be_fe_drafts(&web, "接口");
    drafts[0].depends_on = vec!["T2".into()];
    drafts.insert(
        0,
        reviews::TaskDraft {
            r#ref: "T0".into(),
            kind: "doc".into(),
            project: f.project_id.clone(),
            title: "和环无关的文档".into(),
            ..Default::default()
        },
    );
    let err = f.engine.apply_split(&ticket, drafts).unwrap_err();
    assert!(err.to_string().contains("环"), "{err}");
    assert!(
        tasks::list_for_ticket(&lock(f.db()), &ticket)
            .unwrap()
            .is_empty(),
        "有环时一个子任务都不该建，否则改完重试会重复"
    );
}

#[test]
fn start_development_queues_reworked_dependents_after_their_prerequisite() {
    let f = fixture(2, None);
    let (ticket, first) = f.bug("先后", "x [mock:steps=1] [mock:delay=50]", "medium");
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
    f.wait_for("后续自动入队", || {
        !runs::list_for_task(&lock(f.db()), &second)
            .unwrap()
            .is_empty()
    });
    f.wait_idle(&ticket);
    // 两个都打回：后续要等前一个返工完
    f.engine.reject_task(&second, "跟着改").unwrap();
    f.engine.reject_task(&first, "还差一步").unwrap();

    let started = f.engine.start_development(&ticket).unwrap();
    assert_eq!(started.len(), 1);
    assert_eq!(started[0].task_id.as_deref(), Some(first.as_str()));
    f.wait_for("前一个返工完后，后续自动入队", || {
        runs::list_for_task(&lock(f.db()), &second).unwrap().len() == 2
    });
    assert_eq!(f.run(&started[0].id).status, "succeeded");
    f.wait_idle(&ticket);
    assert_eq!(f.task(&second).status, "review");
}

#[test]
fn deferred_task_switched_to_full_auto_waits_for_the_user() {
    let f = fixture(2, None);
    let (ticket, first) = f.bug("先后", "x [mock:steps=1] [mock:delay=300]", "medium");
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
    f.wait_for("后续自动入队", || {
        !runs::list_for_task(&lock(f.db()), &second)
            .unwrap()
            .is_empty()
    });
    f.wait_idle(&ticket);
    f.engine.reject_task(&second, "跟着改").unwrap();
    f.engine.reject_task(&first, "还差一步").unwrap();
    let started = f.engine.start_development(&ticket).unwrap();
    assert_eq!(started.len(), 1);

    // 等前一个返工的时候把后续改成 full_auto
    tasks::update(
        &lock(f.db()),
        &second,
        tasks::TaskInput {
            title: "后续".into(),
            permission_profile: Some("full_auto".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(f.run(&started[0].id).finished_at, None, "前一个还在返工");
    assert_eq!(f.wait_done(&started[0].id).status, "succeeded");
    // 依赖方的入队发生在 RunFinished 事件之前
    f.wait_for("前一个返工的 RunFinished", || {
        f.events.lock().unwrap().iter().any(
            |e| matches!(e, EngineEvent::RunFinished { run_id, .. } if *run_id == started[0].id),
        )
    });
    assert_eq!(
        runs::list_for_task(&lock(f.db()), &second).unwrap().len(),
        1,
        "full_auto 不会被自动入队"
    );
    assert_eq!(f.task(&second).status, "changes_requested");
    // 用户自己点开始（界面上会先确认）还是可以的
    let run = f
        .engine
        .start_task(&second, StartOptions::default())
        .unwrap();
    assert_eq!(f.wait_done(&run.id).status, "succeeded");
}

#[test]
fn full_auto_tasks_are_never_started_without_the_user() {
    let f = fixture(2, None);
    let (ticket, first) = f.bug("权限", "x [mock:steps=1] [mock:delay=50]", "medium");
    let make = |title: &str, depends_on: Vec<String>| {
        tasks::create(
            &mut lock(f.db()),
            tasks::TaskInput {
                ticket_id: ticket.clone(),
                kind: "bug".into(),
                project_id: f.project_id.clone(),
                title: title.into(),
                permission_profile: Some("full_auto".into()),
                depends_on,
                ..Default::default()
            },
        )
        .unwrap()
        .id
    };
    let unattended = make("不经确认执行命令", vec![]);
    let dependent = make("依赖满足后", vec![first.clone()]);
    // 先让工单进入开发中
    let run = f
        .engine
        .start_task(&first, StartOptions::default())
        .unwrap();
    f.wait_done(&run.id);
    assert_eq!(f.task(&dependent).status, "pending", "依赖满足，已解锁");

    let started = f.engine.start_development(&ticket).unwrap();
    assert!(started.is_empty(), "批量开始不带上 full_auto 的子任务");
    f.wait_idle(&ticket);
    for id in [&unattended, &dependent] {
        assert!(runs::list_for_task(&lock(f.db()), id).unwrap().is_empty());
        assert_eq!(f.task(id).status, "pending");
    }
    // 用户自己点开始（界面在这一步弹确认）仍然可以
    let run = f
        .engine
        .start_task(&unattended, StartOptions::default())
        .unwrap();
    assert_eq!(run.permission.as_deref(), Some("full_auto"));
    f.engine.stop_run(&run.id).unwrap();
    f.wait_done(&run.id);
}

#[test]
fn failed_review_keeps_the_error_and_can_be_retried() {
    let f = fixture(1, None);
    let (ticket, _) = f.mixed("会失败的预审", "正文 [mock:fail]");
    let run = f.engine.start_review(&ticket).unwrap();
    assert_eq!(f.wait_done(&run.id).status, "failed");
    f.wait_for("工单回到人工审核", || {
        f.ticket_status(&ticket) == "human_review"
    });
    let review = reviews::list(&lock(f.db()), &ticket)
        .unwrap()
        .pop()
        .unwrap();
    assert!(review.error.is_some());
    assert!(reviews::issues(&lock(f.db()), &ticket).unwrap().is_empty());
    assert!(f
        .events
        .lock()
        .unwrap()
        .iter()
        .any(|e| matches!(e, EngineEvent::ReviewFinished { error: Some(_), .. })));

    // 改掉正文后重试成功
    tickets::save_prd(&mut lock(f.db()), &ticket, "正文", None).unwrap();
    let run = f.engine.start_review(&ticket).unwrap();
    assert_eq!(f.wait_done(&run.id).status, "succeeded");
    f.wait_for("工单回到人工审核", || {
        f.ticket_status(&ticket) == "human_review"
    });
    assert_eq!(reviews::issues(&lock(f.db()), &ticket).unwrap().len(), 2);

    // 排队中的预审被停掉：工单同样回到人工审核
    let (other, _) = {
        let mut conn = lock(f.db());
        let t = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: "占住并发".into(),
                kind: "bug".into(),
                content_md: "x [mock:steps=20] [mock:delay=200]".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let task = tasks::create(
            &mut conn,
            tasks::TaskInput {
                ticket_id: t.id.clone(),
                kind: "bug".into(),
                project_id: f.project_id.clone(),
                title: "占位".into(),
                ..Default::default()
            },
        )
        .unwrap();
        (t.id, task.id)
    };
    let _ = other;
    let busy_task = tasks::list_all(&lock(f.db())).unwrap().pop().unwrap().id;
    let busy = f
        .engine
        .start_task(&busy_task, StartOptions::default())
        .unwrap();
    f.wait_run(&busy.id, "占位运行开始", |r| r.status == "running");
    let queued = f.engine.start_review(&ticket).unwrap();
    assert_eq!(f.run(&queued.id).status, "queued");
    f.engine.stop_run(&queued.id).unwrap();
    assert_eq!(f.ticket_status(&ticket), "human_review");
    f.engine.stop_run(&busy.id).unwrap();
    f.wait_done(&busy.id);
}

#[test]
fn review_interrupted_by_exit_returns_to_human_review_on_next_start() {
    let f = fixture_with(1, None, |conn| {
        let t = tickets::create(
            conn,
            tickets::TicketInput {
                title: "上次没跑完".into(),
                kind: "feature_backend".into(),
                content_md: "正文".into(),
                ..Default::default()
            },
        )
        .unwrap();
        tickets::transition(conn, &t.id, "ai_review").unwrap();
    });
    let conn = lock(f.db());
    let t = tickets::list(&conn, None).unwrap().pop().unwrap();
    assert_eq!(t.status, "human_review");
    let review = reviews::list(&conn, &t.id).unwrap().pop().unwrap();
    assert!(review.error.unwrap().contains("重新运行"));
    assert_eq!(review.run_id, None);
}

#[test]
fn switching_agent_starts_a_new_session_with_a_handoff_summary() {
    let f = fixture(1, None);
    let (_ticket, task) = f.bug("换 Agent", "x [mock:steps=1] [mock:delay=20]", "medium");
    let r1 = f.engine.start_task(&task, StartOptions::default()).unwrap();
    f.wait_done(&r1.id);
    f.engine.reject_task(&task, "还差一步").unwrap();
    // 要求新开会话：不续接，Prompt 是完整的首轮说明 + 上一轮的情况
    let plan = f
        .engine
        .plan(
            &task,
            &StartOptions {
                fresh_session: true,
                ..Default::default()
            },
        )
        .unwrap();
    assert!(plan.resume.is_none());
    let h = plan.handoff.expect("新会话要有上一轮的摘要");
    assert_eq!(h.from_run_id, r1.id);
    assert!(plan.prompt.contains("## 上一轮的情况") && plan.prompt.contains("TIDEWATCH_DEMO.md"));
    assert!(plan.prompt.contains("还差一步"));
    let r2 = f
        .engine
        .start_task(
            &task,
            StartOptions {
                fresh_session: true,
                ..Default::default()
            },
        )
        .unwrap();
    let recorded: String = lock(f.db())
        .query_row(
            "SELECT from_run_ids_json FROM context_handoff WHERE target_run_id = ?1",
            [&r2.id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(recorded.contains(&r1.id));
    f.wait_done(&r2.id);
}

#[test]
fn dependents_that_already_ran_wait_for_a_rejected_prerequisite() {
    let f = fixture(1, None);
    // 这里手动控制后续子任务什么时候开始
    {
        let conn = lock(f.db());
        let mut st = settings::get(&conn).unwrap();
        st.auto_start_unblocked = false;
        settings::save(&conn, &st).unwrap();
    }
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

#[test]
fn guards_flag_dangerous_commands_and_stop_runs_over_budget() {
    let f = fixture(2, None);
    // 默认只标出来，不停
    let (_, task) = f.bug(
        "危险命令",
        "x [mock:danger] [mock:steps=1] [mock:delay=30]",
        "medium",
    );
    let r = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "succeeded", "{:?}", r.error);
    let events = runs::events(&lock(f.db()), &r.id, 0).unwrap();
    let flagged = events
        .iter()
        .find(|e| e.kind == "tool_call" && e.payload.get("danger").is_some())
        .expect("工具调用带上危险原因");
    assert!(flagged.payload["danger"]
        .as_str()
        .unwrap()
        .contains("rm -r"));
    assert!(events
        .iter()
        .any(|e| e.kind == "guard" && e.payload["kind"] == "danger"));
    f.wait_for("危险命令提醒", || {
        f.events.lock().unwrap().iter().any(|e| matches!(e,
            EngineEvent::Guard { run_id, kind, stopped, .. } if *run_id == r.id && kind == "danger" && !stopped))
    });

    // 打开"发现即停止"
    {
        let conn = lock(f.db());
        let mut st = settings::get(&conn).unwrap();
        st.stop_on_danger = true;
        settings::save(&conn, &st).unwrap();
    }
    let (_, task) = f.bug(
        "危险命令 2",
        "x [mock:danger] [mock:steps=3] [mock:delay=200]",
        "medium",
    );
    let r = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "failed");
    assert!(
        r.error.as_deref().unwrap_or("").contains("危险命令"),
        "{:?}",
        r.error
    );
    assert_eq!(f.task(&task).status, "failed");

    // Token 上限
    {
        let conn = lock(f.db());
        let mut st = settings::get(&conn).unwrap();
        st.stop_on_danger = false;
        st.budget_tokens_per_run = 1;
        settings::save(&conn, &st).unwrap();
    }
    let (_, task) = f.bug("超预算", "x [mock:steps=2] [mock:delay=30]", "medium");
    let r = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "failed");
    assert!(
        r.error.as_deref().unwrap_or("").contains("超过上限"),
        "{:?}",
        r.error
    );
}

#[test]
fn prd_images_are_handed_to_the_agent_with_their_authority() {
    use tidewatch_core::store::attachments;
    const PNG: &[u8] = &[
        0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0, 0, 0, 0x0d, b'I', b'H', b'D', b'R', 0,
        0, 0, 2, 0, 0, 0, 3, 8, 2, 0, 0, 0, 0, 0, 0, 0,
    ];
    let f = fixture(1, None);
    let workspace = {
        let conn = lock(f.db());
        settings::get(&conn)
            .unwrap()
            .workspace(&f.repo.parent().unwrap().join("data"))
    };
    // 还没有工单时贴的图，建单时认领
    let (shot, old) = {
        let conn = lock(f.db());
        let shot = attachments::add(&conn, &workspace, None, "截图.png", PNG, "user").unwrap();
        let mut other = PNG.to_vec();
        other.push(1);
        let old = attachments::add(&conn, &workspace, None, "原图.png", &other, "prd").unwrap();
        (shot.id, old.id)
    };
    let body = format!(
        "结算页显示优惠明细 [mock:steps=1] [mock:delay=30]\n\n![需求方截图](attachment:{shot} \"authoritative\")\n\n![原 PRD 图](attachment:{old} \"reference; superseded-by={shot}\")\n"
    );
    let (_, task) = f.bug("带图的 Bug", &body, "medium");
    let r = f.engine.start_task(&task, StartOptions::default()).unwrap();
    let r = f.wait_done(&r.id);
    assert_eq!(r.status, "succeeded", "{:?}", r.error);

    // Agent 拿到的是文件路径和图片清单；开发阶段不发已被取代的图
    let prompt = f.run(&r.id).prompt;
    assert!(prompt.contains("## 图片清单"), "{prompt}");
    assert!(prompt.contains("权威"), "{prompt}");
    assert!(!prompt.contains(&format!("attachment:{shot}")), "{prompt}");
    let events = runs::events(&lock(f.db()), &r.id, 0).unwrap();
    let images = events
        .iter()
        .find(|e| e.kind == "images")
        .expect("images 事件");
    let files = images.payload["files"].as_array().unwrap();
    assert_eq!(files.len(), 1, "{files:?}");
    // 这一轮结束后副本被清掉，原图还在
    assert!(!workspace.join("tmp").join(&r.id).exists());
    let conn = lock(f.db());
    let a = attachments::get(&conn, &shot).unwrap();
    assert!(a.ticket_id.is_some());
    assert_eq!(attachments::read(&workspace, &a).unwrap(), PNG);
}

#[test]
fn failed_checks_are_sent_back_to_the_agent_until_they_pass_or_attempts_run_out() {
    // 修复 Prompt 的第一行带 "auto-fix"，演示 Agent 会把它写进文件：第一次自动修复后检查就过了
    let f = fixture(1, Some("git grep -q auto-fix -- TIDEWATCH_DEMO.md"));
    let (ticket, task) = f.bug(
        "检查先失败后通过",
        "x [mock:steps=1] [mock:delay=30]",
        "medium",
    );
    let first = f.engine.start_task(&task, StartOptions::default()).unwrap();
    f.wait_for("自动修复后进入待验收", || {
        f.task(&task).status == "review"
    });
    let all = runs::list_for_task(&lock(f.db()), &task).unwrap();
    assert_eq!(
        all.iter().map(|r| r.purpose.as_str()).collect::<Vec<_>>(),
        ["dev", "test_fix"]
    );
    assert_eq!(all[0].id, first.id);
    assert!(all[1].prompt.contains("没通过的检查"), "{}", all[1].prompt);
    assert!(
        all[1].prompt.contains("git grep -q auto-fix"),
        "{}",
        all[1].prompt
    );
    assert_eq!(
        all[1].resume_session_id, all[0].agent_session_id,
        "续接原会话"
    );
    assert!(runs::events(&lock(f.db()), &first.id, 0)
        .unwrap()
        .iter()
        .any(|e| e.kind == "auto_fix" && e.payload["attempt"] == 1));
    assert_eq!(
        runs::test_reports(&lock(f.db()), &all[1].id).unwrap()[0].exit_code,
        Some(0)
    );
    assert_eq!(f.ticket_status(&ticket), "acceptance");

    // 怎么修都不过：修满次数后交给人
    let f = fixture(1, Some("git no-such-subcommand"));
    {
        let conn = lock(f.db());
        let mut st = settings::get(&conn).unwrap();
        st.auto_fix_attempts = 1;
        settings::save(&conn, &st).unwrap();
    }
    let (ticket, task) = f.bug("检查一直失败", "x [mock:steps=1] [mock:delay=30]", "medium");
    f.engine.start_task(&task, StartOptions::default()).unwrap();
    f.wait_for("修满次数后进入待验收", || {
        f.task(&task).status == "review"
    });
    let all = runs::list_for_task(&lock(f.db()), &task).unwrap();
    assert_eq!(
        all.iter().map(|r| r.purpose.as_str()).collect::<Vec<_>>(),
        ["dev", "test_fix"]
    );
    assert_eq!(f.ticket_status(&ticket), "acceptance");
    f.wait_for("最后一轮带上没过的检查", || {
        f.events.lock().unwrap().iter().any(|e| matches!(e,
            EngineEvent::RunFinished { run_id, failed_checks, .. } if *run_id == all[1].id && !failed_checks.is_empty()))
    });
}
