//! 外部同步：从 GitHub Issues 拉工单、按链接导入飞书文档。
//!
//! 单向拉取为主：外部内容变了只记成"源已变更"，由用户决定要不要导入为新的 PRD 版本。
//! 网络请求都在不持有数据库锁的时候做。
pub mod feishu;
pub mod github;
pub mod http;

use std::path::Path;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::db::{lock, Db};
use crate::secrets::SecretStore;
use crate::store::{add_timeline, from_json, projects, required, tickets, to_json};
use crate::{Error, Result};

pub const KINDS: &[&str] = &["github", "feishu"];

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct SourceConfig {
    /// GitHub：`owner/name`。
    pub repo: String,
    /// GitHub：只拉带这些 label 的 Issue（全部都要有）；空 = 全部。
    pub labels: Vec<String>,
    /// GitHub：工单完成后回写一条评论并关闭 Issue。
    pub write_back: bool,
    /// 飞书：自建应用的 App ID。
    pub app_id: String,
    /// 接口地址；留空用官方地址（GitHub Enterprise / 飞书私有化部署时填）。
    pub api_base: Option<String>,
    /// 新工单默认绑定的项目。
    pub project_id: Option<String>,
    /// 新工单的类型；GitHub 上带 bug label 的 Issue 总是建成 Bug。
    pub ticket_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SyncSource {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub config: SourceConfig,
    pub enabled: bool,
    pub has_secret: bool,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub created_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct SourceInput {
    pub kind: String,
    pub name: String,
    pub config: SourceConfig,
    pub enabled: bool,
}

/// 工单的外部来源。
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct TicketSource {
    pub ticket_id: String,
    pub source_id: String,
    pub kind: String,
    pub source_name: String,
    pub external_id: String,
    pub url: String,
    pub rev: String,
    /// 外部有一版还没导入的改动。
    pub pending_rev: Option<String>,
    pub pending_title: Option<String>,
    pub pending_md: Option<String>,
    pub pending_at: Option<String>,
    pub remote_state: Option<String>,
    pub written_back_at: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct SyncReport {
    /// 新建的工单编号。
    pub created: Vec<String>,
    /// 外部内容有变化、等用户决定的工单编号。
    pub changed: Vec<String>,
    /// 新进时间线的评论数。
    pub comments: usize,
    /// 已回写并关闭的工单编号。
    pub written_back: Vec<String>,
    pub warnings: Vec<String>,
}

const COLS: &str =
    "id, kind, name, config_json, enabled, secret_ref, last_synced_at, last_error, created_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<SyncSource> {
    Ok(SyncSource {
        id: r.get(0)?,
        kind: r.get(1)?,
        name: r.get(2)?,
        config: from_json(r.get(3)?),
        enabled: r.get::<_, i64>(4)? != 0,
        has_secret: r.get::<_, Option<String>>(5)?.is_some(),
        last_synced_at: r.get(6)?,
        last_error: r.get(7)?,
        created_at: r.get::<_, Option<String>>(8)?.unwrap_or_default(),
    })
}

pub fn list(conn: &Connection) -> Result<Vec<SyncSource>> {
    let mut st = conn.prepare(&format!(
        "SELECT {COLS} FROM sync_source ORDER BY created_at, id"
    ))?;
    let v = st.query_map([], row)?.collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

pub fn get(conn: &Connection, id: &str) -> Result<SyncSource> {
    conn.query_row(
        &format!("SELECT {COLS} FROM sync_source WHERE id = ?1"),
        [id],
        row,
    )
    .optional()?
    .ok_or_else(|| Error::NotFound(format!("同步来源不存在：{id}")))
}

fn check(conn: &Connection, input: &SourceInput) -> Result<(String, SourceConfig)> {
    if !KINDS.contains(&input.kind.as_str()) {
        return Err(Error::invalid(format!(
            "未知的同步来源类型：{}",
            input.kind
        )));
    }
    let name = required(&input.name, "名称")?;
    let mut c = input.config.clone();
    c.api_base = c
        .api_base
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty());
    if let Some(base) = &c.api_base {
        if !base.starts_with("https://") && !base.starts_with("http://") {
            return Err(Error::invalid("接口地址要以 https:// 或 http:// 开头。"));
        }
    }
    c.project_id = c.project_id.filter(|s| !s.trim().is_empty());
    if let Some(p) = &c.project_id {
        projects::get(conn, p)?;
    }
    c.ticket_type = c.ticket_type.trim().to_string();
    if c.ticket_type.is_empty() {
        c.ticket_type = "feature_mixed".into();
    }
    if !tickets::TYPES.contains(&c.ticket_type.as_str()) {
        return Err(Error::invalid(format!("未知工单类型：{}", c.ticket_type)));
    }
    if input.kind == "github" {
        c.repo = github::parse_repo(&c.repo)?;
        c.labels = c
            .labels
            .iter()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect();
        c.app_id.clear();
    } else {
        c.app_id = required(&c.app_id, "App ID")?;
        c.repo.clear();
        c.labels.clear();
        c.write_back = false;
    }
    Ok((name, c))
}

pub fn create(conn: &Connection, input: SourceInput) -> Result<SyncSource> {
    let (name, config) = check(conn, &input)?;
    let id = crate::util::new_id();
    conn.execute(
        "INSERT INTO sync_source (id, kind, name, config_json, enabled, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, input.kind, name, to_json(&config), input.enabled, crate::util::now()],
    )?;
    get(conn, &id)
}

/// 类型建好之后不能改。
pub fn update(conn: &Connection, id: &str, mut input: SourceInput) -> Result<SyncSource> {
    input.kind = get(conn, id)?.kind;
    let (name, config) = check(conn, &input)?;
    conn.execute(
        "UPDATE sync_source SET name = ?2, config_json = ?3, enabled = ?4 WHERE id = ?1",
        rusqlite::params![id, name, to_json(&config), input.enabled],
    )?;
    get(conn, id)
}

/// 删除来源。已经建好的工单留着，只是不再和外部关联。
pub fn delete(conn: &Connection, secrets: &dyn SecretStore, id: &str) -> Result<()> {
    get(conn, id)?;
    secrets.delete(&secret_name(id))?;
    conn.execute("DELETE FROM ticket_source WHERE source_id = ?1", [id])?;
    conn.execute("DELETE FROM sync_source WHERE id = ?1", [id])?;
    Ok(())
}

pub fn secret_name(id: &str) -> String {
    format!("tidewatch/sync/{id}")
}

/// 保存（`Some`）或清除令牌 / App Secret：只进凭据管理器。
pub fn store_secret(
    conn: &Connection,
    secrets: &dyn SecretStore,
    id: &str,
    secret: Option<&str>,
) -> Result<SyncSource> {
    get(conn, id)?;
    let name = secret_name(id);
    let stored = match secret.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => {
            secrets.set(&name, s)?;
            Some(name)
        }
        None => {
            secrets.delete(&name)?;
            None
        }
    };
    conn.execute(
        "UPDATE sync_source SET secret_ref = ?2 WHERE id = ?1",
        rusqlite::params![id, stored],
    )?;
    get(conn, id)
}

const TS_COLS: &str = "ts.ticket_id, ts.source_id, s.kind, s.name, ts.external_id, ts.url, ts.rev, ts.pending_rev, ts.pending_title, ts.pending_md, ts.pending_at, ts.remote_state, ts.written_back_at";

fn ts_row(r: &rusqlite::Row) -> rusqlite::Result<TicketSource> {
    Ok(TicketSource {
        ticket_id: r.get(0)?,
        source_id: r.get(1)?,
        kind: r.get(2)?,
        source_name: r.get(3)?,
        external_id: r.get(4)?,
        url: r.get(5)?,
        rev: r.get(6)?,
        pending_rev: r.get(7)?,
        pending_title: r.get(8)?,
        pending_md: r.get(9)?,
        pending_at: r.get(10)?,
        remote_state: r.get(11)?,
        written_back_at: r.get(12)?,
    })
}

pub fn ticket_source(conn: &Connection, ticket_id: &str) -> Result<Option<TicketSource>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {TS_COLS} FROM ticket_source ts JOIN sync_source s ON s.id = ts.source_id WHERE ts.ticket_id = ?1"
            ),
            [ticket_id],
            ts_row,
        )
        .optional()?)
}

