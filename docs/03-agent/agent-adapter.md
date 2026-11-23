# Agent 适配层

## 1. 思路【已确认】

不自研 Agent，也不依赖各家 SDK 的进程内集成，而是把 **CLI Agent 当作子进程**来调度：给定"工作目录 + Prompt + 环境变量"，读取其结构化（JSON 行）输出。

- 启动 = spawn 子进程；
- 停止 = 结束进程树；
- 恢复 = 以 session id 继续同一会话；
- 观测 = 解析 stdout 的 JSONL 事件流。

> 下文命令参数以写作时（2026-11）的公开文档为准，实现前需按安装的版本复核，适配器需要做版本探测。

## 2. 统一接口

```ts
interface AgentAdapter {
  id: 'claude-code' | 'codex' | string;
  detect(): Promise<{ installed: boolean; version?: string; path?: string }>;
  buildCommand(req: RunRequest): { cmd: string; args: string[]; env: Record<string,string>; cwd: string; stdin?: string };
  parseLine(line: string): RunEvent[];          // stdout 每行 → 统一事件
  extractSessionId(events: RunEvent[]): string | undefined;
  extractUsage(events: RunEvent[]): Usage | undefined;
  supports: { resume: boolean; readOnly: boolean; images: boolean; costReport: boolean };
}

interface RunRequest {
  cwd: string;                 // worktree 路径
  prompt: string;
  resumeSessionId?: string;
  provider: ResolvedProvider;  // 见 providers.md
  model?: string;
  permission: 'read_only' | 'workspace_write' | 'full_auto';
  extraArgs?: string[];
}
```

### 统一事件（RunEvent）

| 类型 | 含义 |
| --- | --- |
| `session_started` | 拿到 session id、模型名 |
| `assistant_message` | Agent 的文本输出 |
| `tool_call` / `tool_result` | 调用工具（读文件、执行命令、编辑文件）及结果 |
| `file_changed` | 文件被修改（从工具调用推断，或运行结束后由 git diff 补全） |
| `usage` | Token 用量 / 费用 |
| `error` | 错误 |
| `completed` | 结束，含最终总结、是否成功 |

原始 JSONL 同时完整落盘，便于调试和适配器升级后重放。

> 【建议】`supports` 将扩展为完整的能力声明 `AgentCapabilities`，并引入 Agent 实例（同一 Agent 多账号 / 多配置）与两种通道（`headless_cli` / `interactive`），见 [Agent 管理 §2–§3、§5](agent-management.md)。

## 3. Claude Code

```powershell
claude -p "<prompt>" `
  --output-format stream-json --verbose `
  --model <model> `
  --permission-mode <default|acceptEdits|plan|bypassPermissions> `
  [--allowedTools "Read,Edit,Bash(npm test:*)"] `
  [--resume <session_id>]
```

- 长 Prompt 通过 **stdin** 传入，避免 Windows 命令行长度限制（约 32K 字符）和转义问题。
- `stream-json` 输出中，`system/init` 事件含 `session_id`；`result` 事件含最终结果、`total_cost_usd`、`usage`、`duration_ms`。
- 自定义 Provider / DeepSeek：通过环境变量注入，见 [providers.md](providers.md)。
- 权限映射：`read_only` → `plan` 或只允许只读工具；`workspace_write` → `acceptEdits` + 允许列表；`full_auto` → `bypassPermissions`（仅在 worktree 内，需用户在设置中显式开启）。
- 无头模式下无法交互式审批工具调用，因此权限必须**预先**通过参数决定。

## 4. Codex

```powershell
codex exec --json `
  -C <worktree> `
  -m <model> `
  --sandbox <read-only|workspace-write|danger-full-access> `
  [-c model_provider=<id>] [-c model_providers.<id>.base_url=...] `
  "<prompt>"

# 恢复
codex exec --json resume <session_id> "<follow-up prompt>"
```

- `--json` 输出 JSONL 事件（线程开始含 thread/session id、条目包括消息、命令执行、文件修改、`turn.completed` 中的 usage）。
- Prompt 也可从 stdin 读取（传 `-`），长 Prompt 同样用 stdin。
- Windows 上 Codex 的沙箱能力与 WSL 的关系需验证（见 [待决问题](../06-roadmap/open-questions.md)）。
- 权限映射：`read_only` → `--sandbox read-only`；`workspace_write` → `--sandbox workspace-write`；`full_auto` → `--full-auto` 或 `danger-full-access`（需显式开启）。

## 5. DeepSeek【已确认】

DeepSeek 不作为独立 Agent，而是作为 **Provider** 接入：

- **主路径**：Claude Code + DeepSeek 的 Anthropic 兼容端点
  `ANTHROPIC_BASE_URL=https://api.deepseek.com/anthropic`，`ANTHROPIC_AUTH_TOKEN=<key>`，`ANTHROPIC_MODEL=deepseek-chat`（或推理模型）。
- **备选**：Codex + OpenAI 兼容端点（`https://api.deepseek.com`，通过 `model_providers` 配置），需验证 Codex 当前版本对 Chat Completions 协议的支持情况；不支持时经 LiteLLM 桥接。

