//! 工单级的流程：AI 预审、定稿后的拆分建议、变更影响分析，以及把确认后的建议落成子任务。
//! 这三种运行都不属于某个子任务：在项目目录里只读地跑，不建 worktree、不提交。
use super::*;
use crate::store::code_reviews::{self, CodeReview};
use crate::store::reviews::{self, Proposal, TaskDraft, TaskImpact};
use std::collections::{HashMap as Map, HashSet};

/// 定稿的结果：工单，以及随之入队的拆分 / 影响分析运行（没有则为空）。
#[derive(Debug, Clone, Serialize)]
pub struct Finalized {
    pub ticket: Ticket,
    pub run: Option<Run>,
}

/// 启动时收拾上次退出留下的半截流程：预审没跑完的工单回到人工审核，没出结果的建议记为失败。
pub(super) fn recover(conn: &mut rusqlite::Connection) -> Result<()> {
    const MSG: &str = "观澜上次退出时这一步还没跑完，请重新运行。";
    let stuck: Vec<(String, Option<String>)> = {
        let mut st = conn.prepare(
            "SELECT t.id, t.current_version_id FROM ticket t WHERE t.status = 'ai_review' AND NOT EXISTS
               (SELECT 1 FROM run r WHERE r.ticket_id = t.id AND r.task_id IS NULL
                AND r.status IN ('queued', 'starting', 'running', 'finalizing', 'stopping'))",
        )?;
        let rows = st
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows
    };
    for (ticket_id, version) in stuck {
        reviews::save_result(conn, &ticket_id, version.as_deref(), "", Err(MSG))?;
        tickets::transition(conn, &ticket_id, "human_review")?;
    }
    conn.execute(
        "UPDATE code_review SET status = 'failed', error = ?1, updated_at = ?2 WHERE status = 'running' AND run_id NOT IN
           (SELECT id FROM run WHERE status IN ('queued', 'starting', 'running', 'finalizing', 'stopping'))",
        rusqlite::params![MSG, crate::util::now()],
    )?;
    conn.execute(
        "UPDATE proposal SET status = 'failed', error = ?1, updated_at = ?2 WHERE status = 'running' AND run_id NOT IN
           (SELECT id FROM run WHERE status IN ('queued', 'starting', 'running', 'finalizing', 'stopping'))",
        rusqlite::params![MSG, crate::util::now()],
    )?;
    Ok(())
}

