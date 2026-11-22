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

## 接口文档
{{api_doc}}

## 工作要求
1. 上面给了接口文档时，严格按文档对接，不要自行假设接口。
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

const TEST_FIX: &str = r#"自动检查没有通过（auto-fix 第 {{attempt}} 次），请在当前 worktree 里修复。

## 没通过的检查
{{failures}}

## 工作要求
1. 先弄清失败原因再改；只修让检查失败的问题，不要改动无关内容。
2. 不要为了让检查通过而删掉、跳过测试或放宽断言；确实是测试本身写错了，在总结里说明。
3. 改完后重新运行上面的命令确认通过。
4. 最后用简短中文总结：失败原因、怎么修的。
"#;

const CODE_REVIEW: &str = r#"你是代码评审者。请以只读方式评审工单 {{ticket.key}}「{{ticket.title}}」里子任务「{{task.title}}」的改动，不要修改任何文件。

## 需求
{{task.spec}}

## 验收标准
{{task.acceptance_criteria}}

## 怎么看改动
当前目录就是这个子任务的 worktree。运行 `git diff {{base}}..HEAD` 查看全部改动，需要时再读相关文件。

## 评审要求
1. 只报真实的问题：功能错误、没覆盖的验收标准、明显的安全或数据风险、会让测试失败的改动。不要报代码风格。
2. 每个问题写清楚在哪个文件、哪一行附近、为什么是问题、建议怎么改。
3. 没有问题就返回空列表。
"#;

const REWORK: &str = r#"上一轮的结果没有通过验收，请在当前 worktree 里继续修改。

## 需要处理的意见
{{instruction}}

## 工作要求
1. 只处理上面的意见，不要改动无关内容。
2. {{project.test_hint}}
3. 最后用简短中文总结这一轮改了什么。
"#;

const REVIEW: &str = r#"你是一名资深的需求评审者。请审阅下面这份需求文档（PRD），找出开发前必须澄清或补充的问题。
你可以只读地查看当前目录里的项目代码，用来发现"与现有实现冲突"的问题；不要修改任何文件。

## 项目背景
{{project.context}}

## 需求（{{ticket.key}} {{ticket.title}}，类型：{{ticket.type}}）
{{prd}}

## 上一轮还没处理完的问题
{{open_issues}}

## 检查维度
- 完整性：背景 / 目标、用户角色、功能点、验收标准是否齐全
- 歧义：模糊词（"适当""尽快""类似 xx"）、未定义的名词
- 数据：字段名、类型、长度、必填、默认值、枚举值
- 流程：异常流程、边界条件、空状态、并发、幂等
- 接口：前后端交互点是否明确，是否需要新接口或修改旧接口
- 权限与安全：谁能操作、越权、敏感数据
- 非功能：性能、分页、兼容性
- 一致性：PRD 内部前后矛盾；与现有代码或已有功能冲突
- 可测试性：验收标准是否可以验证
- 拆分：涉及前端、后端还是两者

严重程度：blocker（不解决无法开发）、major（很可能返工）、minor（细节）、suggestion（建议）。
已经在上一轮列出、现在仍未解决的问题不要换个说法重复提出。
"#;

const SPLIT: &str = r#"下面这份需求已经定稿。请把它拆成可以分别交给编码 Agent 的子任务。
你可以只读地查看当前目录里的项目代码；不要修改任何文件。

## 需求（{{ticket.key}} {{ticket.title}}，类型：{{ticket.type}}）
{{prd}}

## 验收标准
{{ticket.acceptance_criteria}}

## 可用的项目
{{projects}}

## 拆分要求
- 后端子任务：数据模型、接口、业务逻辑、单元测试；第一步先产出接口文档。
- 前端子任务：页面、组件、状态管理、接口对接；依赖后端的接口文档（dependency_kind 用 api_doc）。
- 只涉及一端的需求不要硬拆成两端；子任务数量尽量少，每个都能独立验收。
- 每个子任务写清：标题、需求摘录（只摘与它有关的部分）、它自己的验收标准。
- project 必须是上面"可用的项目"里的名字。
"#;

