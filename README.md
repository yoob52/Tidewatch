# 观澜 · Tidewatch

> 观澜：静观潮汐，掌控每一次涨落。—— 一个为独立全栈开发者打造的、本地优先的个人工单系统，用来把 PRD / Bug 变成可审核、可调度、可追踪的 AI Agent 开发任务。

## 一句话定位

**在你自己的 Windows 主机上，把"收到的需求"一路推进到"合并的代码"：PRD 预审 → 人工定稿 → 拆分前后端子任务 → 自动调度 Claude Code / Codex / DeepSeek 等本地 Agent → 实时观察 → 人工验收。**

## 当前状态

🚧 **开发中**。M0 技术验证和 M1 MVP 已落地：项目、工单与看板、Provider、子任务在独立 worktree 里交给 Agent 运行（实时日志、停止 / 恢复）、自动提交与检查、Diff 验收、合并、统计。AI 预审与自动拆分在 M2，见 [路线图](docs/06-roadmap/roadmap.md)。

## 开发

代码结构：

| 路径 | 内容 |
| --- | --- |
| `crates/tidewatch-core` | Rust 核心（不依赖 Tauri）：SQLite 迁移与各表的读写（`store/`）、工作流规则、执行引擎（`engine.rs`：排队、并发、worktree、运行、提交、检查、合并）、Agent 适配器（Claude Code / Codex / 演示 Agent）、进程树、Provider 注入、密钥存放 |
| `crates/twctl` | 命令行：`doctor`、`run`、`plan`、`demo`、`worktree`，以及内置的演示 Agent |
| `src-tauri` | Tauri 2 桌面壳，把核心暴露成命令 |
| `src` | React + TypeScript 前端，样式沿用 `prototype-v2/` |

```bash
npm install
npm run tauri dev          # 启动桌面应用（开发）
npm run tauri build        # 打包（Windows 上产出 NSIS 安装包）
cargo test -p tidewatch-core -p twctl
```

没有 Claude Code / Codex 账号也能试：在「设置」里把默认 Agent 选成「演示 Agent」，它会在 worktree 里写一个 `TIDEWATCH_DEMO.md`。开发时可以用环境变量 `TIDEWATCH_DATA_DIR` 把数据库和 worktree 放到别的目录。

命令行演示（M0）：在一个 git 仓库里建 worktree，让 Agent 做小改动，中途停止再续接，提交后看 diff 并清理。

```bash
cargo run -p twctl -- doctor
cargo run -p twctl -- demo --repo <仓库路径> --agents mock                # 不需要账号
cargo run -p twctl -- demo --repo <仓库路径> --agents claude_code,codex   # 需要本机已登录
DEEPSEEK_API_KEY=... cargo run -p twctl -- run --agent claude_code --deepseek --cwd <目录> --prompt "..."
# 其它兼容端点（中转站等）：Claude Code 走 Anthropic 协议，Codex 走 OpenAI Responses 协议
MY_KEY=... cargo run -p twctl -- run --agent codex --base-url https://example.com/v1 --api-key-env MY_KEY --model <模型> --cwd <目录> --prompt "..."
```

Agent 子进程只继承系统变量（PATH、用户目录、代理、工具链位置等）和它自己的配置变量；shell 里的其它变量不会传过去，需要时用 `--pass-env NAME` 点名放行。

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
