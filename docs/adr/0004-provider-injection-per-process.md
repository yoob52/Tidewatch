# ADR-0004 Provider 按进程注入，不改全局配置

- 状态：已接受
## 背景
用户可能使用中转、国产模型或公司网关；同时日常也在用这些 CLI，不能被观澜改乱。
## 决策
Provider 作为独立配置（名称、协议、地址、Key、模型）。启动 Agent 时：Claude Code 注入 `ANTHROPIC_BASE_URL` / `ANTHROPIC_AUTH_TOKEN` / `ANTHROPIC_MODEL` 等环境变量；Codex 通过 `-c` 覆盖 `model_providers` 与 `model_provider`，Key 走环境变量。协议不匹配时通过 LiteLLM 等代理桥接。
## 影响
- 同时运行的两个 Agent 可使用不同 Provider。
- 不同 CLI 版本的配置项可能变化，适配器负责兼容。
