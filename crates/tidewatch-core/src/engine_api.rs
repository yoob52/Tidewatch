//! 本地接口用到的引擎能力：运行令牌、Agent 委派、Agent 提问。
//! 委派出去的运行和交叉审查一样是工单级运行：只读，在调用方的 worktree 里看，不建分支、不提交。
use super::ticket_flow::{enqueue_ticket_run, plan_ticket_run, TicketPlan};
use super::*;
use crate::agent::McpServer;
use crate::store::delegations;
use crate::store::questions::{self, Question};

/// 运行令牌在 Agent 子进程里的环境变量名。
pub const RUN_TOKEN_ENV: &str = "TIDEWATCH_RUN_TOKEN";
const RUN_TOKEN_PREFIX: &str = "twr_";
/// 委派最多套几层：Agent 派 Agent 再派 Agent。
const MAX_DELEGATE_DEPTH: u32 = 2;
/// 一轮运行最多同时挂着几个没跑完的委派。
const MAX_ACTIVE_DELEGATES: usize = 4;

const ACTIVE: &[&str] = &["starting", "running", "finalizing"];

/// 本地接口的地址，和签发运行令牌用的密钥（每次启动随机生成，只在内存里）。
#[derive(Clone)]
pub struct ApiEndpoint {
    /// `http://127.0.0.1:<端口>`
    pub base_url: String,
    pub secret: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct DelegateInput {
    pub prompt: String,
    pub agent: Option<String>,
    pub provider_id: Option<String>,
    pub model: Option<String>,
    /// 幂等键：同一个调用方重试时返回同一轮运行。
    pub client_request_id: Option<String>,
}

fn sign(secret: &str, run_id: &str) -> String {
    crate::util::sha256_hex(format!("{secret}:{run_id}").as_bytes())
}

/// 委派运行在哪里跑：调用方所在子任务的 worktree；调用方是工单级运行时在项目目录。
pub(super) fn delegate_cwd(
    conn: &rusqlite::Connection,
    run: &Run,
    ticket: &Ticket,
) -> Result<(Project, PathBuf)> {
    let d = delegations::by_run(conn, &run.id)?
        .ok_or_else(|| Error::invalid("找不到这轮委派的调用方。"))?;
    match &d.task_id {
        Some(task_id) => {
            let task = tasks::get(conn, task_id)?;
            let cwd = task
                .worktree_path
                .clone()
                .ok_or_else(|| Error::invalid("调用方子任务的 worktree 已经不在了。"))?;
            Ok((projects::get(conn, &task.project_id)?, PathBuf::from(cwd)))
        }
        None => {
            let project = plan_ticket_run(conn, ticket)?.project;
            let cwd = PathBuf::from(&project.local_path);
            Ok((project, cwd))
        }
    }
}

impl Inner {
    /// 给这一轮运行注入观澜的 MCP 服务：接口开着、Agent 支持 MCP 时才有。
    pub(super) fn mcp_for(&self, run: &Run, kind: AgentKind) -> Option<(McpServer, String)> {
        if !matches!(
            kind,
            AgentKind::ClaudeCode | AgentKind::Codex | AgentKind::OpenCode | AgentKind::Mock
        ) {
            return None;
        }
        Some((
            McpServer {
                url: format!("{}/mcp", self.api.lock().unwrap().clone()?.base_url),
                token_env: RUN_TOKEN_ENV.into(),
            },
            self.run_token(&run.id)?,
        ))
    }

    fn run_token(&self, run_id: &str) -> Option<String> {
        let api = self.api.lock().unwrap().clone()?;
        Some(format!(
            "{RUN_TOKEN_PREFIX}{run_id}_{}",
            sign(&api.secret, run_id)
        ))
    }

