# 本地 HTTP / MCP 接口与轻量协作（M4）

观澜可以在本机开一个 HTTP 服务，给三类调用方用：脚本、带令牌的其他人、运行中的 Agent。默认关闭，在「设置 → 接口与成员」里打开。

## 1. 入口

| 路径 | 用途 | 凭据 |
| --- | --- | --- |
| `/` | 只读看板页面（静态页面，数据经 REST 取，令牌填在页面里） | 无（页面本身不含数据） |
| `/api/v1/health` | 健康检查 | 无 |
| `/api/v1/*` | REST | 成员令牌 `tw_…` |
| `/mcp` | MCP（Streamable HTTP，JSON-RPC，协议版本 `2025-03-26`） | 成员令牌，或运行令牌 `twr_…` |

- 默认端口 `47600`，只监听 `127.0.0.1`。打开「远程访问」后监听所有网卡（`0.0.0.0`），同一局域网或经 VPN / 隧道的人可以访问。
- 令牌放在 `Authorization: Bearer <令牌>` 里。没有令牌或令牌不对一律 `401`；角色不够 `403`。

## 2. 成员与角色

| 角色 | 能做什么 |
| --- | --- |
| `owner` | 全部，包括合并、以 `full_auto` 开始子任务、成员管理 |
| `member` | 建单、评论、指派、发起预审、开始 / 停止运行、回答 Agent 的问题、验收通过 / 打回、通过自定义环节 |
| `viewer` | 只读 |

- 成员在「设置 → 接口与成员」里添加；令牌只在创建或重置时显示一次。
- 经接口做的操作，时间线上记成员名；本机桌面端的操作不记名。
- 工单可以指派给成员（`assignee`），只是标记，不限制谁能操作。
- 至少保留一个可用的 `owner`。停用成员或重置令牌后旧令牌立刻失效。

## 3. REST

前缀 `/api/v1`。请求体和响应都是 JSON。

| 方法与路径 | 角色 | 说明 |
| --- | --- | --- |
| `GET /me` | 任意 | 当前令牌对应的成员 |
| `GET /members` | 任意 | 成员列表（非 owner 看不到别人的令牌信息） |
| `GET /projects` | 任意 | 项目列表 |
| `GET /tickets?q=` | 任意 | 工单列表，可搜索 |
| `POST /tickets` | member | 新建草稿工单 |
| `GET /tickets/{id}` | 任意 | 工单详情：PRD、子任务、指派、自定义环节、时间线、运行 |
| `POST /tickets/{id}/comments` | member | 评论 |
| `POST /tickets/{id}/assign` | member | 指派（`member_id` 为空是取消指派） |
| `POST /tickets/{id}/stages/{stage_id}/pass` | member | 通过一个自定义环节（可带 `note`） |
| `POST /tickets/{id}/review` | member | 发起 AI 预审 |
| `POST /tickets/{id}/start` | member | 开始开发 |
| `GET /tasks/{id}`、`GET /tasks/{id}/diff` | 任意 | 子任务及其运行、Diff |
| `POST /tasks/{id}/start` | member | 开始子任务；`full_auto` 的只有 owner 能开始，并且要带 `"confirm_full_auto": true` |
| `POST /tasks/{id}/accept`、`/reject` | member | 验收通过 / 打回（`note`，可带 `rerun`）。`full_auto` 的子任务带 `rerun` 时和开始一样：只有 owner，并且要带 `"confirm_full_auto": true` |
| `POST /tasks/{id}/merge` | owner | 合并 |
| `GET /runs/{id}`、`GET /runs/{id}/events?after=` | 任意 | 运行与事件 |
| `POST /runs/{id}/stop` | member | 停止运行 |
| `GET /questions`、`POST /questions/{id}/answer` | 任意 / member | Agent 提的问题与回答 |

`{id}` 处工单可以用编号（`TW-12`）。

## 4. MCP 工具

| 工具 | 调用方 | 作用 |
| --- | --- | --- |
| `tw_ticket_read` | 成员、运行 | 读工单：PRD、验收标准、子任务、接口文档路径 |
| `tw_ask_user` | 成员、运行 | 把问题挂到工单的「需要你」。不等待；回答进下一轮 Prompt |
| `tw_task_delegate` | 运行 | 把一件只读的事委派给另一个 Agent，在调用方的 worktree 里只读运行 |
| `tw_task_status`、`tw_task_wait` | 运行 | 查询 / 等待委派出去的运行 |
| `tw_ticket_list`、`tw_ticket_create` | 成员 | 列出、新建工单 |

- **运行令牌**：每轮运行开始时生成，经环境变量 `TIDEWATCH_RUN_TOKEN` 和 Agent 的 MCP 配置交给那一轮的 Agent，运行结束即失效；只能读写它自己那张工单。
- **委派的限制**：只读；用调用方的 worktree，不建分支、不提交；最多套两层；一个调用方同时最多 4 个；不占普通并发名额；停掉调用方会连带停掉它委派出去的运行。
- **提问**：问题有 `open` / `answered` / `dismissed` 三种状态；回答只注入一次。

## 5. 安全

- 成员令牌在库里只存 SHA-256 和末尾几位提示，原文不落盘。
- 带 `Origin` 的请求，`Origin` 必须就是接口自己，别的网页发来的请求返回 `403`。
- 请求体上限 2 MB。
- 接口是明文 HTTP。打开「远程访问」等于把看板和操作暴露给能连到这台机器的人：只在可信网络里用，或者放在 VPN / 带 TLS 的反向代理后面。
- `full_auto` 的子任务、合并、成员管理不对 `member` 开放。
- Provider 密钥、同步来源的凭据都不经接口返回。

## 6. 验证情况

- 令牌与角色、REST、MCP 工具、Agent 经 MCP 提问和委派：端到端测试（演示 Agent），Linux 与 Windows CI 都跑。
- 真实 Agent 经 MCP 调观澜的工具：Codex 和 OpenCode 各跑通一次（`gpt-6.1-sol`）。Claude Code 能加载工具列表，但中转站额度用完（HTTP 429），没有跑到工具调用。Gemini CLI、Aider 没有用真实账号验证。
- 远程访问（监听所有网卡）没有跨机器、跨网络验证。
