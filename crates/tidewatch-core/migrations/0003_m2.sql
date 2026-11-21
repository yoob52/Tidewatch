-- M2：预审、拆分、变更

-- AI 给出的拆分建议 / 变更影响分析，等人确认后才落成子任务
CREATE TABLE proposal (
  id TEXT PRIMARY KEY,
  ticket_id TEXT NOT NULL,
  kind TEXT NOT NULL,              -- split | impact
  prd_version_id TEXT,
  run_id TEXT,
  status TEXT NOT NULL,            -- pending | applied | discarded | failed
  content_json TEXT,               -- 解析后的建议（人可以改）
  raw TEXT,                        -- Agent 的原始输出；解析失败时靠它排查
  error TEXT,
  created_at TEXT, updated_at TEXT
);
CREATE INDEX idx_proposal_ticket ON proposal(ticket_id, kind);

-- 预审解析失败时保留原文
ALTER TABLE review ADD COLUMN error TEXT;
ALTER TABLE review ADD COLUMN scope_json TEXT;
ALTER TABLE review ADD COLUMN criteria_draft_json TEXT;
-- 问题在一次预审里的编号（I-1），以及重新预审时沿用的上一轮问题
ALTER TABLE review_issue ADD COLUMN code TEXT;

-- 变更：记下发起变更时的定稿版本和原因，再次定稿时用来做影响分析
ALTER TABLE ticket ADD COLUMN change_from_version_id TEXT;
ALTER TABLE ticket ADD COLUMN change_reason TEXT;

-- 打回时的行内评论属于哪一次打回
ALTER TABLE comment ADD COLUMN batch_id TEXT;

CREATE INDEX idx_review_issue_ticket ON review_issue(ticket_id);
CREATE INDEX idx_artifact_task ON artifact(task_id, kind);