在界面上体现为预置的 Provider 模板"DeepSeek（Anthropic 兼容）"，用户只需填写 Key。

## 6. 进程管理（Windows 要点）

| 问题 | 方案 |
| --- | --- |
| `claude` / `codex` 实际是 `.cmd` 包装 | 通过 `where` 解析真实路径；必要时直接以 `node <cli.js>` 启动 |
| 停止要杀整个进程树 | 使用 Windows **Job Object** 关联子进程，停止时 `TerminateJobObject`；兜底 `taskkill /T /F /PID` |
| 优雅停止 | 先发送 Ctrl+Break / 关闭 stdin，等待 N 秒再强杀 |
| 编码 | 强制 UTF-8（设置 `PYTHONIOENCODING`、`chcp 65001` 不可靠，以字节流按 UTF-8 解码） |
| 路径 | 统一使用绝对路径；注意 worktree 路径不要过长（git `core.longpaths=true`） |
| 应用退出 | 询问是否停止运行中的任务；未停止的进程在下次启动时标记为 `orphaned` 并尝试清理 |

## 7. 会话恢复策略

1. 优先用原 Agent + 原 session id 恢复（保留完整上下文）。
2. 若恢复失败（会话过期、换了 Agent / Provider），新开会话，Prompt 中附带：原任务、上一轮总结、当前 `git diff --stat`、打回意见。
3. 每个 Run 记录 `parent_run_id`，形成运行链。

> 【建议】第 2 步的"附带总结"升级为显式的上下文交接记录（策略 `delta` / `full_summary` / `manual`，可查看、可作废），见 [Agent 管理 §7](agent-management.md)。

## 8. 扩展其他 Agent（P2）

新增 Agent = 实现一个 `AgentAdapter`。候选：Gemini CLI、Aider、OpenCode、Qwen Code。适配器以配置 + 少量代码描述，未来可考虑声明式适配（命令模板 + JSON 路径映射）。

已实现（M4）：

| Agent | 调用方式 | 续接 | 只读模式 | Provider 协议 | 验证情况 |
|---|---|---|---|---|---|
| OpenCode | `opencode run --format json`，Prompt 走 stdin；Provider 通过 `OPENCODE_CONFIG_CONTENT` 内联注入，不改用户配置文件 | `-s <会话 id>` | 权限 `edit` / `bash` 设为 deny | Anthropic、OpenAI Responses、OpenAI Chat | 真实跑通：改动、停止续接、预审与拆分（OpenAI 兼容端点）。Anthropic 协议未验证 |
| Gemini CLI | `gemini -o stream-json`，Prompt 走 stdin | `-r <会话 id>` | `--approval-mode plan` | Gemini | 只验证了无效 key 的报错路径；成功路径按文档实现，**未用真实账号验证** |
| Aider | `aider --message-file`，纯文本输出，按行识别改动、用量和报错 | `--restore-chat-history`（对话记录在 worktree 里，没有会话 id） | `--dry-run` | Anthropic、OpenAI Responses、OpenAI Chat | 真实跑通：改动、预审与拆分（OpenAI 兼容端点）。Anthropic 协议未验证 |

Aider 的限制：
- 它不会自己读文件，只看得到仓库地图；Prompt 里没点名的已有文件，它可能改不到。
- 模型调用失败时退出码仍是 0，成败从输出里的 `litellm.*Error` 判断。
- 它在工作目录生成 `.aider*` 文件；运行前写进 `.git/info/exclude`，不进提交。
- 不是终端时按 80 列折行，会断开 JSON；适配器把 `COLUMNS` 设大来避免。

## Windows 上的 Shell

设置里的「Agent 使用的 Shell」（`agent_shell`）只改这次运行的环境变量，见 `crates/tidewatch-core/src/shell.rs`：

| 选项 | Codex | Claude Code |
|---|---|---|
| `auto` | 优先使用 PATH 上的实际 `pwsh.exe`；只有 Store 别名时查找实际安装路径；否则回退到系统的 `powershell.exe` | 默认 Git Bash |
| `pwsh` | 使用 PATH 上已有的 PowerShell 7，或将 MSI / Microsoft Store 的实际安装目录加入 PATH 最前 | 同左，并设 `CLAUDE_CODE_USE_POWERSHELL_TOOL=1` |
| `powershell` | 从 PATH 去掉所有带 `pwsh.exe` 的目录 | 同左，并设 `CLAUDE_CODE_USE_POWERSHELL_TOOL=1` |
| `git_bash` | 不受影响（Codex 只用 PowerShell） | `CLAUDE_CODE_GIT_BASH_PATH` 指向 Git for Windows 的 bash |

任何选项下都会从 PATH 去掉装着 Store 版 `pwsh.exe` 别名的 WindowsApps 目录：Agent 的沙箱启动它会报「拒绝访问」（错误 5）。Codex 自动选 Shell 时，如果 PATH 上只有 Store 别名，就尝试使用已注册包中的 `pwsh.exe`。Codex 不读 `SHELL` / `COMSPEC`，两个 Agent 都不能用 cmd 当 Shell，所以没有 cmd 选项。

