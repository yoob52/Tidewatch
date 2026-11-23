//! Codex 交互模式：`codex app-server`（stdio JSON-RPC，桌面版同一套协议）。
//! 运行中的追加指令走 `turn/steer`，并进当前这一轮；这一轮已结束时改用 `turn/start` 开新的一轮。
//! 关闭 stdin 后 app-server 自行退出。
use super::*;
use serde_json::{json, Value};
use std::collections::HashMap;

const THREAD_REQ: u64 = 1;

fn rpc(id: u64, method: &str, params: Value) -> String {
    format!(
        "{}\n",
        json!({ "id": id, "method": method, "params": params })
    )
}

fn sandbox(p: PermissionMode) -> &'static str {
    match p {
        PermissionMode::ReadOnly => "read-only",
        PermissionMode::WorkspaceWrite => "workspace-write",
        PermissionMode::FullAuto => "danger-full-access",
    }
}

/// 启动时写进 stdin 的握手和建 / 续接会话请求。
pub fn handshake(spec: &RunSpec) -> String {
    let mut params = json!({
        "cwd": spec.cwd.display().to_string(),
        "approvalPolicy": "never",
        "sandbox": sandbox(spec.permission),
    });
    if let Some(m) = &spec.model {
        params["model"] = json!(m);
    }
    let thread = match &spec.resume_session {
        Some(id) => {
            params["threadId"] = json!(id);
            rpc(THREAD_REQ, "thread/resume", params)
        }
        None => rpc(THREAD_REQ, "thread/start", params),
    };
    let init = json!({"id": 0, "method": "initialize", "params": {"clientInfo": {"name": "tidewatch", "version": env!("CARGO_PKG_VERSION")}}});
    format!("{init}\n{}\n{thread}", json!({"method": "initialized"}))
}

fn text_input(text: &str) -> Value {
    json!({ "type": "text", "text": text })
}

pub struct CodexAppParser {
    prompt: Vec<Value>,
    reasoning_effort: Option<String>,
    thread: Option<String>,
    turn: Option<String>,
    next_id: u64,
    /// 已发出、还没收到响应的请求 → 追加指令原文（turn/start 为空串）
    awaiting: HashMap<u64, String>,
    /// 还发不出去的追加指令：会话没建好，或 turn/steer 被拒、要等这一轮结束再开新的一轮
    early: Vec<String>,
    /// 追加指令被拒的次数，超过上限就报错不再重试
    rejected: HashMap<String, u8>,
    input: Vec<String>,
    started: bool,
    failed: bool,
    last_message: String,
    total: Usage,
    turns: u64,
}

impl CodexAppParser {
    pub fn new(spec: &RunSpec) -> Self {
        let mut prompt = vec![text_input(&spec.prompt)];
        for f in &spec.images {
            prompt.push(json!({ "type": "localImage", "path": f.display().to_string() }));
        }
        CodexAppParser {
            prompt,
            reasoning_effort: spec.reasoning_effort.clone(),
            thread: None,
            turn: None,
            next_id: THREAD_REQ + 1,
            awaiting: HashMap::new(),
            early: vec![],
            rejected: HashMap::new(),
            input: vec![],
            started: false,
            failed: false,
            last_message: String::new(),
            total: Usage::default(),
            turns: 0,
        }
    }

    fn request(&mut self, method: &str, params: Value, steer: &str) {
        let id = self.next_id;
        self.next_id += 1;
        self.awaiting.insert(id, steer.to_string());
        self.input.push(rpc(id, method, params));
    }

    fn start_turn(&mut self, input: Vec<Value>, steer: &str) {
        let thread = self.thread.clone().unwrap_or_default();
        let mut params = json!({ "threadId": thread, "input": input });
        if let Some(effort) = &self.reasoning_effort {
            params["effort"] = json!(effort);
        }
        self.request("turn/start", params, steer);
    }

    fn send_steer(&mut self, text: &str) {
        match (&self.thread, &self.turn) {
            (Some(thread), Some(turn)) => {
                let params = json!({ "threadId": thread, "expectedTurnId": turn, "input": [text_input(text)] });
                self.request("turn/steer", params, text);
            }
            (Some(_), None) => self.start_turn(vec![text_input(text)], text),
            _ => self.early.push(text.to_string()),
        }
    }

    fn finished(&mut self, ok: bool, summary: String, duration_ms: Option<u64>) -> AgentEvent {
        self.turns += 1;
        AgentEvent::Finished {
            ok,
            summary,
            usage: Some(self.total.clone()),
            num_turns: Some(self.turns),
            duration_ms,
        }
    }

