//! 本地 HTTP / MCP 接口：给脚本、其他人（带令牌）和运行中的 Agent 用。
//!
//! - `/api/v1/*`：REST，成员令牌（`tw_…`）。
//! - `/mcp`：MCP（Streamable HTTP，JSON-RPC），成员令牌或运行令牌（`twr_…`，只在那一轮运行期间有效）。
//! - `/`：只读看板页面（静态文件，数据经 REST 取）。
//!
//! 默认只监听 127.0.0.1；开了"远程访问"才监听所有网卡。没有令牌的请求一律 401。
mod mcp;
mod rest;

use crate::db::lock;
use crate::engine::{ApiEndpoint, Engine};
use crate::store::members::{self, Member};
use crate::store::runs::Run;
use crate::{Error, Result};
use serde_json::{json, Value};
use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

/// 请求体上限：PRD 正文足够，挡住误传的大文件。
const MAX_BODY: usize = 2 * 1024 * 1024;
const WORKERS: usize = 8;

/// 谁在调用。
#[derive(Debug, Clone)]
pub(crate) enum Caller {
    Member(Member),
    /// 运行中的 Agent：只能用 MCP 工具，范围限于它所在的工单。
    Run(Box<Run>),
}

/// 接口层的错误：带 HTTP 状态码。
#[derive(Debug)]
pub(crate) struct ApiError {
    pub status: u16,
    pub message: String,
}

impl ApiError {
    pub(crate) fn new(status: u16, message: impl Into<String>) -> Self {
        ApiError {
            status,
            message: message.into(),
        }
    }
    pub(crate) fn forbidden() -> Self {
        ApiError::new(403, "这个令牌没有权限做这件事。")
    }
}

impl From<Error> for ApiError {
    fn from(e: Error) -> Self {
        let status = match &e {
            Error::NotFound(_) => 404,
            Error::Invalid(_) => 400,
            _ => 500,
        };
        ApiError::new(status, e.to_string())
    }
}

pub(crate) type ApiResult<T> = std::result::Result<T, ApiError>;

pub struct ApiServer {
    server: Arc<tiny_http::Server>,
    engine: Arc<Engine>,
    stop: Arc<AtomicBool>,
    workers: Vec<JoinHandle<()>>,
    addr: SocketAddr,
}

impl ApiServer {
    /// 启动接口并告诉引擎：之后开始的运行会带上 MCP 服务。端口传 0 由系统挑一个（测试用）。
    pub fn start(engine: Arc<Engine>, port: u16, remote: bool) -> Result<ApiServer> {
        let host = if remote { "0.0.0.0" } else { "127.0.0.1" };
        let server = tiny_http::Server::http((host, port))
            .map_err(|e| Error::invalid(format!("接口启动失败（{host}:{port}）：{e}")))?;
        let addr = server
            .server_addr()
            .to_ip()
            .ok_or_else(|| Error::invalid("接口没有拿到监听地址。"))?;
        let server = Arc::new(server);
        let stop = Arc::new(AtomicBool::new(false));
        engine.set_api(Some(ApiEndpoint {
            // Agent 和观澜在同一台机器上，始终走回环地址
            base_url: format!("http://127.0.0.1:{}", addr.port()),
            secret: crate::util::new_token(""),
        }));
        let workers = (0..WORKERS)
            .map(|i| {
                let (server, engine, stop) = (server.clone(), engine.clone(), stop.clone());
                std::thread::Builder::new()
                    .name(format!("tw-api-{i}"))
                    .spawn(move || {
                        while !stop.load(Ordering::SeqCst) {
                            match server.recv_timeout(Duration::from_millis(200)) {
                                Ok(Some(req)) => handle(&engine, req),
                                Ok(None) => {}
                                Err(_) => break,
                            }
                        }
                    })
            })
            .collect::<std::io::Result<Vec<_>>>()?;
        Ok(ApiServer {
            server,
            engine,
            stop,
            workers,
            addr,
        })
    }

    pub fn port(&self) -> u16 {
        self.addr.port()
    }

    pub fn base_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.addr.port())
    }

    pub fn stop(mut self) {
        self.shutdown();
    }

    fn shutdown(&mut self) {
        self.engine.set_api(None);
        self.stop.store(true, Ordering::SeqCst);
        self.server.unblock();
        for h in self.workers.drain(..) {
            let _ = h.join();
        }
    }
}

impl Drop for ApiServer {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn header<'a>(req: &'a tiny_http::Request, name: &str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.as_str().as_str().eq_ignore_ascii_case(name))
        .map(|h| h.value.as_str())
}

