# ADR-0006 Agent 实例 + 能力声明 + 应用自有运行模型

- 状态：提议
## 背景
[ADR-0002](0002-agents-as-cli-subprocess.md) 决定以 CLI 子进程接入 Agent，但"一次 Run = 一次进程、`supports` 四个布尔值"不足以表达：同一 Agent 多账号、不同 Agent 能力差异（审批、引导、回滚）、换 Agent 时的上下文延续、应用重启后的恢复。参考 [T3 Code](https://github.com/pingdotgg/t3code) 的编排设计。
## 决策
- 区分 Agent 驱动（`agent_kind`）与 Agent 实例（可执行文件 + 配置目录 + 启动参数），按实例路由。
- 适配器如实上报能力（`AgentCapabilities`），业务与界面只看能力，缺能力走明确的降级策略，不模拟 Agent 没有的权限档位。
- 观澜自有 Run / Attempt / 执行节点 / 运行时请求 / 上下文交接 / 检查点 ID，Agent 原生 ID 只作引用。
- 适配器支持两种通道：`headless_cli`（M0/M1）与 `interactive`（后续），仍属"以 CLI 子进程接入"，不引入自研 Agent 循环。
- 有副作用的操作先以"命令 + 事件 + 待执行副作用"同一事务落盘，再执行。
细节见 [Agent 管理](../03-agent/agent-management.md)。
## 备选方案
- 保持现状，按 Agent 名字分支处理：短期简单，Agent 一多或加入交互通道后难维护。
- 照搬 T3 Code 的线程模型：观澜以工单 / 子任务为中心，不需要独立的对话线程与远程环境。
## 影响
- 数据模型新增 `agent_instance`、`run_attempt`、`runtime_request`、`context_handoff`、`command_receipt`、`pending_effect`。
- 适配器接口变大，但新增 Agent 只需实现能力内的部分。
- 交互通道的协议细节待验证（[Q16](../06-roadmap/open-questions.md)）。
