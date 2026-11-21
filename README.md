# 观澜 · Tidewatch

> 观澜：静观潮汐，掌控每一次涨落。—— 一个为独立全栈开发者打造的、本地优先的个人工单系统，用来把 PRD / Bug 变成可审核、可调度、可追踪的 AI Agent 开发任务。

## 一句话定位

**在你自己的 Windows 主机上，把"收到的需求"一路推进到"合并的代码"：PRD 预审 → 人工定稿 → 拆分前后端子任务 → 自动调度 Claude Code / Codex / DeepSeek 等本地 Agent → 实时观察 → 人工验收。**

## 当前状态

📐 **设计阶段**。本仓库目前只包含设计文档，尚未开始编码。

## 文档导航

| 文档 | 内容 |
| --- | --- |
| [docs/README.md](docs/README.md) | 文档总索引与阅读顺序 |
| [产品愿景与痛点](docs/01-product/vision.md) | User Story、痛点、设计原则、非目标 |
| [功能清单](docs/01-product/features.md) | 全部功能模块与 MVP / V1 / V2 分级 |
| [工单模型](docs/02-workflow/ticket-model.md) | 工单类型、字段、子任务、版本 |
| [工作流与状态机](docs/02-workflow/workflow.md) | 状态、流转规则、Bug 快速通道、变更回流 |
| [PRD 审核](docs/02-workflow/prd-review.md) | AI 预审、问题清单、人工审核、版本对比 |
| [工单来源](docs/02-workflow/ticket-sources.md) | 手动、文件导入、外部同步 |
| [Agent 适配层](docs/03-agent/agent-adapter.md) | Claude Code / Codex / DeepSeek 接入 |
| [自定义 Provider](docs/03-agent/providers.md) | 模型供应商配置与按进程注入 |
| [执行与隔离](docs/03-agent/execution.md) | worktree、并发、日志、成本、测试、Diff |
| [Prompt 与上下文](docs/03-agent/prompt-context.md) | 如何给 Agent 组装任务上下文 |
| [系统架构](docs/04-architecture/overview.md) | Tauri 进程模型、模块划分 |
| [数据模型](docs/04-architecture/data-model.md) | SQLite 表结构 |
| [安全与存储](docs/04-architecture/security-storage.md) | 密钥、目录布局、权限边界 |
| [界面设计](docs/05-ui/ui-design.md) | 页面、交互与线框 |
| [路线图](docs/06-roadmap/roadmap.md) | 里程碑与验收标准 |
| [待决问题](docs/06-roadmap/open-questions.md) | 尚未拍板的问题 |
| [架构决策记录](docs/adr/) | ADR |
| [术语表](docs/glossary.md) | 统一用词 |
