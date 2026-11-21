//! Prompt 组装：项目卡片 + 工单正文 + 子任务说明 → 发给 Agent 的任务说明。
//! 模板是 Markdown + `{{变量}}`；M1 内置开发 / Bug 修复 / 返工三种，自定义在 M2。
use crate::store::projects::Project;
use crate::store::tasks::Task;
use crate::store::tickets::Ticket;

const BACKEND_DEV: &str = r#"你是本项目的后端开发者。请在当前目录（一个独立的 git worktree）中完成以下任务。

## 项目背景
{{project.context}}

## 需求（{{ticket.key}} {{ticket.title}}）
{{task.spec}}

## 验收标准
{{task.acceptance_criteria}}

## 工作要求
1. 第一步：在 `docs/api/{{ticket.key}}.md` 写出本需求涉及的全部接口文档（路径、方法、请求 / 响应结构、字段说明、错误码、示例）。
2. 然后实现功能，并补充必要的单元测试。
3. {{project.test_hint}}
4. 不要修改与本任务无关的文件；不要推送到远程。
5. 最后用简短中文总结：做了什么、改了哪些文件、有哪些未完成或需要人工确认的点。
"#;

const FRONTEND_DEV: &str = r#"你是本项目的前端开发者。请在当前目录（一个独立的 git worktree）中完成以下任务。

## 项目背景
{{project.context}}

## 需求（{{ticket.key}} {{ticket.title}}）
{{task.spec}}

## 验收标准
{{task.acceptance_criteria}}

## 工作要求
1. 如果任务说明里给了接口文档，严格按文档对接，不要自行假设接口。
2. 实现界面与交互，并补充必要的测试。
3. {{project.test_hint}}
4. 不要修改与本任务无关的文件；不要推送到远程。
5. 最后用简短中文总结：做了什么、改了哪些文件、有哪些未完成或需要人工确认的点。
"#;

const BUG_FIX: &str = r#"请在当前目录（一个独立的 git worktree）中修复下面这个 Bug。

## 项目背景
{{project.context}}

## Bug（{{ticket.key}} {{ticket.title}}）
{{task.spec}}

## 验收标准
{{task.acceptance_criteria}}

## 工作要求
1. 先定位并复现问题；条件允许时先写一个能复现它的失败测试。
2. 做最小且正确的修复，不顺带重构无关代码。
3. {{project.test_hint}}
4. 不要推送到远程。
5. 最后用简短中文总结：根因是什么、怎么修的、改了哪些文件、还有什么需要人工确认。
"#;

const REWORK: &str = r#"上一轮的结果没有通过验收，请在当前 worktree 里继续修改。

## 需要处理的意见
{{instruction}}

## 工作要求
1. 只处理上面的意见，不要改动无关内容。
2. {{project.test_hint}}
3. 最后用简短中文总结这一轮改了什么。
"#;

fn or_placeholder(s: &str, placeholder: &str) -> String {
    if s.trim().is_empty() {
        placeholder.to_string()
    } else {
        s.trim().to_string()
    }
}

fn template(task_kind: &str, ticket_type: &str) -> &'static str {
    match (task_kind, ticket_type) {
        ("bug", _) | (_, "bug") => BUG_FIX,
        ("frontend", _) => FRONTEND_DEV,
        _ => BACKEND_DEV,
    }
}

fn fill(template: &str, vars: &[(&str, String)]) -> String {
    let mut out = template.to_string();
    for (k, v) in vars {
        out = out.replace(&format!("{{{{{k}}}}}"), v);
    }
    out
}

fn common_vars(project: &Project, ticket: &Ticket) -> Vec<(&'static str, String)> {
    let test_hint = match &project.commands.test {
        Some(cmd) => format!("完成后运行 `{cmd}` 确保通过。"),
        None => "完成后自行运行相关测试确保通过。".to_string(),
    };
    vec![
        (
            "project.context",
            or_placeholder(
                &project.context_md,
                "（项目没有填写上下文卡片，请先阅读仓库里的 README 和目录结构。）",
            ),
        ),
        ("project.test_hint", test_hint),
        ("ticket.key", ticket.key.clone()),
        ("ticket.title", ticket.title.clone()),
    ]
}

