//! 智能创建的 AI 分析：建单之前，让默认 Agent 只读地看一遍粘贴 / 导入的正文，
//! 给出标题、类型、项目、优先级、标签和 Bug 三要素。结果只用来预填表单，用户确认后才建单。
//!
//! 这时还没有工单，所以不建 Run、不进队列、不记统计；Agent 在一个空目录里跑，看不到任何仓库。
use super::*;
use crate::store::tickets::{Draft, PRIORITIES, TYPES};
use serde_json::Value;

/// 分析最多等这么久。
const TIMEOUT: Duration = Duration::from_secs(180);
/// 交给 Agent 的正文上限（字符）；超出的部分不送。
const MAX_CHARS: usize = 40_000;
/// 取消标记在 `cancels` 里的键（不会和运行 id 撞上）。
const CANCEL_KEY: &str = "smart-analysis";

#[derive(Debug, Clone, Serialize)]
pub struct SmartAnalysis {
    #[serde(flatten)]
    pub draft: Draft,
    /// 做分析的 Agent、Provider 名和模型，给用户看。
    pub agent: String,
    pub provider: Option<String>,
    pub model: Option<String>,
}

fn strings(v: &Value, max: usize, max_chars: usize) -> Vec<String> {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .map(|s| s.trim().chars().take(max_chars).collect::<String>())
                .filter(|s| !s.is_empty())
                .take(max)
                .collect()
        })
        .unwrap_or_default()
}

/// 把 Agent 的回答合到本机规则的结果上：只接受认得的值，其余保持本机规则的判断。
pub fn apply_analysis(mut d: Draft, v: &Value, projects: &[Project]) -> Draft {
    let text = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty());
    if let Some(t) = text("title") {
        d.title = t.chars().take(80).collect();
    }
    if let Some(t) = text("type").filter(|t| TYPES.contains(t)) {
        d.kind = t.into();
    }
    if let Some(p) = text("priority").filter(|p| PRIORITIES.contains(p)) {
        d.priority = p.into();
    }
    if v["labels"].is_array() {
        d.labels = strings(&v["labels"], 5, 20);
    }
    // 项目：按 id 或名字认，认不出来的丢掉；一个都认不出来就保留本机规则选的
    let mut picked: Vec<String> = vec![];
    for want in strings(&v["project_ids"], 8, 200) {
        let found = projects
            .iter()
            .find(|p| p.id == want || p.name.eq_ignore_ascii_case(&want));
        if let Some(p) = found {
            if !picked.contains(&p.id) {
                picked.push(p.id.clone());
            }
        }
    }
    if !picked.is_empty() {
        d.project_ids = picked;
    }
    d.bug_info = if d.kind == "bug" {
        let b = &v["bug"];
        let part = |k: &str| {
            b[k].as_str()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(String::from)
        };
        let (steps, expected, actual) = (part("steps"), part("expected"), part("actual"));
        if steps.is_some() || expected.is_some() || actual.is_some() {
            Some(json!({ "steps": steps, "expected": expected, "actual": actual }))
        } else {
            d.bug_info
        }
    } else {
        None
    };
    let reasons = strings(&v["reasons"], 8, 200);
    d.reasons = if reasons.is_empty() {
        vec!["AI 没有说明判断依据。".into()]
    } else {
        reasons
    };
    d
}

impl Engine {
    /// 取消正在进行的分析（没有在分析就什么都不做）。
    pub fn cancel_smart_analysis(&self) {
        if let Some(c) = self.inner.cancels.lock().unwrap().get(CANCEL_KEY) {
            c.store(true, Ordering::SeqCst);
        }
    }

