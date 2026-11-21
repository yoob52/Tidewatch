# 路线图

> 估时按"一人业余 / 借助 Agent 开发"粗估，仅供排期参考。

## M0 · 技术验证（约 1 周）
- [ ] Tauri 2 空壳 + React + SQLite 跑通
- [ ] Windows 下 spawn `claude -p --output-format stream-json` 并实时解析，Job Object 停止进程树
- [ ] spawn `codex exec --json`，解析事件与 session id，验证 `resume`
- [ ] 通过环境变量让 Claude Code 使用 DeepSeek Anthropic 兼容端点
- [ ] git worktree 创建 / 删除，长路径验证

**验收**：命令行级 Demo 能在一个 worktree 中让两种 Agent 各完成一个小改动并可中途停止。

## M1 · MVP：手动工单 + Agent 执行（约 2–3 周）
- [ ] 项目管理（本地目录、上下文卡片、命令）
- [ ] 工单创建（表单 / 粘贴 / md 导入）、看板
- [ ] Provider 配置（含凭据管理器）与按进程注入
- [ ] 子任务手动创建 → worktree → 运行 → 实时日志 → 停止 / 恢复
- [ ] 自动提交、自动测试、Diff 视图、合并、清理
- [ ] 成本与耗时统计、Windows 通知

**验收**：可以把一个 Bug 工单从创建跑到合并。

## M2 · 工作流与 PRD 审核（约 2–3 周）
- [ ] 完整状态机与 Bug 快速通道
- [ ] PRD 版本与对比
- [ ] AI 预审 → 问题清单 → 采纳 / 忽略 / 待确认
- [ ] 定稿锁定 + AI 拆分 FE/BE 子任务 + 依赖（API 文档产物解锁前端）
- [ ] 验收打回（行内评论 → 返工 Run，优先 resume）
- [ ] 定稿后变更流程与影响分析
- [ ] Prompt 模板可编辑与预览

**验收**：一个前后端混合 PRD 走完 草稿 → 预审 → 定稿 → BE 先出 API 文档 → FE 跟进 → 验收打回一次 → 完成。

## M3 · V1 完善（约 2–3 周）
- [ ] docx / pdf 导入、图片附件与描述
- [ ] 智能创建（自动识别类型 / 项目）
- [ ] AI 交叉 Review、测试失败自动修复
- [ ] 预算护栏、卡住检测、危险命令高亮
- [ ] 推送分支与 `gh pr create`
- [ ] 统计仪表盘、备份导出、自动更新
- [ ] LiteLLM 桥接引导

## M4 · V2 探索
- [ ] 飞书、GitHub Issues 同步
- [ ] 更多 Agent（Gemini CLI、Aider、OpenCode…）
- [ ] 多 Agent 同题对比
- [ ] 本地 HTTP / MCP 接口
- [ ] 自定义工作流
- [ ] 轻量协作（多人、远程访问）
