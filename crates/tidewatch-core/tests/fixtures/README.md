# 事件流样本

- `claude_not_logged_in.jsonl`、`codex_unauthorized.jsonl`：真实 CLI（Claude Code 2.1.330、codex-cli 0.171.0）在未登录状态下的输出，裁掉了无关的大字段。
- `claude_success.jsonl`、`codex_success.jsonl`：按两个 CLI 公开的事件格式手写的成功流，**不是真实录制**；拿到账号后应替换为真实录制。
- `claude_real_resume.jsonl`、`claude_real_api_retry.jsonl`、`codex_real_success.jsonl`：真实录制（Claude Code 2.1.330 + claude-fable-5-2、codex-cli 0.171.0 + gpt-6.1-sol，均经自定义 Provider 注入）。只把工作目录换成了 `/repo`，并去掉了 `init` 事件里的列表字段。
