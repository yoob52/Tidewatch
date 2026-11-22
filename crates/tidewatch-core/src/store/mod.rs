//! 数据访问：项目、工单、子任务、运行、Provider、设置。函数只做库内读写，不碰进程和 git。
pub mod projects;
pub mod providers;
pub mod reviews;
pub mod runs;
pub mod settings;
pub mod stats;
pub mod tasks;
pub mod tickets;

use crate::{Error, Result};
use rusqlite::Connection;
use serde::de::DeserializeOwned;

pub(crate) fn from_json<T: DeserializeOwned + Default>(s: Option<String>) -> T {
    s.and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn to_json<T: serde::Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "null".into())
}

/// 空白字符串当作没填。
pub(crate) fn clean(s: Option<String>) -> Option<String> {
    s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty())
}

pub(crate) fn required(value: &str, what: &str) -> Result<String> {
    let v = value.trim();
    if v.is_empty() {
        return Err(Error::invalid(format!("{what}不能为空。")));
    }
    Ok(v.to_string())
}

pub(crate) fn not_found(what: &str, id: &str) -> Error {
    Error::NotFound(format!("找不到{what}：{id}"))
}

/// 工单时间线。
pub fn add_timeline(
    conn: &Connection,
    ticket_id: &str,
    kind: &str,
    payload: serde_json::Value,
) -> Result<()> {
    conn.execute(
        "INSERT INTO timeline_event (id, ticket_id, ts, type, payload_json) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![crate::util::new_id(), ticket_id, crate::util::now(), kind, payload.to_string()],
    )?;
    Ok(())
}
