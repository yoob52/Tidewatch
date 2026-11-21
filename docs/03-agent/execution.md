# 执行、隔离与观测

## 1. 工作区隔离【已确认】

- **每个子任务一个分支 + 一个 git worktree**：
  - 分支：`tw/<ticket-key>-<kind>`，从项目目标分支（默认 `main`/`master`，可配置）的最新提交切出。
  - worktree 路径：`<工作区>/worktrees/<project>/<ticket-key>-<kind>`（放在项目目录之外，避免被项目工具扫描）。
- 用户日常开发的主目录不受 Agent 影响。
- Bug 工单同样使用 worktree。
- 依赖安装：worktree 首次创建后执行项目的"准备命令"（如 `pnpm install`）。【建议】对 Node 项目优先 pnpm 以共享依赖存储，减少磁盘占用。
- 非 git 项目：退化为复制目录快照（P1），并提示用户初始化 git。

## 2. 调度与并发【已确认】

- 全局并发上限（默认 2），可按 Agent / Provider 单独设上限（应对 API 限流）。
- 队列排序：工单优先级 → 入队时间；依赖未满足的子任务为 `blocked`，不占队列。
- 同一 worktree 同一时间只允许一个 Run。
- 【建议】资源保护：本机 CPU / 内存过高时暂停出队。

## 3. 一次 Run 的生命周期

```
准备 ─▶ 组装 Prompt ─▶ 启动进程 ─▶ 流式解析 ─▶ 进程结束
                                            │
              ┌─────────────────────────────┘
              ▼
   收集 git diff / 提交 ─▶ 自动测试 ─▶ (失败且可自动修复? ─▶ 新 Run) ─▶ 进入 review
```

1. **准备**：确认 worktree 干净或处于上一轮状态；记录起始 commit。
2. **组装 Prompt**：见 [prompt-context.md](prompt-context.md)。
3. **启动**：适配器生成命令，注入 Provider 环境，写入 Run 记录。
4. **流式解析**：事件写入数据库与原始日志文件，推送到前端。
5. **结束后**：
   - 若 Agent 未提交，观澜自动 `git add -A && git commit -m "tw(<key>): run #<n>"`，保证每轮一个提交，便于回滚；
   - 计算 Diff 统计；
   - 运行自动测试。

## 4. 可观测性【已确认】

| 维度 | 内容 |
| --- | --- |
| 实时日志 | 结构化时间线：消息、工具调用（可折叠）、命令输出、文件修改；支持"原始 JSON"视图 |
| 状态 | 排队 / 运行中 / 等待测试 / 完成 / 失败 / 已停止；当前在做什么（最近一次工具调用） |
| 耗时 | 总耗时、各阶段耗时 |
| Token 与费用 | 输入 / 输出 / 缓存 Token；Agent 上报的费用，或按 Provider 价格估算 |
| 文件改动 | 实时改动文件列表 |
| 控制 | 停止、恢复、重试、追加指令（停止后以 resume + 新 Prompt 实现） |

### 预算与护栏【建议】
- 每个 Run 可设上限：最长时间、最大费用 / Token；超出自动停止并通知。
- "卡住"检测：N 分钟无事件输出时提醒。
- 危险命令提示：日志中出现 `rm -rf`、`git push --force`、数据库删除等命令时高亮。

## 5. 自动测试【已确认】

- 项目上下文卡片中配置：`lint`、`test`、`build`、`typecheck` 命令，可分别开关。
- Run 结束后在 worktree 中依次执行，输出与退出码记录为测试报告。
- 失败时可"自动修复"：把失败输出摘要作为新 Prompt 继续该会话，最多 N 次（默认 2）。
- 【建议】前端项目可选启动 dev server 并给出本地地址，便于人工验收时一键打开。

## 6. 权限策略

| 档位 | 含义 | Claude Code | Codex |
| --- | --- | --- | --- |
| `read_only` | 只读，用于预审、分析 | `--permission-mode plan` / 只读工具 | `--sandbox read-only` |
| `workspace_write` | 可改 worktree，命令受限 | `acceptEdits` + `--allowedTools` 白名单 | `--sandbox workspace-write` |
| `full_auto` | 完全自动（需显式开启） | `bypassPermissions` | `--full-auto` / `danger-full-access` |

默认：预审 `read_only`，开发 `workspace_write`。命令白名单来自项目卡片中的测试 / 构建命令。

## 7. Review 与合并【已确认】

- **Diff 视图**：相对起始分支的完整 Diff；按 Run 查看增量 Diff；文件树 + 并排对比；支持行内评论（打回时作为意见）。
- **辅助信息**：测试报告、Agent 最终总结、改动统计、费用。
- 【建议】**AI 交叉 Review**（P1）：用另一个 Agent / 模型以只读模式审查 Diff，输出问题列表，供人参考。
- **操作**：
  - 通过 → 子任务 `accepted`；工单全部通过后合并；
  - 打回 → 写意见 → 新 Run；
  - 在 IDE 中打开 worktree 手动修改，观澜检测到新提交后纳入 Diff；
  - 回滚到某次 Run 之前的提交。
- **合并**：`merge --no-ff` 或 `squash`，提交信息自动生成（含工单号与标题）；可选推送并用 `gh pr create` 开 PR（P1）。
- **清理**：合并后删除 worktree，分支按设置保留或删除；运行日志归档保留。