/// 首轮开发的 Prompt。`prd` 是工单当前正文；子任务自己写了说明时，正文作为背景附在后面。
pub fn dev_prompt(project: &Project, ticket: &Ticket, task: &Task, prd: &str) -> String {
    let spec = match (task.spec_md.trim(), prd.trim()) {
        ("", "") => task.title.clone(),
        ("", prd) => prd.to_string(),
        (spec, "") => spec.to_string(),
        (spec, prd) => format!("{spec}\n\n### 工单正文（背景）\n{prd}"),
    };
    let criteria = if task.acceptance_criteria.is_empty() {
        &ticket.acceptance_criteria
    } else {
        &task.acceptance_criteria
    };
    let criteria = if criteria.is_empty() {
        "（未单独列出，以需求描述为准。）".to_string()
    } else {
        criteria
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let mut vars = common_vars(project, ticket);
    vars.push(("task.spec", spec));
    vars.push(("task.acceptance_criteria", criteria));
    fill(template(&task.kind, &ticket.kind), &vars)
}

/// 返工 / 追加指令的 Prompt：续接原会话时只传增量。
pub fn rework_prompt(project: &Project, ticket: &Ticket, instruction: &str) -> String {
    let mut vars = common_vars(project, ticket);
    vars.push(("instruction", instruction.trim().to_string()));
    fill(REWORK, &vars)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> Project {
        Project {
            id: "p".into(),
            name: "商城".into(),
            kind: "backend".into(),
            local_path: "/x".into(),
            git_url: None,
            default_branch: "main".into(),
            context_md: "Rust + axum".into(),
            commands: crate::store::projects::Commands {
                test: Some("cargo test".into()),
                ..Default::default()
            },
            default_agent: None,
            default_provider_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn ticket(kind: &str) -> Ticket {
        serde_json::from_value(serde_json::json!({
            "id": "t", "key": "TW-7", "title": "优惠券叠加", "type": kind, "status": "developing",
            "priority": "medium", "labels": [], "due_date": null, "source_kind": "manual",
            "current_version_id": null, "finalized_version_id": null,
            "acceptance_criteria": ["金额不为负"], "bug_info": null, "default_agent": null,
            "default_provider_id": null, "cancelled_from": null, "created_at": "", "updated_at": "", "projects": []
        }))
        .unwrap()
    }

    fn task(kind: &str, spec: &str) -> Task {
        serde_json::from_value(serde_json::json!({
            "id": "k", "ticket_id": "t", "kind": kind, "project_id": "p", "title": "实现接口", "spec_md": spec,
            "acceptance_criteria": [], "status": "pending", "agent": null, "provider_id": null, "model": null,
            "permission_profile": null, "branch": null, "worktree_path": null, "base_commit": null,
            "merged_commit": null, "depends_on": [], "created_at": "", "updated_at": ""
        }))
        .unwrap()
    }

    #[test]
    fn dev_prompt_fills_every_variable() {
        let p = dev_prompt(
            &project(),
            &ticket("feature_backend"),
            &task("backend", ""),
            "支持两张券叠加",
        );
        assert!(p.contains("Rust + axum"));
        assert!(p.contains("TW-7 优惠券叠加"));
        assert!(p.contains("支持两张券叠加"));
        assert!(p.contains("- 金额不为负"));
        assert!(p.contains("`cargo test`"));
        assert!(p.contains("docs/api/TW-7.md"));
        assert!(!p.contains("{{"), "有变量没替换：{p}");
    }

    #[test]
    fn picks_template_by_kind() {
        let bug = dev_prompt(&project(), &ticket("bug"), &task("bug", "金额为负"), "");
        assert!(bug.contains("修复下面这个 Bug") && bug.contains("金额为负"));
        let fe = dev_prompt(
            &project(),
            &ticket("feature_mixed"),
            &task("frontend", "结算页"),
            "正文",
        );
        assert!(fe.contains("前端开发者") && fe.contains("### 工单正文（背景）"));
        let r = rework_prompt(&project(), &ticket("bug"), " 边界值没处理 ");
        assert!(r.contains("边界值没处理") && !r.contains("{{"));
    }
}
