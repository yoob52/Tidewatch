# 文档总索引

## 推荐阅读顺序

1. **为什么做**：[产品愿景与痛点](01-product/vision.md) → [功能清单](01-product/features.md)
2. **工单怎么流转**：[工单模型](02-workflow/ticket-model.md) → [工作流与状态机](02-workflow/workflow.md) → [PRD 审核](02-workflow/prd-review.md) → [工单来源](02-workflow/ticket-sources.md)
3. **Agent 怎么干活**：[Agent 适配层](03-agent/agent-adapter.md) → [自定义 Provider](03-agent/providers.md) → [执行与隔离](03-agent/execution.md) → [Prompt 与上下文](03-agent/prompt-context.md)
4. **系统怎么搭**：[系统架构](04-architecture/overview.md) → [数据模型](04-architecture/data-model.md) → [安全与存储](04-architecture/security-storage.md)
5. **长什么样**：[界面设计](05-ui/ui-design.md)
6. **怎么推进**：[路线图](06-roadmap/roadmap.md) → [待决问题](06-roadmap/open-questions.md)

## 目录结构

```
docs/
├── README.md                    # 本文件
├── glossary.md                  # 术语表
├── 01-product/
│   ├── vision.md                # 愿景、User Story、痛点、原则、非目标
│   └── features.md              # 功能清单与分级
├── 02-workflow/
│   ├── ticket-model.md          # 工单 / 子任务 / 版本模型
│   ├── workflow.md              # 状态机与流转规则
│   ├── prd-review.md            # PRD 预审与人工审核
│   └── ticket-sources.md        # 工单来源与导入
├── 03-agent/
│   ├── agent-adapter.md         # Agent 适配层
│   ├── providers.md             # 自定义 Provider
│   ├── execution.md             # 执行、隔离、观测、测试、合并
│   └── prompt-context.md        # Prompt 模板与上下文组装
├── 04-architecture/
│   ├── overview.md              # 总体架构
│   ├── data-model.md            # 数据模型
│   └── security-storage.md      # 安全与本地存储
├── 05-ui/
│   └── ui-design.md             # 界面与交互
├── 06-roadmap/
│   ├── roadmap.md               # 里程碑
│   └── open-questions.md        # 待决问题
└── adr/                         # 架构决策记录
    ├── 0001-desktop-shell-tauri.md
    ├── 0002-agents-as-cli-subprocess.md
    ├── 0003-worktree-per-ticket.md
    ├── 0004-provider-injection-per-process.md
    └── 0005-local-first-sqlite.md
```

## 文档约定

- 文档使用中文；代码标识、命令、字段名保留英文。
- 标注 **【已确认】** 的内容是与需求方讨论后确定的；标注 **【建议】** 的是设计者补充、待确认的内容。
- 涉及外部 CLI 参数的描述以各工具官方文档为准，实现前需按当时版本复核（见 [待决问题](06-roadmap/open-questions.md)）。