fn by_external(
    conn: &Connection,
    source_id: &str,
    external_id: &str,
) -> Result<Option<TicketSource>> {
    Ok(conn
        .query_row(
            &format!(
                "SELECT {TS_COLS} FROM ticket_source ts JOIN sync_source s ON s.id = ts.source_id
                 WHERE ts.source_id = ?1 AND ts.external_id = ?2"
            ),
            [source_id, external_id],
            ts_row,
        )
        .optional()?)
}

fn of_source(conn: &Connection, source_id: &str) -> Result<Vec<TicketSource>> {
    let mut st = conn.prepare(&format!(
        "SELECT {TS_COLS} FROM ticket_source ts JOIN sync_source s ON s.id = ts.source_id
         WHERE ts.source_id = ?1 ORDER BY ts.ticket_id"
    ))?;
    let v = st
        .query_map([source_id], ts_row)?
        .collect::<rusqlite::Result<_>>()?;
    Ok(v)
}

fn link(
    conn: &Connection,
    ticket_id: &str,
    source: &SyncSource,
    external_id: &str,
    url: &str,
    rev: &str,
    remote_state: Option<&str>,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ticket_source (ticket_id, source_id, external_id, url, rev, remote_state) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![ticket_id, source.id, external_id, url, rev, remote_state],
    )?;
    conn.execute(
        "UPDATE ticket SET source_url = ?2, source_rev = ?3 WHERE id = ?1",
        rusqlite::params![ticket_id, url, rev],
    )?;
    add_timeline(
        conn,
        ticket_id,
        "source_linked",
        json!({ "kind": source.kind, "url": url, "source": source.name }),
    )
}

