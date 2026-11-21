# 功能清单

优先级：**P0 = MVP 必须**，**P1 = V1**，**P2 = V2 及以后**。

## 1. 项目管理（Project）

| 功能 | 说明 | 优先级 |
| --- | --- | --- |
| 注册项目 | 指定本地目录或 git URL（URL 自动 clone 到工作区） | P0 |
| 项目类型 | `backend` / `frontend` / `fullstack` / `other` | P0 |
| 项目上下文卡片 | 技术栈、目录说明、代码规范、启动 / 测试 / 构建命令，作为 Agent 上下文 | P0 |
| 自动探测 | 根据 `package.json`、`pom.xml`、`go.mod`、`pyproject.toml` 等推测技术栈和命令 | P1 |
| 默认 Agent / Provider | 项目级默认值，工单可覆盖 | P0 |
| 项目健康检查 | 检查目录存在、是否 git 仓库、工作区是否干净、主分支名 | P1 |

## 2. 工单（Ticket）

| 功能 | 说明 | 优先级 |
| --- | --- | --- |
| 手动创建 / 粘贴 | 表单 + Markdown 编辑器，支持直接粘贴整段 PRD | P0 |
| 工单类型 | `feature_backend` / `feature_frontend` / `feature_mixed` / `bug` / `chore` | P0 |
| 绑定项目 | 绑定一个或多个项目（前端、后端） | P0 |
| 文件导入 | `.md` / `.docx` / `.pdf` → Markdown，保留图片附件 | P1（md 为 P0）|
| 附件管理 | 截图、原型图、接口样例 | P1 |
| PRD 图片效力 | 原始 PRD 图仅参考；可粘贴自己的截图替换或补充，并标记哪张是权威图；图片随预审和开发发给 Agent（见 [PRD 中的图片](../02-workflow/prd-images.md)） | P1 |
| 标签、优先级、截止日期 | 基本管理字段 | P0 |
| 看板 / 列表视图 | 按状态分列的看板；可筛选 | P0 |
| 全文搜索 | 标题、PRD、评论 | P1 |
| 外部同步 | 飞书文档 / 飞书项目、GitHub Issues 拉取 | P2 |
| 工单模板 | Bug 模板（复现步骤、期望、实际）、需求模板 | P1 |

## 3. 工作流与 PRD 审核

| 功能 | 说明 | 优先级 |
| --- | --- | --- |
| 状态机 | 草稿 → AI 预审 → 人工审核 → 已定稿 → 开发中 → 验收中 → 已完成 | P0 |
| AI 预审 | 输出结构化问题清单（严重度、类别、定位、建议） | P0 |
| 问题处理 | 每个问题可"采纳 / 忽略 / 已手动修改"，可一键让 AI 生成修改建议 | P0 |
| PRD 版本 | 每次保存产生新版本，可对比任意两版 | P0 |
| 定稿锁定 | 定稿后 PRD 只读；改动需"发起变更"产生新版本并回到审核 | P0 |
| 任务拆分 | 定稿时 AI 建议拆分 FE/BE 子任务，人工确认 | P0 |
| 子任务依赖 | 前端子任务依赖后端"API 文档"产物 | P0 |
| Bug 快速通道 | Bug 跳过预审与审核 | P0 |
| 验收打回 | 填写意见打回，自动生成下一轮运行 | P0 |
| 自定义工作流 | 用户自定义状态与转换 | P2 |

## 4. Agent 编排

| 功能 | 说明 | 优先级 |
| --- | --- | --- |
| Claude Code 适配 | `claude -p --output-format stream-json` | P0 |
| Codex 适配 | `codex exec --json` | P0 |
| DeepSeek | 通过 Claude Code 的 Anthropic 兼容端点 | P0 |
| 自定义 Provider | 名称、协议、URL、Key、模型；按工单 / Agent 选择 | P0 |
| 协议桥接 | 引导用户配置 LiteLLM 等代理，处理协议不匹配 | P1 |
| 运行队列 | 并发上限、排队、优先级 | P0 |
| 停止 / 恢复 | 停止 = 结束进程树；恢复 = 基于 session id 继续 | P0 |
| 实时日志 | 结构化事件流（消息、工具调用、文件修改） | P0 |
| 成本统计 | 耗时、输入/输出 Token、估算费用 | P0 |
| 权限策略 | 每次运行的沙箱 / 允许工具等级 | P0 |
| 多 Agent 对比 | 同一任务派给两个 Agent，比较结果择优 | P2 |
| AI 交叉 Review | 一个 Agent 写，另一个 Agent 评审 Diff | P1 |
| 其他 Agent | Gemini CLI、Aider、OpenCode 等 | P2 |

## 5. 代码隔离与交付

| 功能 | 说明 | 优先级 |
| --- | --- | --- |
| worktree / 分支 | 每个子任务一个 worktree 与分支 | P0 |
| 自动测试 | 运行后自动执行项目测试 / lint / 构建命令 | P0 |
| Diff 视图 | 文件树 + 并排 Diff | P0 |
| 合并 | 人工确认后合并到目标分支（merge / squash） | P0 |
| 推送 / 开 PR | 推送分支、调用 `gh` 开 PR | P1 |
| 在 IDE 中打开 | 一键用 VS Code / Cursor 打开 worktree | P0 |
| 清理 | 工单完成后清理 worktree | P0 |

## 6. 系统

| 功能 | 说明 | 优先级 |
| --- | --- | --- |
| 托盘与通知 | 运行完成 / 失败 / 需要人工时 Windows 通知 | P0 |
| 环境检测 | 检测 `git`、`claude`、`codex`、`node` 是否可用及版本 | P0 |
| 设置 | 工作区路径、并发上限、默认 Agent、主题 | P0 |
| 数据备份 / 导出 | 导出数据库与工单为 Markdown | P1 |
| 统计仪表盘 | 每周完成工单数、各 Agent 成功率与花费 | P1 |
| 本地 HTTP / MCP 接口 | 让外部 Agent 反向查询 / 更新工单 | P2 |