const IMPACT: &str = r#"下面这份需求在定稿后发生了变更。请对比新旧两版，判断每个已有子任务受到的影响。
你可以只读地查看当前目录里的项目代码；不要修改任何文件。

## 变更原因
{{change.reason}}

## 旧版需求
{{prd.old}}

## 旧版验收标准
{{criteria.old}}

## 新版需求
{{prd.new}}

## 新版验收标准
{{criteria.new}}

## 已有的子任务
{{tasks}}

## 要求
- 对每个已有子任务给出 impact：none（不受影响）或 rework（需要追加工作），rework 时写明要追加做什么。
- 已有子任务覆盖不了的新增内容，放进 new_tasks。
- 不要因为措辞调整就判定为 rework；只看行为、数据、接口、验收标准上的变化。
"#;

const REVIEW_OUTPUT: &str = r#"
<!-- tw:purpose=review -->
## 输出格式
最后一条消息只输出一个 JSON 对象（可以放在 ```json 代码块里），不要有其它文字：
{
  "summary": "一句话评价这份 PRD",
  "scope": { "backend": true, "frontend": true },
  "issues": [
    {
      "id": "I-1",
      "severity": "blocker | major | minor | suggestion",
      "category": "completeness | ambiguity | data | flow | api | security | nonfunctional | consistency | testability | split",
      "location": "PRD 中的原文片段或章节名",
      "problem": "问题描述",
      "question": "需要向需求方确认的问题（没有就省略）",
      "suggestion": "建议写进 PRD 的文字（可直接采纳）"
    }
  ],
  "acceptance_criteria_draft": ["可验证的验收标准"]
}
"#;

const SPLIT_OUTPUT: &str = r#"
<!-- tw:purpose=split -->
## 输出格式
最后一条消息只输出一个 JSON 对象（可以放在 ```json 代码块里），不要有其它文字：
{
  "tasks": [
    {
      "ref": "T1",
      "kind": "backend | frontend",
      "project": "项目名",
      "title": "子任务标题",
      "spec": "需求摘录（Markdown）",
      "acceptance_criteria": ["这个子任务自己的验收标准"],
      "depends_on": ["它依赖的子任务的 ref"],
      "dependency_kind": "api_doc | done"
    }
  ]
}
"#;

