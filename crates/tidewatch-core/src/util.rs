use chrono::{SecondsFormat, Utc};

/// ULID：可按时间排序的主键。
pub fn new_id() -> String {
    ulid::Ulid::generate().to_string()
}

/// ISO 8601 UTC 时间字符串。
pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

/// 密钥脱敏：`sk-****abcd`。
pub fn mask_secret(s: &str) -> String {
    let n = s.chars().count();
    if n <= 8 {
        return "****".into();
    }
    let head: String = s.chars().take(3).collect();
    let tail: String = s.chars().skip(n - 4).collect();
    format!("{head}****{tail}")
}

/// 规范化路径。Windows 上 `std::fs::canonicalize` 会返回 `\\?\` 前缀的路径，git 和多数 CLI 不认，这里去掉它。
pub fn canonical(path: &std::path::Path) -> std::io::Result<std::path::PathBuf> {
    dunce::canonicalize(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_secrets() {
        assert_eq!(mask_secret("sk-1234567890abcd"), "sk-****abcd");
        assert_eq!(mask_secret("short"), "****");
    }

    #[test]
    fn ids_are_sortable() {
        let a = new_id();
        std::thread::sleep(std::time::Duration::from_millis(2));
        let b = new_id();
        assert!(a < b);
    }
}
