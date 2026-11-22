//! 多 Agent 同题对比：同一个子任务交给几个 Agent 各做一遍，各在自己的 worktree 和分支里，
//! 人看完结果选定一个，其余的取消。选定之前不能验收通过、不能合并。
use super::*;
use crate::store::tasks::CompareOption;

/// 一次对比最多几个候选（不含原来的那个）。
const MAX_CANDIDATES: usize = 3;

impl Engine {
    /// 开始对比：给 `task_id` 建候选子任务，连同它自己一起入队。返回整组（原任务在前）。
    pub fn compare_task(&self, task_id: &str, picks: Vec<CompareOption>) -> Result<Vec<Task>> {
        if picks.is_empty() || picks.len() > MAX_CANDIDATES {
            return Err(Error::invalid(format!(
                "对比要另选 1 到 {MAX_CANDIDATES} 个 Agent。"
            )));
        }
        let (original, candidates) = {
            let mut conn = lock(&self.inner.db);
            let task = tasks::get(&conn, task_id)?;
            if task.status == "blocked" {
                return Err(Error::invalid("子任务还在等依赖的子任务完成。"));
            }
            if task.status != "pending" || !runs::list_for_task(&conn, task_id)?.is_empty() {
                return Err(Error::invalid("只有还没开始过的子任务可以发起对比。"));
            }
            if task.compare_of.is_some() || tasks::comparing(&conn, &task)? {
                return Err(Error::invalid("这个子任务已经在对比里了。"));
            }
            let ticket = tickets::get(&conn, &task.ticket_id)?;
            // 原任务的配置起不来就什么都不建
            plan_run(&conn, &task, &ticket, &StartOptions::default())?;
            let candidates = tasks::add_candidates(&mut conn, &task, &picks)?;
            let ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
            // 候选的 Provider 和 Agent 协议不匹配等问题现在就报，不留下半组
            for c in &candidates {
                if let Err(e) = plan_run(&conn, c, &ticket, &StartOptions::default()) {
                    tasks::drop_candidates(&conn, &ids)?;
                    return Err(e);
                }
            }
            (task, candidates)
        };
        let ids: Vec<String> = candidates.iter().map(|c| c.id.clone()).collect();
        if let Err(e) = self.start_task(&original.id, StartOptions::default()) {
            tasks::drop_candidates(&lock(&self.inner.db), &ids)?;
            return Err(e);
        }
        add_timeline(
            &lock(&self.inner.db),
            &original.ticket_id,
            "compare_started",
            json!({
                "task_id": original.id,
                "candidates": ids,
                "agents": candidates.iter().map(|c| c.agent.clone()).collect::<Vec<_>>(),
            }),
        )?;
        for c in &candidates {
            // 入队失败的候选留在「待开始」，可以单独再点开始或取消
            if let Err(e) = self.start_task(&c.id, StartOptions::default()) {
                add_timeline(
                    &lock(&self.inner.db),
                    &original.ticket_id,
                    "compare_start_failed",
                    json!({ "task_id": c.id, "title": c.title, "error": e.to_string() }),
                )?;
            }
        }
        self.inner.changed(
            Some(original.ticket_id.clone()),
            Some(original.id.clone()),
            None,
        );
        tasks::compare_group(&lock(&self.inner.db), &original)
    }

    /// 采用这个结果：同组其余的候选取消（worktree 删掉、分支留着），依赖它们的子任务改为依赖这个。
    pub fn adopt_candidate(&self, task_id: &str) -> Result<Task> {
        let (winner, losers) = {
            let conn = lock(&self.inner.db);
            let winner = tasks::get(&conn, task_id)?;
            let group = tasks::compare_group(&conn, &winner)?;
            if group.len() < 2 {
                return Err(Error::invalid("这个子任务没有在对比。"));
            }
            if !matches!(winner.status.as_str(), "review" | "accepted") {
                return Err(Error::invalid(format!(
                    "子任务处于「{}」，跑完到待验收之后才能采用。",
                    workflow::task_status_label(&winner.status)
                )));
            }
            let losers: Vec<Task> = group.into_iter().filter(|t| t.id != winner.id).collect();
            if let Some(busy) = losers
                .iter()
                .find(|t| matches!(t.status.as_str(), "queued" | "running"))
            {
                return Err(Error::invalid(format!(
                    "「{}」还在排队或运行，先停掉它再采用。",
                    busy.title
                )));
            }
            (winner, losers)
        };
        for l in &losers {
            tasks::move_dependents(&lock(&self.inner.db), &l.id, &winner.id)?;
            self.cancel_task(&l.id)?;
        }
        {
            let conn = lock(&self.inner.db);
            add_timeline(
                &conn,
                &winner.ticket_id,
                "compare_adopted",
                json!({
                    "task_id": winner.id,
                    "dropped": losers.iter().map(|t| t.id.clone()).collect::<Vec<_>>(),
                }),
            )?;
            tasks::refresh_blocked(&conn, &winner.ticket_id)?;
        }
        self.inner.changed(
            Some(winner.ticket_id.clone()),
            Some(winner.id.clone()),
            None,
        );
        tasks::get(&lock(&self.inner.db), task_id)
    }
}
