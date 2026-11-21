//! 项目：本地目录、上下文卡片、命令、默认 Agent / Provider。
use super::*;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Commands {
    pub setup: Option<String>,
    pub lint: Option<String>,
    pub typecheck: Option<String>,
    pub test: Option<String>,
    pub build: Option<String>,
    pub dev: Option<String>,
}

impl Commands {
    fn cleaned(self) -> Self {
        Commands {
            setup: clean(self.setup),
            lint: clean(self.lint),
            typecheck: clean(self.typecheck),
            test: clean(self.test),
            build: clean(self.build),
            dev: clean(self.dev),
        }
    }
    /// Run 结束后依次执行的检查命令：(名称, 命令)。
    pub fn checks(&self) -> Vec<(&'static str, String)> {
        [
            ("lint", &self.lint),
            ("typecheck", &self.typecheck),
            ("test", &self.test),
            ("build", &self.build),
        ]
        .into_iter()
        .filter_map(|(n, c)| c.clone().map(|c| (n, c)))
        .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub local_path: String,
    pub git_url: Option<String>,
    pub default_branch: String,
    pub context_md: String,
    pub commands: Commands,
    pub default_agent: Option<String>,
    pub default_provider_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProjectInput {
    pub name: String,
    pub kind: String,
    pub local_path: String,
    pub git_url: Option<String>,
    /// 为空则取仓库当前分支。
    pub default_branch: Option<String>,
    pub context_md: String,
    pub commands: Commands,
    pub default_agent: Option<String>,
    pub default_provider_id: Option<String>,
}

const KINDS: &[&str] = &["backend", "frontend", "fullstack", "other"];
const COLS: &str = "id, name, kind, local_path, git_url, default_branch, context_md, commands_json, default_agent, default_provider_id, created_at, updated_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: r.get(0)?,
        name: r.get(1)?,
        kind: r.get(2)?,
        local_path: r.get(3)?,
        git_url: r.get(4)?,
        default_branch: r.get::<_, Option<String>>(5)?.unwrap_or_else(|| "main".into()),
        context_md: r.get::<_, Option<String>>(6)?.unwrap_or_default(),
        commands: from_json(r.get(7)?),
        default_agent: r.get(8)?,
        default_provider_id: r.get(9)?,
        created_at: r.get::<_, Option<String>>(10)?.unwrap_or_default(),
        updated_at: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
    })
}

struct Checked {
    name: String,
    kind: String,
    local_path: String,
    default_branch: String,
    default_agent: Option<String>,
}

fn check(input: &ProjectInput) -> Result<Checked> {
    let name = required(&input.name, "项目名称")?;
    let kind = if input.kind.trim().is_empty() {
        "other".to_string()
    } else {
        input.kind.trim().to_string()
    };
    if !KINDS.contains(&kind.as_str()) {
        return Err(Error::invalid(format!("未知项目类型：{kind}")));
    }
    let raw = required(&input.local_path, "本地目录")?;
    let path = Path::new(&raw);
    if !path.is_dir() {
        return Err(Error::invalid(format!("目录不存在：{raw}")));
    }
    if !crate::git::is_repo(path) {
        return Err(Error::invalid(format!(
            "{raw} 不是 git 仓库。观澜用 worktree 隔离每个子任务，请先在该目录执行 git init 并提交一次。"
        )));
    }
    let local_path = crate::util::canonical(path)?.display().to_string();
    let default_branch = match clean(input.default_branch.clone()) {
        Some(b) => {
            if !crate::git::branch_exists(path, &b) {
                return Err(Error::invalid(format!("仓库里没有分支：{b}")));
            }
            b
        }
        None => {
            let b = crate::git::current_branch(path)?;
            if b == "HEAD" {
                return Err(Error::invalid(
                    "仓库当前不在任何分支上（detached HEAD），请填写目标分支。",
                ));
            }
            b
        }
    };
    let default_agent = clean(input.default_agent.clone());
    if let Some(a) = &default_agent {
        crate::agent::AgentKind::parse(a)?;
    }
    Ok(Checked {
        name,
        kind,
        local_path,
        default_branch,
        default_agent,
    })
}

