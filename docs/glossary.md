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
| 运行 | Run | 一次 Agent 进程从启动到结束 |
| Agent | Agent | Claude Code、Codex 等本地 CLI 编码助手 |
| Provider | Provider | 模型供应商配置：协议、地址、Key、模型 |
| 适配器 | Adapter | 把某个 Agent 的 CLI 封装为统一接口的模块 |
| worktree | git worktree | 同一仓库的额外工作目录，每个子任务一个 |
| 验收 | Acceptance | 人工检查 Diff、测试与功能，决定通过或打回 |
| 打回 | Reject | 验收不通过并附意见，触发返工 |
| 快速通道 | Fast Track | Bug 跳过 PRD 审核直接开发 |
| 上下文卡片 | Project Context | 项目级背景信息，自动拼入 Prompt |
