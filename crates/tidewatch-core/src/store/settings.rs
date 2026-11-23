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
    /// 可以是 full_auto：每次开始都要用户确认，AI 预审 / 拆分 / 影响分析也跟着以 full_auto 运行。
    pub default_permission: String,
    /// Windows 上 Agent 跑命令用的 Shell：auto / pwsh / powershell / git_bash，见 `crate::shell`。
    pub agent_shell: String,
    /// Run 结束后自动跑项目的 lint / typecheck / test / build 命令。
    pub auto_test: bool,
    /// 自动检查没过时，最多让 Agent 自己修几次；0 表示不自动修。
    pub auto_fix_attempts: u32,
    /// 子任务进入待验收时，自动让另一个 Agent 审查 Diff。
    pub auto_cross_review: bool,
    /// 启动时联网看一眼有没有新版本（只读 GitHub 上的版本号）。默认关：观澜默认不联网。
    pub check_updates: bool,
    /// `merge`（--no-ff）或 `squash`。
    pub merge_mode: String,
    pub delete_branch_after_merge: bool,
    pub notifications: bool,
    /// 点窗口右上角的关闭按钮时缩到系统托盘、后台继续跑；关掉则直接退出。
    pub close_to_tray: bool,
    /// 单次 Run 的最长时间（分钟）；0 表示不限。
    pub run_timeout_minutes: u32,
    /// 确认拆分后自动开始开发。
    pub auto_develop: bool,
    /// 依赖满足（对方完成，或接口文档已产出）的子任务自动入队。
    pub auto_start_unblocked: bool,
    /// 单次 Run 的花费上限（美元）；0 表示不限。超过即停止。
    pub budget_usd_per_run: f64,
    /// 单次 Run 的 Token 上限（输入 + 输出）；0 表示不限。
    pub budget_tokens_per_run: u64,
    /// Agent 连续这么多分钟没有任何输出算卡住；0 表示不检测。
    pub stuck_minutes: u32,
    /// 卡住时：`notify` 只提醒，`stop` 直接停止这一轮。
    pub stuck_action: String,
    /// 发现危险命令时停止这一轮（只能在 Agent 报出命令后停，拦不住已经开始执行的那一条）。
    pub stop_on_danger: bool,
    /// 开发阶段也把已被取代的参考图发给 Agent（默认不发，减少误导）。
    pub send_superseded_images: bool,
    /// 开启本地 HTTP / MCP 接口。默认关。
    pub api_enabled: bool,
    pub api_port: u16,
    /// 接口监听所有网卡（局域网里的其他人可以用令牌访问）；否则只监听 127.0.0.1。
    pub api_remote: bool,
}

impl Settings {
    /// 工作区根目录：worktree、附件、临时文件都在这下面。
    pub fn workspace(&self, data_dir: &std::path::Path) -> std::path::PathBuf {
        match self
            .workspace_dir
            .as_deref()
            .filter(|d| !d.trim().is_empty())
        {
            Some(d) => std::path::PathBuf::from(d),
            None => data_dir.join("workspace"),
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            workspace_dir: None,
            max_concurrency: 2,
            default_agent: "claude_code".into(),
            default_provider_id: None,
            default_permission: "workspace_write".into(),
            agent_shell: "auto".into(),
            auto_test: true,
            auto_fix_attempts: 2,
            auto_cross_review: false,
            check_updates: false,
            merge_mode: "merge".into(),
            delete_branch_after_merge: true,
            notifications: true,
            close_to_tray: true,
            run_timeout_minutes: 60,
            auto_develop: false,
            auto_start_unblocked: true,
            budget_usd_per_run: 0.0,
            budget_tokens_per_run: 0,
            stuck_minutes: 10,
            stuck_action: "notify".into(),
            stop_on_danger: false,
            send_superseded_images: false,
            api_enabled: false,
            api_port: 47600,
            api_remote: false,
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
    crate::agent::PermissionMode::parse(&s.default_permission)?;
    crate::shell::validate(&s.agent_shell)?;
    if !matches!(s.merge_mode.as_str(), "merge" | "squash") {
        return Err(Error::invalid("合并方式只能是 merge 或 squash。"));
    }
    if !matches!(s.stuck_action.as_str(), "notify" | "stop") {
        return Err(Error::invalid("卡住时的处理只能是 notify 或 stop。"));
    }
    if !(s.budget_usd_per_run >= 0.0 && s.budget_usd_per_run.is_finite()) {
        return Err(Error::invalid("花费上限不能是负数。"));
    }
    if s.api_port < 1024 {
        return Err(Error::invalid("接口端口要在 1024 到 65535 之间。"));
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
        assert_eq!(save(&conn, &s).unwrap().default_permission, "full_auto");
        s.default_permission = "yolo".into();
        assert!(save(&conn, &s).is_err());
        s.default_permission = "full_auto".into();
        s.agent_shell = "cmd".into();
        assert!(save(&conn, &s).is_err(), "Agent 不能用 cmd 当 Shell");
    }

    #[test]
    fn sequences_count_up() {
        let conn = crate::db::open_in_memory().unwrap();
        assert_eq!(next_seq(&conn, "ticket").unwrap(), 1);
        assert_eq!(next_seq(&conn, "ticket").unwrap(), 2);
        assert_eq!(next_seq(&conn, "other").unwrap(), 1);
    }
}
