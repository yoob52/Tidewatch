# ADR-0002 Agent 以 CLI 子进程方式接入

- 状态：已接受
## 背景
需要同时驱动 Claude Code、Codex，并支持 DeepSeek 与自定义 Provider。
## 决策
以子进程运行 `claude -p --output-format stream-json` 与 `codex exec --json`，传入工作目录与 Prompt，解析 JSONL；停止即结束进程树，恢复用 session id。
## 备选
- **各家 SDK（Claude Agent SDK、Codex SDK）**：集成更深，但需要 Node/Python 运行时并跟随 SDK 变更；可作为未来适配器的另一种实现。
- **自研 Agent 循环直接调模型 API**：工作量大，且失去 CLI 已有的工具、权限与项目规则能力。
## 影响
- 与用户日常使用的 CLI 行为一致，复用其登录态和 `CLAUDE.md` / `AGENTS.md`。
- 依赖 CLI 输出格式的稳定性，适配器需做版本探测并保存原始日志以便重放。