/// 外部内容和已导入的一样就清掉待定；不一样且还没记过这一版，就记成待定。返回是否新记了一版。
fn note_remote(
    conn: &Connection,
    ts: &TicketSource,
    rev: &str,
    title: &str,
    md: impl FnOnce() -> Result<String>,
) -> Result<bool> {
    if rev == ts.rev {
        if ts.pending_rev.is_some() {
            clear_pending(conn, &ts.ticket_id, &ts.rev)?;
        }
        return Ok(false);
    }
    if ts.pending_rev.as_deref() == Some(rev) {
        return Ok(false);
    }
    conn.execute(
        "UPDATE ticket_source SET pending_rev = ?2, pending_title = ?3, pending_md = ?4, pending_at = ?5 WHERE ticket_id = ?1",
        rusqlite::params![ts.ticket_id, rev, title, md()?, crate::util::now()],
    )?;
    add_timeline(
        conn,
        &ts.ticket_id,
        "source_changed",
        json!({ "url": ts.url }),
    )?;
    Ok(true)
}

fn clear_pending(conn: &Connection, ticket_id: &str, rev: &str) -> Result<()> {
    conn.execute(
        "UPDATE ticket_source SET rev = ?2, pending_rev = NULL, pending_title = NULL, pending_md = NULL, pending_at = NULL WHERE ticket_id = ?1",
        rusqlite::params![ticket_id, rev],
    )?;
    conn.execute(
        "UPDATE ticket SET source_rev = ?2 WHERE id = ?1",
        rusqlite::params![ticket_id, rev],
    )?;
    Ok(())
}

/// 把外部的新版本导入为新的 PRD 版本。
///
/// 正文已锁定（已定稿 / 开发中）的工单要先发起变更：调用方先调 `Engine::request_change`，
/// 这里只在正文可编辑时写入。
pub fn apply_pending(conn: &mut Connection, ticket_id: &str) -> Result<tickets::Ticket> {
    let t = tickets::get(conn, ticket_id)?;
    let ts = ticket_source(conn, &t.id)?.ok_or_else(|| Error::invalid("这张工单没有外部来源。"))?;
    let (Some(rev), Some(md)) = (ts.pending_rev.clone(), ts.pending_md.clone()) else {
        return Err(Error::invalid("外部来源没有新的变更。"));
    };
    let what = if ts.kind == "github" {
        "GitHub Issue"
    } else {
        "飞书文档"
    };
    let mut version = None;
    if tickets::current_prd(conn, &t.id)?.is_none_or(|v| v.content_md != md) {
        let note = format!("从{what}同步");
        version = Some(tickets::save_prd_as(conn, &t.id, &md, "import", Some(&note))?.version);
    }
    if let Some(title) = ts.pending_title.as_deref().map(str::trim) {
        if !title.is_empty() && title != t.title {
            tickets::rename(conn, &t.id, title)?;
        }
    }
    clear_pending(conn, &t.id, &rev)?;
    add_timeline(
        conn,
        &t.id,
        "source_applied",
        json!({ "version": version, "url": ts.url }),
    )?;
    tickets::get(conn, &t.id)
}