    /// 深度优先停掉这一轮委派出去、还没结束的运行。
    pub(super) fn stop_delegated(&self, run_id: &str) {
        let children = delegations::children(&lock(&self.db), run_id).unwrap_or_default();
        for d in children {
            self.stop_delegated(&d.run_id);
            let run = {
                let conn = lock(&self.db);
                let Ok(run) = runs::get(&conn, &d.run_id) else {
                    continue;
                };
                match run.status.as_str() {
                    "queued" => {
                        let _ = runs::set_fields(
                            &conn,
                            &run.id,
                            &[("status", &"stopped"), ("finished_at", &crate::util::now())],
                        );
                        let _ = set_effect(&conn, &run.id, "done");
                    }
                    "starting" | "running" | "finalizing" | "stopping" => {
                        if let Some(flag) = self.cancels.lock().unwrap().get(&run.id) {
                            flag.store(true, Ordering::SeqCst);
                        }
                        let _ = conn.execute(
                            "UPDATE run SET status = 'stopping' WHERE id = ?1 AND status IN ('starting', 'running')",
                            [&run.id],
                        );
                        continue;
                    }
                    _ => continue,
                }
                run
            };
            self.conclude_ticket_run(&run, Err("调用方已结束，委派被停止。"), None);
        }
    }
}

impl Engine {
    /// 本地接口启动 / 关闭时告诉引擎：之后启动的运行才会带上 MCP 服务。
    pub fn set_api(&self, api: Option<ApiEndpoint>) {
        *self.inner.api.lock().unwrap() = api;
    }

    /// 这一轮运行的令牌（接口没开时为空）。只交给这一轮的 Agent 子进程。
    pub fn run_token(&self, run_id: &str) -> Option<String> {
        self.inner.run_token(run_id)
    }

    /// 校验运行令牌：签名要对，而且那一轮还在跑。
    pub fn run_for_token(&self, token: &str) -> Option<Run> {
        let rest = token.strip_prefix(RUN_TOKEN_PREFIX)?;
        let (run_id, sig) = rest.rsplit_once('_')?;
        let api = self.inner.api.lock().unwrap().clone()?;
        let want = sign(&api.secret, run_id);
        // 定长比较，不因为前缀相同而提前返回
        let same = want.len() == sig.len()
            && want
                .bytes()
                .zip(sig.bytes())
                .fold(0u8, |acc, (a, b)| acc | (a ^ b))
                == 0;
        if !same {
            return None;
        }
        let run = runs::get(&lock(&self.inner.db), run_id).ok()?;
        ACTIVE.contains(&run.status.as_str()).then_some(run)
    }

    /// Agent 委派：在调用方的工单下派生一轮只读运行，交给另一个（或同一种）Agent。
    pub fn delegate(&self, parent_run_id: &str, input: DelegateInput) -> Result<Run> {
        let run = {
            let conn = lock(&self.inner.db);
            let parent = runs::get(&conn, parent_run_id)?;
            let request = crate::store::clean(input.client_request_id.clone());
            if let Some(req) = &request {
                if let Some(d) = delegations::by_request(&conn, &parent.id, req)? {
                    return runs::get(&conn, &d.run_id);
                }
            }
            if !ACTIVE.contains(&parent.status.as_str()) {
                return Err(Error::invalid("调用方的运行已经结束，不能再委派。"));
            }
            let prompt = crate::store::required(&input.prompt, "委派的任务说明")?;
            if delegations::depth(&conn, &parent.id)? >= MAX_DELEGATE_DEPTH {
                return Err(Error::invalid(format!(
                    "委派最多套 {MAX_DELEGATE_DEPTH} 层。"
                )));
            }
            let active = delegations::children(&conn, &parent.id)?
                .iter()
                .filter_map(|d| runs::get(&conn, &d.run_id).ok())
                .filter(|r| r.finished_at.is_none())
                .count();
            if active >= MAX_ACTIVE_DELEGATES {
                return Err(Error::invalid(format!(
                    "这一轮已经有 {active} 个委派在跑，等它们结束再派。"
                )));
            }
            let ticket_id = parent
                .ticket_id
                .clone()
                .ok_or_else(|| Error::invalid("调用方的运行不属于任何工单。"))?;
            let ticket = tickets::get(&conn, &ticket_id)?;
            // 调用方自己就是被委派的：沿用它所在的子任务
            let task_id = match &parent.task_id {
                Some(t) => Some(t.clone()),
                None => delegations::by_run(&conn, &parent.id)?.and_then(|d| d.task_id),
            };
            let project = match &task_id {
                Some(t) => projects::get(&conn, &tasks::get(&conn, t)?.project_id)?,
                None => plan_ticket_run(&conn, &ticket)?.project,
            };
            let kind = match crate::store::clean(input.agent.clone()) {
                Some(a) => AgentKind::parse(&a)?,
                None => AgentKind::parse(&parent.agent)?,
            };
            let provider = match crate::store::clean(input.provider_id.clone()) {
                Some(id) => Some(providers::get(&conn, &id)?),
                // 同一种 Agent 沿用调用方的 Provider；换了 Agent 就找第一个协议对得上的
                None if kind.as_str() == parent.agent => parent
                    .provider_id
                    .as_deref()
                    .map(|id| providers::get(&conn, id))
                    .transpose()?,
                None => providers::list(&conn)?.into_iter().find(|p| {
                    p.config().is_ok_and(|c| {
                        c.protocol != crate::provider::Protocol::Native && c.protocol.supports(kind)
                    })
                }),
            };
            if let Some(p) = &provider {
                crate::provider::inject(kind, &p.config()?, None)?;
            }
            let plan = TicketPlan {
                project,
                agent: kind.as_str().into(),
                provider_id: provider.as_ref().map(|p| p.id.clone()),
                model: crate::store::clean(input.model.clone())
                    .or(provider.and_then(|p| p.default_model)),
                reasoning_effort: None,
                resume: None,
                permission: "read_only",
            };
            let prompt = format!(
                "你是被另一个 Agent 委派来协助的。只读地查看当前目录里的代码，不要修改任何文件；完成后用文字给出结论。\n\n## 委派的任务\n\n{prompt}"
            );
            let run = enqueue_ticket_run(&conn, &ticket, &plan, "delegate", &prompt)?;
            delegations::create(
                &conn,
                &parent.id,
                &run.id,
                task_id.as_deref(),
                request.as_deref(),
            )?;
            run
        };
        self.inner
            .changed(run.ticket_id.clone(), None, Some(run.id.clone()));
        self.inner.wake();
        Ok(run)
    }

