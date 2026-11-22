-- M3：AI 交叉审查。另一个 Agent 只读审查子任务的 Diff，结果供人参考，不改变子任务状态
CREATE TABLE code_review (
  id TEXT PRIMARY KEY,
  task_id TEXT NOT NULL,
  ticket_id TEXT NOT NULL,
  run_id TEXT NOT NULL,
  head_commit TEXT,                -- 审查时 worktree 的 HEAD；之后又有提交就说明结果过期
  agent TEXT NOT NULL,
  model TEXT,
  status TEXT NOT NULL,            -- running | done | failed
  summary TEXT,
  findings_json TEXT NOT NULL DEFAULT '[]',
  error TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE INDEX idx_code_review_task ON code_review(task_id, created_at);
CREATE INDEX idx_code_review_run ON code_review(run_id);