/// 需不需要先发起变更才能导入（正文已锁定，但还能走变更流程）。
pub fn needs_change_request(t: &tickets::Ticket) -> Result<bool> {
    let editable = matches!(t.status.as_str(), "draft" | "human_review")
        || (t.kind == "bug" && matches!(t.status.as_str(), "developing" | "acceptance"));
    if editable {
        return Ok(false);
    }
    if matches!(t.status.as_str(), "finalized" | "developing") {
        return Ok(true);
    }
    Err(Error::invalid(format!(
        "工单处于「{}」，现在不能导入外部的新版本。可以忽略这次变更，或者等这个阶段结束。",
        crate::workflow::ticket_status_label(&t.status)
    )))
}

/// 不导入这一版：之后外部再改才会重新提示。
pub fn dismiss_pending(conn: &Connection, ticket_id: &str) -> Result<()> {
    let t = tickets::get(conn, ticket_id)?;
    let ts = ticket_source(conn, &t.id)?.ok_or_else(|| Error::invalid("这张工单没有外部来源。"))?;
    let Some(rev) = ts.pending_rev else {
        return Err(Error::invalid("外部来源没有新的变更。"));
    };
    clear_pending(conn, &t.id, &rev)?;
    add_timeline(conn, &t.id, "source_dismissed", json!({ "url": ts.url }))
}

fn secret_of(secrets: &dyn SecretStore, source: &SyncSource) -> Result<Option<String>> {
    if !source.has_secret {
        return Ok(None);
    }
    secrets.get(&secret_name(&source.id))
}

fn ticket_projects(conn: &Connection, cfg: &SourceConfig) -> Vec<tickets::TicketProject> {
    cfg.project_id
        .as_deref()
        .and_then(|id| projects::get(conn, id).ok())
        .map(|p| {
            vec![tickets::TicketProject {
                project_id: p.id,
                role: if p.kind == "frontend" {
                    "frontend"
                } else {
                    "backend"
                }
                .into(),
                subdir: None,
            }]
        })
        .unwrap_or_default()
}

/// 同步一个来源。失败原因记在来源上（`last_error`），也作为错误返回。
pub fn run(
    db: &Db,
    secrets: &dyn SecretStore,
    workspace: &Path,
    source_id: &str,
) -> Result<SyncReport> {
    let source = get(&lock(db), source_id)?;
    if !source.enabled {
        return Err(Error::invalid(format!(
            "同步来源「{}」已停用。",
            source.name
        )));
    }
    let started = crate::util::now();
    let result = match source.kind.as_str() {
        "github" => run_github(db, secrets, &source),
        _ => run_feishu(db, secrets, workspace, &source),
    };
    let conn = lock(db);
    match &result {
        Ok(_) => conn.execute(
            "UPDATE sync_source SET last_synced_at = ?2, last_error = NULL WHERE id = ?1",
            rusqlite::params![source.id, started],
        )?,
        Err(e) => conn.execute(
            "UPDATE sync_source SET last_error = ?2 WHERE id = ?1",
            rusqlite::params![source.id, e.to_string()],
        )?,
    };
    result
}

/// 上次同步之前 10 分钟：两边时钟有偏差也不漏。
fn since(last: Option<&str>) -> Option<String> {
    let t = chrono::DateTime::parse_from_rfc3339(last?).ok()?;
    Some(
        (t - chrono::Duration::minutes(10))
            .with_timezone(&chrono::Utc)
            .to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
    )
}

