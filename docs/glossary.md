# 术语表

| 术语 | 英文 / 标识 | 解释 |
| --- | --- | --- |
| 工单 | Ticket | 一个需求或 Bug，承载工作流状态 |
| PRD 版本 | PrdVersion | 需求正文的不可变快照 |
| AI 预审 | AI Review | 由 Agent 检查 PRD 并输出问题清单 |
| 问题清单 | Review Issues | 预审输出的结构化问题，带严重度与建议 |
| 定稿 | Finalize | 锁定 PRD 版本并拆分子任务 |
| 变更 | Change Request | 定稿后修改需求，产生新版本并回到审核 |
| 子任务 | Task | 定稿后拆出的执行单元（后端 / 前端 / Bug） |
| 产物 | Artifact | 子任务的输出，如 API 文档、测试报告 |
| 运行 | Run | 子任务的一轮工作（开发、返工、修复…），可包含多次尝试 |
| 尝试 | Attempt | Run 内的一次 Agent 进程 / 轮次（首次、引导重启、重试、崩溃恢复） |
| Agent | Agent | Claude Code、Codex 等本地 CLI 编码助手 |
| Provider | Provider | 模型供应商配置：协议、地址、Key、模型 |
| 适配器 | Adapter | 把某个 Agent 的 CLI 封装为统一接口的模块 |
| Agent 实例 | Agent Instance | 某种 Agent 的一份具体配置：可执行文件、配置目录（账号）、启动参数 |
| 能力声明 | Capabilities | 适配器如实上报的能力（resume、审批、引导…），界面与调度据此决定行为 |
| 运行时请求 | Runtime Request | Agent 运行中发起的审批或提问，持久化等待人回复 |
| 上下文交接 | Context Handoff | 换 Agent / 会话时生成的可追溯交接摘要 |
| 检查点 | Checkpoint | Run 前后的隐藏 git ref，用于 Diff 与回滚 |
| worktree | git worktree | 同一仓库的额外工作目录，每个子任务一个 |
| 验收 | Acceptance | 人工检查 Diff、测试与功能，决定通过或打回 |
| 打回 | Reject | 验收不通过并附意见，触发返工 |
| 快速通道 | Fast Track | Bug 跳过 PRD 审核直接开发 |
| 上下文卡片 | Project Context | 项目级背景信息，自动拼入 Prompt |
