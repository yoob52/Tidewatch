//! SQLite：打开、迁移。迁移为版本化 SQL 脚本，版本记录在 `PRAGMA user_version`。
use crate::Result;
pub use rusqlite::Connection;
use std::path::Path;

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_m1.sql"),
    include_str!("../migrations/0003_m2.sql"),
    include_str!("../migrations/0004_m3.sql"),
    include_str!("../migrations/0005_m3_review.sql"),
    include_str!("../migrations/0006_m4.sql"),
];

/// 多线程共用的连接：调度线程、运行线程和界面命令都经它读写。
pub type Db = std::sync::Arc<std::sync::Mutex<Connection>>;

pub fn shared(conn: Connection) -> Db {
    std::sync::Arc::new(std::sync::Mutex::new(conn))
}

/// 取锁；持锁线程 panic 过也继续用（SQLite 事务自身保证一致性）。
pub fn lock(db: &Db) -> std::sync::MutexGuard<'_, Connection> {
    db.lock().unwrap_or_else(|e| e.into_inner())
}

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let conn = Connection::open(path)?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    init(conn)
}

pub fn open_in_memory() -> Result<Connection> {
    init(Connection::open_in_memory()?)
}

fn init(mut conn: Connection) -> Result<Connection> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    migrate(&mut conn)?;
    Ok(conn)
}

pub fn schema_version(conn: &Connection) -> Result<u32> {
    Ok(conn.query_row("PRAGMA user_version", [], |r| r.get(0))?)
}

pub fn sqlite_version(conn: &Connection) -> Result<String> {
    Ok(conn.query_row("SELECT sqlite_version()", [], |r| r.get(0))?)
}

fn migrate(conn: &mut Connection) -> Result<()> {
    let current = schema_version(conn)? as usize;
    for (i, sql) in MIGRATIONS.iter().enumerate().skip(current) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (i + 1) as u32)?;
        tx.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_and_has_fts5() {
        let conn = open_in_memory().unwrap();
        assert_eq!(schema_version(&conn).unwrap() as usize, MIGRATIONS.len());
        conn.execute(
            "INSERT INTO ticket_fts (title, content, ticket_id) VALUES ('优惠券 叠加', '结算页 展示', 't1')",
            [],
        )
        .unwrap();
        let id: String = conn
            .query_row(
                "SELECT ticket_id FROM ticket_fts WHERE ticket_fts MATCH '结算页'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(id, "t1");
    }

    #[test]
    fn reopening_is_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a/tidewatch.db");
        drop(open(&p).unwrap());
        let conn = open(&p).unwrap();
        assert_eq!(schema_version(&conn).unwrap() as usize, MIGRATIONS.len());
    }
}
