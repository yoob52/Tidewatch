# Agent 管理（参考 T3 Code）

> 参考对象：[T3 Code](https://t3.codes/)（[pingdotgg/t3code](https://github.com/pingdotgg/t3code)），一个开源的"编码 Agent 控制台"，同时驱动 Claude Code、Codex 等十余种本地 Agent。本文只取它在 **Agent 管理** 上经过验证的做法，按观澜的场景（单机 Windows、工单驱动、Claude Code + Codex）裁剪。
>
> 本文全部内容为 **【建议】**，待需求方确认后再改为【已确认】；**不涉及实现**，实现前仍需按 [待决问题](../06-roadmap/open-questions.md) 逐项验证。

## 1. 先说结论：采纳什么、不采纳什么

| # | T3 Code 的做法 | 观澜是否采纳 | 落到观澜的哪里 |
| --- | --- | --- | --- |
| 1 | **驱动（Driver）与实例（Instance）分离**：同一种 Agent 可配多个实例（不同账号 / 配置目录 / 启动参数），按实例路由 | 采纳 | §2 Agent 实例 |
| 2 | **能力声明（Capabilities）**：适配器如实上报能做什么，业务和界面只看能力、不看 Agent 名字；缺能力时有明确降级策略 | 采纳 | §3 能力与降级 |
| 3 | **Run 与 Attempt、执行节点树**：一次"轮次"可以有多次尝试（引导重启、重试、崩溃恢复）；工具调用、审批、子 Agent 是 Run 下的节点 | 采纳（简化） | §4 运行模型 |
| 4 | **"Agent 结束"和"收尾完成"是两个里程碑**：提交、Diff、测试不计入 Agent 耗时，也不让界面显示"仍在运行" | 采纳 | §4.3 完成屏障 |
| 5 | **运行时请求（审批 / 提问）持久化**：应用自有 ID，重启后仍可见，回调失效时标记 `not_resumable` | 采纳（随交互通道上线） | §5 审批与提问 |
| 6 | **追加消息的三种分发方式**：原地引导 / 中断后重启 / 排队到下一轮 | 采纳 | §6 追加指令 |
| 7 | **切换 Agent 时生成显式的上下文交接（Context Handoff）**，而不是偷偷拼 Prompt | 采纳 | §7 上下文交接 |
| 8 | **检查点用隐藏 git ref**，回滚文件必须和 Agent 会话一起协调 | 采纳 | §8 检查点与回滚 |
| 9 | **权限档位只提供 Agent 原生能执行的**；子任务 / 委派不能提权 | 采纳 | §9 权限 |
| 10 | **命令 → 事件 + 待执行副作用（Outbox）同一事务提交**，副作用由 worker 执行；命令带回执、可幂等重试 | 采纳 | §10 可靠性 |
| 11 | **原始日志只做有界诊断**，结构化事件才是事实来源 | 采纳 | §10.3 |
| 12 | **健康检查不能有副作用**（不建会话、不起 MCP、不弹登录） | 采纳 | §2.3 |
| 13 | **用真实 CLI 录制的回放测试** 验证适配器 | 采纳 | §11 测试 |
| 14 | **应用内 MCP 服务**：Agent 可以委派子任务、读写其他会话 | 部分采纳（P2） | §12 Agent 委派 |
| 15 | 远程环境、手机端、T3 Connect、多机同步 | 不采纳 | 与"单机本地优先"冲突，见 [愿景 §4](../01-product/vision.md) 非目标（不做云端同步） |
| 16 | 十余种 Agent、ACP 注册表 | 暂不采纳 | 保持 Claude Code + Codex，扩展点见 [适配层 §8](agent-adapter.md) |
| 17 | 对话线程（Thread）作为一等对象 | 不采纳 | 观澜以 **工单 / 子任务** 为中心，Run 挂在子任务下，不另起"线程"概念 |

## 2. Agent 实例

### 2.1 驱动与实例

T3 Code 区分 **Driver**（一种集成，如 Claude Code）和 **Provider Instance**（一份具体配置与账号生命周期）。两个账号用同一个驱动，也不共享会话、模型列表或计费。观澜采纳这一划分：

| 概念 | 观澜标识 | 含义 | 例子 |
| --- | --- | --- | --- |
| Agent 驱动 | `agent_kind` | 适配器类型，代码里实现 | `claude-code`、`codex` |
| Agent 实例 | `agent_instance` | 驱动 + 可执行文件 + 配置目录 + 启动参数 + 环境变量 | "Claude 工作号"、"Claude 个人号"、"Codex 默认" |
| Provider | `provider` | 模型接入点（协议、地址、Key、模型），见 [providers.md](providers.md) | DeepSeek、公司网关 |

一次 Run 由 **Agent 实例 + Provider + 模型** 三者确定。Provider 仍按进程注入（[ADR-0004](../adr/0004-provider-injection-per-process.md)），Agent 实例决定"用哪个 CLI、哪套登录态"。

### 2.2 实例字段

| 字段 | 说明 |
| --- | --- |
| `name` | 显示名 |
| `agent_kind` | `claude-code` / `codex` |
| `binary_path` | 为空则按 `where` 解析；可指定具体路径（多版本共存） |
| `config_dir` | Claude 对应 `CLAUDE_CONFIG_DIR`，Codex 对应 `CODEX_HOME`；为空 = 用户日常配置与登录态 |
| `launch_args` | 额外启动参数（如 `--settings`、`--append-system-prompt-file`）；**环境变量不写在这里** |
| `extra_env` | 额外环境变量 |
| `enabled` | 全局开关；项目可单独开 / 关（见 §2.4） |
| `status_json` | 最近一次检测结果：是否安装、版本、登录账号、可用模型、检测时间 |

要点（来自 T3 Code 的经验）：
- **多账号靠配置目录隔离**：Claude 用 `CLAUDE_CONFIG_DIR`，不要改 `HOME`（会让凭据找不到）。登录也要带同一变量执行，例如 `CLAUDE_CONFIG_DIR=<dir> claude auth login`。这同时回答了 [Q4](../06-roadmap/open-questions.md)：隔离做成"每个实例可选的配置目录"，默认留空沿用日常登录态。
- **会话只能在同一配置目录的实例间延续**：换了配置目录就换了本地会话存储，resume 必然失败，应直接走 §7 的交接。
- **清除未配置的环境凭据**：实例启动时只注入它自己的 Key，避免两个实例悄悄共用一个账号或账单。

### 2.3 检测与健康检查

- **轻量检测**（启动时、设置页打开时、定时）：只做版本探测、登录状态读取等 **无副作用** 操作；**不得** 新建 Agent 会话（会启动 MCP、执行 hooks、甚至弹出登录浏览器）。
- **完整检测**（用户点"用 Agent 测试"时）：才真正启动一次最小任务，即 [providers.md §6](providers.md) 的端到端测试。
- 模型列表缓存不代表当前可用；一次权威的空结果要清空旧列表。
- 检测超时可配置（T3 Code 遇到过 Codex 检测超时，提供了延长超时的环境变量）。

### 2.4 选择优先级与项目覆盖

在 [providers.md §5](providers.md) 的优先级基础上，Agent 实例同样按：

```
子任务指定 > 工单指定 > 项目默认 > 全局默认
```

另加 **项目级开关**：某个实例可以只在某项目禁用 / 启用。被禁用后，该项目内新 Run（含排队中的）不得使用它；已在运行的 Run 跑完为止。设置页对每个可覆盖项显示"值来自：内置默认 / 全局 / 项目覆盖"，并可一键重置为继承。

### 2.5 升级

观澜 **不代替用户升级 CLI**（默认只提示新版本与升级命令）。理由来自 T3 Code：只有能证明是哪个安装器装的（npm、winget、scoop…）才能安全升级，否则会装出第二份副本；且运行中的 Run 持有旧版本，升级不能替换正在被使用的可执行文件。若未来做一键升级（P2），必须：确认安装来源 → 等待或提示停止该实例的运行中任务 → 升级后复检版本。

## 3. 能力与降级

### 3.1 能力声明

适配器在检测或会话开始时上报能力，**按版本** 记录在实例状态中。业务层（调度、工作流）和界面只读能力，不写 `if agent == 'codex'`。

```ts
interface AgentCapabilities {
  transport: 'headless_cli' | 'interactive';   // 见 §5.1
  resume: boolean;                 // 能用 session id 继续会话
  interrupt: 'graceful' | 'kill_only';
  steerActive: boolean;            // 运行中直接追加指令
  approvals: boolean;              // 运行中逐项审批工具调用
  questions: boolean;              // Agent 可结构化地向人提问
  todoList: boolean;               // 上报实时待办 / 计划
  subagents: boolean;              // 上报子 Agent 生命周期
  rollbackConversation: boolean;   // 能把会话回退到第 N 轮
  forkConversation: boolean;
  images: boolean;                 // 接受图片输入（见 PRD 图片）
  usageReport: 'cost' | 'tokens' | 'none';
  permissionModes: PermissionMode[]; // 原生能执行的档位，见 §9
}
```

原则：**能力必须如实**。声明了却在运行时失败，算 bug；宁可关掉能力走降级。

### 3.2 降级策略

| 缺少的能力 | 降级行为 |
| --- | --- |
| `resume` 失败 / 不支持 | 新开会话 + 上下文交接（§7），而不是报错 |
| `steerActive` | 中断当前轮 → 以"原任务 + 新指令"重启，作为同一 Run 的新 Attempt（§6） |
| `interrupt = kill_only` | 停止按钮提示"将强制结束进程"，结束后标记 `stopped` |
| `approvals` | 只能用预设权限档位运行，不显示审批面板 |
| `questions` | Agent 的问题只作为普通文本出现在日志，不生成"待回答"项 |
| `todoList` | 不显示实时待办面板，只看消息 |
| `rollbackConversation` | 回滚只恢复文件，下一轮新开会话并附交接（§8） |
| `usageReport = none` | 按 Provider 价格估算，并标注"估算" |

界面根据能力决定按钮是 **可用 / 降级可用（附说明）/ 不可用**，不让用户撞上 Agent 特有的报错。

## 4. 运行模型

### 4.1 Run、Attempt、执行节点

现有设计中"一次 Run = 一次进程"。参考 T3 Code 的 V2 编排，拆成三层：

```
子任务 Task
  Run #1（purpose=dev）           ← 用户可见的一轮，计入轮次
    Attempt 1（reason=initial, status=interrupted）
    Attempt 2（reason=steering_restart, status=completed）
      节点: tool_call(Read) / tool_call(Bash: pnpm test) / approval / subagent(…)
  Run #2（purpose=rework, parent_run_id=#1）
```

| 层 | 含义 | 何时新建 |
| --- | --- | --- |
| **Run** | 一轮工作（开发、返工、测试修复…），对应一个检查点、一份 Diff、一次计费汇总 | 开始开发、打回返工、自动修复、追加指令选择"排队" |
| **Attempt** | Run 内的一次 Agent 进程 / 轮次 | 首次启动、引导重启、失败重试、进程崩溃后恢复 |
| **执行节点** | Attempt 内的工具调用、审批、提问、子 Agent | 由事件流解析得到 |

规则：
- 只有最终 Attempt 完成才算 Run 完成，并产生该 Run 的检查点；被中断的 Attempt 保留在时间线中供排查。
- **应用自有 ID 为主，Agent 的 session / turn / item id 只是引用**。Agent 给的 id 弱或缺失时，按"所属 Attempt + 序号"分配，保证事件可关联。
- 子 Agent 的完成 **不会** 结束父 Run。

### 4.2 状态

Run 状态在现有基础上细化：

```
queued → starting → running ⇄ waiting_input（等待审批/回答）
                       │
                       ▼
                 agent_finished → finalizing（提交/Diff/测试）→ succeeded | failed
       任意时刻 → stopping → stopped；进程丢失 → orphaned
```

### 4.3 完成屏障

Agent 进程结束 ≠ Run 结束。Run 标记为终态前依次完成：

1. 刷完输出缓冲，关闭未结束的消息；
2. 定格待办 / 计划；
3. 捕获检查点（§8）、计算 Diff；
4. 自动提交（[execution.md §3](execution.md)）；
5. 运行自动测试；
6. 发布终态。

**Agent 耗时**（`agent_finished_at - started_at`）与 **收尾耗时** 分开记录；界面上 Agent 结束后即显示"收尾中"，不再显示"Agent 运行中"。收尾之后发现的东西（迟到的 Diff、测试结果）不得延长 Agent 耗时。

## 5. 审批与提问

### 5.1 两种通道

| 通道 | 说明 | 能做到 |
| --- | --- | --- |
| `headless_cli`（现有，M0/M1） | `claude -p --output-format stream-json`、`codex exec --json`，权限启动前决定 | 观测、停止、resume |
| `interactive`（建议 M2 之后） | 长驻进程 + 双向协议：Codex 的 `codex app-server`（stdio JSON-RPC），Claude Code 的流式输入 / 权限回调（T3 Code 用的是 Claude Agent SDK） | 另加：逐项审批、结构化提问、运行中引导、会话回退 |

T3 Code 正是靠 `interactive` 通道实现审批和引导的。观澜 **M0/M1 继续用 `headless_cli`**（[ADR-0002](../adr/0002-agents-as-cli-subprocess.md) 不变），但适配器接口按两种通道设计，后续把 `interactive` 作为同一 Agent 的另一种实现加入，见 [ADR-0006](../adr/0006-agent-instances-and-capabilities.md)。Claude 侧在 Rust 中不引入 Node 运行时的可行做法需验证（[Q16](../06-roadmap/open-questions.md)）。

### 5.2 运行时请求（RuntimeRequest）

`interactive` 通道下，审批和提问统一建模为运行时请求：

| 字段 | 说明 |
| --- | --- |
| `id` | 应用自有 ID |
| `run_id` / `attempt_id` / `node_id` | 归属 |
| `kind` | `command_approval` / `file_change_approval` / `user_input` |
| `agent_request_ref` | Agent 原生请求 ID 与选项 ID（**原样保留**，显示文字不一定是合法回复） |
| `payload_json` | 命令、文件、问题与选项 |
| `status` | `pending` / `resolved` / `declined` / `expired` / `not_resumable` |
| `response_json`、`resolved_at` | 回复 |

规则：
- 请求 **持久化**，应用重启后仍在"待处理"中；若 Agent 侧回调已失效，回复时返回 `not_resumable`，提示用户以追加指令的方式继续。
- 每个请求都要有 Agent 认可的"拒绝"选项；拒绝时可附文字意见，直接交给提问的 Agent。
- 子 Agent 的审批显示在父 Run 中，标明来自哪个子 Agent。
- 回复幂等：重复提交同一回复不得重复发送。
- 有待处理请求时：Run 状态为 `waiting_input`，工单卡片和托盘显示"需要你"，并发 Windows 通知（[ui-design.md §3](../05-ui/ui-design.md) 通知）。
- 等待人回复的 Run **不占并发名额**（可配置），避免两个等审批的任务把队列卡死。

## 6. 追加指令

现有设计中"追加指令 = 停止后 resume + 新 Prompt"。改为让用户（或系统）选择分发方式：

```ts
type DispatchMode =
  | 'steer_active'     // 能力 steerActive：直接注入当前轮，Run 不变
  | 'restart_active'   // 中断当前 Attempt，以新指令开启同一 Run 的新 Attempt
  | 'queue_next'       // 当前 Run 结束后作为新 Run 执行（打回意见默认走这个）
  | 'start_now';       // 当前无运行时
```

- 默认：有 `steerActive` → `steer_active`；否则 → `restart_active`。
- 用户明确"作为下一轮"时，即使支持引导也排队。
- 停止按钮会 **暂停该子任务的队列**，避免自动化（自动修复、排队消息）在停止后又把它拉起来；需要手动"继续"。

## 7. 上下文交接

### 7.1 何时需要

- 返工时换了 Agent（如 Claude Code 写的后端，交给 Codex 修）；
- resume 失败、会话过期、换了配置目录或 Provider；
- 回滚后原会话已与文件状态不一致（§8）；
- 用户选择"干净上下文"重开。

### 7.2 交接是显式数据

交接不是在 Prompt 里随手拼一段，而是一条可查看、可追溯的记录：

| 字段 | 说明 |
| --- | --- |
| `id`、`task_id`、`target_run_id` | 被哪个 Run 使用 |
| `from_runs` | 覆盖的 Run 范围 |
| `strategy` | `delta`（目标会话只缺了几轮）/ `full_summary`（新会话）/ `manual`（人工编写） |
| `summary_md` | 交接内容 |
| `created_by` | 由谁生成：离开的 Agent / 接手的 Agent / 配置的"小模型" / 本地规则拼装 |
| `status` | `ready` / `superseded`（重新生成时旧的作废，不原地修改） |

### 7.3 交接内容

**包含**：任务目标与约束（来自定稿 PRD / 子任务说明）、已做决定、已改文件列表、执行过的命令与测试结果、待办状态、未解决的问题与审批、当前检查点与 `git diff --stat`、已知失败、打回意见、子 Agent 结论。

**不包含**：协议噪音、隐藏推理过程、已被后续轮次取代的工具输出、能用文件摘要代替的大段 Diff。

### 7.4 策略选择

1. 目标会话已覆盖所有轮次 → 不交接，直接 resume；
2. 目标会话可 resume，只缺几轮 → `delta`；
3. resume 失败、设置不兼容或会话已"过期" → 新会话 + `full_summary`。

"过期"信号：离开后又跑了太多轮、交接链过长、模型 / 权限档位变化、会话被回滚、用户选择干净上下文、摘要超出上下文预算。

这替代并细化了 [agent-adapter.md §7](agent-adapter.md) 的"会话恢复策略"。

## 8. 检查点与回滚

- 每个 Run 开始前、结束后捕获检查点，存为 **隐藏 git ref**（如 `refs/tidewatch/checkpoints/<task>/<run>`），不在用户分支上增加提交；Diff 和"回滚到某次 Run 之前"都基于它。现有的"每轮自动提交"仍保留在子任务分支上，两者互不冲突。
- 回滚 **必须与 Agent 会话协调**：
  - Agent 支持 `rollbackConversation` → 文件和会话一起回退；
  - 不支持 → 只回退文件，把原会话标记为"已分歧"，下一轮新开会话并附 `full_summary` 交接；
  - 先检查能力再动文件，不能出现"文件已回退、会话却拒绝"的半成品状态。
- 被回滚的 Run 标记 `rolled_back`，覆盖这些 Run 的交接标记 `superseded`。

## 9. 权限

参考 T3 Code 的四档模式，与现有三档对齐：

| T3 Code | 观澜档位 | 说明 | 依赖 |
| --- | --- | --- | --- |
| — | `read_only` | 只读（预审、分析、交叉 Review） | 两种通道 |
| Supervised | `supervised`（新增） | 命令和文件修改都要人审批 | `interactive` + `approvals` |
| Auto-accept edits | `workspace_write` | 自动接受 worktree 内的编辑，命令受白名单限制 | 两种通道 |
| Full access | `full_auto` | 不询问（需显式开启） | 两种通道 |

规则：
- **只提供 Agent 原生能执行的档位**（来自能力 `permissionModes`）。不用"观澜替 Agent 回答审批"来模拟一个它没有的档位：观澜的检查弱于 Agent 自己的沙箱。存量配置选了不支持的档位时，降到更窄的档位运行并提示。
- 档位优先级与 Provider 相同：子任务 > 工单 > 项目 > 全局。
- **委派 / 子任务不能提权**：由某个 Run 派生的 Run（交叉 Review、委派任务）的档位只能等于或窄于父 Run。
- 即使在 `full_auto` 下，Claude Code 的危险删除检查等原生保护仍生效；日志中保留其警告。

## 10. 可靠性：意图先落盘，副作用后执行

### 10.1 命令、事件与 Outbox

现有架构用进程内事件总线（[overview.md §3](../04-architecture/overview.md)）。参考 T3 Code，把"启动 Run、停止、回复审批、捕获检查点"等有外部副作用的操作改为：

```
命令（如 StartRun）
  └─ 一个 SQLite 事务内：
       写领域事件（RunQueued…）+ 更新读模型 + 写命令回执 + 写待执行副作用（effect）
  └─ 提交后通知界面
副作用 worker
  └─ 领取 effect → 启动进程 / 发送回复 / 打检查点
  └─ 结果作为新命令回写（RunStarted / RunFailed…）
```

- 命令确认 = 意图已提交，**不代表** 进程已启动。
- 命令带 `command_id`，重复提交返回同一回执，不重复启动进程。
- 外部 I/O 不放进事务。
- 应用重启：未完成的 effect 中，绑定已丢失进程的不能简单重放——先把对应 Run / Attempt 标记 `orphaned` 并清理进程，再接收新工作。这让 [execution.md](execution.md) 里的"孤儿进程恢复"有了明确的依据。

### 10.2 并发与锁

- 同一子任务的命令串行处理（等价于"同一 worktree 同一时间只允许一个 Run"）。
- 调度器读持久化状态决定出队，不依赖内存中的计时假设。

### 10.3 日志与诊断

- **事实来源是结构化事件**（`run_event`），原始 JSONL 只作有界诊断：按大小轮转，单条超大 payload 截断为结构摘要（保留 ID、方法、状态、错误字段）。
- 在复制、脱敏之前就过滤 token 级增量和重复帧，避免日志成本失控。
- 事件结构变化要考虑旧数据回放：已落盘的旧事件必须仍可解码。

## 11. 测试

- 用 **真实 CLI 录制的会话回放** 测试适配器：录制 Claude Code / Codex 的原始输出，回放经过真实的解析、编排与读模型，只替换"进程"这一层。
- 共享场景至少覆盖：正常完成、中断、resume、resume 失败后交接、审批（通过 / 拒绝）、提问、子 Agent、进程崩溃、超大输出。
- CLI 升级时重新录制，作为 [agent-adapter.md](agent-adapter.md) "版本探测" 的回归依据。

## 12. Agent 委派（P2）

T3 Code 给每个运行中的 Agent 注入应用自己的 MCP 服务，Agent 可以创建子任务、等待结果、读取其他会话。观澜对应 [roadmap M4](../06-roadmap/roadmap.md) 的"本地 MCP 接口"，建议的最小工具集：

| 工具 | 用途 |
| --- | --- |
| `tw_ticket_read` | 读取当前工单的定稿 PRD、子任务、API 文档产物 |
| `tw_task_delegate` | 在当前工单下派生一个 Run（如让另一个 Agent 做只读交叉 Review），可选独立 worktree |
| `tw_task_status` / `tw_task_wait` | 查询 / 等待被委派任务的结果 |
| `tw_ask_user` | 把问题挂到工单的"需要你"（无 `interactive` 通道时的提问兜底） |

约束：
- 委派只能在调用方 Run 活跃时发起，档位不能高于调用方（§9）。
- 带 `client_request_id`，重试返回同一任务，不重复创建。
- 子任务的问题要能冒泡到父 Run 或人，否则没人看会一直卡住。
- 停止父 Run 时，深度优先停止它委派出去的所有 Run。
- MCP 服务不可达时，Agent 的这一轮仍应能完成。

## 13. 用量统计

- 优先用 Agent 自报的用量 / 费用；否则按 Provider 价格估算，并明确标注 **"估算，非实际账单"**（订阅登录态下尤其如此）。
- 按实例统计（同一驱动的不同账号分开），可选读取 Agent 自己的本地会话历史补全观澜外的用量（P2）。
- Claude Code 会定期清理本地会话记录，观澜自己的 `run` 记录是长期依据。

## 14. 对界面的影响

在 [ui-design.md](../05-ui/ui-design.md) 基础上：
- **运行面板**：工具调用按连续分组折叠（"运行了 2 条命令、修改 3 个文件"），失败的始终展开；审批、提问、错误永不隐藏；可选"只显示失败的工具调用"。
- **状态词**：运行中 / 需要审批 / 需要你回答 / 收尾中 / 已完成；窗口标题与托盘同步显示。
- **设置 → Agent**：实例列表（驱动、版本、登录账号、可用模型、最近检测时间）、"轻量检测 / 用 Agent 测试"两个按钮、项目级开关、每个设置项的"值来自"标记。
- **能力驱动的按钮**：引导、回滚、审批面板按能力显示为可用 / 降级（附说明）/ 不可用。
