//! 子任务：可派给 Agent 的最小执行单元，各自一个项目、一个分支、一个 worktree。
use super::*;
use crate::workflow;
use serde::{Deserialize, Serialize};

pub const KINDS: &[&str] = &["backend", "frontend", "bug", "doc"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub ticket_id: String,
    pub kind: String,
    pub project_id: String,
    pub title: String,
    pub spec_md: String,
    pub acceptance_criteria: Vec<String>,
    pub status: String,
    pub agent: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub permission_profile: Option<String>,
    pub branch: Option<String>,
    pub worktree_path: Option<String>,
    pub base_commit: Option<String>,
    pub merged_commit: Option<String>,
    pub depends_on: Vec<String>,
    /// `depends_on` 里只等接口文档的那些：对方产出接口文档就解锁，不用等它做完。
    #[serde(default)]
    pub api_doc_deps: Vec<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 子任务的产物。目前只有接口文档（`api_doc`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artifact {
    pub id: String,
    pub task_id: String,
    pub kind: String,
    /// 仓库内的相对路径。
    pub path: String,
    /// 含有它的提交；Agent 还在跑、没提交时为空。
    pub commit_sha: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct TaskInput {
    pub ticket_id: String,
    pub kind: String,
    pub project_id: String,
    pub title: String,
    pub spec_md: String,
    pub acceptance_criteria: Vec<String>,
    pub agent: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    pub permission_profile: Option<String>,
    pub depends_on: Vec<String>,
    pub api_doc_deps: Vec<String>,
}

const COLS: &str = "id, ticket_id, kind, project_id, title, spec_md, acceptance_criteria_json, status, agent, provider_id, model, permission_profile, branch, worktree_path, base_commit, merged_commit, created_at, updated_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Task> {
    Ok(Task {
        id: r.get(0)?,
        ticket_id: r.get(1)?,
        kind: r.get(2)?,
        project_id: r.get(3)?,
        title: r.get::<_, Option<String>>(4)?.unwrap_or_default(),
        spec_md: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
        acceptance_criteria: from_json(r.get(6)?),
        status: r.get(7)?,
        agent: r.get(8)?,
        provider_id: r.get(9)?,
        model: r.get(10)?,
        permission_profile: r.get(11)?,
        branch: r.get(12)?,
        worktree_path: r.get(13)?,
        base_commit: r.get(14)?,
        merged_commit: r.get(15)?,
        depends_on: vec![],
        api_doc_deps: vec![],
        created_at: r.get::<_, Option<String>>(16)?.unwrap_or_default(),
        updated_at: r.get::<_, Option<String>>(17)?.unwrap_or_default(),
    })
}

fn with_deps(conn: &Connection, mut t: Task) -> Result<Task> {
    let mut st = conn.prepare(
        "SELECT depends_on_task_id, artifact_kind FROM task_dependency WHERE task_id = ?1 ORDER BY 1",
    )?;
    let deps = st
        .query_map([&t.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, kind) in deps {
        if kind.as_deref() == Some("api_doc") {
            t.api_doc_deps.push(id.clone());
        }
        t.depends_on.push(id);
    }
    Ok(t)
}

fn check(conn: &Connection, input: &TaskInput) -> Result<()> {
    if !KINDS.contains(&input.kind.trim()) {
        return Err(Error::invalid(format!("未知子任务类型：{}", input.kind)));
    }
    super::projects::get(conn, &input.project_id)?;
    if let Some(a) = clean(input.agent.clone()) {
        crate::agent::AgentKind::parse(&a)?;
    }
    if let Some(p) = clean(input.permission_profile.clone()) {
        crate::agent::PermissionMode::parse(&p)?;
    }
    if let Some(p) = clean(input.provider_id.clone()) {
        super::providers::get(conn, &p)?;
    }
    Ok(())
}

/// 手动创建子任务。依赖没完成时先是"等依赖"。
pub fn create(conn: &mut Connection, input: TaskInput) -> Result<Task> {
    let ticket = super::tickets::get(conn, &input.ticket_id)?;
    if matches!(ticket.status.as_str(), "done" | "cancelled") {
        return Err(Error::invalid("工单已结束，不能再加子任务。"));
    }
    check(conn, &input)?;
    let title = required(&input.title, "子任务标题")?;
    for d in &input.depends_on {
        let dep = get(conn, d)?;
        if dep.ticket_id != ticket.id {
            return Err(Error::invalid("只能依赖同一工单下的子任务。"));
        }
    }
    let tx = conn.transaction()?;
    let id = crate::util::new_id();
    let now = crate::util::now();
    tx.execute(
        "INSERT INTO task (id, ticket_id, kind, project_id, title, spec_md, acceptance_criteria_json, status,
         agent, provider_id, model, permission_profile, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', ?8, ?9, ?10, ?11, ?12, ?12)",
        rusqlite::params![
            id,
            ticket.id,
            input.kind.trim(),
            input.project_id,
            title,
            input.spec_md,
            to_json(&input.acceptance_criteria),
            clean(input.agent),
            clean(input.provider_id),
            clean(input.model),
            clean(input.permission_profile),
            now
        ],
    )?;
    for d in &input.depends_on {
        let kind = if input.api_doc_deps.contains(d) {
            "api_doc"
        } else {
            "done"
        };
        tx.execute(
            "INSERT OR IGNORE INTO task_dependency (task_id, depends_on_task_id, artifact_kind) VALUES (?1, ?2, ?3)",
            rusqlite::params![id, d, kind],
        )?;
    }
    add_timeline(
        &tx,
        &ticket.id,
        "task_created",
        serde_json::json!({ "task_id": id, "title": title }),
    )?;
    tx.commit()?;
    refresh_blocked(conn, &ticket.id)?;
    get(conn, &id)
}

/// 改执行配置与说明；运行中不能改。
pub fn update(conn: &Connection, id: &str, input: TaskInput) -> Result<Task> {
    let t = get(conn, id)?;
    if matches!(t.status.as_str(), "queued" | "running") {
        return Err(Error::invalid("子任务在排队或运行中，停止后才能修改。"));
    }
    let mut input = input;
    input.kind = t.kind.clone();
    input.project_id = t.project_id.clone();
    check(conn, &input)?;
    conn.execute(
        "UPDATE task SET title = ?2, spec_md = ?3, acceptance_criteria_json = ?4, agent = ?5, provider_id = ?6,
         model = ?7, permission_profile = ?8, updated_at = ?9 WHERE id = ?1",
        rusqlite::params![
            id,
            required(&input.title, "子任务标题")?,
            input.spec_md,
            to_json(&input.acceptance_criteria),
            clean(input.agent),
            clean(input.provider_id),
            clean(input.model),
            clean(input.permission_profile),
            crate::util::now()
        ],
    )?;
    get(conn, id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Task> {
    let t = conn
        .query_row(&format!("SELECT {COLS} FROM task WHERE id = ?1"), [id], row)
        .map_err(|_| not_found("子任务", id))?;
    with_deps(conn, t)
}

pub fn list_for_ticket(conn: &Connection, ticket_id: &str) -> Result<Vec<Task>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM task WHERE ticket_id = ?1 ORDER BY created_at, id"
    ))?;
    let rows = st
        .query_map([ticket_id], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|t| with_deps(conn, t)).collect()
}

pub fn list_all(conn: &Connection) -> Result<Vec<Task>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM task ORDER BY created_at, id"))?;
    let rows = st
        .query_map([], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter().map(|t| with_deps(conn, t)).collect()
}

pub fn set_status(conn: &Connection, id: &str, status: &str) -> Result<()> {
    debug_assert!(workflow::TASK_STATUSES.contains(&status));
    conn.execute(
        "UPDATE task SET status = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![id, status, crate::util::now()],
    )?;
    Ok(())
}

pub fn set_workspace(
    conn: &Connection,
    id: &str,
    branch: &str,
    worktree: &str,
    base_commit: &str,
) -> Result<()> {
    conn.execute(
        "UPDATE task SET branch = ?2, worktree_path = ?3, base_commit = ?4, updated_at = ?5 WHERE id = ?1",
        rusqlite::params![id, branch, worktree, base_commit, crate::util::now()],
    )?;
    Ok(())
}

pub fn set_merged(conn: &Connection, id: &str, commit: &str) -> Result<()> {
    conn.execute(
        "UPDATE task SET status = 'merged', merged_commit = ?2, worktree_path = NULL, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![id, commit, crate::util::now()],
    )?;
    Ok(())
}

/// 还没满足的第一个依赖：依赖的子任务没到待验收 / 已通过 / 已合并；只等接口文档的，文档还没出。
/// 不只看"等依赖"状态：依赖被打回时，后面的子任务可能正处于已停止 / 失败 / 排队。
pub fn unmet_dependency(conn: &Connection, task: &Task) -> Result<Option<Task>> {
    for d in &task.depends_on {
        let dep = get(conn, d)?;
        let met = workflow::task_satisfies_dependency(&dep.status)
            || (task.api_doc_deps.contains(d) && artifact(conn, d, "api_doc")?.is_some());
        if !met {
            return Ok(Some(dep));
        }
    }
    Ok(None)
}

/// 按依赖是否满足，在"待开始"和"等依赖"之间切换该工单下的子任务。返回这次解锁的子任务 id。
pub fn refresh_blocked(conn: &Connection, ticket_id: &str) -> Result<Vec<String>> {
    let mut unblocked = Vec::new();
    for t in list_for_ticket(conn, ticket_id)? {
        if !matches!(t.status.as_str(), "pending" | "blocked") {
            continue;
        }
        let mut ready = true;
        for d in &t.depends_on {
            let s: String =
                conn.query_row("SELECT status FROM task WHERE id = ?1", [d], |r| r.get(0))?;
            ready &= workflow::task_satisfies_dependency(&s)
                || (t.api_doc_deps.contains(d) && artifact(conn, d, "api_doc")?.is_some());
        }
        let want = if ready { "pending" } else { "blocked" };
        if want != t.status {
            set_status(conn, &t.id, want)?;
            if ready {
                unblocked.push(t.id);
            }
        }
    }
    Ok(unblocked)
}

/// 记下子任务的产物；同一类已有记录时更新路径和提交。
pub fn record_artifact(
    conn: &Connection,
    task_id: &str,
    kind: &str,
    path: &str,
    commit_sha: Option<&str>,
) -> Result<Artifact> {
    match artifact(conn, task_id, kind)? {
        Some(a) => {
            conn.execute(
                "UPDATE artifact SET path = ?2, commit_sha = COALESCE(?3, commit_sha) WHERE id = ?1",
                rusqlite::params![a.id, path, commit_sha],
            )?;
        }
        None => {
            conn.execute(
                "INSERT INTO artifact (id, task_id, kind, path, commit_sha, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                rusqlite::params![crate::util::new_id(), task_id, kind, path, commit_sha, crate::util::now()],
            )?;
        }
    }
    Ok(artifact(conn, task_id, kind)?.expect("刚写入的产物"))
}

pub fn artifact(conn: &Connection, task_id: &str, kind: &str) -> Result<Option<Artifact>> {
    use rusqlite::OptionalExtension;
    Ok(conn
        .query_row(
            "SELECT id, task_id, kind, path, commit_sha, created_at FROM artifact
             WHERE task_id = ?1 AND kind = ?2 ORDER BY created_at DESC LIMIT 1",
            [task_id, kind],
            |r| {
                Ok(Artifact {
                    id: r.get(0)?,
                    task_id: r.get(1)?,
                    kind: r.get(2)?,
                    path: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                    commit_sha: r.get(4)?,
                    created_at: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
                })
            },
        )
        .optional()?)
}

pub fn artifacts_for_ticket(conn: &Connection, ticket_id: &str) -> Result<Vec<Artifact>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.task_id, a.kind, a.path, a.commit_sha, a.created_at FROM artifact a
         JOIN task t ON t.id = a.task_id WHERE t.ticket_id = ?1 ORDER BY a.created_at",
    )?;
    let rows = st
        .query_map([ticket_id], |r| {
            Ok(Artifact {
                id: r.get(0)?,
                task_id: r.get(1)?,
                kind: r.get(2)?,
                path: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
                commit_sha: r.get(4)?,
                created_at: r.get::<_, Option<String>>(5)?.unwrap_or_default(),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{projects, tickets};

    pub(crate) fn fixture() -> (Connection, tempfile::TempDir, String, String) {
        let mut conn = crate::db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        crate::git::testutil::init_repo(dir.path());
        let p = projects::create(
            &conn,
            projects::ProjectInput {
                name: "demo".into(),
                local_path: dir.path().display().to_string(),
                ..Default::default()
            },
        )
        .unwrap();
        let t = tickets::create(
            &mut conn,
            tickets::TicketInput {
                title: "修 Bug".into(),
                kind: "bug".into(),
                ..Default::default()
            },
        )
        .unwrap();
        (conn, dir, p.id, t.id)
    }

    fn input(ticket: &str, project: &str, title: &str) -> TaskInput {
        TaskInput {
            ticket_id: ticket.into(),
            kind: "bug".into(),
            project_id: project.into(),
            title: title.into(),
            ..Default::default()
        }
    }

    #[test]
    fn dependencies_block_until_satisfied() {
        let (mut conn, _dir, p, t) = fixture();
        let a = create(&mut conn, input(&t, &p, "后端")).unwrap();
        let mut i = input(&t, &p, "前端");
        i.depends_on = vec![a.id.clone()];
        let b = create(&mut conn, i).unwrap();
        assert_eq!(
            (a.status.as_str(), b.status.as_str()),
            ("pending", "blocked")
        );
        assert_eq!(b.depends_on, vec![a.id.clone()]);
        set_status(&conn, &a.id, "review").unwrap();
        refresh_blocked(&conn, &t).unwrap();
        assert_eq!(get(&conn, &b.id).unwrap().status, "pending");
    }

    #[test]
    fn validates_input() {
        let (mut conn, _dir, p, t) = fixture();
        let mut i = input(&t, &p, "x");
        i.kind = "ops".into();
        assert!(create(&mut conn, i).is_err());
        let mut i = input(&t, &p, "x");
        i.agent = Some("cursor".into());
        assert!(create(&mut conn, i).is_err());
        assert!(create(&mut conn, input(&t, "nope", "x")).is_err());
        let a = create(&mut conn, input(&t, &p, "x")).unwrap();
        set_status(&conn, &a.id, "running").unwrap();
        assert!(update(&conn, &a.id, input(&t, &p, "y")).is_err());
    }
}
