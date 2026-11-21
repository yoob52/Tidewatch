-- 初始结构，来自 docs/04-architecture/data-model.md
-- 项目
CREATE TABLE project (
  id TEXT PRIMARY KEY,
  name TEXT NOT NULL,
  kind TEXT NOT NULL,              -- backend | frontend | fullstack | other
  local_path TEXT NOT NULL,
  git_url TEXT,
  default_branch TEXT DEFAULT 'main',
  context_md TEXT,                 -- 项目上下文卡片
  commands_json TEXT,              -- {setup, lint, test, build, typecheck, dev}
  default_agent TEXT,
  default_provider_id TEXT,
  created_at TEXT, updated_at TEXT
);

-- 工单
CREATE TABLE ticket (
  id TEXT PRIMARY KEY,
  key TEXT UNIQUE NOT NULL,        -- TW-42
  title TEXT NOT NULL,
  type TEXT NOT NULL,              -- feature_backend | feature_frontend | feature_mixed | bug | chore
  status TEXT NOT NULL,            -- draft | ai_review | human_review | finalized | developing | acceptance | done | cancelled
  priority TEXT DEFAULT 'medium',
  labels_json TEXT,
  due_date TEXT,
  source_kind TEXT,                -- manual | file | feishu | github
  source_url TEXT, source_rev TEXT,
  current_version_id TEXT,
  finalized_version_id TEXT,
  acceptance_criteria_json TEXT,
  bug_info_json TEXT,
  default_agent TEXT, default_provider_id TEXT,
  created_at TEXT, updated_at TEXT
);

CREATE TABLE ticket_project (
  ticket_id TEXT, project_id TEXT,
  role TEXT,                       -- backend | frontend
  subdir TEXT,                     -- monorepo 子目录
  PRIMARY KEY (ticket_id, project_id, role)
);

-- PRD 版本（不可变）
CREATE TABLE prd_version (
  id TEXT PRIMARY KEY,
  ticket_id TEXT NOT NULL,
  version INTEGER NOT NULL,
  content_md TEXT NOT NULL,
  author TEXT NOT NULL,            -- human | ai
  origin TEXT,                     -- import | edit | ai_suggestion | normalize | change_request
  note TEXT,
  created_at TEXT,
  UNIQUE (ticket_id, version)
);

-- 预审
CREATE TABLE review (
  id TEXT PRIMARY KEY,
  ticket_id TEXT, prd_version_id TEXT, run_id TEXT,
  summary TEXT, raw_json TEXT, created_at TEXT
);
CREATE TABLE review_issue (
  id TEXT PRIMARY KEY,
  review_id TEXT, ticket_id TEXT,
  severity TEXT,                   -- blocker | major | minor | suggestion
  category TEXT, location TEXT, problem TEXT, question TEXT, suggestion TEXT,
  resolution TEXT,                 -- open | accepted | manual | ignored | pending_confirm
  resolution_note TEXT,
  resolved_in_version_id TEXT,
  updated_at TEXT
);

-- 子任务
CREATE TABLE task (
  id TEXT PRIMARY KEY,
  ticket_id TEXT NOT NULL,
  kind TEXT NOT NULL,              -- backend | frontend | bug | doc
  project_id TEXT NOT NULL,
  title TEXT, spec_md TEXT, acceptance_criteria_json TEXT,
  status TEXT NOT NULL,
  agent TEXT, provider_id TEXT, model TEXT,
  permission_profile TEXT,
  branch TEXT, worktree_path TEXT, base_commit TEXT,
  created_at TEXT, updated_at TEXT
);
CREATE TABLE task_dependency (
  task_id TEXT, depends_on_task_id TEXT,
  artifact_kind TEXT,              -- api_doc | done
  PRIMARY KEY (task_id, depends_on_task_id)
);
CREATE TABLE artifact (
  id TEXT PRIMARY KEY,
  task_id TEXT, kind TEXT,         -- api_doc | test_report | diff_summary
  path TEXT, commit_sha TEXT, created_at TEXT
);

-- 运行
CREATE TABLE run (
  id TEXT PRIMARY KEY,
  task_id TEXT,                    -- 预审等工单级运行时为空
  ticket_id TEXT,
  purpose TEXT,                    -- review | normalize | split | dev | rework | test_fix | impact | code_review
  parent_run_id TEXT,
  agent TEXT, provider_id TEXT, model TEXT,
  agent_session_id TEXT,
  prompt TEXT,
  command_json TEXT,               -- 实际命令（脱敏）
  status TEXT,                     -- queued | running | succeeded | failed | stopped | orphaned
  exit_code INTEGER,
  started_at TEXT, finished_at TEXT,
  input_tokens INTEGER, output_tokens INTEGER, cache_tokens INTEGER,
  cost_usd REAL, cost_source TEXT, -- reported | estimated
  start_commit TEXT, end_commit TEXT,
  summary TEXT,
  raw_log_path TEXT,
  agent_instance_id TEXT,
  agent_finished_at TEXT,
  checkpoint_before_ref TEXT, checkpoint_after_ref TEXT
);
CREATE TABLE run_event (
  run_id TEXT, seq INTEGER,
  ts TEXT, type TEXT, payload_json TEXT,
  PRIMARY KEY (run_id, seq)
);
CREATE TABLE test_report (
  id TEXT PRIMARY KEY, run_id TEXT, task_id TEXT,
  command TEXT, exit_code INTEGER, output_path TEXT, duration_ms INTEGER, created_at TEXT
);

