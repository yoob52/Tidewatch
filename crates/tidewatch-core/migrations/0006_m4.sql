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