    /// 被委派的运行的状态；只有调用方自己（或它的上级）能看。
    pub fn delegated_run(&self, parent_run_id: &str, run_id: &str) -> Result<Run> {
        let conn = lock(&self.inner.db);
        let mut cur = run_id.to_string();
        loop {
            match delegations::by_run(&conn, &cur)? {
                Some(d) if d.parent_run_id == parent_run_id => return runs::get(&conn, run_id),
                Some(d) => cur = d.parent_run_id,
                None => return Err(not_found("委派的运行", run_id)),
            }
        }
    }

    /// Agent 把问题挂到工单的"需要你"。`run_id` 为空表示经接口由人或脚本提的。
    pub fn ask_user(
        &self,
        ticket_id: &str,
        run_id: Option<&str>,
        body: &str,
        options: &[String],
        client_request_id: Option<&str>,
    ) -> Result<Question> {
        let (q, ticket) = {
            let conn = lock(&self.inner.db);
            let ticket = tickets::get(&conn, ticket_id)?;
            let task_id = match run_id {
                Some(id) => {
                    let run = runs::get(&conn, id)?;
                    match run.task_id {
                        Some(t) => Some(t),
                        None => delegations::by_run(&conn, id)?.and_then(|d| d.task_id),
                    }
                }
                None => None,
            };
            let before = questions::list_for_ticket(&conn, &ticket.id)?.len();
            let q = questions::ask(
                &conn,
                questions::NewQuestion {
                    ticket_id: &ticket.id,
                    task_id: task_id.as_deref(),
                    run_id,
                    body,
                    options,
                    client_request_id,
                },
            )?;
            // 重试拿回的是旧问题：不再通知一遍
            if questions::list_for_ticket(&conn, &ticket.id)?.len() == before {
                return Ok(q);
            }
            (q, ticket)
        };
        self.inner
            .changed(Some(ticket.id.clone()), q.task_id.clone(), None);
        self.inner.emit(EngineEvent::QuestionAsked {
            question_id: q.id.clone(),
            ticket_id: ticket.id,
            ticket_key: ticket.key,
            body: q.body.clone(),
        });
        Ok(q)
    }

    /// 接口直接改了工单（建单、评论、指派）：让桌面界面跟着刷新。
    pub fn ticket_changed(&self, ticket_id: &str) {
        self.inner.changed(Some(ticket_id.into()), None, None);
    }

    pub fn answer_question(&self, id: &str, answer: &str, by: Option<&str>) -> Result<Question> {
        let q = questions::answer(&lock(&self.inner.db), id, answer, by)?;
        self.inner
            .changed(Some(q.ticket_id.clone()), q.task_id.clone(), None);
        Ok(q)
    }

    pub fn dismiss_question(&self, id: &str) -> Result<Question> {
        let q = questions::dismiss(&lock(&self.inner.db), id)?;
        self.inner
            .changed(Some(q.ticket_id.clone()), q.task_id.clone(), None);
        Ok(q)
    }
}