pub fn create(conn: &Connection, input: ProjectInput) -> Result<Project> {
    let c = check(&input)?;
    let id = crate::util::new_id();
    let now = crate::util::now();
    conn.execute(
        &format!("INSERT INTO project ({COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11)"),
        rusqlite::params![
            id,
            c.name,
            c.kind,
            c.local_path,
            clean(input.git_url),
            c.default_branch,
            input.context_md,
            to_json(&input.commands.cleaned()),
            c.default_agent,
            clean(input.default_provider_id),
            now
        ],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: ProjectInput) -> Result<Project> {
    get(conn, id)?;
    let c = check(&input)?;
    conn.execute(
        "UPDATE project SET name = ?2, kind = ?3, local_path = ?4, git_url = ?5, default_branch = ?6,
         context_md = ?7, commands_json = ?8, default_agent = ?9, default_provider_id = ?10, updated_at = ?11
         WHERE id = ?1",
        rusqlite::params![
            id,
            c.name,
            c.kind,
            c.local_path,
            clean(input.git_url),
            c.default_branch,
            input.context_md,
            to_json(&input.commands.cleaned()),
            c.default_agent,
            clean(input.default_provider_id),
            crate::util::now()
        ],
    )?;
    get(conn, id)
}

pub fn get(conn: &Connection, id: &str) -> Result<Project> {
    conn.query_row(&format!("SELECT {COLS} FROM project WHERE id = ?1"), [id], row)
        .map_err(|_| not_found("项目", id))
}

pub fn list(conn: &Connection) -> Result<Vec<Project>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM project ORDER BY created_at"))?;
    let rows = st.query_map([], row)?.collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

/// 删除项目；还有子任务用着它时拒绝。
pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let used: i64 = conn.query_row(
        "SELECT COUNT(*) FROM task WHERE project_id = ?1 AND status NOT IN ('merged', 'cancelled')",
        [id],
        |r| r.get(0),
    )?;
    if used > 0 {
        return Err(Error::invalid(format!(
            "还有 {used} 个未结束的子任务在用这个项目，不能删除。"
        )));
    }
    conn.execute("DELETE FROM ticket_project WHERE project_id = ?1", [id])?;
    conn.execute("DELETE FROM project WHERE id = ?1", [id])?;
    Ok(())
}

/// 仓库根目录下的 AGENTS.md / CLAUDE.md，供"导入为上下文卡片"。
pub fn read_agent_docs(local_path: &str) -> Vec<(String, String)> {
    ["AGENTS.md", "CLAUDE.md"]
        .into_iter()
        .filter_map(|n| {
            std::fs::read_to_string(Path::new(local_path).join(n))
                .ok()
                .map(|c| (n.to_string(), c))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        crate::git::testutil::init_repo(dir.path());
        dir
    }

    fn input(dir: &Path) -> ProjectInput {
        ProjectInput {
            name: "商城后端".into(),
            kind: "backend".into(),
            local_path: dir.display().to_string(),
            commands: Commands {
                test: Some(" cargo test ".into()),
                lint: Some("  ".into()),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn create_update_list_delete() {
        let conn = crate::db::open_in_memory().unwrap();
        let dir = repo();
        let p = create(&conn, input(dir.path())).unwrap();
        assert_eq!(p.default_branch, crate::git::current_branch(dir.path()).unwrap());
        assert_eq!(p.commands.test.as_deref(), Some("cargo test"));
        assert_eq!(p.commands.lint, None);
        assert_eq!(p.commands.checks(), vec![("test", "cargo test".to_string())]);

        let mut i = input(dir.path());
        i.name = "商城 API".into();
        i.context_md = "Rust + axum".into();
        let p2 = update(&conn, &p.id, i).unwrap();
        assert_eq!(p2.name, "商城 API");
        assert_eq!(list(&conn).unwrap().len(), 1);
        delete(&conn, &p.id).unwrap();
        assert!(get(&conn, &p.id).is_err());
    }

    #[test]
    fn rejects_non_repo_and_unknown_branch() {
        let conn = crate::db::open_in_memory().unwrap();
        let plain = tempfile::tempdir().unwrap();
        let e = create(&conn, input(plain.path())).unwrap_err();
        assert!(e.to_string().contains("git init"));
        let dir = repo();
        let mut i = input(dir.path());
        i.default_branch = Some("no-such-branch".into());
        assert!(create(&conn, i).is_err());
    }
}
