# 路线图

> 估时按"一人业余 / 借助 Agent 开发"粗估，仅供排期参考。

## M0 · 技术验证（约 1 周）
- [x] Tauri 2 空壳 + React + SQLite 跑通
- [ ] Windows 下 spawn `claude -p --output-format stream-json` 并实时解析，Job Object 停止进程树（Linux 上用真实 Claude Code 跑通改动 / 停止 / 续接；Windows 上 Job Object 与演示 Agent 由 CI 验证，真实 Claude Code 还没在 Windows 上跑过）
- [x] spawn `codex exec --json`，解析事件与 session id，验证 `resume`（Linux，经自定义 Provider）
- [ ] 通过环境变量让 Claude Code 使用 DeepSeek Anthropic 兼容端点（同一套注入已用另一个 Anthropic 兼容中转站跑通；DeepSeek 本身还没有 Key 实测）
- [x] git worktree 创建 / 删除，长路径验证
- [ ] 【建议】`CLAUDE_CONFIG_DIR` / `CODEX_HOME` 多实例隔离与登录；录制两种 Agent 的真实会话作为回放测试样本（见 [Agent 管理](../03-agent/agent-management.md)）

**验收**：命令行级 Demo 能在一个 worktree 中让两种 Agent 各完成一个小改动并可中途停止。

## M1 · MVP：手动工单 + Agent 执行（约 2–3 周）
- [x] 项目管理（本地目录、上下文卡片、命令）
- [x] 工单创建（表单 / 粘贴 / md 导入）、看板
- [x] Provider 配置（含凭据管理器）与按进程注入
- [x] 子任务手动创建 → worktree → 运行 → 实时日志 → 停止 / 恢复
- [x] 自动提交、自动测试、Diff 视图、合并、清理
- [ ] 成本与耗时统计、Windows 通知（统计已完成；通知已接入，还没在 Windows 上实际弹出过）
- [ ] 【建议】Agent 实例与能力声明、Run / Attempt、完成屏障（"收尾中"）、命令落盘与孤儿恢复、检查点 ref（Agent 实例与能力声明还没做，其余已完成）

**验收**：可以把一个 Bug 工单从创建跑到合并。

**M1 结果**：执行引擎用演示 Agent 的端到端测试走通了"建 Bug 工单 → 子任务 → 运行 → 停止 → 续接 → 检查 → 验收 → 合并 → 清理"，以及排队、幂等、失败、打回返工和孤儿恢复（Linux 与 Windows CI）。桌面界面的各页面在 Linux 上用真实数据打开看过；**界面上的操作（点击建单、开始、验收、合并）还没有逐项点过**，真实的 Claude Code / Codex 也还没有经桌面应用跑过（M0 只在命令行里验证）。这一版没有 AI 预审和 AI 拆分：「AI 预审」一列需要手动点「预审完成」，子任务要手动创建，这些在 M2。

## M2 · 工作流与 PRD 审核（约 2–3 周）
- [ ] 完整状态机与 Bug 快速通道
- [ ] PRD 版本与对比
- [ ] AI 预审 → 问题清单 → 采纳 / 忽略 / 待确认
- [ ] 定稿锁定 + AI 拆分 FE/BE 子任务 + 依赖（API 文档产物解锁前端）
- [ ] 验收打回（行内评论 → 返工 Run，优先 resume）
- [ ] 定稿后变更流程与影响分析
- [ ] Prompt 模板可编辑与预览
- [ ] 【建议】换 Agent / resume 失败时的显式上下文交接；追加指令的分发方式

**验收**：一个前后端混合 PRD 走完 草稿 → 预审 → 定稿 → BE 先出 API 文档 → FE 跟进 → 验收打回一次 → 完成。

## M3 · V1 完善（约 2–3 周）
- [ ] docx / pdf 导入、图片附件与描述；权威图 / 参考图标记并随预审、开发发给 Agent（见 [PRD 中的图片](../02-workflow/prd-images.md)）
- [ ] 智能创建（自动识别类型 / 项目）
- [ ] AI 交叉 Review、测试失败自动修复
- [ ] 预算护栏、卡住检测、危险命令高亮
- [ ] 推送分支与 `gh pr create`
- [ ] 统计仪表盘、备份导出、自动更新
- [ ] LiteLLM 桥接引导
- [ ] 【建议】交互通道（`codex app-server` 等）：逐项审批、结构化提问、`supervised` 档位、运行中引导、会话回滚

## M4 · V2 探索
- [ ] 飞书、GitHub Issues 同步
- [ ] 更多 Agent（Gemini CLI、Aider、OpenCode…）
- [ ] 多 Agent 同题对比
- [ ] 本地 HTTP / MCP 接口（【建议】含 Agent 委派工具，见 [Agent 管理 §12](../03-agent/agent-management.md)）
- [ ] 自定义工作流
- [ ] 轻量协作（多人、远程访问）