    fn on_response(&mut self, id: u64, v: &Value, out: &mut Vec<AgentEvent>) {
        let err = v.pointer("/error/message").and_then(|m| m.as_str());
        if id == THREAD_REQ {
            if let Some(e) = err {
                self.failed = true;
                out.push(AgentEvent::Error {
                    message: e.to_string(),
                });
                out.push(self.finished(false, e.to_string(), None));
                return;
            }
            let thread = v
                .pointer("/result/thread/id")
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            self.thread = Some(thread.to_string());
            out.push(AgentEvent::SessionStarted {
                session_id: thread.to_string(),
                model: v
                    .pointer("/result/model")
                    .and_then(|m| m.as_str())
                    .map(str::to_string),
            });
            let prompt = std::mem::take(&mut self.prompt);
            self.start_turn(prompt, "");
            self.started = true;
            return;
        }
        let Some(steer) = self.awaiting.remove(&id) else {
            return;
        };
        match err {
            Some(e) if !steer.is_empty() => {
                let n = self.rejected.entry(steer.clone()).or_default();
                *n += 1;
                if *n > 2 {
                    self.rejected.remove(&steer);
                    out.push(AgentEvent::Error {
                        message: e.to_string(),
                    });
                } else if self.turn.is_none() {
                    self.start_turn(vec![text_input(&steer)], &steer);
                } else {
                    // 这一轮正在结束（turn/completed 还没到）：等它结束再开新的一轮
                    self.early.push(steer);
                }
            }
            Some(e) => out.push(AgentEvent::Error {
                message: e.to_string(),
            }),
            None => {
                self.rejected.remove(&steer);
                if let Some(t) = v.pointer("/result/turn/id").and_then(|t| t.as_str()) {
                    self.turn = Some(t.to_string());
                    for s in std::mem::take(&mut self.early) {
                        self.send_steer(&s);
                    }
                }
            }
        }
    }

    fn on_item(&mut self, item: &Value, done: bool, out: &mut Vec<AgentEvent>) {
        let id = str_of(item, "id");
        match item
            .get("type")
            .and_then(|t| t.as_str())
            .unwrap_or_default()
        {
            "agentMessage" if done => {
                self.last_message = str_of(item, "text");
                out.push(AgentEvent::Text {
                    text: self.last_message.clone(),
                });
            }
            "reasoning" if done => {
                let parts: Vec<String> = ["summary", "content"]
                    .iter()
                    .flat_map(|k| {
                        item.get(*k)
                            .and_then(|a| a.as_array())
                            .cloned()
                            .unwrap_or_default()
                    })
                    .filter_map(|p| p.as_str().map(str::to_string))
                    .collect();
                if !parts.is_empty() {
                    out.push(AgentEvent::Thinking {
                        text: parts.join("\n"),
                    });
                }
            }
            "commandExecution" => out.push(AgentEvent::Command {
                command: str_of(item, "command"),
                output: str_of(item, "aggregatedOutput"),
                exit_code: item.get("exitCode").and_then(|c| c.as_i64()),
                done,
            }),
            "fileChange" if done => {
                for c in item
                    .get("changes")
                    .and_then(|c| c.as_array())
                    .cloned()
                    .unwrap_or_default()
                {
                    out.push(AgentEvent::FileChange {
                        path: str_of(&c, "path"),
                        kind: c
                            .pointer("/kind/type")
                            .and_then(|k| k.as_str())
                            .unwrap_or_default()
                            .to_string(),
                    });
                }
            }
            "mcpToolCall" if done => out.push(AgentEvent::ToolResult {
                id,
                output: match item.pointer("/error/message").and_then(|m| m.as_str()) {
                    Some(e) => e.to_string(),
                    None => item
                        .get("result")
                        .map(|r| r.to_string())
                        .unwrap_or_default(),
                },
                is_error: item.get("status").and_then(|s| s.as_str()) == Some("failed"),
            }),
            "mcpToolCall" => out.push(AgentEvent::ToolCall {
                id,
                name: format!("{}.{}", str_of(item, "server"), str_of(item, "tool")),
                input: item.get("arguments").cloned().unwrap_or(Value::Null),
            }),
            "webSearch" if done => out.push(AgentEvent::ToolCall {
                id,
                name: "web_search".into(),
                input: json!({ "query": str_of(item, "query") }),
            }),
            _ => {}
        }
    }
}

