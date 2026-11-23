//! 给 Run 追加指令（Q16 交互通道）：运行中直接引导、打断后带指令重开，或排到这轮结束后接着跑。
//! 只有支持流式输入的 Agent（Claude Code、演示 Agent）能运行中引导，其他 Agent 自动改为排队。
use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FollowupMode {
    /// 写进运行中 Agent 的 stdin，在同一会话里接着做。
    Steer,
    /// 停掉当前这轮，结束后续接会话、带着指令重新开始。
    Restart,
    /// 等这轮自然结束，再续接会话、带着指令开始下一轮。
    Queue,
}

#[derive(Debug, Clone, Serialize)]
pub struct FollowupResult {
    /// 实际采用的方式：steered / queued / restarting / started。
    pub outcome: String,
    /// `started` 时新开的 Run。
    pub run_id: Option<String>,
}

fn result(outcome: &str, run_id: Option<String>) -> FollowupResult {
    FollowupResult {
        outcome: outcome.into(),
        run_id,
    }
}

impl Engine {
    pub fn send_followup(
        &self,
        run_id: &str,
        text: &str,
        mode: FollowupMode,
    ) -> Result<FollowupResult> {
        let text = text.trim();
        if text.is_empty() {
            return Err(Error::invalid("指令不能为空。"));
        }
        let run = runs::get(&lock(&self.inner.db), run_id)?;
        let task_id = run
            .task_id
            .clone()
            .ok_or_else(|| Error::invalid("只有子任务的运行可以追加指令。"))?;
        let instruction = || StartOptions {
            instruction: Some(text.to_string()),
            ..Default::default()
        };
        let active = matches!(
            run.status.as_str(),
            "queued" | "starting" | "running" | "finalizing" | "stopping"
        );
        if !active {
            let next = self.start_task(&task_id, instruction())?;
            return Ok(result("started", Some(next.id)));
        }
        if mode == FollowupMode::Steer {
            let inbox = self.inner.steers.lock().unwrap().get(run_id).cloned();
            if inbox.is_some_and(|i| i.push(text.to_string())) {
                return Ok(result("steered", None));
            }
        }
        if mode == FollowupMode::Restart && run.status == "queued" {
            self.stop_run(run_id)?;
            let next = self.start_task(&task_id, instruction())?;
            return Ok(result("started", Some(next.id)));
        }
        let restart = mode == FollowupMode::Restart && run.status != "stopping";
        self.inner
            .queue_followup(run_id, text, if restart { "restart" } else { "queue" });
        if restart {
            self.stop_run(run_id)?;
            return Ok(result("restarting", None));
        }
        Ok(result("queued", None))
    }

    /// 这轮运行此刻能否直接引导（Agent 在跑且 stdin 还开着）。
    pub fn can_steer(&self, run_id: &str) -> bool {
        self.inner
            .steers
            .lock()
            .unwrap()
            .get(run_id)
            .is_some_and(|i| !i.is_closed())
    }
}

impl Inner {
    pub(super) fn queue_followup(&self, run_id: &str, text: &str, mode: &str) {
        self.followups
            .lock()
            .unwrap()
            .entry(run_id.to_string())
            .or_default()
            .push(text.to_string());
        self.log(run_id, "followup", json!({ "text": text, "mode": mode }));
    }

    pub(super) fn has_followups(&self, run_id: &str) -> bool {
        self.followups.lock().unwrap().contains_key(run_id)
    }

    /// 这轮结束后把排队的追加指令合成一条，续接会话开始下一轮。
    pub(super) fn start_followups(&self, run: &Run) {
        let Some(list) = self.followups.lock().unwrap().remove(&run.id) else {
            return;
        };
        let Some(task_id) = &run.task_id else { return };
        let opts = StartOptions {
            instruction: Some(list.join("\n\n")),
            ..Default::default()
        };
        if let Err(e) = self.enqueue_task(task_id, opts) {
            self.log(
                &run.id,
                "error",
                json!({ "message": format!("没能开始追加指令的下一轮：{e}") }),
            );
        }
    }
}
