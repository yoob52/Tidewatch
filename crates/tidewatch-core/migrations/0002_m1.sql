-- M1：取消后恢复、合并记录、Run 的排队时间 / 轮次 / 失败原因 / 改动统计
ALTER TABLE ticket ADD COLUMN cancelled_from TEXT;      -- 取消前的状态，恢复时用
ALTER TABLE task ADD COLUMN merged_commit TEXT;
ALTER TABLE run ADD COLUMN number INTEGER;              -- 子任务内的第几轮
ALTER TABLE run ADD COLUMN queued_at TEXT;
ALTER TABLE run ADD COLUMN permission TEXT;
ALTER TABLE run ADD COLUMN resume_session_id TEXT;      -- 本轮要续接的 Agent 会话
ALTER TABLE run ADD COLUMN error TEXT;
ALTER TABLE run ADD COLUMN diff_stat_json TEXT;         -- [{path, added, removed}]
CREATE INDEX idx_task_status ON task(status);