-- 验收与评论
CREATE TABLE comment (
  id TEXT PRIMARY KEY,
  ticket_id TEXT, task_id TEXT, run_id TEXT,
  kind TEXT,                       -- note | rejection | clarification | inline
  file_path TEXT, line INTEGER,    -- 行内评论
  body_md TEXT, created_at TEXT
);

-- 时间线
CREATE TABLE timeline_event (
  id TEXT PRIMARY KEY, ticket_id TEXT, ts TEXT, type TEXT, payload_json TEXT
);

-- Provider（密钥不在库中）
CREATE TABLE provider (
  id TEXT PRIMARY KEY,
  name TEXT, protocol TEXT, base_url TEXT,
  secret_ref TEXT,                 -- 凭据管理器中的条目名
  models_json TEXT, default_model TEXT, small_model TEXT,
  extra_env_json TEXT, extra_headers_json TEXT,
  pricing_json TEXT, proxy TEXT,
  created_at TEXT, updated_at TEXT
);

CREATE TABLE attachment (
  id TEXT PRIMARY KEY, ticket_id TEXT, file_name TEXT, path TEXT, mime TEXT, description TEXT, created_at TEXT
);
-- 【建议】Agent 管理（见 03-agent/agent-management.md）
CREATE TABLE agent_instance (
  id TEXT PRIMARY KEY, name TEXT,
  agent_kind TEXT,                -- claude-code / codex
  binary_path TEXT, config_dir TEXT,
  launch_args_json TEXT, extra_env_json TEXT,
  enabled INTEGER, status_json TEXT, capabilities_json TEXT
);
CREATE TABLE project_agent_instance (project_id TEXT, agent_instance_id TEXT, enabled INTEGER,
  PRIMARY KEY (project_id, agent_instance_id));
CREATE TABLE run_attempt (
  id TEXT PRIMARY KEY, run_id TEXT, ordinal INTEGER,
  reason TEXT,                    -- initial / steering_restart / retry / recovery
  agent_session_id TEXT, agent_turn_ref TEXT,   -- Agent 原生 ID 只作引用
  transport TEXT,                 -- headless_cli / interactive
  status TEXT, started_at TEXT, finished_at TEXT, raw_log_path TEXT
);
CREATE TABLE runtime_request (
  id TEXT PRIMARY KEY, run_id TEXT, attempt_id TEXT, node_ref TEXT,
  kind TEXT,                      -- command_approval / file_change_approval / user_input
  agent_request_ref TEXT, payload_json TEXT,
  status TEXT,                    -- pending / resolved / declined / expired / not_resumable
  response_json TEXT, created_at TEXT, resolved_at TEXT
);
CREATE TABLE context_handoff (
  id TEXT PRIMARY KEY, task_id TEXT, target_run_id TEXT,
  from_run_ids_json TEXT,
  strategy TEXT,                  -- delta / full_summary / manual
  summary_md TEXT, created_by TEXT,
  status TEXT,                    -- ready / superseded
  created_at TEXT
);
CREATE TABLE command_receipt (command_id TEXT PRIMARY KEY, result_json TEXT, created_at TEXT);
CREATE TABLE pending_effect (
  id TEXT PRIMARY KEY, kind TEXT, payload_json TEXT,
  status TEXT,                    -- pending / running / done / failed / orphaned
  attempts INTEGER, created_at TEXT, updated_at TEXT
);

-- 图片附件的来源、效力与按版本的引用索引（prd_image）见 02-workflow/prd-images.md

CREATE TABLE setting (key TEXT PRIMARY KEY, value_json TEXT);

-- 全文检索
CREATE VIRTUAL TABLE ticket_fts USING fts5(title, content, ticket_id UNINDEXED);

CREATE INDEX idx_ticket_status ON ticket(status);
CREATE INDEX idx_prd_version_ticket ON prd_version(ticket_id);
CREATE INDEX idx_task_ticket ON task(ticket_id);
CREATE INDEX idx_run_task ON run(task_id);
CREATE INDEX idx_run_ticket ON run(ticket_id);
CREATE INDEX idx_run_status ON run(status);
CREATE INDEX idx_timeline_ticket ON timeline_event(ticket_id, ts);
CREATE INDEX idx_pending_effect_status ON pending_effect(status);