/// 定稿后还有没确认的拆分建议 / 影响分析时，不能直接开始开发。
pub(super) fn check_no_open_proposal(conn: &rusqlite::Connection, ticket_id: &str) -> Result<()> {
    let open: Option<(String, String)> = {
        use rusqlite::OptionalExtension;
        conn.query_row(
            "SELECT kind, status FROM proposal WHERE ticket_id = ?1 AND status IN ('running', 'pending') LIMIT 1",
            [ticket_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
    };
    match open {
        None => Ok(()),
        Some((kind, status)) => {
            let what = if kind == "impact" {
                "变更影响分析"
            } else {
                "拆分建议"
            };
            Err(Error::invalid(if status == "running" {
                format!("{what}还在生成，出来并确认后才能开始开发。")
            } else {
                format!("{what}还没有确认；确认或放弃后才能开始开发。")
            }))
        }
    }
}

/// 工单级运行在哪个项目里跑、由谁来跑。
struct TicketPlan {
    project: Project,
    agent: String,
    provider_id: Option<String>,
    model: Option<String>,
}

fn plan_ticket_run(conn: &rusqlite::Connection, ticket: &Ticket) -> Result<TicketPlan> {
    // 优先在后端项目里看代码；只绑了一个项目就用它
    let bound = ticket
        .projects
        .iter()
        .find(|p| p.role == "backend")
        .or(ticket.projects.first())
        .ok_or_else(|| {
            Error::invalid("工单还没有绑定项目。AI 要在项目目录里只读地查看代码，请先绑定项目。")
        })?;
    let project = projects::get(conn, &bound.project_id)?;
    let st = settings::get(conn)?;
    let agent =
        first([&ticket.default_agent, &project.default_agent]).unwrap_or(st.default_agent.clone());
    let kind = AgentKind::parse(&agent)?;
    let provider_id = first([
        &ticket.default_provider_id,
        &project.default_provider_id,
        &st.default_provider_id,
    ]);
    let provider = provider_id
        .as_deref()
        .map(|id| providers::get(conn, id))
        .transpose()?;
    if let Some(p) = &provider {
        crate::provider::inject(kind, &p.config()?, None)?;
    }
    Ok(TicketPlan {
        project,
        agent: kind.as_str().into(),
        provider_id,
        model: provider.and_then(|p| p.default_model),
    })
}

fn enqueue_ticket_run(
    conn: &rusqlite::Connection,
    ticket: &Ticket,
    plan: &TicketPlan,
    purpose: &str,
    prompt: &str,
) -> Result<Run> {
    let run = runs::enqueue(
        conn,
        runs::NewRun {
            task_id: None,
            ticket_id: &ticket.id,
            purpose,
            parent_run_id: None,
            agent: &plan.agent,
            provider_id: plan.provider_id.as_deref(),
            model: plan.model.as_deref(),
            permission: "read_only",
            resume_session_id: None,
            prompt,
        },
    )?;
    let now = crate::util::now();
    conn.execute(
        "INSERT INTO pending_effect (id, kind, payload_json, status, attempts, created_at, updated_at)
         VALUES (?1, 'run', ?2, 'pending', 0, ?3, ?3)",
        rusqlite::params![run.id, json!({ "run_id": run.id }).to_string(), now],
    )?;
    add_timeline(
        conn,
        &ticket.id,
        "run_queued",
        json!({ "run_id": run.id, "purpose": purpose, "number": run.number, "agent": run.agent }),
    )?;
    Ok(run)
}

fn project_lines(conn: &rusqlite::Connection, ticket: &Ticket) -> Result<Vec<String>> {
    let mut out = vec![];
    for bound in &ticket.projects {
        let p = projects::get(conn, &bound.project_id)?;
        out.push(format!(
            "{} | kind={} | role={}",
            p.name, p.kind, bound.role
        ));
    }
    Ok(out)
}

fn task_line(t: &Task) -> String {
    format!("{} | {} | {} | {}", t.id, t.kind, t.status, t.title)
}

fn prd_text(conn: &rusqlite::Connection, version_id: Option<&str>) -> String {
    version_id
        .and_then(|v| tickets::prd_version(conn, v).ok())
        .map(|v| v.content_md)
        .unwrap_or_default()
}

/// 拆分或影响分析的 Prompt；定稿后调用。返回 (类别, Prompt)。
fn proposal_prompt(
    conn: &rusqlite::Connection,
    ticket: &Ticket,
    plan: &TicketPlan,
) -> Result<(&'static str, String)> {
    let ov = crate::prompt::load_overrides(conn)?;
    let prd = prd_text(conn, ticket.finalized_version_id.as_deref());
    let existing: Vec<Task> = tasks::list_for_ticket(conn, &ticket.id)?
        .into_iter()
        .filter(|t| t.status != "cancelled")
        .collect();
    if ticket.change_from_version_id.is_some() && !existing.is_empty() {
        let old = prd_text(conn, ticket.change_from_version_id.as_deref());
        let lines: Vec<String> = existing.iter().map(task_line).collect();
        Ok((
            "impact",
            crate::prompt::impact_prompt(
                &ov,
                ticket.change_reason.as_deref().unwrap_or(""),
                (&old, &tickets::change_from_criteria(conn, &ticket.id)?),
                (&prd, &ticket.acceptance_criteria),
                &lines,
            ),
        ))
    } else {
        Ok((
            "split",
            crate::prompt::split_prompt(
                &ov,
                &plan.project,
                ticket,
                &prd,
                &project_lines(conn, ticket)?,
            ),
        ))
    }
}

/// 交叉审查由谁来做。都不填时换一个和写代码的不同的 Agent。
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CodeReviewOptions {
    pub agent: Option<String>,
    pub provider_id: Option<String>,
}

/// 选审查者：指定了就用指定的；否则换成另一种 Agent，并找一个它能用的 Provider。
fn plan_code_review(
    conn: &rusqlite::Connection,
    task: &Task,
    project: Project,
    opts: &CodeReviewOptions,
) -> Result<TicketPlan> {
    let author = runs::list_for_task(conn, &task.id)?.pop();
    let author_kind = author
        .as_ref()
        .and_then(|r| AgentKind::parse(&r.agent).ok());
    let kind = match crate::store::clean(opts.agent.clone()) {
        Some(a) => AgentKind::parse(&a)?,
        None => match author_kind {
            Some(AgentKind::ClaudeCode) => AgentKind::Codex,
            Some(AgentKind::Mock) | None => AgentKind::Mock,
            Some(_) => AgentKind::ClaudeCode,
        },
    };
    let provider = match crate::store::clean(opts.provider_id.clone()) {
        Some(id) => Some(providers::get(conn, &id)?),
        None => {
            let same = author
                .as_ref()
                .filter(|_| author_kind == Some(kind))
                .and_then(|r| r.provider_id.clone());
            match same {
                Some(id) => providers::get(conn, &id).ok(),
                // 换了 Agent：找第一个协议对得上、且不是"官方默认登录"的 Provider；没有就用它自己的登录
                None => providers::list(conn)?.into_iter().find(|p| {
                    p.config().is_ok_and(|c| {
                        c.protocol != crate::provider::Protocol::Native && c.protocol.supports(kind)
                    })
                }),
            }
        }
    };
    if let Some(p) = &provider {
        crate::provider::inject(kind, &p.config()?, None)?;
    }
    Ok(TicketPlan {
        project,
        agent: kind.as_str().into(),
        provider_id: provider.as_ref().map(|p| p.id.clone()),
        model: provider.and_then(|p| p.default_model),
    })
}

impl Engine {
    /// AI 交叉审查：让另一个 Agent 在子任务的 worktree 里只读地看 Diff，给出问题列表。
    /// 结果只供参考，不改变子任务的状态。
    pub fn start_code_review(&self, task_id: &str, opts: CodeReviewOptions) -> Result<Run> {
        self.inner.start_code_review(task_id, &opts)
    }

    pub fn code_reviews(&self, task_id: &str) -> Result<Vec<CodeReview>> {
        code_reviews::list_for_task(&lock(&self.inner.db), task_id)
    }

    /// 提交 AI 预审：工单进入"AI 预审中"，预审运行入队。结束后（无论成败）回到人工审核。
    pub fn start_review(&self, ticket_id: &str) -> Result<Run> {
        let run = {
            let mut conn = lock(&self.inner.db);
            let ticket = tickets::get(&conn, ticket_id)?;
            if ticket.status == "ai_review" {
                return Err(Error::invalid("这张工单的预审正在进行。"));
            }
            if !matches!(ticket.status.as_str(), "draft" | "human_review") {
                return Err(Error::invalid(format!(
                    "工单处于「{}」，不能提交预审。",
                    workflow::ticket_status_label(&ticket.status)
                )));
            }
            let plan = plan_ticket_run(&conn, &ticket)?;
            let prd = tickets::current_prd(&conn, &ticket.id)?
                .map(|v| v.content_md)
                .unwrap_or_default();
            if prd.trim().is_empty() {
                return Err(Error::invalid("正文是空的，没有可以预审的内容。"));
            }
            let open: Vec<String> = reviews::issues(&conn, &ticket.id)?
                .into_iter()
                .filter(|i| reviews::is_unresolved(&i.resolution))
                .map(|i| format!("[{}][{}] {}", i.code, i.severity, i.problem))
                .collect();
            let ov = crate::prompt::load_overrides(&conn)?;
            let prompt = crate::prompt::review_prompt(&ov, &plan.project, &ticket, &prd, &open);
            tickets::transition(&mut conn, &ticket.id, "ai_review")?;
            let tx = conn.transaction()?;
            let run = enqueue_ticket_run(&tx, &ticket, &plan, "review", &prompt)?;
            tx.commit()?;
            run
        };
        self.inner
            .changed(run.ticket_id.clone(), None, Some(run.id.clone()));
        self.inner.wake();
        Ok(run)
    }

    /// 定稿：校验（没有未处理的阻塞级问题、验收标准不为空）并锁定当前版本。
    /// 变更后再次定稿且已有子任务时，入队"影响分析"；否则 `split` 为真时入队"拆分"。
    pub fn finalize(&self, ticket_id: &str, split: bool) -> Result<Finalized> {
        let (ticket, run) = {
            let mut conn = lock(&self.inner.db);
            let before = tickets::get(&conn, ticket_id)?;
            let has_tasks = tasks::list_for_ticket(&conn, &before.id)?
                .iter()
                .any(|t| t.status != "cancelled");
            let impact = before.change_from_version_id.is_some() && has_tasks;
            // 要跑 AI 的话，跑不了的原因（没绑项目、Provider 不匹配）在定稿前就报出来
            let plan = if impact || split {
                Some(plan_ticket_run(&conn, &before)?)
            } else {
                None
            };
            let ticket = tickets::transition(&mut conn, &before.id, "finalized")?;
            add_timeline(
                &conn,
                &ticket.id,
                "finalized",
                json!({ "version_id": ticket.finalized_version_id }),
            )?;
            if !impact {
                tickets::clear_change(&conn, &ticket.id)?;
            }
            let run = match plan {
                Some(plan) => {
                    let (kind, prompt) = proposal_prompt(&conn, &ticket, &plan)?;
                    let tx = conn.transaction()?;
                    let run = enqueue_ticket_run(&tx, &ticket, &plan, kind, &prompt)?;
                    reviews::begin_proposal(
                        &tx,
                        &ticket.id,
                        kind,
                        ticket.finalized_version_id.as_deref(),
                        &run.id,
                    )?;
                    tx.commit()?;
                    Some(run)
                }
                None => None,
            };
            (tickets::get(&conn, &ticket.id)?, run)
        };
        self.inner.changed(
            Some(ticket.id.clone()),
            None,
            run.as_ref().map(|r| r.id.clone()),
        );
        self.inner.wake();
        Ok(Finalized { ticket, run })
    }

    /// 重新生成拆分建议 / 影响分析（上一份失败了，或者不满意）。
    pub fn regenerate_proposal(&self, ticket_id: &str) -> Result<Run> {
        let run = {
            let mut conn = lock(&self.inner.db);
            let ticket = tickets::get(&conn, ticket_id)?;
            if ticket.status != "finalized" {
                return Err(Error::invalid(
                    "只有已定稿、还没开始开发的工单可以重新生成建议。",
                ));
            }
            let running: i64 = conn.query_row(
                "SELECT COUNT(*) FROM proposal WHERE ticket_id = ?1 AND status = 'running'",
                [&ticket.id],
                |r| r.get(0),
            )?;
            if running > 0 {
                return Err(Error::invalid("建议正在生成中。"));
            }
            let plan = plan_ticket_run(&conn, &ticket)?;
            let (kind, prompt) = proposal_prompt(&conn, &ticket, &plan)?;
            let tx = conn.transaction()?;
            let run = enqueue_ticket_run(&tx, &ticket, &plan, kind, &prompt)?;
            reviews::begin_proposal(
                &tx,
                &ticket.id,
                kind,
                ticket.finalized_version_id.as_deref(),
                &run.id,
            )?;
            tx.commit()?;
            run
        };
        self.inner
            .changed(run.ticket_id.clone(), None, Some(run.id.clone()));
        self.inner.wake();
        Ok(run)
    }

    /// 放弃这份建议（改为手动建子任务，或认为变更不影响已有子任务）。
    pub fn discard_proposal(&self, proposal_id: &str) -> Result<()> {
        let ticket_id = {
            let conn = lock(&self.inner.db);
            let p = reviews::proposal(&conn, proposal_id)?;
            if p.status == "running" {
                return Err(Error::invalid("建议还在生成，先停止那一轮运行。"));
            }
            if p.status == "applied" {
                return Err(Error::invalid("这份建议已经确认过了。"));
            }
            reviews::set_proposal_status(&conn, &p.id, "discarded")?;
            if p.kind == "impact" {
                tickets::clear_change(&conn, &p.ticket_id)?;
            }
            add_timeline(
                &conn,
                &p.ticket_id,
                "proposal_discarded",
                json!({ "kind": p.kind }),
            )?;
            p.ticket_id
        };
        self.inner.changed(Some(ticket_id), None, None);
        Ok(())
    }

    /// 确认拆分：按（人改过的）建议创建子任务和依赖。
    pub fn apply_split(&self, ticket_id: &str, drafts: Vec<TaskDraft>) -> Result<Vec<Task>> {
        let (ticket, created, auto) = {
            let mut conn = lock(&self.inner.db);
            let ticket = tickets::get(&conn, ticket_id)?;
            if ticket.status != "finalized" {
                return Err(Error::invalid(format!(
                    "工单处于「{}」，只有已定稿的工单可以确认拆分。",
                    workflow::ticket_status_label(&ticket.status)
                )));
            }
            if drafts.is_empty() {
                return Err(Error::invalid("至少要有一个子任务。"));
            }
            let created = create_drafts(&mut conn, &ticket, &drafts)?;
            if let Some(p) = reviews::latest_proposal(&conn, &ticket.id, "split")? {
                if p.status != "applied" {
                    reviews::set_proposal_status(&conn, &p.id, "applied")?;
                }
            }
            add_timeline(
                &conn,
                &ticket.id,
                "split_applied",
                json!({ "tasks": created.iter().map(|t| &t.id).collect::<Vec<_>>() }),
            )?;
            let auto = settings::get(&conn)?.auto_develop;
            (ticket, created, auto)
        };
        self.inner.changed(Some(ticket.id.clone()), None, None);
        if auto {
            if let Err(e) = self.start_development(&ticket.id) {
                eprintln!("自动开始开发失败：{e}");
            }
        }
        let conn = lock(&self.inner.db);
        created.iter().map(|t| tasks::get(&conn, &t.id)).collect()
    }

    /// 确认影响分析：受影响的子任务追加工作，需要的话新增子任务。变更到此处理完。
    pub fn apply_impact(
        &self,
        ticket_id: &str,
        impacts: Vec<TaskImpact>,
        new_tasks: Vec<TaskDraft>,
    ) -> Result<Vec<Task>> {
        let (ticket, auto) = {
            let mut conn = lock(&self.inner.db);
            let ticket = tickets::get(&conn, ticket_id)?;
            if ticket.status != "finalized" {
                return Err(Error::invalid(format!(
                    "工单处于「{}」，只有变更后再次定稿的工单可以确认影响分析。",
                    workflow::ticket_status_label(&ticket.status)
                )));
            }
            let rework: Vec<(Task, String)> = impacts
                .iter()
                .filter(|i| i.impact == "rework")
                .map(|i| {
                    let task = tasks::get(&conn, &i.task_id)?;
                    if task.ticket_id != ticket.id {
                        return Err(Error::invalid("影响分析里有不属于这张工单的子任务。"));
                    }
                    let instruction = i.instruction.trim().to_string();
                    if instruction.is_empty() {
                        return Err(Error::invalid(format!(
                            "子任务「{}」标了要追加工作，但没有写要做什么。",
                            task.title
                        )));
                    }
                    if matches!(task.status.as_str(), "queued" | "running") {
                        return Err(Error::invalid(format!(
                            "子任务「{}」还在运行，等它结束或先停止，再确认影响分析。",
                            task.title
                        )));
                    }
                    Ok((task, instruction))
                })
                .collect::<Result<_>>()?;
            let reason = ticket.change_reason.clone().unwrap_or_default();
            // 已合并的子任务 worktree 已清理，另开一个子任务做增量
            let mut follow_ups: Vec<TaskDraft> = rework
                .iter()
                .filter(|(task, _)| task.status == "merged")
                .map(|(task, instruction)| TaskDraft {
                    r#ref: format!("follow-{}", task.id),
                    kind: task.kind.clone(),
                    project: task.project_id.clone(),
                    title: format!("变更追加：{}", task.title),
                    spec: instruction.clone(),
                    ..Default::default()
                })
                .collect();
            follow_ups.extend(new_tasks);
            // 新子任务先整体校验，再动已有子任务：校验不过时什么都不改
            let plan = plan_drafts(&conn, &ticket, &follow_ups)?;
            for (task, instruction) in &rework {
                match task.status.as_str() {
                    "cancelled" | "merged" => {}
                    // 还没开始：说明里补上变更，首轮就按新需求做
                    "pending" | "blocked" => {
                        conn.execute(
                            "UPDATE task SET spec_md = ?2, updated_at = ?3 WHERE id = ?1",
                            rusqlite::params![
                                task.id,
                                format!(
                                    "{}\n\n### 变更追加\n{instruction}\n",
                                    task.spec_md.trim_end()
                                ),
                                crate::util::now()
                            ],
                        )?;
                    }
                    // 做过了：在原 worktree 里增量返工
                    _ => {
                        tasks::set_status(&conn, &task.id, "changes_requested")?;
                        // 需求变了，旧的接口文档不再算数：等它的子任务要等返工重新写出文档
                        tasks::remove_artifact(&conn, &task.id, "api_doc")?;
                        add_timeline(
                            &conn,
                            &ticket.id,
                            "task_rejected",
                            json!({
                                "task_id": task.id,
                                "reason": "change",
                                "note": instruction,
                                "instruction": format!("需求在定稿后发生了变更（{reason}）。请在已有改动的基础上追加：\n{instruction}"),
                            }),
                        )?;
                    }
                }
            }
            create_planned(&mut conn, &ticket, &follow_ups, &plan)?;
            tasks::refresh_blocked(&conn, &ticket.id)?;
            tickets::clear_change(&conn, &ticket.id)?;
            if let Some(p) = reviews::latest_proposal(&conn, &ticket.id, "impact")? {
                reviews::set_proposal_status(&conn, &p.id, "applied")?;
            }
            add_timeline(
                &conn,
                &ticket.id,
                "impact_applied",
                json!({ "rework": rework.iter().map(|(t, _)| &t.id).collect::<Vec<_>>(), "new_tasks": follow_ups.len() }),
            )?;
            (ticket, settings::get(&conn)?.auto_develop)
        };
        self.inner.changed(Some(ticket.id.clone()), None, None);
        if auto {
            if let Err(e) = self.start_development(&ticket.id) {
                eprintln!("自动开始开发失败：{e}");
            }
        }
        tasks::list_for_ticket(&lock(&self.inner.db), &ticket.id)
    }

    /// 开始开发：能开始的子任务（待开始、已打回）全部入队；等依赖的在依赖满足后自动入队。
    /// 权限是 full_auto 的子任务不在这里入队：它每次开始都要用户单独确认。
    pub fn start_development(&self, ticket_id: &str) -> Result<Vec<Run>> {
        let startable: Vec<String> = {
            let mut conn = lock(&self.inner.db);
            let ticket = tickets::get(&conn, ticket_id)?;
            if !matches!(ticket.status.as_str(), "finalized" | "developing") {
                return Err(Error::invalid(format!(
                    "工单处于「{}」，还不能开始开发。",
                    workflow::ticket_status_label(&ticket.status)
                )));
            }
            check_no_open_proposal(&conn, &ticket.id)?;
            let all = tasks::list_for_ticket(&conn, &ticket.id)?;
            if all.iter().all(|t| t.status == "cancelled") {
                return Err(Error::invalid(
                    "还没有子任务。先确认拆分建议，或手动添加子任务。",
                ));
            }
            let default_permission = settings::get(&conn)?.default_permission;
            let mut startable: Vec<String> = vec![];
            let mut waiting = 0;
            let mut deferred = false;
            for t in all
                .iter()
                .filter(|t| matches!(t.status.as_str(), "pending" | "changes_requested"))
            {
                if super::needs_confirmation(t, &default_permission) {
                    waiting += 1;
                } else if tasks::unmet_dependency(&conn, t)?.is_some() {
                    // 返工的子任务还在等它依赖的那个返工完：依赖满足后再入队
                    tasks::set_start_when_ready(&conn, &t.id, true)?;
                    waiting += 1;
                    deferred = true;
                } else {
                    startable.push(t.id.clone());
                }
            }
            if startable.is_empty() && waiting == 0 && ticket.status == "finalized" {
                // 变更没有影响到任何子任务：直接回到开发 / 验收
                tickets::transition(&mut conn, &ticket.id, "developing")?;
                if workflow::all_tasks_ready_for_acceptance(all.iter().map(|t| t.status.as_str())) {
                    tickets::transition(&mut conn, &ticket.id, "acceptance")?;
                }
            } else if startable.is_empty() && deferred && ticket.status == "finalized" {
                // 只有等依赖的返工：工单先回到开发中，依赖满足后它们才会自动入队
                tickets::transition(&mut conn, &ticket.id, "developing")?;
            }
            startable
        };
        let mut started = vec![];
        for id in &startable {
            started.push(self.inner.enqueue_task(id, StartOptions::default())?);
        }
        self.inner.changed(Some(ticket_id.into()), None, None);
        Ok(started)
    }

    /// 发起变更：定稿后的工单回到人工审核。`stop_running` 为真时停掉正在跑的子任务，否则让它们跑完。
    pub fn request_change(
        &self,
        ticket_id: &str,
        reason: &str,
        stop_running: bool,
    ) -> Result<Ticket> {
        let (ticket, active) = {
            let mut conn = lock(&self.inner.db);
            let ticket = tickets::request_change(&mut conn, ticket_id, reason)?;
            tasks::clear_start_when_ready(&conn, &ticket.id)?;
            // 按旧版需求生成、还没确认的建议作废
            conn.execute(
                "UPDATE proposal SET status = 'discarded', updated_at = ?2
                 WHERE ticket_id = ?1 AND status IN ('pending', 'failed')",
                rusqlite::params![ticket.id, crate::util::now()],
            )?;
            let active: Vec<String> = runs::list_for_ticket(&conn, &ticket.id)?
                .into_iter()
                .filter(|r| runs::ACTIVE.contains(&r.status.as_str()))
                .filter(|r| stop_running || r.task_id.is_none())
                .map(|r| r.id)
                .collect();
            (ticket, active)
        };
        for run_id in active {
            if let Err(e) = self.stop_run(&run_id) {
                eprintln!("停止运行 {run_id} 失败：{e}");
            }
        }
        self.inner.changed(Some(ticket.id.clone()), None, None);
        tickets::get(&lock(&self.inner.db), &ticket.id)
    }
}

/// 校验过的一批子任务草稿：每个草稿落在哪个项目，以及依赖在前的创建顺序。
struct DraftPlan {
    project_ids: Vec<String>,
    order: Vec<usize>,
}

/// 把建议里的子任务按依赖顺序建出来。`depends_on` 可以写同一批里的 ref，也可以写已有子任务的 id。
fn create_drafts(
    conn: &mut rusqlite::Connection,
    ticket: &Ticket,
    drafts: &[TaskDraft],
) -> Result<Vec<Task>> {
    let plan = plan_drafts(conn, ticket, drafts)?;
    create_planned(conn, ticket, drafts, &plan)
}

/// 整体校验一批草稿（标题、类型、项目、依赖、有没有环），不写库：有问题时一个子任务都不建。
fn plan_drafts(
    conn: &rusqlite::Connection,
    ticket: &Ticket,
    drafts: &[TaskDraft],
) -> Result<DraftPlan> {
    let all_projects = projects::list(conn)?;
    let existing: HashSet<String> = tasks::list_for_ticket(conn, &ticket.id)?
        .into_iter()
        .map(|t| t.id)
        .collect();
    let mut refs = HashSet::new();
    let mut project_ids = vec![];
    for (n, d) in drafts.iter().enumerate() {
        let label = if d.title.trim().is_empty() {
            format!("第 {} 个子任务", n + 1)
        } else {
            format!("子任务「{}」", d.title.trim())
        };
        if d.title.trim().is_empty() {
            return Err(Error::invalid(format!("{label}没有标题。")));
        }
        if !tasks::KINDS.contains(&d.kind.trim()) {
            return Err(Error::invalid(format!("{label}的类型不对：{}", d.kind)));
        }
        if let Some(agent) = d.agent.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
            AgentKind::parse(agent)?;
        }
        let wanted = d.project.trim();
        let project = all_projects
            .iter()
            .find(|p| p.id == wanted)
            .or_else(|| {
                all_projects
                    .iter()
                    .find(|p| p.name.eq_ignore_ascii_case(wanted))
            })
            .ok_or_else(|| {
                Error::invalid(format!(
                    "{label}指定的项目不存在：{}",
                    if wanted.is_empty() {
                        "（未指定）"
                    } else {
                        wanted
                    }
                ))
            })?;
        project_ids.push(project.id.clone());
        if !d.r#ref.trim().is_empty() && !refs.insert(d.r#ref.trim().to_string()) {
            return Err(Error::invalid(format!("子任务编号重复：{}", d.r#ref)));
        }
    }
    for d in drafts {
        for dep in &d.depends_on {
            let dep = dep.trim();
            if dep == d.r#ref.trim() || !(refs.contains(dep) || existing.contains(dep)) {
                return Err(Error::invalid(format!(
                    "子任务「{}」依赖的 {dep} 不存在。",
                    d.title.trim()
                )));
            }
        }
    }
    // 依赖在前
    let mut placed: HashSet<&str> = HashSet::new();
    let mut order: Vec<usize> = vec![];
    while order.len() < drafts.len() {
        let before = order.len();
        for (n, d) in drafts.iter().enumerate() {
            if order.contains(&n) {
                continue;
            }
            let ready = d
                .depends_on
                .iter()
                .all(|dep| placed.contains(dep.trim()) || existing.contains(dep.trim()));
            if ready {
                order.push(n);
                if !d.r#ref.trim().is_empty() {
                    placed.insert(d.r#ref.trim());
                }
            }
        }
        if order.len() == before {
            return Err(Error::invalid(
                "子任务之间的依赖成了环，请去掉其中一条依赖。",
            ));
        }
    }
    Ok(DraftPlan { project_ids, order })
}

fn create_planned(
    conn: &mut rusqlite::Connection,
    ticket: &Ticket,
    drafts: &[TaskDraft],
    plan: &DraftPlan,
) -> Result<Vec<Task>> {
    let mut created: Map<String, String> = Map::new();
    let mut out: Vec<Option<Task>> = vec![None; drafts.len()];
    for &n in &plan.order {
        let d = &drafts[n];
        let deps: Vec<String> = d
            .depends_on
            .iter()
            .map(|dep| {
                created
                    .get(dep.trim())
                    .cloned()
                    .unwrap_or(dep.trim().to_string())
            })
            .collect();
        let task = tasks::create(
            conn,
            tasks::TaskInput {
                ticket_id: ticket.id.clone(),
                kind: d.kind.trim().into(),
                project_id: plan.project_ids[n].clone(),
                title: d.title.trim().into(),
                spec_md: d.spec.clone(),
                acceptance_criteria: d.acceptance_criteria.clone(),
                agent: d.agent.clone(),
                api_doc_deps: if d.dependency_kind.as_deref() == Some("api_doc") {
                    deps.clone()
                } else {
                    vec![]
                },
                depends_on: deps,
                ..Default::default()
            },
        )?;
        if !d.r#ref.trim().is_empty() {
            created.insert(d.r#ref.trim().to_string(), task.id.clone());
        }
        out[n] = Some(task);
    }
    Ok(out.into_iter().flatten().collect())
}

impl Inner {
    pub(super) fn start_code_review(&self, task_id: &str, opts: &CodeReviewOptions) -> Result<Run> {
        let run = {
            let mut conn = lock(&self.db);
            let task = tasks::get(&conn, task_id)?;
            if !matches!(
                task.status.as_str(),
                "review" | "accepted" | "changes_requested" | "failed" | "stopped"
            ) {
                return Err(Error::invalid(format!(
                    "子任务处于「{}」，有了改动并且不在运行时才能审查。",
                    workflow::task_status_label(&task.status)
                )));
            }
            if code_reviews::running_for_task(&conn, task_id)? {
                return Err(Error::invalid("这个子任务的交叉审查正在进行。"));
            }
            let (Some(worktree), Some(base)) = (
                task.worktree_path
                    .as_ref()
                    .map(PathBuf::from)
                    .filter(|p| p.is_dir()),
                task.base_commit.clone(),
            ) else {
                return Err(Error::invalid(
                    "子任务的 worktree 已经不在了，没有可以审查的改动。",
                ));
            };
            let head = git::rev_parse(&worktree, "HEAD")?;
            if head == base {
                return Err(Error::invalid("子任务还没有提交任何改动。"));
            }
            let ticket = tickets::get(&conn, &task.ticket_id)?;
            let project = projects::get(&conn, &task.project_id)?;
            let plan = plan_code_review(&conn, &task, project, opts)?;
            let prd = tickets::current_prd(&conn, &ticket.id)?
                .map(|v| v.content_md)
                .unwrap_or_default();
            let ov = crate::prompt::load_overrides(&conn)?;
            let prompt =
                crate::prompt::code_review_prompt(&ov, &plan.project, &ticket, &task, &prd, &base);
            let tx = conn.transaction()?;
            let run = enqueue_ticket_run(&tx, &ticket, &plan, "code_review", &prompt)?;
            code_reviews::create(
                &tx,
                code_reviews::NewCodeReview {
                    task_id: &task.id,
                    ticket_id: &ticket.id,
                    run_id: &run.id,
                    head_commit: Some(&head),
                    agent: &plan.agent,
                    model: plan.model.as_deref(),
                },
            )?;
            tx.commit()?;
            run
        };
        self.changed(
            run.ticket_id.clone(),
            Some(task_id.into()),
            Some(run.id.clone()),
        );
        self.wake();
        Ok(run)
    }

    /// 跑一轮工单级运行（预审 / 拆分 / 影响分析 / 交叉审查），把结果写回。
    pub(super) fn execute_ticket_run(&self, run: &Run, cancel: &AtomicBool) {
        let res = self.ticket_run_steps(run, cancel);
        let stopped = cancel.load(Ordering::SeqCst);
        let (status, error) = match &res {
            Ok(_) => ("succeeded", None),
            Err(e) if stopped => ("stopped", Some(e.to_string())),
            Err(e) => ("failed", Some(e.to_string())),
        };
        if let Some(e) = &error {
            self.log(&run.id, "error", json!({ "message": e }));
        }
        // 运行的终态和它的结论（预审结果 / 建议、工单状态）在同一次持锁里写入：
        // 否则界面可能读到"运行已成功、工单还在预审中"的中间状态。
        let finish = Some((status, error.as_deref()));
        match &res {
            Ok(text) => self.conclude_ticket_run(run, Ok(text), finish),
            Err(e) => self.conclude_ticket_run(run, Err(&e.to_string()), finish),
        }
        self.log(&run.id, "status", json!({ "status": status }));
    }

    fn ticket_run_steps(&self, run: &Run, cancel: &AtomicBool) -> Result<String> {
        let ticket_id = run.ticket_id.clone().unwrap_or_default();
        let (project, cwd, st, provider) = {
            let conn = lock(&self.db);
            let ticket = tickets::get(&conn, &ticket_id)?;
            // 交叉审查在子任务的 worktree 里看；其它在项目目录里看
            let (project, cwd) = if run.purpose == "code_review" {
                let cr = code_reviews::by_run(&conn, &run.id)?
                    .ok_or_else(|| Error::invalid("找不到这轮审查对应的子任务。"))?;
                let task = tasks::get(&conn, &cr.task_id)?;
                let cwd = task.worktree_path.clone().ok_or_else(|| {
                    Error::invalid("子任务的 worktree 已经不在了，没有可以审查的改动。")
                })?;
                (projects::get(&conn, &task.project_id)?, PathBuf::from(cwd))
            } else {
                let project = plan_ticket_run(&conn, &ticket)?.project;
                let cwd = PathBuf::from(&project.local_path);
                (project, cwd)
            };
            let provider = run
                .provider_id
                .as_deref()
                .map(|id| providers::get(&conn, id))
                .transpose()?;
            (project, cwd, settings::get(&conn)?, provider)
        };
        if cancel.load(Ordering::SeqCst) {
            return Err(Error::invalid("在 Agent 启动前被停止。"));
        }
        let kind = AgentKind::parse(&run.agent)?;
        if !cwd.is_dir() {
            return Err(Error::invalid(format!("目录不存在：{}", cwd.display())));
        }
        let outcome = self.agent_phase(
            run,
            kind,
            &cwd,
            &project,
            &st,
            &provider,
            cancel,
            &mut || {},
        )?;
        if outcome.stopped {
            return Err(Error::invalid("已停止。"));
        }
        if !outcome.succeeded() {
            let detail = if outcome.timed_out {
                "运行超时".to_string()
            } else if let Some(reason) = &outcome.abort_reason {
                reason.clone()
            } else if !outcome.summary.trim().is_empty() {
                outcome.summary.trim().to_string()
            } else {
                outcome.stderr_tail.trim().to_string()
            };
            return Err(Error::invalid(format!(
                "Agent 没有正常结束（退出码 {:?}）：{detail}",
                outcome.exit_code
            )));
        }
        Ok(outcome.summary)
    }

    /// 工单级运行结束（或没开始就被停掉）之后：预审结果 / 建议入库，工单离开"AI 预审中"，通知界面。
    ///
    /// `finish` 是运行自己的终态（状态、错误）；排队中就被撤掉的运行已经写过终态，传 `None`。
    pub(super) fn conclude_ticket_run(
        &self,
        run: &Run,
        output: std::result::Result<&str, &str>,
        finish: Option<(&str, Option<&str>)>,
    ) {
        let ticket_id = run.ticket_id.clone().unwrap_or_default();
        let mut task_id = None;
        let res: Result<EngineEvent> = (|| {
            let mut conn = lock(&self.db);
            if let Some((status, error)) = finish {
                let _ = runs::set_fields(
                    &conn,
                    &run.id,
                    &[
                        ("status", &status),
                        ("finished_at", &crate::util::now()),
                        ("error", &error),
                    ],
                );
                let _ = set_effect(
                    &conn,
                    &run.id,
                    if status == "failed" { "failed" } else { "done" },
                );
            }
            let ticket = tickets::get(&conn, &ticket_id)?;
            if run.purpose == "code_review" {
                let cr = code_reviews::finish(&conn, &run.id, output)?
                    .ok_or_else(|| Error::invalid("找不到这轮审查的记录。"))?;
                task_id = Some(cr.task_id.clone());
                let task = tasks::get(&conn, &cr.task_id)?;
                let blockers = cr
                    .findings
                    .iter()
                    .filter(|f| f.severity == "blocker")
                    .count();
                add_timeline(
                    &conn,
                    &ticket.id,
                    "code_review_finished",
                    json!({ "run_id": run.id, "task_id": cr.task_id, "agent": cr.agent, "findings": cr.findings.len(),
                            "blockers": blockers, "error": cr.error }),
                )?;
                Ok(EngineEvent::CodeReviewFinished {
                    ticket_id: ticket.id,
                    task_id: cr.task_id,
                    ticket_key: ticket.key,
                    task_title: task.title,
                    findings: cr.findings.len(),
                    blockers,
                    error: cr.error,
                })
            } else if run.purpose == "review" {
                let review = reviews::save_result(
                    &mut conn,
                    &ticket.id,
                    ticket.current_version_id.as_deref(),
                    &run.id,
                    output,
                )?;
                let found: Vec<reviews::Issue> = reviews::issues(&conn, &ticket.id)?
                    .into_iter()
                    .filter(|i| i.review_id == review.id)
                    .collect();
                let blockers = found.iter().filter(|i| i.severity == "blocker").count();
                if ticket.status == "ai_review" {
                    tickets::transition(&mut conn, &ticket.id, "human_review")?;
                }
                add_timeline(
                    &conn,
                    &ticket.id,
                    "review_finished",
                    json!({ "run_id": run.id, "issues": found.len(), "blockers": blockers, "error": review.error }),
                )?;
                Ok(EngineEvent::ReviewFinished {
                    ticket_id: ticket.id,
                    ticket_key: ticket.key,
                    ticket_title: ticket.title,
                    issues: found.len(),
                    blockers,
                    error: review.error,
                })
            } else {
                let p: Option<Proposal> = reviews::finish_proposal(&conn, &run.id, output)?;
                let error = p.as_ref().and_then(|p| p.error.clone());
                add_timeline(
                    &conn,
                    &ticket.id,
                    "proposal_ready",
                    json!({ "run_id": run.id, "kind": run.purpose, "error": error }),
                )?;
                Ok(EngineEvent::ProposalReady {
                    ticket_id: ticket.id,
                    ticket_key: ticket.key,
                    ticket_title: ticket.title,
                    kind: run.purpose.clone(),
                    error,
                })
            }
        })();
        self.changed(run.ticket_id.clone(), task_id, Some(run.id.clone()));
        match res {
            Ok(ev) => self.emit(ev),
            Err(e) => eprintln!("写回工单级运行的结果失败：{e}"),
        }
    }
}
