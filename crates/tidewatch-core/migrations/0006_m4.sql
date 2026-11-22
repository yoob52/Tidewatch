-- M4：本地 HTTP / MCP 接口、成员、Agent 委派与提问

-- 成员：每人一个接口令牌；库里只存令牌的 SHA-256。
CREATE TABLE member (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  role TEXT NOT NULL,               -- owner | member | viewer
  token_hash TEXT UNIQUE,
  token_hint TEXT,                  -- 令牌末 4 位，便于辨认
  disabled INTEGER NOT NULL DEFAULT 0,
  created_at TEXT, last_seen_at TEXT
);
ALTER TABLE ticket ADD COLUMN assignee_id TEXT;
ALTER TABLE timeline_event ADD COLUMN actor TEXT;   -- 成员名；本机操作为空

-- Agent 通过 MCP 委派出去的运行
CREATE TABLE delegation (
  id TEXT PRIMARY KEY,
  parent_run_id TEXT NOT NULL,
  run_id TEXT NOT NULL UNIQUE,
  task_id TEXT,                     -- 调用方所在的子任务（工单级运行为空）
  client_request_id TEXT,
  created_at TEXT,
  UNIQUE (parent_run_id, client_request_id)
);

-- Agent 提给人的问题（"需要你"）
CREATE TABLE question (
  id TEXT PRIMARY KEY,
  ticket_id TEXT NOT NULL,
  task_id TEXT,
  run_id TEXT,
  body TEXT NOT NULL,
  options_json TEXT,
  client_request_id TEXT,
  status TEXT NOT NULL,             -- open | answered | dismissed
  answer TEXT,
  answered_by TEXT,
  consumed_run_id TEXT,             -- 回答已经写进哪一轮的 Prompt
  created_at TEXT, answered_at TEXT,
  UNIQUE (run_id, client_request_id)
);
CREATE INDEX question_ticket ON question (ticket_id, status);

-- 外部同步：一个来源是一个 GitHub 仓库（按 label 拉 Issue）或一个飞书自建应用（按链接导入文档）。
-- 令牌 / App Secret 在凭据管理器里，这里只记条目名。
CREATE TABLE sync_source (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,              -- github | feishu
  name TEXT NOT NULL,
  config_json TEXT NOT NULL DEFAULT '{}',
  enabled INTEGER NOT NULL DEFAULT 1,
  secret_ref TEXT,
  last_synced_at TEXT,
  last_error TEXT,
  resume_since TEXT,               -- 上次没拉完：下次从这个更新时间接着拉
  created_at TEXT
);

-- 工单和它的外部来源。rev 是已经导入的那一版；外部又改了就记在 pending_* 里等用户决定。
CREATE TABLE ticket_source (
  ticket_id TEXT PRIMARY KEY,
  source_id TEXT NOT NULL,
  external_id TEXT NOT NULL,       -- GitHub：Issue 编号；飞书：文档 id
  url TEXT NOT NULL,
  rev TEXT NOT NULL,
  pending_rev TEXT,
  pending_title TEXT,
  pending_md TEXT,
  pending_at TEXT,
  remote_state TEXT,               -- GitHub：open | closed
  written_back_at TEXT,
  UNIQUE (source_id, external_id)
);

-- 自定义工作流：给某几类工单加上自己的环节（卡在定稿前或完成前，逐个由人通过）。
CREATE TABLE workflow (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  config_json TEXT NOT NULL DEFAULT '{}',   -- { ticket_types, stages: [{ id, name, gate, hint }], auto_done }
  enabled INTEGER NOT NULL DEFAULT 1,
  created_at TEXT
);

-- 工单已经通过的自定义环节。工单退回前面的状态时清掉。
CREATE TABLE ticket_stage (
  ticket_id TEXT NOT NULL,
  stage_id TEXT NOT NULL,
  passed_at TEXT NOT NULL,
  actor TEXT,
  note TEXT,
  PRIMARY KEY (ticket_id, stage_id)
);

-- 多 Agent 同题对比：候选子任务指向被对比的那个子任务。选定一个之后其余的取消。
ALTER TABLE task ADD COLUMN compare_of TEXT;