fn authenticate(engine: &Engine, req: &tiny_http::Request) -> ApiResult<Caller> {
    let token = header(req, "authorization")
        .and_then(|v| v.strip_prefix("Bearer ").or(v.strip_prefix("bearer ")))
        .map(str::trim)
        .ok_or_else(|| ApiError::new(401, "缺少令牌：请带上 Authorization: Bearer <令牌>。"))?;
    if let Some(run) = engine.run_for_token(token) {
        return Ok(Caller::Run(Box::new(run)));
    }
    match members::by_token(&lock(engine.db()), token)? {
        Some(m) => Ok(Caller::Member(m)),
        None => Err(ApiError::new(401, "令牌无效，或者已经停用。")),
    }
}

fn respond(req: tiny_http::Request, status: u16, content_type: &str, body: Vec<u8>) {
    let mut resp = tiny_http::Response::from_data(body).with_status_code(status);
    for (k, v) in [
        ("Content-Type", content_type),
        ("Cache-Control", "no-store"),
        ("X-Content-Type-Options", "nosniff"),
    ] {
        if let Ok(h) = tiny_http::Header::from_bytes(k.as_bytes(), v.as_bytes()) {
            resp.add_header(h);
        }
    }
    let _ = req.respond(resp);
}

fn respond_json(req: tiny_http::Request, status: u16, body: &Value) {
    respond(
        req,
        status,
        "application/json; charset=utf-8",
        body.to_string().into_bytes(),
    );
}

fn read_body(req: &mut tiny_http::Request) -> ApiResult<Value> {
    let mut buf = Vec::new();
    req.as_reader()
        .take(MAX_BODY as u64 + 1)
        .read_to_end(&mut buf)
        .map_err(|e| ApiError::new(400, format!("读取请求体失败：{e}")))?;
    if buf.len() > MAX_BODY {
        return Err(ApiError::new(413, "请求体太大（上限 2 MB）。"));
    }
    if buf.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    serde_json::from_slice(&buf)
        .map_err(|e| ApiError::new(400, format!("请求体不是合法的 JSON：{e}")))
}

/// 浏览器里的其他网页不该能调本机接口：带 Origin 的请求，Origin 必须就是接口自己。
fn same_origin(req: &tiny_http::Request) -> bool {
    match (header(req, "origin"), header(req, "host")) {
        (Some(origin), Some(host)) => origin
            .strip_prefix("http://")
            .is_some_and(|o| o.eq_ignore_ascii_case(host)),
        (Some(_), None) => false,
        (None, _) => true,
    }
}

fn handle(engine: &Arc<Engine>, mut req: tiny_http::Request) {
    let method = req.method().as_str().to_ascii_uppercase();
    let url = req.url().to_string();
    let (path, query) = url.split_once('?').unwrap_or((&url, ""));
    let path = path.trim_end_matches('/').to_string();
    let query = query.to_string();

    if !same_origin(&req) {
        return respond_json(req, 403, &json!({ "error": "不接受跨站请求。" }));
    }
    // 不需要令牌的：看板页面本身和健康检查
    if method == "GET" && path.is_empty() {
        return respond(
            req,
            200,
            "text/html; charset=utf-8",
            include_str!("board.html").as_bytes().to_vec(),
        );
    }
    if method == "GET" && path == "/api/v1/health" {
        return respond_json(req, 200, &json!({ "ok": true, "app": "tidewatch" }));
    }

    let result: ApiResult<(u16, Value)> = (|| {
        let caller = authenticate(engine, &req)?;
        let body = if matches!(method.as_str(), "POST" | "PUT" | "PATCH") {
            read_body(&mut req)?
        } else {
            json!({})
        };
        if path == "/mcp" {
            return match method.as_str() {
                "POST" => mcp::handle(engine, &caller, body),
                // 不提供服务端推送流
                _ => Err(ApiError::new(405, "MCP 接口只接受 POST。")),
            };
        }
        match path.strip_prefix("/api/v1") {
            Some(rest_path) => match &caller {
                Caller::Member(m) => rest::handle(engine, m, &method, rest_path, &query, body),
                Caller::Run(_) => Err(ApiError::new(403, "运行令牌只能用于 MCP 接口。")),
            },
            None => Err(ApiError::new(404, "没有这个接口。")),
        }
    })();
    match result {
        // MCP 通知没有响应体
        Ok((202, _)) => respond(req, 202, "application/json", vec![]),
        Ok((status, body)) => respond_json(req, status, &body),
        Err(e) => respond_json(req, e.status, &json!({ "error": e.message })),
    }
}
