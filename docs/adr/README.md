# 架构决策记录（ADR）

格式：背景 → 决策 → 备选方案 → 影响。状态：提议 / 已接受 / 已废弃。

| 编号 | 标题 | 状态 |
| --- | --- | --- |
| [0001](0001-desktop-shell-tauri.md) | 桌面壳采用 Tauri 2 | 提议 |
| [0002](0002-agents-as-cli-subprocess.md) | Agent 以 CLI 子进程方式接入 | 已接受 |
| [0003](0003-worktree-per-ticket.md) | 每个子任务一个 git worktree | 已接受 |
| [0004](0004-provider-injection-per-process.md) | Provider 按进程注入，不改全局配置 | 已接受 |
| [0005](0005-local-first-sqlite.md) | 本地优先，SQLite 存储 | 已接受 |
| [0006](0006-agent-instances-and-capabilities.md) | Agent 实例 + 能力声明 + 应用自有运行模型 | 提议 |
