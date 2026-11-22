# 事件流样本

- `claude_not_logged_in.jsonl`、`codex_unauthorized.jsonl`：真实 CLI（Claude Code 2.1.330、codex-cli 0.171.0）在未登录状态下的输出，裁掉了无关的大字段。
- `claude_success.jsonl`、`codex_success.jsonl`：按两个 CLI 公开的事件格式手写的成功流，**不是真实录制**；拿到账号后应替换为真实录制。
- `claude_real_resume.jsonl`、`claude_real_api_retry.jsonl`、`codex_real_success.jsonl`：真实录制（Claude Code 2.1.330 + claude-fable-5-2、codex-cli 0.171.0 + gpt-6.1-sol，均经自定义 Provider 注入）。只把工作目录换成了 `/repo`，并去掉了 `init` 事件里的列表字段。
- `opencode_edit.jsonl`、`opencode_resume_readonly.jsonl`、`opencode_error.jsonl`：真实录制（OpenCode 1.19.6 + gpt-6.1-sol，经 `OPENCODE_CONFIG_CONTENT` 注入自定义 Provider）。依次是：改文件；用 `-s` 续接且 `edit` / `bash` 为 deny；无效 key。工作目录换成了 `/repo`。
- `aider_edit.txt`、`aider_bad_key.txt`：真实录制（Aider 0.86.2 + gpt-6.1-sol）。Aider 只有纯文本输出；无效 key 那次的退出码是 0。
- `gemini_bad_key.jsonl`：真实录制（Gemini CLI 0.70.0，无效 key）。
- `gemini_success.jsonl`：按 Gemini CLI 文档里的 `stream-json` 事件格式手写的成功流，**不是真实录制**（没有可用的 Gemini 账号）。
