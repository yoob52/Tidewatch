# 待决问题

| # | 问题 | 当前倾向 | 需要做什么 |
| --- | --- | --- | --- |
| Q1 | Tauri 还是 Electron？ | Tauri 2 | M0 验证 Rust 侧进程管理与开发体验；若团队/个人更熟 Node 可改 Electron |
| Q2 | Codex 在原生 Windows 下的沙箱能力是否足够，还是需要 WSL？ | 原生优先 | M0 实测 `--sandbox workspace-write` 行为 |
| Q3 | Codex 当前版本是否仍支持 `wire_api = "chat"`（用于 DeepSeek 等 OpenAI Chat 兼容端点）？ | 不支持则走 LiteLLM | 安装最新版验证 |
| Q4 | 是否默认隔离 `CLAUDE_CONFIG_DIR` / `CODEX_HOME`？会不会影响订阅登录态？ | 做成 Agent 实例的可选配置目录，默认留空（不隔离），见 [Agent 管理 §2](../03-agent/agent-management.md) | M0 验证 |
| Q5 | API 文档格式用 Markdown 还是 OpenAPI？ | 默认 Markdown，项目可选 OpenAPI | 看后端项目现状 |
| Q6 | API 文档是否需要人工确认后才解锁前端？ | 可选，默认自动解锁 | 用后再定 |
| Q7 | 预审默认用 Agent（可读代码）还是直接 Chat API？ | Agent 只读模式 | 对比质量与成本 |
| Q8 | 合并策略默认 squash 还是 merge？ | squash | 按个人习惯 |
| Q9 | 非 git 项目是否支持？ | P1 以快照方式支持 | — |
| Q10 | 飞书同步的具体对象：飞书文档、飞书项目、还是多维表格？ | 待需求方确认 | 确认实际 PRD 来源 |
| Q11 | 是否需要手机端查看 / 通知（例如运行完成推送到手机）？ | 暂不做 | 后续可通过 Webhook 实现 |
| Q12 | 费用估算的价格表是否内置？ | 用户在 Provider 中填写 | — |
| Q13 | 需求方粘贴的截图是否默认标为权威图？ | 默认权威，可改 | 用后再定，见 [PRD 中的图片](../02-workflow/prd-images.md) |
| Q14 | 是否允许把原始 PRD 图直接标为权威图？ | 允许，但需手动确认 | 待需求方确认 |
| Q15 | 开发阶段是否默认不发已被取代的参考图？ | 默认不发，预审阶段全部发 | 对比效果后再定 |
| Q16 | 交互通道怎么接：Codex 用 `codex app-server`（stdio JSON-RPC）；Claude Code 用 CLI 的流式输入 / 权限回调，还是引入 Node 跑 Claude Agent SDK？ | M0/M1 只用 headless CLI；交互通道 M3 再接，Claude 侧优先不引入 Node | M2 结束前分别验证两种 Agent 的审批与引导协议，见 [Agent 管理 §5](../03-agent/agent-management.md) |
| Q17 | 等待审批 / 回答的 Run 是否占用并发名额？ | 默认不占 | 用后再定 |
| Q18 | 上下文交接摘要由谁生成：离开的 Agent、接手的 Agent、便宜小模型，还是本地规则拼装？ | 本地规则拼装 + 可选离开的 Agent 总结 | 对比质量与成本 |
| Q19 | 是否提供一键升级 Agent CLI？ | 不提供，只提示 | 需能识别安装来源（npm / winget / scoop）后再考虑 |
