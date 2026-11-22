//! 同步用的 HTTP 请求：有超时、走系统代理、响应有大小上限，状态码交给调用方判断。
use std::time::Duration;

use serde_json::Value;

use crate::{Error, Result};

const MAX_JSON_BYTES: u64 = 8 * 1024 * 1024;

pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Response {
    pub fn ok(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn json(&self) -> Value {
        serde_json::from_slice(&self.body).unwrap_or(Value::Null)
    }
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(30)))
        .http_status_as_error(false)
        .proxy(ureq::Proxy::try_from_env())
        .build()
        .into()
}

/// `body` 为 `Some` 时按 JSON 发送。`limit`：响应体上限（字节）。
pub fn request(
    method: &str,
    url: &str,
    headers: &[(&str, String)],
    body: Option<&Value>,
    limit: u64,
) -> Result<Response> {
    let fail = |e: &dyn std::fmt::Display| Error::invalid(format!("请求 {url} 失败：{e}"));
    let mut builder = ureq::http::Request::builder()
        .method(method)
        .uri(url)
        .header("User-Agent", "Tidewatch");
    for (k, v) in headers {
        builder = builder.header(*k, v.as_str());
    }
    let agent = agent();
    let mut resp = match body {
        Some(b) => {
            let req = builder
                .header("Content-Type", "application/json; charset=utf-8")
                .body(b.to_string())
                .map_err(|e| fail(&e))?;
            agent.run(req)
        }
        None => {
            let req = builder.body(()).map_err(|e| fail(&e))?;
            agent.run(req)
        }
    }
    .map_err(|e| fail(&e))?;
    let status = resp.status().as_u16();
    let content_type = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_string();
    let body = resp
        .body_mut()
        .with_config()
        .limit(limit)
        .read_to_vec()
        .map_err(|e| fail(&e))?;
    Ok(Response {
        status,
        content_type,
        body,
    })
}

pub fn get(url: &str, headers: &[(&str, String)]) -> Result<Response> {
    request("GET", url, headers, None, MAX_JSON_BYTES)
}

pub fn send(method: &str, url: &str, headers: &[(&str, String)], body: &Value) -> Result<Response> {
    request(method, url, headers, Some(body), MAX_JSON_BYTES)
}

/// 查询串里的一个值。
pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}
