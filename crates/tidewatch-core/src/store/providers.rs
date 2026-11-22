//! Provider：模型接入点。密钥不进库，只存凭据管理器里的条目名。
use super::*;
use crate::provider::{Protocol, ProviderConfig};
use serde::{Deserialize, Serialize};

/// 每百万 Token 的价格（美元），用于在 Agent 不自报费用时估算。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Pricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub cache_per_mtok: f64,
}

impl Pricing {
    pub fn estimate(&self, usage: &crate::agent::Usage) -> f64 {
        (usage.input_tokens as f64 * self.input_per_mtok
            + usage.output_tokens as f64 * self.output_per_mtok
            + usage.cached_tokens as f64 * self.cache_per_mtok)
            / 1_000_000.0
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Provider {
    pub id: String,
    pub name: String,
    pub protocol: String,
    pub base_url: Option<String>,
    /// 凭据管理器中的条目名；没有密钥（官方默认）时为空。
    pub secret_ref: Option<String>,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub small_model: Option<String>,
    pub extra_env: Vec<(String, String)>,
    pub pricing: Option<Pricing>,
    pub proxy: Option<String>,
    /// 模型只能处理文字：PRD 里的图片改发文字描述。
    pub text_only: bool,
    pub created_at: String,
    pub updated_at: String,
}

impl Provider {
    pub fn config(&self) -> Result<ProviderConfig> {
        let mut extra_env = self.extra_env.clone();
        if let Some(p) = &self.proxy {
            for k in ["HTTPS_PROXY", "HTTP_PROXY"] {
                extra_env.push((k.into(), p.clone()));
            }
        }
        Ok(ProviderConfig {
            name: self.name.clone(),
            protocol: Protocol::parse(&self.protocol)?,
            base_url: self.base_url.clone(),
            default_model: self.default_model.clone(),
            small_model: self.small_model.clone(),
            extra_env,
        })
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct ProviderInput {
    pub name: String,
    pub protocol: String,
    pub base_url: Option<String>,
    pub models: Vec<String>,
    pub default_model: Option<String>,
    pub small_model: Option<String>,
    pub extra_env: Vec<(String, String)>,
    pub pricing: Option<Pricing>,
    pub proxy: Option<String>,
    pub text_only: bool,
}

const COLS: &str = "id, name, protocol, base_url, secret_ref, models_json, default_model, small_model, extra_env_json, pricing_json, proxy, created_at, updated_at, text_only";

fn row(r: &rusqlite::Row) -> rusqlite::Result<Provider> {
    Ok(Provider {
        id: r.get(0)?,
        name: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
        protocol: r
            .get::<_, Option<String>>(2)?
            .unwrap_or_else(|| "native".into()),
        base_url: r.get(3)?,
        secret_ref: r.get(4)?,
        models: from_json(r.get(5)?),
        default_model: r.get(6)?,
        small_model: r.get(7)?,
        extra_env: from_json(r.get(8)?),
        pricing: r
            .get::<_, Option<String>>(9)?
            .and_then(|s| serde_json::from_str(&s).ok()),
        proxy: r.get(10)?,
        created_at: r.get::<_, Option<String>>(11)?.unwrap_or_default(),
        updated_at: r.get::<_, Option<String>>(12)?.unwrap_or_default(),
        text_only: r.get::<_, Option<bool>>(13)?.unwrap_or(false),
    })
}

fn check(input: ProviderInput) -> Result<ProviderInput> {
    let name = required(&input.name, "Provider 名称")?;
    let protocol = Protocol::parse(input.protocol.trim())?;
    let base_url = clean(input.base_url);
    if let Some(u) = &base_url {
        if !(u.starts_with("http://") || u.starts_with("https://")) {
            return Err(Error::invalid("Base URL 要以 http:// 或 https:// 开头。"));
        }
    }
    let mut models: Vec<String> = input
        .models
        .into_iter()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
        .collect();
    models.dedup();
    let default_model = clean(input.default_model);
    if let Some(m) = &default_model {
        if !models.contains(m) {
            models.insert(0, m.clone());
        }
    }
    let extra_env = input
        .extra_env
        .into_iter()
        .map(|(k, v)| (k.trim().to_string(), v))
        .filter(|(k, _)| !k.is_empty())
        .collect();
    Ok(ProviderInput {
        name,
        protocol: protocol.as_str().into(),
        base_url,
        models,
        default_model,
        small_model: clean(input.small_model),
        extra_env,
        pricing: input.pricing,
        proxy: clean(input.proxy),
        text_only: input.text_only,
    })
}

pub fn create(conn: &Connection, input: ProviderInput) -> Result<Provider> {
    let i = check(input)?;
    let id = crate::util::new_id();
    conn.execute(
        &format!("INSERT INTO provider ({COLS}) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?11, ?12)"),
        rusqlite::params![
            id,
            i.name,
            i.protocol,
            i.base_url,
            to_json(&i.models),
            i.default_model,
            i.small_model,
            to_json(&i.extra_env),
            i.pricing.as_ref().map(to_json),
            i.proxy,
            crate::util::now(),
            i.text_only
        ],
    )?;
    get(conn, &id)
}

pub fn update(conn: &Connection, id: &str, input: ProviderInput) -> Result<Provider> {
    get(conn, id)?;
    let i = check(input)?;
    conn.execute(
        "UPDATE provider SET name = ?2, protocol = ?3, base_url = ?4, models_json = ?5, default_model = ?6,
         small_model = ?7, extra_env_json = ?8, pricing_json = ?9, proxy = ?10, updated_at = ?11, text_only = ?12 WHERE id = ?1",
        rusqlite::params![
            id,
            i.name,
            i.protocol,
            i.base_url,
            to_json(&i.models),
            i.default_model,
            i.small_model,
            to_json(&i.extra_env),
            i.pricing.as_ref().map(to_json),
            i.proxy,
            crate::util::now(),
            i.text_only
        ],
    )?;
    get(conn, id)
}

/// 记下（或清掉）密钥在凭据管理器里的条目名。
pub fn set_secret_ref(conn: &Connection, id: &str, secret_ref: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE provider SET secret_ref = ?2, updated_at = ?3 WHERE id = ?1",
        rusqlite::params![id, secret_ref, crate::util::now()],
    )?;
    Ok(())
}

/// 保存（`Some`）或清除（`None` / 空串）Provider 的密钥：密钥进凭据管理器，库里只记条目名。
pub fn store_secret(
    conn: &Connection,
    secrets: &dyn crate::secrets::SecretStore,
    id: &str,
    secret: Option<&str>,
) -> Result<Provider> {
    get(conn, id)?;
    let name = secret_name(id);
    match secret.map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => {
            secrets.set(&name, s)?;
            set_secret_ref(conn, id, Some(&name))?;
        }
        None => {
            secrets.delete(&name)?;
            set_secret_ref(conn, id, None)?;
        }
    }
    get(conn, id)
}

/// 凭据管理器里的条目名。
pub fn secret_name(id: &str) -> String {
    format!("tidewatch/provider/{id}")
}

pub fn get(conn: &Connection, id: &str) -> Result<Provider> {
    conn.query_row(
        &format!("SELECT {COLS} FROM provider WHERE id = ?1"),
        [id],
        row,
    )
    .map_err(|_| not_found(" Provider", id))
}

pub fn list(conn: &Connection) -> Result<Vec<Provider>> {
    let mut st = conn.prepare(&format!("SELECT {COLS} FROM provider ORDER BY created_at"))?;
    let rows = st
        .query_map([], row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows)
}

pub fn delete(conn: &Connection, id: &str) -> Result<()> {
    let active: i64 = conn.query_row(
        "SELECT COUNT(*) FROM run WHERE provider_id = ?1 AND status IN ('queued', 'starting', 'running', 'finalizing')",
        [id],
        |r| r.get(0),
    )?;
    if active > 0 {
        return Err(Error::invalid("有正在使用这个 Provider 的运行，不能删除。"));
    }
    conn.execute(
        "UPDATE project SET default_provider_id = NULL WHERE default_provider_id = ?1",
        [id],
    )?;
    conn.execute(
        "UPDATE ticket SET default_provider_id = NULL WHERE default_provider_id = ?1",
        [id],
    )?;
    conn.execute(
        "UPDATE task SET provider_id = NULL WHERE provider_id = ?1 AND status NOT IN ('merged', 'cancelled')",
        [id],
    )?;
    let mut st = settings::get(conn)?;
    if st.default_provider_id.as_deref() == Some(id) {
        st.default_provider_id = None;
        settings::save(conn, &st)?;
    }
    conn.execute("DELETE FROM provider WHERE id = ?1", [id])?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crud_and_config() {
        let conn = crate::db::open_in_memory().unwrap();
        let p = create(
            &conn,
            ProviderInput {
                name: "DeepSeek".into(),
                protocol: "anthropic".into(),
                base_url: Some("https://api.deepseek.com/anthropic".into()),
                default_model: Some("deepseek-chat".into()),
                proxy: Some("http://127.0.0.1:7890".into()),
                pricing: Some(Pricing {
                    input_per_mtok: 1.0,
                    output_per_mtok: 2.0,
                    cache_per_mtok: 0.0,
                }),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(p.models, vec!["deepseek-chat"]);
        assert_eq!(p.secret_ref, None);
        let cfg = p.config().unwrap();
        assert!(cfg.extra_env.iter().any(|(k, _)| k == "HTTPS_PROXY"));
        let usage = crate::agent::Usage {
            input_tokens: 1_000_000,
            output_tokens: 500_000,
            ..Default::default()
        };
        assert_eq!(p.pricing.unwrap().estimate(&usage), 2.0);

        set_secret_ref(&conn, &p.id, Some(&secret_name(&p.id))).unwrap();
        assert!(get(&conn, &p.id).unwrap().secret_ref.is_some());
        // 库里任何一列都不该出现密钥：这里只有条目名
        let st = settings::Settings {
            default_provider_id: Some(p.id.clone()),
            ..Default::default()
        };
        settings::save(&conn, &st).unwrap();
        delete(&conn, &p.id).unwrap();
        assert!(list(&conn).unwrap().is_empty());
        assert_eq!(
            settings::get(&conn).unwrap().default_provider_id,
            None,
            "删掉的 Provider 不能还是全局默认"
        );
    }

    #[test]
    fn secret_never_reaches_the_database() {
        use crate::secrets::SecretStore;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("tw.db");
        let conn = crate::db::open(&path).unwrap();
        let secrets = crate::secrets::MemoryStore::default();
        let p = create(
            &conn,
            ProviderInput {
                name: "中转".into(),
                protocol: "anthropic".into(),
                ..Default::default()
            },
        )
        .unwrap();
        let p = store_secret(&conn, &secrets, &p.id, Some(" sk-top-secret-0123456789 ")).unwrap();
        let name = p.secret_ref.clone().unwrap();
        assert_eq!(
            secrets.get(&name).unwrap().as_deref(),
            Some("sk-top-secret-0123456789")
        );
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")
            .unwrap();
        for entry in std::fs::read_dir(dir.path()).unwrap() {
            let bytes = std::fs::read(entry.unwrap().path()).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("sk-top-secret"));
        }
        let p = store_secret(&conn, &secrets, &p.id, Some("")).unwrap();
        assert_eq!(p.secret_ref, None);
        assert_eq!(secrets.get(&name).unwrap(), None);
    }

    #[test]
    fn rejects_bad_input() {
        let conn = crate::db::open_in_memory().unwrap();
        let bad = |f: fn(&mut ProviderInput)| {
            let mut i = ProviderInput {
                name: "x".into(),
                protocol: "anthropic".into(),
                ..Default::default()
            };
            f(&mut i);
            create(&conn, i).is_err()
        };
        assert!(bad(|i| i.name = " ".into()));
        assert!(bad(|i| i.protocol = "grpc".into()));
        assert!(bad(|i| i.base_url = Some("ftp://x".into())));
    }
}