    /// 让默认 Agent 分析一段正文。同一时间只做一个；失败时调用方继续用本机规则的结果。
    pub fn smart_analyze(&self, md: &str) -> Result<SmartAnalysis> {
        if md.trim().is_empty() {
            return Err(Error::invalid("正文是空的，没有可以分析的内容。"));
        }
        let (all, st, provider) = {
            let conn = lock(&self.inner.db);
            let st = settings::get(&conn)?;
            let provider = st
                .default_provider_id
                .as_deref()
                .map(|id| providers::get(&conn, id))
                .transpose()?;
            (projects::list(&conn)?, st, provider)
        };
        let kind = AgentKind::parse(&st.default_agent)?;
        let local = tickets::smart_draft(md, &all);
        let body: String = local.content_md.chars().take(MAX_CHARS).collect();
        let mut spec = RunSpec {
            prompt: crate::prompt::smart_prompt(&body, &all),
            permission: PermissionMode::ReadOnly,
            // 正文可能是别人给的：里面的话不能让 Agent 去读这台电脑上的文件
            no_tools: true,
            binary: if kind == AgentKind::Mock {
                self.inner.cfg.mock_binary.clone()
            } else {
                None
            },
            ..Default::default()
        };
        if let Some(p) = &provider {
            let secret = match &p.secret_ref {
                Some(name) => self.inner.secrets.get(name)?,
                None => None,
            };
            let inj = crate::provider::inject(kind, &p.config()?, secret.as_deref())?;
            spec.env = inj.env;
            spec.extra_args = inj.args;
            spec.model = inj.model;
        }

        let cancel = Arc::new(AtomicBool::new(false));
        {
            let mut cancels = self.inner.cancels.lock().unwrap();
            if self.inner.hold.load(Ordering::SeqCst) {
                return Err(Error::invalid("正在安装更新，现在不能开始 AI 分析。"));
            }
            if cancels.contains_key(CANCEL_KEY) {
                return Err(Error::invalid("上一次 AI 分析还没结束。"));
            }
            cancels.insert(CANCEL_KEY.into(), cancel.clone());
        }
        // 空目录：分析只看正文和项目卡片，不让 Agent 读任何仓库
        let cwd = self
            .inner
            .cfg
            .data_dir
            .join("tmp")
            .join(format!("smart-{}", crate::util::new_id()));
        let res = std::fs::create_dir_all(&cwd)
            .map_err(Error::from)
            .and_then(|_| {
                spec.cwd = cwd.clone();
                run_agent(
                    adapter(kind).as_ref(),
                    &spec,
                    &RunOptions {
                        cancel,
                        timeout: Some(TIMEOUT),
                        ..Default::default()
                    },
                    &mut |_| {},
                )
            });
        self.inner.cancels.lock().unwrap().remove(CANCEL_KEY);
        let _ = std::fs::remove_dir_all(&cwd);

        let outcome = res?;
        if outcome.stopped {
            return Err(Error::invalid("已取消 AI 分析。"));
        }
        if !outcome.succeeded() {
            let detail = if outcome.timed_out {
                format!("超过 {} 秒没有结果", TIMEOUT.as_secs())
            } else if !outcome.summary.trim().is_empty() {
                outcome.summary.trim().chars().take(300).collect()
            } else {
                outcome.stderr_tail.trim().chars().take(300).collect()
            };
            return Err(Error::invalid(format!("AI 分析没有完成：{detail}")));
        }
        let v = crate::store::reviews::extract_json(&outcome.summary)
            .map_err(|_| Error::invalid("AI 的回答不是约定的 JSON，已保留本机规则识别的结果。"))?;
        Ok(SmartAnalysis {
            draft: apply_analysis(local, &v, &all),
            agent: kind.as_str().into(),
            provider: provider.as_ref().map(|p| p.name.clone()),
            model: spec.model,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(id: &str, name: &str) -> Project {
        Project {
            id: id.into(),
            name: name.into(),
            kind: "backend".into(),
            local_path: String::new(),
            git_url: None,
            default_branch: "main".into(),
            context_md: String::new(),
            commands: Default::default(),
            default_agent: None,
            default_provider_id: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn only_recognised_values_replace_the_local_guess() {
        let all = [project("p1", "shop-api"), project("p2", "shop-web")];
        let local = tickets::smart_draft("# 导出报错\n点导出后页面报错。", &all);
        let (kind, priority) = (local.kind.clone(), local.priority.clone());

        // 认不得的类型 / 优先级 / 项目：保持本机规则的判断
        let d = apply_analysis(
            local.clone(),
            &json!({"title":"  ","type":"epic","priority":"p0","project_ids":["nope"]}),
            &all,
        );
        assert_eq!(d.title, local.title);
        assert_eq!(d.kind, kind);
        assert_eq!(d.priority, priority);
        assert_eq!(d.project_ids, local.project_ids);
        assert_eq!(d.reasons, vec!["AI 没有说明判断依据。".to_string()]);

        // 项目可以给 id 也可以给名字，重复的只留一个；标签最多 5 个
        let d = apply_analysis(
            local.clone(),
            &json!({
                "title": "订单导出报错",
                "type": "bug",
                "priority": "urgent",
                "project_ids": ["SHOP-WEB", "p2", "p1"],
                "labels": ["a", "b", "c", "d", "e", "f", 7, ""],
                "bug": {"steps": "点导出", "expected": "", "actual": "500"},
                "reasons": ["类型：在描述报错"],
            }),
            &all,
        );
        assert_eq!(d.title, "订单导出报错");
        assert_eq!(d.kind, "bug");
        assert_eq!(d.priority, "urgent");
        assert_eq!(d.project_ids, vec!["p2".to_string(), "p1".to_string()]);
        assert_eq!(d.labels.len(), 5);
        let bug = d.bug_info.unwrap();
        assert_eq!(bug["steps"], "点导出");
        assert!(bug["expected"].is_null());
        assert_eq!(bug["actual"], "500");

        // 不是 Bug 就不带三要素，哪怕 AI 给了
        let d = apply_analysis(local, &json!({"type":"chore","bug":{"steps":"x"}}), &all);
        assert!(d.bug_info.is_none());
    }
}