const IMPACT_OUTPUT: &str = r#"
<!-- tw:purpose=impact -->
## 输出格式
最后一条消息只输出一个 JSON 对象（可以放在 ```json 代码块里），不要有其它文字：
{
  "summary": "一句话说明这次变更影响了什么",
  "tasks": [
    { "task_id": "已有子任务的 id", "impact": "none | rework", "instruction": "rework 时要追加做的事" }
  ],
  "new_tasks": [
    { "ref": "N1", "kind": "backend | frontend", "project": "项目名", "title": "", "spec": "", "acceptance_criteria": [], "depends_on": [], "dependency_kind": "done" }
  ]
}
"#;

/// 可编辑的模板。输出格式（JSON 结构）由系统固定追加，不在模板里，改模板不会让解析失效。
pub struct TemplateDef {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub default: &'static str,
    pub variables: &'static [&'static str],
}

const COMMON_VARS: &[&str] = &[
    "project.context",
    "project.test_hint",
    "ticket.key",
    "ticket.title",
];

pub const TEMPLATES: &[TemplateDef] = &[
    TemplateDef {
        name: "backend_dev",
        title: "后端开发",
        description: "后端子任务的首轮 Prompt",
        default: BACKEND_DEV,
        variables: &["task.spec", "task.acceptance_criteria"],
    },
    TemplateDef {
        name: "frontend_dev",
        title: "前端开发",
        description: "前端子任务的首轮 Prompt",
        default: FRONTEND_DEV,
        variables: &["task.spec", "task.acceptance_criteria", "api_doc"],
    },
    TemplateDef {
        name: "bug_fix",
        title: "Bug 修复",
        description: "Bug 工单的首轮 Prompt",
        default: BUG_FIX,
        variables: &["task.spec", "task.acceptance_criteria"],
    },
    TemplateDef {
        name: "rework",
        title: "返工 / 追加指令",
        description: "打回或追加指令时，续接原会话发送的增量 Prompt",
        default: REWORK,
        variables: &["instruction"],
    },
    TemplateDef {
        name: "test_fix",
        title: "检查失败自动修复",
        description: "自动检查没过时，续接原会话发送的修复 Prompt",
        default: TEST_FIX,
        variables: &["attempt", "failures"],
    },
    TemplateDef {
        name: "code_review",
        title: "AI 交叉审查",
        description: "另一个 Agent 只读审查子任务的 Diff；输出格式由系统固定追加",
        default: CODE_REVIEW,
        variables: &[
            "task.title",
            "task.spec",
            "task.acceptance_criteria",
            "base",
        ],
    },
    TemplateDef {
        name: "review",
        title: "PRD 预审",
        description: "AI 预审；输出格式由系统固定追加",
        default: REVIEW,
        variables: &["ticket.type", "prd", "open_issues"],
    },
    TemplateDef {
        name: "split",
        title: "拆分子任务",
        description: "定稿后生成拆分建议；输出格式由系统固定追加",
        default: SPLIT,
        variables: &[
            "ticket.type",
            "prd",
            "ticket.acceptance_criteria",
            "projects",
        ],
    },
    TemplateDef {
        name: "impact",
        title: "变更影响分析",
        description: "定稿后变更、再次定稿时生成；输出格式由系统固定追加",
        default: IMPACT,
        variables: &[
            "change.reason",
            "prd.old",
            "criteria.old",
            "prd.new",
            "criteria.new",
            "tasks",
        ],
    },
];

/// 用户改过的模板：模板名 → 内容。没改过的不在里面。
pub type Overrides = std::collections::HashMap<String, String>;

pub fn template_def(name: &str) -> crate::Result<&'static TemplateDef> {
    TEMPLATES
        .iter()
        .find(|t| t.name == name)
        .ok_or_else(|| crate::Error::invalid(format!("没有名为 {name} 的 Prompt 模板。")))
}

pub fn variables(def: &TemplateDef) -> Vec<&'static str> {
    let mut v: Vec<&str> = match def.name {
        "review" | "split" => vec!["project.context", "ticket.key", "ticket.title"],
        "impact" => vec![],
        _ => COMMON_VARS.to_vec(),
    };
    v.extend(def.variables);
    v
}

pub fn load_overrides(conn: &rusqlite::Connection) -> crate::Result<Overrides> {
    let mut st = conn.prepare("SELECT key, value_json FROM setting WHERE key LIKE 'prompt:%'")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = Overrides::new();
    for row in rows {
        let (k, v) = row?;
        if let Ok(text) = serde_json::from_str::<String>(&v) {
            out.insert(k["prompt:".len()..].to_string(), text);
        }
    }
    Ok(out)
}

/// 保存自定义模板；`None` 或空白恢复默认。
pub fn save_override(
    conn: &rusqlite::Connection,
    name: &str,
    content: Option<&str>,
) -> crate::Result<()> {
    let def = template_def(name)?;
    let key = format!("prompt:{name}");
    match content.map(str::trim).filter(|c| !c.is_empty()) {
        Some(c) if c != def.default.trim() => {
            conn.execute(
                "INSERT INTO setting (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                rusqlite::params![key, serde_json::to_string(c)?],
            )?;
        }
        _ => {
            conn.execute("DELETE FROM setting WHERE key = ?1", [key])?;
        }
    }
    Ok(())
}

fn text<'a>(ov: &'a Overrides, name: &str) -> &'a str {
    match ov.get(name) {
        Some(t) => t.as_str(),
        None => TEMPLATES
            .iter()
            .find(|t| t.name == name)
            .map(|t| t.default)
            .unwrap_or(""),
    }
}

/// 模板里出现、但这个模板不认识的 `{{变量}}`（多半是拼错了）。
pub fn unknown_variables(name: &str, content: &str) -> crate::Result<Vec<String>> {
    let known = variables(template_def(name)?);
    let mut out = Vec::new();
    let mut rest = content;
    while let Some(i) = rest.find("{{") {
        let Some(j) = rest[i..].find("}}") else { break };
        let var = rest[i + 2..i + j].trim().to_string();
        if !known.contains(&var.as_str()) && !out.contains(&var) {
            out.push(var);
        }
        rest = &rest[i + j + 2..];
    }
    Ok(out)
}

/// 给设置页用的模板信息：默认内容、当前内容、可用变量。
#[derive(Debug, Clone, serde::Serialize)]
pub struct TemplateInfo {
    pub name: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub default: &'static str,
    pub content: String,
    pub customized: bool,
    pub variables: Vec<&'static str>,
}

pub fn template_infos(ov: &Overrides) -> Vec<TemplateInfo> {
    TEMPLATES
        .iter()
        .map(|t| TemplateInfo {
            name: t.name,
            title: t.title,
            description: t.description,
            default: t.default,
            content: text(ov, t.name).to_string(),
            customized: ov.contains_key(t.name),
            variables: variables(t),
        })
        .collect()
}

/// 用示例数据把模板填出来，给用户看改完之后 Agent 实际会收到什么（含系统追加的输出格式）。
pub fn preview(name: &str, content: &str) -> crate::Result<String> {
    let def = template_def(name)?;
    let sample = |var: &str| -> String {
        match var {
            "project.context" => "技术栈：Spring Boot 3 + MyBatis；接口统一返回 Result<T>；金额用分。",
            "project.test_hint" => "完成后运行 `./gradlew test` 确保通过。",
            "ticket.key" => "TW-12",
            "ticket.title" => "优惠券叠加规则与结算页展示",
            "ticket.type" => "feature_mixed",
            "task.spec" => "支持满减券与折扣券同时使用，固定先折扣后满减。",
            "task.acceptance_criteria" => "- 两张券叠加后金额不为负\n- 结算页显示逐券抵扣明细",
            "api_doc" => "POST /api/checkout/price-preview：入参商品行与券 ID 列表，返回逐券抵扣明细与实付金额。",
            "instruction" => "边界值没处理：满减后金额为 0 时不应再打折。",
            "attempt" => "1",
            "failures" => "### test：`./gradlew test`（退出码 1）\n```\nCouponTest > stacking FAILED\n    expected: 0 but was: -100\n```",
            "task.title" => "后端：优惠券叠加试算",
            "base" => "3f2a9c1",
            "prd" | "prd.new" => "## 背景\n用户希望两张券可以叠加使用。\n\n## 规则\n先折扣、后满减。",
            "prd.old" => "## 背景\n用户希望两张券可以叠加使用。",
            "criteria.old" => "- 两张券叠加后金额不为负",
            "criteria.new" => "- 两张券叠加后金额不为负\n- 最多叠加三张",
            "open_issues" => "- [I-2][major] 没有约定叠加上限",
            "projects" => "- shop-api | kind=backend | role=backend\n- shop-web | kind=frontend | role=frontend",
            "change.reason" => "运营要求叠加上限改为三张。",
            "tasks" => "- 01JABC | backend | review | 后端：试算接口\n- 01JABD | frontend | pending | 前端：结算页",
            other => return format!("（示例：{other}）"),
        }
        .to_string()
    };
    let vars: Vec<(&str, String)> = variables(def).into_iter().map(|v| (v, sample(v))).collect();
    let suffix = match name {
        "review" => REVIEW_OUTPUT,
        "split" => SPLIT_OUTPUT,
        "impact" => IMPACT_OUTPUT,
        "code_review" => CODE_REVIEW_FORMAT,
        _ => "",
    };
    Ok(fill(content, &vars) + suffix)
}

fn bullets(items: &[String], empty: &str) -> String {
    if items.is_empty() {
        empty.to_string()
    } else {
        items
            .iter()
            .map(|c| format!("- {c}"))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// AI 预审。`open_issues`：上一轮还没处理完的问题（每条一行）。
pub fn review_prompt(
    ov: &Overrides,
    project: &Project,
    ticket: &Ticket,
    prd: &str,
    open_issues: &[String],
) -> String {
    let mut vars = common_vars(project, ticket);
    vars.push(("ticket.type", ticket.kind.clone()));
    vars.push(("prd", or_placeholder(prd, "（正文为空）")));
    vars.push(("open_issues", bullets(open_issues, "（没有）")));
    fill(text(ov, "review"), &vars) + REVIEW_OUTPUT
}

/// 定稿后的拆分。`projects`：每个可用项目一行（名字、类型、在工单里的角色）。
pub fn split_prompt(
    ov: &Overrides,
    project: &Project,
    ticket: &Ticket,
    prd: &str,
    projects: &[String],
) -> String {
    let mut vars = common_vars(project, ticket);
    vars.push(("ticket.type", ticket.kind.clone()));
    vars.push(("prd", or_placeholder(prd, "（正文为空）")));
    vars.push((
        "ticket.acceptance_criteria",
        bullets(&ticket.acceptance_criteria, "（未列出）"),
    ));
    vars.push(("projects", bullets(projects, "（没有绑定项目）")));
    fill(text(ov, "split"), &vars) + SPLIT_OUTPUT
}

/// 变更影响分析。`tasks`：每个已有子任务一行（id、类型、状态、标题）。
pub fn impact_prompt(
    ov: &Overrides,
    reason: &str,
    (old_prd, old_criteria): (&str, &[String]),
    (new_prd, new_criteria): (&str, &[String]),
    tasks: &[String],
) -> String {
    let vars = vec![
        ("change.reason", or_placeholder(reason, "（未填写）")),
        ("prd.old", or_placeholder(old_prd, "（空）")),
        ("criteria.old", bullets(old_criteria, "（未列出）")),
        ("prd.new", or_placeholder(new_prd, "（空）")),
        ("criteria.new", bullets(new_criteria, "（未列出）")),
        ("tasks", bullets(tasks, "（还没有子任务）")),
    ];
    fill(text(ov, "impact"), &vars) + IMPACT_OUTPUT
}

fn or_placeholder(s: &str, placeholder: &str) -> String {
    if s.trim().is_empty() {
        placeholder.to_string()
    } else {
        s.trim().to_string()
    }
}

/// 子任务首轮开发用哪个模板。
pub fn dev_template_name(task_kind: &str, ticket_type: &str) -> &'static str {
    match (task_kind, ticket_type) {
        ("bug", _) | (_, "bug") => "bug_fix",
        ("frontend", _) => "frontend_dev",
        _ => "backend_dev",
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
/// `api_doc`：依赖的后端子任务产出的接口文档（前端子任务用）。
pub fn dev_prompt(
    ov: &Overrides,
    project: &Project,
    ticket: &Ticket,
    task: &Task,
    prd: &str,
    api_doc: Option<&str>,
) -> String {
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
    vars.push((
        "api_doc",
        or_placeholder(
            api_doc.unwrap_or(""),
            "（没有提供接口文档；需要接口时先查看后端代码，不要自行假设。）",
        ),
    ));
    fill(text(ov, dev_template_name(&task.kind, &ticket.kind)), &vars)
}

/// 返工 / 追加指令的 Prompt：续接原会话时只传增量。
pub fn rework_prompt(
    ov: &Overrides,
    project: &Project,
    ticket: &Ticket,
    instruction: &str,
) -> String {
    let mut vars = common_vars(project, ticket);
    vars.push(("instruction", instruction.trim().to_string()));
    fill(text(ov, "rework"), &vars)
}

pub fn test_fix_prompt(
    ov: &Overrides,
    project: &Project,
    ticket: &Ticket,
    attempt: u32,
    failures: &str,
) -> String {
    let mut vars = common_vars(project, ticket);
    vars.push(("attempt", attempt.to_string()));
    vars.push(("failures", failures.trim().to_string()));
    fill(text(ov, "test_fix"), &vars)
}

/// 交叉审查的输出格式，固定追加在模板后面。
pub const CODE_REVIEW_FORMAT: &str = r#"
## 输出格式
只输出一个 JSON 对象，不要任何其它文字，也不要用代码块包起来：
{"summary": "一两句总体评价", "findings": [{"severity": "blocker | major | minor", "file": "相对路径", "line": 行号或 null, "title": "一句话问题", "detail": "为什么是问题", "suggestion": "建议怎么改"}]}
<!-- tw:purpose=code_review -->
"#;

pub fn code_review_prompt(
    ov: &Overrides,
    project: &Project,
    ticket: &Ticket,
    task: &Task,
    prd: &str,
    base: &str,
) -> String {
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
    let spec = match (task.spec_md.trim(), prd.trim()) {
        ("", "") => task.title.clone(),
        ("", prd) => prd.to_string(),
        (spec, "") => spec.to_string(),
        (spec, prd) => format!("{spec}\n\n### 工单正文（背景）\n{prd}"),
    };
    let mut vars = common_vars(project, ticket);
    vars.push(("task.title", task.title.clone()));
    vars.push(("task.spec", spec));
    vars.push(("task.acceptance_criteria", criteria));
    vars.push(("base", base.to_string()));
    let mut p = fill(text(ov, "code_review"), &vars);
    p.push_str(CODE_REVIEW_FORMAT);
    p
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
            &Overrides::new(),
            &project(),
            &ticket("feature_backend"),
            &task("backend", ""),
            "支持两张券叠加",
            None,
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
        let bug = dev_prompt(
            &Overrides::new(),
            &project(),
            &ticket("bug"),
            &task("bug", "金额为负"),
            "",
            None,
        );
        assert!(bug.contains("修复下面这个 Bug") && bug.contains("金额为负"));
        let fe = dev_prompt(
            &Overrides::new(),
            &project(),
            &ticket("feature_mixed"),
            &task("frontend", "结算页"),
            "正文",
            Some("GET /coupons"),
        );
        assert!(fe.contains("前端开发者") && fe.contains("### 工单正文（背景）"));
        assert!(fe.contains("## 接口文档\nGET /coupons") && !fe.contains("{{"));
        let r = rework_prompt(
            &Overrides::new(),
            &project(),
            &ticket("bug"),
            " 边界值没处理 ",
        );
        assert!(r.contains("边界值没处理") && !r.contains("{{"));
    }

    #[test]
    fn custom_templates_replace_defaults_and_flag_unknown_variables() {
        let conn = crate::db::open_in_memory().unwrap();
        assert!(load_overrides(&conn).unwrap().is_empty());
        save_override(
            &conn,
            "rework",
            Some("请处理：{{instruction}}（{{ticket.key}}）"),
        )
        .unwrap();
        let ov = load_overrides(&conn).unwrap();
        let r = rework_prompt(&ov, &project(), &ticket("bug"), "边界值");
        assert_eq!(r, "请处理：边界值（TW-7）");
        assert_eq!(
            unknown_variables("rework", "{{instruction}} {{task.spec}} {{ nope }}").unwrap(),
            vec!["task.spec", "nope"]
        );
        // 输出格式由系统追加，改模板不影响解析
        save_override(&conn, "review", Some("只看数据定义：{{prd}}")).unwrap();
        let ov = load_overrides(&conn).unwrap();
        let p = review_prompt(&ov, &project(), &ticket("feature_mixed"), "正文", &[]);
        assert!(p.starts_with("只看数据定义：正文") && p.contains("tw:purpose=review"));
        // 存回默认内容或空白 = 恢复默认
        save_override(&conn, "rework", Some("  ")).unwrap();
        save_override(&conn, "review", None).unwrap();
        assert!(load_overrides(&conn).unwrap().is_empty());
        assert!(save_override(&conn, "nope", Some("x")).is_err());

        // 预览：每个模板的默认内容用示例数据都能填满
        for t in template_infos(&ov) {
            let p = preview(t.name, t.default).unwrap();
            assert!(!p.contains("{{"), "{} 还有没填的变量：{p}", t.name);
        }
        assert!(preview("rework", "{{instruction}} {{nope}}")
            .unwrap()
            .ends_with("{{nope}}"));
    }
}
