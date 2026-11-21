//! 全局设置：整份存在 `setting` 表的 `app` 键下。
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// 工作区：worktree、运行日志都放这里；为空则用数据目录下的 `workspace`。
    pub workspace_dir: Option<String>,
    /// 同时运行的 Agent 数上限。
    pub max_concurrency: u32,
    pub default_agent: String,
    pub default_provider_id: Option<String>,
    pub default_permission: String,
    /// Run 结束后自动跑项目的 lint / typecheck / test / build 命令。
    pub auto_test: bool,
    /// `merge`（--no-ff）或 `squash`。
    pub merge_mode: String,
    pub delete_branch_after_merge: bool,
    pub notifications: bool,
    /// 单次 Run 的最长时间（分钟）；0 表示不限。
    pub run_timeout_minutes: u32,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            workspace_dir: None,
            max_concurrency: 2,
            default_agent: "claude_code".into(),
            default_provider_id: None,
            default_permission: "workspace_write".into(),
            auto_test: true,
            merge_mode: "merge".into(),
            delete_branch_after_merge: true,
            notifications: true,
            run_timeout_minutes: 60,
        }
    }
}

const KEY: &str = "app";

pub fn get(conn: &Connection) -> Result<Settings> {
    let raw: Option<String> = conn
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [KEY],
            |r| r.get(0),
        )
        .ok();
    Ok(raw
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default())
}

pub fn save(conn: &Connection, s: &Settings) -> Result<Settings> {
    let mut s = s.clone();
    s.workspace_dir = clean(s.workspace_dir);
    s.default_provider_id = clean(s.default_provider_id);
    if s.max_concurrency == 0 {
        return Err(Error::invalid("并发上限至少为 1。"));
    }
    crate::agent::AgentKind::parse(&s.default_agent)?;
    if crate::agent::PermissionMode::parse(&s.default_permission)?
        == crate::agent::PermissionMode::FullAuto
    {
        return Err(Error::invalid(
            "全自动不能设为全局默认权限；需要时请在单个子任务上明确选择。",
        ));
    }
    if !matches!(s.merge_mode.as_str(), "merge" | "squash") {
        return Err(Error::invalid("合并方式只能是 merge 或 squash。"));
    }
    conn.execute(
        "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        rusqlite::params![KEY, to_json(&s)],
    )?;
    Ok(s)
}

/// 取下一个序号（工单编号等），从 1 开始。
pub fn next_seq(conn: &Connection, name: &str) -> Result<u64> {
    let key = format!("seq:{name}");
    let cur: u64 = conn
        .query_row(
            "SELECT value_json FROM setting WHERE key = ?1",
            [&key],
            |r| r.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let next = cur + 1;
    conn.execute(
        "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
        rusqlite::params![key, next.to_string()],
    )?;
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_then_roundtrip() {
        let conn = crate::db::open_in_memory().unwrap();
        assert_eq!(get(&conn).unwrap(), Settings::default());
        let mut s = Settings {
            max_concurrency: 3,
            workspace_dir: Some("  ".into()),
            ..Default::default()
        };
        let saved = save(&conn, &s).unwrap();
        assert_eq!(saved.workspace_dir, None);
        assert_eq!(get(&conn).unwrap().max_concurrency, 3);
        s.max_concurrency = 0;
        assert!(save(&conn, &s).is_err());
        s.max_concurrency = 1;
        s.default_permission = "full_auto".into();
        assert!(save(&conn, &s).is_err(), "全自动不能当全局默认");
    }

    #[test]
    fn sequences_count_up() {
        let conn = crate::db::open_in_memory().unwrap();
        assert_eq!(next_seq(&conn, "ticket").unwrap(), 1);
        assert_eq!(next_seq(&conn, "ticket").unwrap(), 2);
        assert_eq!(next_seq(&conn, "other").unwrap(), 1);
    }
}
