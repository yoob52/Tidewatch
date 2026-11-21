# 工单模型

## 1. 核心概念

```
Project (项目)  ←─绑定─┐
                       │
Ticket (工单) ──1:N── PrdVersion (PRD 版本)
   │  └──1:N── ReviewIssue (预审问题，挂在某个版本上)
   │
   └──1:N── Task (子任务：BE / FE / BUG / DOC)
               └──1:N── Run (一次 Agent 运行)
                           └──1:N── RunEvent (日志事件)
```

- **工单（Ticket）**：需求或 Bug 的"容器"，承载工作流状态。
- **PRD 版本（PrdVersion）**：工单需求正文的不可变快照。工单的"当前 PRD"指向最新版本；定稿时记录"定稿版本"。
- **子任务（Task）**：定稿后拆出的、可派给 Agent 的最小执行单元，各自绑定一个项目和一个 worktree。
- **运行（Run）**：一次 Agent 进程的生命周期，包含 Prompt、Provider、日志、成本、结果。

## 2. 工单类型【已确认 + 建议细化】

| 类型 | 典型来源 | 是否走 PRD 审核 | 默认拆分 |
| --- | --- | --- | --- |
| `feature_backend` | 后端 PRD | 是 | 1 个 BE 子任务（含 API 文档产物） |
| `feature_frontend` | 前端 PRD | 是 | 1 个 FE 子任务 |
| `feature_mixed` | 前后端混合 PRD | 是 | BE（先）+ FE（依赖 BE 的 API 文档） |
| `bug` | Bug 反馈 | **否**（快速通道） | 1 个 BUG 子任务，绑定受影响项目 |
| `chore` | 重构、升级依赖等 | 可选 | 1 个子任务 |

## 3. 工单字段

| 字段 | 说明 |
| --- | --- |
| `key` | 人类可读编号，如 `TW-42` |
| `title` | 标题 |
| `type` | 见上表 |
| `status` | 见 [工作流](workflow.md) |
| `priority` | `urgent` / `high` / `medium` / `low` |
| `projects` | 绑定的项目列表及角色（frontend / backend） |
| `source` | `manual` / `file` / `feishu` / `github`，及原始链接或文件 |
| `current_version_id` | 当前 PRD 版本 |
| `finalized_version_id` | 定稿版本（未定稿为空） |
| `labels`、`due_date` | 管理字段 |
| `default_agent` / `default_provider` | 可覆盖项目默认值 |
| `bug_info` | 仅 Bug：复现步骤、期望结果、实际结果、环境、日志/截图 |
| `acceptance_criteria` | 验收标准清单（可由 AI 预审补全） |

## 4. PRD 版本

- 每次"保存"产生新版本，版本号递增（v1, v2, …），内容不可变。
- 每个版本记录：正文 Markdown、作者（`human` / `ai`）、修改说明、来源（导入 / 手动编辑 / 采纳 AI 建议 / 变更）。
- 支持任意两版之间的 Diff。
- **定稿后变更**：发起变更 → 基于定稿版本创建新草稿版本 → 工单回到"人工审核" → 再次定稿后，系统对比新旧定稿版本，提示哪些子任务受影响（见 [工作流 §5](workflow.md#5-定稿后的变更)）。

## 5. 子任务（Task）

| 字段 | 说明 |
| --- | --- |
| `kind` | `backend` / `frontend` / `bug` / `doc` |
| `project_id` | 在哪个项目执行 |
| `title`、`spec` | 从定稿 PRD 中摘出的、该端相关的需求片段 + 验收标准 |
| `depends_on` | 依赖的子任务及依赖的**产物**（如 `api_doc`） |
| `agent` / `provider` / `model` | 执行配置 |
| `permission_profile` | 权限等级（见 [执行](../03-agent/execution.md#6-权限策略)） |
| `branch` / `worktree_path` | 隔离环境 |
| `status` | `pending` / `blocked` / `queued` / `running` / `review` / `changes_requested` / `accepted` / `merged` / `cancelled` |
| `artifacts` | 产物：API 文档路径、测试报告、Diff 摘要 |

### 产物（Artifact）约定

- **API 文档**：后端子任务必须产出，约定路径 `docs/api/<ticket-key>.md`（或 OpenAPI `openapi/<ticket-key>.yaml`），内容包括接口路径、方法、请求/响应结构、错误码、示例。
- 后端子任务进入"API 文档就绪"后，前端子任务从 `blocked` 变为 `pending`，并在 Prompt 中自动附上该 API 文档。
- 【建议】API 文档也作为一个独立的"人工确认点"：可选开启"API 文档需确认后才解锁前端"。

## 6. 评论与时间线

工单有统一时间线，记录：状态变更、PRD 版本、预审结果、运行开始/结束、验收意见、合并记录。评论可 @ 某个子任务或某次运行。
