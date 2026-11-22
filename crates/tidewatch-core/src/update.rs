//! 检查更新：只读 GitHub 上最新发布的版本号，和本机版本比较。不下载、不安装，
//! 只在用户打开了开关或手动点「检查更新」时联网，请求里不带任何本机数据。
use crate::{Error, Result};
use serde::Serialize;
use std::time::Duration;

pub const RELEASES_URL: &str = "https://api.github.com/repos/yoob52/Tidewatch/releases/latest";

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct UpdateInfo {
    pub current: String,
    /// 最新发布的版本；还没有任何发布时为空。
    pub latest: Option<String>,
    pub newer: bool,
    /// 发布页地址。
    pub url: Option<String>,
    pub notes: Option<String>,
}

/// `1.2.3` / `v1.2.3` / `1.2.3-beta.1` → 数字部分；预发布标记单独返回。
fn parse(v: &str) -> Option<(Vec<u64>, bool)> {
    let v = v.trim().trim_start_matches(['v', 'V']);
    let (core, pre) = match v.split_once(['-', '+']) {
        Some((c, _)) => (c, v.contains('-')),
        None => (v, false),
    };
    let nums = core
        .split('.')
        .map(|p| p.parse::<u64>().ok())
        .collect::<Option<Vec<_>>>()?;
    (!nums.is_empty()).then_some((nums, pre))
}

/// `latest` 是否比 `current` 新。看不懂的版本号一律当作不新。
pub fn is_newer(current: &str, latest: &str) -> bool {
    let (Some((mut a, a_pre)), Some((mut b, b_pre))) = (parse(current), parse(latest)) else {
        return false;
    };
    let n = a.len().max(b.len());
    a.resize(n, 0);
    b.resize(n, 0);
    // 数字相同：正式版比预发布版新
    b > a || (b == a && a_pre && !b_pre)
}

/// 解析 GitHub `releases/latest` 的返回。
pub fn from_release_json(current: &str, body: &str) -> Result<UpdateInfo> {
    let v: serde_json::Value = serde_json::from_str(body)
        .map_err(|e| Error::invalid(format!("更新信息不是合法的 JSON：{e}")))?;
    let tag = v["tag_name"]
        .as_str()
        .filter(|t| !t.trim().is_empty())
        .ok_or_else(|| Error::invalid("更新信息里没有版本号。"))?;
    Ok(UpdateInfo {
        current: current.into(),
        latest: Some(tag.trim_start_matches(['v', 'V']).to_string()),
        newer: is_newer(current, tag),
        url: v["html_url"].as_str().map(String::from),
        notes: v["body"]
            .as_str()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(String::from),
    })
}

/// 联网查一次。`url` 一般是 [`RELEASES_URL`]。
pub fn check(current: &str, url: &str) -> Result<UpdateInfo> {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(10)))
        .http_status_as_error(false)
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .into();
    let mut resp = agent
        .get(url)
        .header("User-Agent", "Tidewatch")
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| Error::invalid(format!("检查更新失败：{e}")))?;
    let status = resp.status().as_u16();
    // 仓库还没有任何发布
    if status == 404 {
        return Ok(UpdateInfo {
            current: current.into(),
            latest: None,
            newer: false,
            url: None,
            notes: None,
        });
    }
    if !(200..300).contains(&status) {
        return Err(Error::invalid(format!(
            "检查更新失败：服务器返回 {status}。"
        )));
    }
    let body = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| Error::invalid(format!("检查更新失败：{e}")))?;
    from_release_json(current, &body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};

    #[test]
    fn compares_versions() {
        assert!(is_newer("0.1.0", "v0.2.0"));
        assert!(is_newer("0.9.9", "0.10.0"));
        assert!(is_newer("1.0", "1.0.1"));
        assert!(is_newer("1.0.0-beta.1", "1.0.0"));
        assert!(!is_newer("1.0.0", "1.0.0"));
        assert!(!is_newer("1.0.0", "v1.0.0-rc.1"));
        assert!(!is_newer("1.2.0", "1.1.9"));
        assert!(!is_newer("1.0.0", "nightly"));
    }

    /// 起一个只回一次的本机 HTTP 服务。
    fn serve(status: &str, body: &'static str) -> String {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        let status = status.to_string();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut buf = [0u8; 2048];
            let _ = s.read(&mut buf);
            let _ = write!(
                s,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        });
        format!("http://{addr}/latest")
    }

    #[test]
    fn reads_the_latest_release() {
        let url = serve(
            "200 OK",
            r#"{"tag_name":"v0.3.0","html_url":"https://github.com/yoob52/Tidewatch/releases/tag/v0.3.0","body":"修了两个问题\n"}"#,
        );
        let u = check("0.1.0", &url).unwrap();
        assert!(u.newer);
        assert_eq!(u.latest.as_deref(), Some("0.3.0"));
        assert_eq!(u.notes.as_deref(), Some("修了两个问题"));
        assert!(u.url.unwrap().ends_with("/v0.3.0"));
    }

    #[test]
    fn no_release_yet_is_not_an_error_but_a_server_failure_is() {
        let u = check(
            "0.1.0",
            &serve("404 Not Found", r#"{"message":"Not Found"}"#),
        )
        .unwrap();
        assert_eq!((u.latest, u.newer), (None, false));
        let e = check("0.1.0", &serve("500 Internal Server Error", "{}")).unwrap_err();
        assert!(e.to_string().contains("500"), "{e}");
    }
}