fn run_github(db: &Db, secrets: &dyn SecretStore, source: &SyncSource) -> Result<SyncReport> {
    let cfg = &source.config;
    let token = secret_of(secrets, source)?.or_else(github::token_from_gh);
    let client = github::Client {
        api: cfg.api_base.as_deref().unwrap_or(github::DEFAULT_API),
        repo: &cfg.repo,
        token: token.as_deref(),
    };
    let issues = client.issues(
        &cfg.labels,
        since(source.last_synced_at.as_deref()).as_deref(),
    )?;
    let mut report = SyncReport::default();
    let mut to_close: Vec<(String, String, u64)> = vec![];
    {
        let mut conn = lock(db);
        for issue in &issues {
            let number = issue.number.to_string();
            let ticket_id = match by_external(&conn, &source.id, &number)? {
                Some(ts) => {
                    conn.execute(
                        "UPDATE ticket_source SET remote_state = ?2 WHERE ticket_id = ?1",
                        rusqlite::params![ts.ticket_id, issue.state],
                    )?;
                    if note_remote(&conn, &ts, &issue.rev(), &issue.title, || {
                        Ok(body_of(issue))
                    })? {
                        report.changed.push(tickets::get(&conn, &ts.ticket_id)?.key);
                    }
                    ts.ticket_id
                }
                None if issue.state == "open" => {
                    let is_bug = issue.labels.iter().any(|l| l.eq_ignore_ascii_case("bug"));
                    let bound = ticket_projects(&conn, cfg);
                    let t = tickets::create(
                        &mut conn,
                        tickets::TicketInput {
                            title: issue.title.clone(),
                            kind: if is_bug {
                                "bug".into()
                            } else {
                                cfg.ticket_type.clone()
                            },
                            labels: issue.labels.clone(),
                            content_md: body_of(issue),
                            projects: bound,
                            source_kind: Some("github".into()),
                            ..Default::default()
                        },
                    )?;
                    link(
                        &conn,
                        &t.id,
                        source,
                        &number,
                        &issue.url,
                        &issue.rev(),
                        Some(&issue.state),
                    )?;
                    report.created.push(t.key);
                    t.id
                }
                // 已经关掉、观澜里也没有的 Issue 不建单
                None => continue,
            };
            for c in &issue.comments {
                let seen: bool = conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM timeline_event WHERE ticket_id = ?1 AND type = 'source_comment'
                       AND json_extract(payload_json, '$.comment_id') = ?2)",
                    rusqlite::params![ticket_id, c.id as i64],
                    |r| r.get(0),
                )?;
                if !seen {
                    let body: String = c.body.chars().take(4000).collect();
                    add_timeline(
                        &conn,
                        &ticket_id,
                        "source_comment",
                        json!({ "comment_id": c.id, "author": c.author, "body": body, "url": c.url }),
                    )?;
                    report.comments += 1;
                }
            }
        }
        if cfg.write_back {
            let mut st = conn.prepare(
                "SELECT t.id, t.key, ts.external_id FROM ticket_source ts JOIN ticket t ON t.id = ts.ticket_id
                 WHERE ts.source_id = ?1 AND t.status = 'done' AND ts.written_back_at IS NULL
                   AND COALESCE(ts.remote_state, 'open') = 'open'",
            )?;
            to_close = st
                .query_map([&source.id], |r| {
                    Ok((
                        r.get(0)?,
                        r.get(1)?,
                        r.get::<_, String>(2)?.parse().unwrap_or(0),
                    ))
                })?
                .collect::<rusqlite::Result<_>>()?;
        }
    }
    for (ticket_id, key, number) in to_close {
        let comment = format!("这个 Issue 已在观澜（Tidewatch）里完成并验收（工单 {key}）。");
        match client.comment_and_close(number, &comment) {
            Ok(()) => {
                let conn = lock(db);
                conn.execute(
                    "UPDATE ticket_source SET written_back_at = ?2, remote_state = 'closed' WHERE ticket_id = ?1",
                    rusqlite::params![ticket_id, crate::util::now()],
                )?;
                add_timeline(
                    &conn,
                    &ticket_id,
                    "source_written_back",
                    json!({ "number": number }),
                )?;
                report.written_back.push(key);
            }
            Err(e) => report.warnings.push(format!("{key} 没有回写：{e}")),
        }
    }
    Ok(report)
}

