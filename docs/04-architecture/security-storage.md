# 安全与本地存储

## 1. 目录布局

```
%APPDATA%\Tidewatch\              # 配置与数据库
  tidewatch.db
  settings.json
  agent-homes\                    # 可选：隔离的 CLAUDE_CONFIG_DIR / CODEX_HOME
<工作区>（默认 D:\Tidewatch 或用户指定）
  repos\<project>\                # 由 git URL clone 的项目
  worktrees\<project>\<key>-<kind>\
  attachments\<ticket-key>\
  logs\<run-id>.jsonl             # 原始运行日志
  tmp\<run-id>\                   # 本轮上下文文件（PRD 全文等）
```

工作区建议放在短路径、非系统盘，避免 Windows 长路径问题和杀毒软件频繁扫描（可提示用户把工作区加入 Defender 排除项）。

## 2. 密钥

- API Key 存储在 **Windows 凭据管理器**，数据库只存条目名 `secret_ref`。
- 注入时仅放入子进程环境变量；日志、命令记录、Prompt 预览中一律脱敏（`sk-****abcd`）。
- 导出 / 备份默认不包含密钥。

## 3. Agent 权限边界

- Agent 只在 worktree 中运行；默认档位不允许完全自动。
- `full_auto` 需在设置中逐项目开启，并在启动前二次确认。
- 运行环境变量白名单：只传必要的系统变量 + Provider 变量，避免把用户 shell 中的其他密钥泄露给 Agent。
- 观澜从不自动 `git push`；推送和开 PR 由用户点击触发。
- 危险命令高亮与（可选）拦截：基于 Agent 的工具调用事件识别。

## 4. 数据安全

- 本地优先：除用户配置的 Provider 外，不向任何服务发送数据；无遥测（或默认关闭）。
- 每日自动备份数据库（保留 7 份），支持一键导出工单为 Markdown 归档。
- 外部同步凭据（飞书、GitHub Token）同样存于凭据管理器。
