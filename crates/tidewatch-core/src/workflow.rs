//! 工单与子任务的状态机（docs/02-workflow/workflow.md）。
use crate::{Error, Result};

pub const TICKET_STATUSES: &[&str] = &[
    "draft",
    "ai_review",
    "human_review",
    "finalized",
    "developing",
    "acceptance",
    "done",
    "cancelled",
];

pub fn ticket_status_label(s: &str) -> &str {
    match s {
        "draft" => "草稿",
        "ai_review" => "AI 预审",
        "human_review" => "人工审核",
        "finalized" => "已定稿",
        "developing" => "开发中",
        "acceptance" => "验收中",
        "done" => "已完成",
        "cancelled" => "已取消",
        other => other,
    }
}

/// 工单能否从 `from` 转到 `to`；不能时返回给用户看的原因。
/// `cancelled_from` 是取消前的状态：已取消的工单只能恢复到那里。
pub fn check_ticket_transition(
    ticket_type: &str,
    from: &str,
    to: &str,
    cancelled_from: Option<&str>,
) -> Result<()> {
    let ok = match (from, to) {
        ("draft", "ai_review")
        | ("ai_review", "human_review")
        | ("human_review", "ai_review")
        | ("human_review", "finalized")
        | ("finalized", "developing")
        | ("developing", "acceptance")
        | ("acceptance", "developing")
        | ("acceptance", "done")
        | ("finalized", "human_review")
        | ("developing", "human_review") => true,
        // Bug 快速通道：跳过预审与人工审核
        ("draft", "developing") => {
            if ticket_type == "bug" {
                true
            } else {
                return Err(Error::invalid(
                    "只有 Bug 工单可以从草稿直接开始开发；需求类工单要先预审、定稿。",
                ));
            }
        }
        ("draft" | "human_review" | "developing", "cancelled") => true,
        ("cancelled", to) => {
            if cancelled_from == Some(to) {
                true
            } else {
                return Err(Error::invalid(format!(
                    "已取消的工单只能恢复到取消前的状态（{}）。",
                    ticket_status_label(cancelled_from.unwrap_or("draft"))
                )));
            }
        }
        _ => false,
    };
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(format!(
            "工单不能从「{}」直接变为「{}」。",
            ticket_status_label(from),
            ticket_status_label(to)
        )))
    }
}

pub const TASK_STATUSES: &[&str] = &[
    "pending",
    "blocked",
    "queued",
    "running",
    "review",
    "changes_requested",
    "accepted",
    "merged",
    "failed",
    "stopped",
    "cancelled",
];

pub fn task_status_label(s: &str) -> &str {
    match s {
        "pending" => "待开始",
        "blocked" => "等依赖",
        "queued" => "排队中",
        "running" => "运行中",
        "review" => "待验收",
        "changes_requested" => "已打回",
        "accepted" => "已通过",
        "merged" => "已合并",
        "failed" => "失败",
        "stopped" => "已停止",
        "cancelled" => "已取消",
        other => other,
    }
}

/// 子任务此刻能否开始一轮新的 Run。
pub fn check_task_can_start(status: &str) -> Result<()> {
    match status {
        "pending" | "review" | "changes_requested" | "failed" | "stopped" => Ok(()),
        "blocked" => Err(Error::invalid("子任务还在等依赖的子任务完成。")),
        "queued" | "running" => Err(Error::invalid("子任务已经在排队或运行中。")),
        other => Err(Error::invalid(format!(
            "子任务处于「{}」，不能再运行。",
            task_status_label(other)
        ))),
    }
}

/// 依赖的子任务到了这些状态，才算可以解锁后面的子任务。
pub fn task_satisfies_dependency(status: &str) -> bool {
    matches!(status, "review" | "accepted" | "merged")
}

/// 工单下所有子任务都到了待验收 / 已通过 / 已合并（已取消的不算），工单进入验收。
pub fn all_tasks_ready_for_acceptance<'a>(statuses: impl IntoIterator<Item = &'a str>) -> bool {
    let mut any = false;
    for s in statuses {
        match s {
            "cancelled" => {}
            "review" | "accepted" | "merged" => any = true,
            _ => return false,
        }
    }
    any
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticket_transitions_follow_the_diagram() {
        let ok = |t, a, b| check_ticket_transition(t, a, b, None).is_ok();
        assert!(ok("feature_backend", "draft", "ai_review"));
        assert!(ok("feature_backend", "human_review", "finalized"));
        assert!(ok("feature_backend", "developing", "human_review"));
        assert!(ok("feature_backend", "acceptance", "done"));
        assert!(!ok("feature_backend", "draft", "done"));
        assert!(!ok("feature_backend", "done", "developing"));
        assert!(!ok("feature_backend", "acceptance", "cancelled"));
    }

    #[test]
    fn only_bugs_take_the_fast_path() {
        assert!(check_ticket_transition("bug", "draft", "developing", None).is_ok());
        let e = check_ticket_transition("feature_mixed", "draft", "developing", None).unwrap_err();
        assert!(e.to_string().contains("Bug"));
    }

    #[test]
    fn cancelled_restores_only_to_previous_status() {
        assert!(
            check_ticket_transition("bug", "cancelled", "developing", Some("developing")).is_ok()
        );
        assert!(check_ticket_transition("bug", "cancelled", "draft", Some("developing")).is_err());
    }

    #[test]
    fn acceptance_needs_every_live_task_done() {
        assert!(all_tasks_ready_for_acceptance([
            "review",
            "accepted",
            "cancelled"
        ]));
        assert!(!all_tasks_ready_for_acceptance(["review", "running"]));
        assert!(!all_tasks_ready_for_acceptance(["cancelled"]));
        assert!(!all_tasks_ready_for_acceptance([]));
    }

    #[test]
    fn task_start_rules() {
        assert!(check_task_can_start("pending").is_ok());
        assert!(check_task_can_start("stopped").is_ok());
        assert!(check_task_can_start("running").is_err());
        assert!(check_task_can_start("blocked").is_err());
        assert!(check_task_can_start("merged").is_err());
    }
}
