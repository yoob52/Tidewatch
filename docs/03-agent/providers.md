# 自定义 Provider

## 1. 目标【已确认】

用户可能不使用官方渠道（中转站、公司网关、国产模型、本地模型），所以 **Provider 是独立配置**，可以按工单 / 子任务 / Agent 选择，并且**按进程注入，不修改全局配置**。

## 2. Provider 模型

| 字段 | 说明 |
| --- | --- |
| `name` | 显示名，如"DeepSeek""公司网关""OpenRouter" |
| `protocol` | `anthropic` / `openai_responses` / `openai_chat` |
| `base_url` | API 地址 |
| `api_key` | 存于 Windows 凭据管理器，数据库只存引用（见 [安全](../04-architecture/security-storage.md)） |
| `models` | 可用模型列表及默认模型；可选"小/快模型"（用于预审、摘要等轻任务） |
| `extra_headers` / `extra_env` | 额外请求头或环境变量 |
| `pricing` | 可选：每百万 Token 价格，用于估算费用 |
| `proxy` | 可选：HTTP 代理 |

## 3. 兼容矩阵

| Agent | 原生支持的协议 | 注入方式 |
| --- | --- | --- |
| Claude Code | `anthropic` | 环境变量 `ANTHROPIC_BASE_URL`、`ANTHROPIC_AUTH_TOKEN`（或 `ANTHROPIC_API_KEY`）、`ANTHROPIC_MODEL`、`ANTHROPIC_SMALL_FAST_MODEL` |
| Codex | `openai_responses`（及部分版本的 `openai_chat`） | `-c` 覆盖 `model_providers.<id>`（`name`、`base_url`、`env_key`、`wire_api`）并 `-c model_provider=<id>`；Key 通过 `env_key` 指向的环境变量注入 |

**协议不匹配**（如想让 Claude Code 用只有 OpenAI 协议的模型）：通过 LiteLLM 等代理桥接。观澜提供：
- P1：检测到不匹配时给出提示和 LiteLLM 配置示例；
- P2：可选内置/托管一个本地 LiteLLM 进程，Provider 选择"经由桥接"即可。

## 4. 按进程注入【已确认】

```
RunRequest.provider ──▶ Adapter.buildCommand() ──▶ { env, args }
                                              │
                         只作用于本次子进程，不写 ~/.claude、~/.codex
```

- Claude Code：仅设置子进程环境变量。【建议】额外设置 `CLAUDE_CONFIG_DIR` 指向观澜管理的独立目录，避免与用户日常使用的会话/设置相互影响（需验证对登录态的影响，可做成开关）。
- Codex：使用 `-c` 覆盖；【建议】必要时设置 `CODEX_HOME` 指向观澜管理的目录，在其中生成专用 `config.toml`，彻底隔离。
- 使用"官方默认"Provider 时不注入任何变量，沿用用户本机的登录态（如 Claude 订阅 / ChatGPT 登录）。

## 5. 选择优先级

```
子任务指定 > 工单指定 > 项目默认 > Agent 默认 > 全局默认
```

另外可为不同用途指定不同 Provider：如"预审用便宜模型、开发用强模型"。

## 6. 连通性测试

保存 Provider 时可"测试连接"：用最小请求验证 URL、Key、模型可用，并测得延迟；可进一步"用 Agent 测试"——以该 Provider 启动 Agent 执行一个 `echo` 级别的任务，验证端到端可用。

## 7. 预置模板

| 模板 | 协议 | 适用 Agent |
| --- | --- | --- |
| 官方默认（本机登录态） | — | Claude Code / Codex |
| Anthropic API | anthropic | Claude Code |
| OpenAI API | openai_responses | Codex |
| DeepSeek（Anthropic 兼容） | anthropic | Claude Code |
| DeepSeek（OpenAI 兼容） | openai_chat | Codex（视版本）/ 经 LiteLLM |
| 自定义 Anthropic 兼容 | anthropic | Claude Code |
| 自定义 OpenAI 兼容 | openai_* | Codex |
| 本地 LiteLLM 桥接 | 任意 → 任意 | 两者 |