impl EventParser for CodexAppParser {
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let line = line.trim();
        if line.is_empty() {
            return vec![];
        }
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            return vec![AgentEvent::Raw {
                line: line.to_string(),
            }];
        };
        let mut out = vec![];
        let id = v.get("id").and_then(|i| i.as_u64());
        let Some(method) = v.get("method").and_then(|m| m.as_str()) else {
            if let Some(id) = id {
                self.on_response(id, &v, &mut out);
            }
            return out;
        };
        if let Some(rid) = v.get("id") {
            // 审批策略是 never，正常不会有服务端请求；有就明确拒绝，免得它一直等
            self.input.push(format!(
                "{}\n",
                json!({ "id": rid, "error": { "code": -32601, "message": format!("tidewatch does not handle {method}") } })
            ));
            return out;
        }
        let p = v.get("params").cloned().unwrap_or(Value::Null);
        let other_thread = p
            .get("threadId")
            .and_then(|t| t.as_str())
            .is_some_and(|t| Some(t) != self.thread.as_deref());
        if other_thread
            && matches!(
                method,
                "turn/started" | "turn/completed" | "thread/tokenUsage/updated"
            )
        {
            // 子 Agent 的线程：它的轮次不代表主会话的这一轮
            return out;
        }
        match method {
            "turn/started" => {
                self.turn = p
                    .pointer("/turn/id")
                    .and_then(|t| t.as_str())
                    .map(str::to_string);
                for s in std::mem::take(&mut self.early) {
                    self.send_steer(&s);
                }
            }
            "item/started" | "item/completed" => {
                if let Some(item) = p.get("item") {
                    self.on_item(item, method == "item/completed", &mut out);
                }
            }
            "thread/tokenUsage/updated" => {
                let u = p
                    .pointer("/tokenUsage/total")
                    .cloned()
                    .unwrap_or(Value::Null);
                let cached = u64_of(&u, "cachedInputTokens");
                self.total = Usage {
                    input_tokens: u64_of(&u, "inputTokens").saturating_sub(cached),
                    output_tokens: u64_of(&u, "outputTokens"),
                    cached_tokens: cached,
                    cost_usd: None,
                };
            }
            "turn/completed" => {
                self.turn = None;
                let pending = std::mem::take(&mut self.early);
                if !pending.is_empty() && self.thread.is_some() {
                    let input = pending.iter().map(|t| text_input(t)).collect();
                    self.start_turn(input, &pending.join("\n\n"));
                }
                let duration = p.pointer("/turn/durationMs").and_then(|d| d.as_u64());
                let status = p
                    .pointer("/turn/status")
                    .and_then(|s| s.as_str())
                    .unwrap_or("completed");
                if status == "completed" {
                    let summary = self.last_message.clone();
                    out.push(self.finished(true, summary, duration));
                } else {
                    let message = p
                        .pointer("/turn/error/message")
                        .and_then(|m| m.as_str())
                        .unwrap_or(status)
                        .to_string();
                    out.push(AgentEvent::Error {
                        message: message.clone(),
                    });
                    out.push(self.finished(false, message, duration));
                }
            }
            "error" if p.get("willRetry").and_then(|w| w.as_bool()) != Some(true) => {
                out.push(AgentEvent::Error {
                    message: p
                        .pointer("/error/message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("error")
                        .to_string(),
                });
            }
            _ => {}
        }
        out
    }

    fn take_input(&mut self) -> Vec<String> {
        std::mem::take(&mut self.input)
    }

    fn steer(&mut self, text: &str) -> Option<String> {
        self.send_steer(text);
        Some(self.input.drain(..).collect())
    }

    fn idle(&self) -> Option<bool> {
        Some(
            self.failed
                || (self.started
                    && self.turn.is_none()
                    && self.awaiting.is_empty()
                    && self.early.is_empty()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> RunSpec {
        RunSpec {
            prompt: "do it".into(),
            cwd: "/w".into(),
            interactive: true,
            ..Default::default()
        }
    }

    fn sent(p: &mut CodexAppParser) -> Vec<Value> {
        p.take_input()
            .iter()
            .map(|l| serde_json::from_str(l.trim()).unwrap())
            .collect()
    }

    #[test]
    fn handshake_starts_or_resumes_thread() {
        let h = handshake(&spec());
        assert!(
            h.contains("\"method\":\"initialize\"")
                && h.contains("thread/start")
                && h.contains("\"approvalPolicy\":\"never\"")
        );
        let mut s = spec();
        s.resume_session = Some("th1".into());
        assert!(handshake(&s).contains("\"method\":\"thread/resume\""));
    }

    #[test]
    fn steers_active_turn_and_starts_new_turn_after() {
        let mut spec = spec();
        spec.reasoning_effort = Some("high".into());
        let mut p = CodexAppParser::new(&spec);
        assert_eq!(p.idle(), Some(false));
        // 会话建好前的追加指令先攒着
        assert_eq!(p.steer("early").as_deref(), Some(""));
        let ev = p.parse_line(r#"{"id":1,"result":{"thread":{"id":"th1"},"model":"gpt"}}"#);
        assert!(
            matches!(&ev[0], AgentEvent::SessionStarted { session_id, .. } if session_id == "th1")
        );
        let reqs = sent(&mut p);
        assert_eq!(reqs[0]["method"], "turn/start");
        assert_eq!(reqs[0]["params"]["input"][0]["text"], "do it");
        assert_eq!(reqs[0]["params"]["effort"], "high");
        p.parse_line(r#"{"id":2,"result":{"turn":{"id":"t1","status":"inProgress"}}}"#);
        let reqs = sent(&mut p);
        assert_eq!(reqs[0]["method"], "turn/steer");
        assert_eq!(reqs[0]["params"]["expectedTurnId"], "t1");
        p.parse_line(r#"{"id":3,"result":{"turnId":"t1"}}"#);
        let line = p.steer("more").unwrap();
        assert!(line.contains("turn/steer") && line.contains("more"));
        p.parse_line(r#"{"method":"item/completed","params":{"item":{"type":"agentMessage","id":"m","text":"done"}}}"#);
        p.parse_line(r#"{"method":"thread/tokenUsage/updated","params":{"tokenUsage":{"total":{"inputTokens":10,"cachedInputTokens":2,"outputTokens":5}}}}"#);
        let ev = p.parse_line(r#"{"method":"turn/completed","params":{"turn":{"id":"t1","status":"completed","durationMs":7}}}"#);
        assert!(
            matches!(&ev[0], AgentEvent::Finished { ok: true, summary, usage: Some(u), .. } if summary == "done" && u.input_tokens == 8)
        );
        // 发出的 turn/steer 撞上这一轮结束：改成开新的一轮
        assert_eq!(p.idle(), Some(false));
        p.parse_line(r#"{"id":4,"error":{"code":-32600,"message":"no active turn"}}"#);
        let reqs = sent(&mut p);
        assert_eq!(reqs[0]["method"], "turn/start");
        assert_eq!(reqs[0]["params"]["input"][0]["text"], "more");
        p.parse_line(r#"{"id":5,"result":{"turn":{"id":"t2","status":"inProgress"}}}"#);
        p.parse_line(r#"{"method":"turn/completed","params":{"turn":{"id":"t2","status":"failed","error":{"message":"boom"}}}}"#);
        assert_eq!(p.idle(), Some(true));
    }

    #[test]
    fn ignores_subagent_turns_and_retries_rejected_steer_after_turn_ends() {
        let mut p = CodexAppParser::new(&spec());
        p.parse_line(r#"{"id":1,"result":{"thread":{"id":"th1"}}}"#);
        p.parse_line(r#"{"id":2,"result":{"turn":{"id":"t1","status":"inProgress"}}}"#);
        sent(&mut p);
        let ev = p.parse_line(
            r#"{"method":"turn/completed","params":{"threadId":"sub","turn":{"id":"s1","status":"completed"}}}"#,
        );
        assert!(ev.is_empty());
        assert_eq!(p.idle(), Some(false));
        p.steer("late");
        // turn/steer 被拒时 turn/completed 还没到：先留着，这一轮结束后开新的一轮
        p.parse_line(r#"{"id":3,"error":{"code":-32600,"message":"expected turn mismatch"}}"#);
        assert!(sent(&mut p).is_empty());
        p.parse_line(r#"{"method":"turn/completed","params":{"threadId":"th1","turn":{"id":"t1","status":"completed"}}}"#);
        let reqs = sent(&mut p);
        assert_eq!(reqs[0]["method"], "turn/start");
        assert_eq!(reqs[0]["params"]["input"][0]["text"], "late");
        assert_eq!(p.idle(), Some(false));
    }

    #[test]
    fn rejects_server_requests_and_thread_errors() {
        let mut p = CodexAppParser::new(&spec());
        p.parse_line(r#"{"id":"r1","method":"item/commandExecution/requestApproval","params":{}}"#);
        assert!(sent(&mut p)[0]["error"]["code"] == -32601);
        let ev = p.parse_line(r#"{"id":1,"error":{"code":1,"message":"bad thread"}}"#);
        assert!(matches!(&ev[1], AgentEvent::Finished { ok: false, .. }));
        assert_eq!(p.idle(), Some(true));
    }
}
