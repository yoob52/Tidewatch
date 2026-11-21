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

## 8. 扩展其他 Agent（P2）

新增 Agent = 实现一个 `AgentAdapter`。候选：Gemini CLI、Aider、OpenCode、Qwen Code。适配器以配置 + 少量代码描述，未来可考虑声明式适配（命令模板 + JSON 路径映射）。
