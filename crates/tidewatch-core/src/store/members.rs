//! 成员与接口令牌。令牌只在创建 / 重置时返回一次，库里只存 SHA-256。
use super::*;
use rusqlite::OptionalExtension;
use serde::{Deserialize, Serialize};

pub const TOKEN_PREFIX: &str = "tw_";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// owner | member | viewer
    pub role: String,
    pub token_hint: Option<String>,
    pub disabled: bool,
    pub created_at: String,
    pub last_seen_at: Option<String>,
}

impl Member {
    /// 能改数据（建单、评论、开始 / 停止、回答问题、验收）。
    pub fn can_write(&self) -> bool {
        matches!(self.role.as_str(), "owner" | "member")
    }
    /// 能动本机仓库和设置（合并、推送、成员管理）。
    pub fn is_owner(&self) -> bool {
        self.role == "owner"
    }
}

const COLS: &str = "id, name, role, token_hint, disabled, created_at, last_seen_at";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Member> {
    Ok(Member {
        id: r.get(0)?,
        name: r.get(1)?,
        role: r.get(2)?,
        token_hint: r.get(3)?,
        disabled: r.get::<_, i64>(4)? != 0,
        created_at: r.get(5)?,
        last_seen_at: r.get(6)?,
    })
}

fn check_role(role: &str) -> Result<()> {
    if !matches!(role, "owner" | "member" | "viewer") {
        return Err(Error::invalid("角色只能是 owner、member 或 viewer。"));
    }
    Ok(())
}

fn fresh_token() -> (String, String, String) {
    let token = crate::util::new_token(TOKEN_PREFIX);
    let hash = crate::util::sha256_hex(token.as_bytes());
    let hint = token[token.len() - 4..].to_string();
    (token, hash, hint)
}

/// 新建成员，返回（成员，令牌明文）。
pub fn create(conn: &Connection, name: &str, role: &str) -> Result<(Member, String)> {
    let name = required(name, "成员名")?;
    check_role(role)?;
    let (token, hash, hint) = fresh_token();
    let id = crate::util::new_id();
    conn.execute(
        "INSERT INTO member (id, name, role, token_hash, token_hint, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, name, role, hash, hint, crate::util::now()],
    )?;
    Ok((get(conn, &id)?, token))
}

pub fn get(conn: &Connection, id: &str) -> Result<Member> {
    conn.query_row(
        &format!("SELECT {COLS} FROM member WHERE id = ?1"),
        [id],
        row,
    )
    .optional()?
    .ok_or_else(|| not_found("成员", id))
}

pub fn list(conn: &Connection) -> Result<Vec<Member>> {
    let mut stmt = conn.prepare(&format!("SELECT {COLS} FROM member ORDER BY id"))?;
    let rows = stmt.query_map([], row)?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}

/// 至少要留一个可用的 owner，否则没人能再管理成员。
fn check_owner_remains(conn: &Connection, id: &str) -> Result<()> {
    let others: i64 = conn.query_row(
        "SELECT COUNT(*) FROM member WHERE role = 'owner' AND disabled = 0 AND id != ?1",
        [id],
        |r| r.get(0),
    )?;
    if others == 0 && get(conn, id)?.is_owner() {
        return Err(Error::invalid("至少要保留一个可用的 owner。"));
    }
    Ok(())
}

pub fn update(
    conn: &Connection,
    id: &str,
    name: &str,
    role: &str,
    disabled: bool,
) -> Result<Member> {
    let name = required(name, "成员名")?;
    check_role(role)?;
    if role != "owner" || disabled {
        check_owner_remains(conn, id)?;
    }
    conn.execute(
        "UPDATE member SET name = ?2, role = ?3, disabled = ?4 WHERE id = ?1",
        rusqlite::params![id, name, role, disabled as i64],
    )?;
    get(conn, id)
}

/// 重置令牌：旧令牌立即失效。
pub fn reset_token(conn: &Connection, id: &str) -> Result<(Member, String)> {
    get(conn, id)?;
    let (token, hash, hint) = fresh_token();
    conn.execute(
        "UPDATE member SET token_hash = ?2, token_hint = ?3 WHERE id = ?1",
        rusqlite::params![id, hash, hint],
    )?;
    Ok((get(conn, id)?, token))
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    check_owner_remains(conn, id)?;
    conn.execute(
        "UPDATE ticket SET assignee_id = NULL WHERE assignee_id = ?1",
        [id],
    )?;
    conn.execute("DELETE FROM member WHERE id = ?1", [id])?;
    Ok(())
}

/// 用令牌找成员；停用的成员不算。
pub fn by_token(conn: &Connection, token: &str) -> Result<Option<Member>> {
    if !token.starts_with(TOKEN_PREFIX) {
        return Ok(None);
    }
    let hash = crate::util::sha256_hex(token.as_bytes());
    let m = conn
        .query_row(
            &format!("SELECT {COLS} FROM member WHERE token_hash = ?1 AND disabled = 0"),
            [hash],
            row,
        )
        .optional()?;
    if let Some(m) = &m {
        conn.execute(
            "UPDATE member SET last_seen_at = ?2 WHERE id = ?1",
            rusqlite::params![m.id, crate::util::now()],
        )?;
    }
    Ok(m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_hashed_and_owner_is_protected() {
        let conn = crate::db::open_in_memory().unwrap();
        let (owner, token) = create(&conn, "刘", "owner").unwrap();
        assert!(token.starts_with("tw_") && token.len() == 67);
        let stored: String = conn
            .query_row("SELECT token_hash FROM member", [], |r| r.get(0))
            .unwrap();
        assert_ne!(stored, token);
        assert_eq!(by_token(&conn, &token).unwrap().unwrap().id, owner.id);
        assert!(by_token(&conn, "tw_nope").unwrap().is_none());
        // 最后一个 owner 不能降级、停用或删除
        assert!(update(&conn, &owner.id, "刘", "member", false).is_err());
        assert!(update(&conn, &owner.id, "刘", "owner", true).is_err());
        assert!(delete(&conn, &owner.id).is_err());
        let (viewer, vt) = create(&conn, "访客", "viewer").unwrap();
        assert!(!viewer.can_write());
        // 重置后旧令牌失效；停用后新令牌也不能用
        let (_, vt2) = reset_token(&conn, &viewer.id).unwrap();
        assert!(by_token(&conn, &vt).unwrap().is_none());
        assert!(by_token(&conn, &vt2).unwrap().is_some());
        update(&conn, &viewer.id, "访客", "viewer", true).unwrap();
        assert!(by_token(&conn, &vt2).unwrap().is_none());
        assert!(create(&conn, "x", "admin").is_err());
    }
}
