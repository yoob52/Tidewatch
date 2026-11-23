# 观澜 · Tidewatch

> 观澜：静观潮汐，掌控每一次涨落。—— 一个为独立全栈开发者打造的、本地优先的个人工单系统，用来把 PRD / Bug 变成可审核、可调度、可追踪的 AI Agent 开发任务。

## 当前功能

- **收集工单**：手动录入，导入 Markdown、TXT、DOCX 或 PDF，保留图片附件；也可以同步 GitHub Issues 或按链接导入飞书云文档。
- **审核与拆分**：AI 预审生成问题清单，人审后定稿；保留 PRD 版本及对比，支持 AI 拆分子任务、依赖、变更影响分析和 Bug 快速通道。
- **运行与验收**：在独立 Git worktree 中排队运行 Agent，实时查看日志，停止后续接；支持自动提交和检查、Diff 验收、打回返工，以及手动推送分支并发起 PR。
- **Agent 与 Provider**：支持 Claude Code、Codex、OpenCode、Aider、Gemini CLI 和内置演示 Agent；可以为兼容的 Agent 配置自定义 Provider，例如让 Claude Code 使用 DeepSeek 的 Anthropic 兼容端点。
- **本地数据与协作**：项目上下文、工单和运行记录保存在本机 SQLite，可备份数据库并导出工单；可选开启本地 REST / MCP 接口，配置成员与角色、指派工单，或在内置流程中加入自定义环节。

未完成的功能及验证进度见[路线图](docs/06-roadmap/roadmap.md)。

## 安装与快速上手

1. 在 [GitHub Releases](https://github.com/yoob52/Tidewatch/releases) 下载可用的 Windows 安装包（`Tidewatch_*_x64-setup.exe`）。也可以按下文从源码构建。
2. 安装 Git，并在「项目」里添加一个本地 Git 仓库。要使用真实 Agent，请另行安装对应的 CLI 并登录，或在「Provider」中配置兼容的模型服务。
3. 先试跑时，可以在「设置 → 默认执行配置」选「演示 Agent」，无需账号。创建工单并启动一个子任务后，它会在 worktree 里写一个 `TIDEWATCH_DEMO.md`，可以用来查看运行日志和 Diff 验收流程。

应用右上角的 GitHub 图标会在系统浏览器中打开仓库。旁边的更新按钮可手动检查版本；检测到新版本后会显示提示，你可以查看更新说明、关闭窗口稍后处理，或者手动确认下载安装。没有签名公钥或安装方式不支持应用内更新时，请从发布页下载安装包。可在「设置 → 本机环境」开启启动时检查（默认关闭）；检查不会自动下载或安装更新。

## 开发

代码结构：

| 路径 | 内容 |
| --- | --- |
| `crates/tidewatch-core` | Rust 核心（不依赖 Tauri）：SQLite 迁移与各表的读写（`store/`）、工作流规则、执行引擎（`engine.rs`：排队、并发、worktree、运行、提交、检查、合并）、Agent 适配器（Claude Code / Codex / 演示 Agent）、进程树、Provider 注入、密钥存放 |
| `crates/twctl` | 命令行：`doctor`、`run`、`plan`、`demo`、`worktree`，以及内置的演示 Agent |
| `src-tauri` | Tauri 2 桌面壳，把核心暴露成命令 |
| `src` | React + TypeScript 前端 |
| `prototype/`、`prototype-v2/` | 独立界面原型，不包含真实桌面后端 |

需要 Node.js 22（与 CI 一致）、npm、稳定版 Rust、Git，以及对应系统的 [Tauri 2 开发依赖](https://v2.tauri.app/start/prerequisites/)。Windows 需要 MSVC 构建工具和 WebView2；Linux 开发需要 WebKitGTK 4.1 等系统库。

```bash
npm ci
npm run tauri dev          # 启动桌面应用（开发）
npm run build              # TypeScript、英文文案检查与前端构建
npm run tauri build        # 打包（Windows 上产出 NSIS 安装包）
cargo test -p tidewatch-core -p twctl
```

开发时可以设置 `TIDEWATCH_DATA_DIR`，把数据库和默认工作区放到独立的数据目录。仅用 `npm run dev` 启动前端没有 Tauri 后端，不能完整使用桌面功能。

命令行演示：在一个 Git 仓库里建 worktree，让 Agent 做小改动，中途停止再续接，提交后看 diff 并清理。

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
| [PRD 中的图片](docs/02-workflow/prd-images.md) | 图片导入、权威图与参考图、Agent 输入 |
| [工单来源](docs/02-workflow/ticket-sources.md) | 手动、文件导入、外部同步 |
| [Agent 适配层](docs/03-agent/agent-adapter.md) | Claude Code / Codex / DeepSeek 接入 |
| [Agent 管理](docs/03-agent/agent-management.md) | Agent 实例、能力与接入规划 |
| [自定义 Provider](docs/03-agent/providers.md) | 模型供应商配置与按进程注入 |
| [执行与隔离](docs/03-agent/execution.md) | worktree、并发、日志、成本、测试、Diff |
| [Prompt 与上下文](docs/03-agent/prompt-context.md) | 如何给 Agent 组装任务上下文 |
| [系统架构](docs/04-architecture/overview.md) | Tauri 进程模型、模块划分 |
| [本地接口](docs/04-architecture/local-api.md) | REST、MCP、令牌和访问范围 |
| [数据模型](docs/04-architecture/data-model.md) | SQLite 表结构 |
| [安全与存储](docs/04-architecture/security-storage.md) | 密钥、目录布局、权限边界 |
| [界面设计](docs/05-ui/ui-design.md) | 页面、交互与线框 |
| [路线图](docs/06-roadmap/roadmap.md) | 里程碑与验收标准 |
| [待决问题](docs/06-roadmap/open-questions.md) | 尚未拍板的问题 |
| [架构决策记录](docs/adr/) | ADR |
| [术语表](docs/glossary.md) | 统一用词 |