fn body_of(issue: &github::Issue) -> String {
    if issue.body.trim().is_empty() {
        format!("（GitHub Issue #{} 没有正文）\n", issue.number)
    } else {
        issue.body.clone()
    }
}

fn feishu_client(secrets: &dyn SecretStore, source: &SyncSource) -> Result<feishu::Client> {
    let secret = secret_of(secrets, source)?.ok_or_else(|| {
        Error::invalid(format!("同步来源「{}」还没有填 App Secret。", source.name))
    })?;
    feishu::Client::login(
        source
            .config
            .api_base
            .as_deref()
            .unwrap_or(feishu::DEFAULT_API),
        &source.config.app_id,
        &secret,
    )
}

/// 飞书没有"列表"：只检查已经导入的文档有没有新版本。
fn run_feishu(
    db: &Db,
    secrets: &dyn SecretStore,
    workspace: &Path,
    source: &SyncSource,
) -> Result<SyncReport> {
    let linked = of_source(&lock(db), &source.id)?;
    let mut report = SyncReport::default();
    if linked.is_empty() {
        return Ok(report);
    }
    let client = feishu_client(secrets, source)?;
    for ts in linked {
        let doc_ref = feishu::DocRef::Docx(ts.external_id.clone());
        let key = tickets::get(&lock(db), &ts.ticket_id)?.key;
        let meta = match client.meta(&doc_ref) {
            Ok(m) => m,
            Err(e) => {
                report.warnings.push(format!("{key}：{e}"));
                continue;
            }
        };
        if meta.revision == ts.rev || ts.pending_rev.as_deref() == Some(&meta.revision) {
            if meta.revision == ts.rev && ts.pending_rev.is_some() {
                clear_pending(&lock(db), &ts.ticket_id, &ts.rev)?;
            }
            continue;
        }
        let doc = match client.fetch(&doc_ref) {
            Ok(d) => d,
            Err(e) => {
                report.warnings.push(format!("{key}：{e}"));
                continue;
            }
        };
        let conn = lock(db);
        let stored = crate::import::store(&conn, workspace, &doc.title, doc.parsed)?;
        report
            .warnings
            .extend(stored.warnings.iter().map(|w| format!("{key}：{w}")));
        if note_remote(&conn, &ts, &doc.revision, &doc.title, || {
            Ok(stored.markdown.clone())
        })? {
            report.changed.push(key);
        }
    }
    Ok(report)
}

/// 按链接把一篇飞书文档导入成草稿工单。返回工单和没能原样导入的内容说明。
pub fn import_feishu(
    db: &Db,
    secrets: &dyn SecretStore,
    workspace: &Path,
    source_id: &str,
    url: &str,
) -> Result<(tickets::Ticket, Vec<String>)> {
    let source = get(&lock(db), source_id)?;
    if source.kind != "feishu" {
        return Err(Error::invalid("这个同步来源不是飞书。"));
    }
    let doc_ref = feishu::parse_url(url)?;
    let client = feishu_client(secrets, &source)?;
    let doc = client.fetch(&doc_ref)?;
    let mut conn = lock(db);
    if let Some(ts) = by_external(&conn, &source.id, &doc.document_id)? {
        return Err(Error::invalid(format!(
            "这篇文档已经导入过了：{}。",
            tickets::get(&conn, &ts.ticket_id)?.key
        )));
    }
    let title = if doc.title.is_empty() {
        "飞书文档".to_string()
    } else {
        doc.title.clone()
    };
    let stored = crate::import::store(&conn, workspace, &title, doc.parsed)?;
    let bound = ticket_projects(&conn, &source.config);
    let t = tickets::create(
        &mut conn,
        tickets::TicketInput {
            title,
            kind: source.config.ticket_type.clone(),
            content_md: stored.markdown,
            projects: bound,
            source_kind: Some("feishu".into()),
            ..Default::default()
        },
    )?;
    let url = url
        .trim()
        .split(['?', '#'])
        .next()
        .unwrap_or("")
        .to_string();
    link(
        &conn,
        &t.id,
        &source,
        &doc.document_id,
        &url,
        &doc.revision,
        None,
    )?;
    Ok((t, stored.warnings))
}
