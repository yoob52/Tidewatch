import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

export interface SystemInfo {
  version: string;
  data_dir: string;
  db_path: string;
  schema_version: number;
  sqlite_version: string;
  git_version: string | null;
  long_paths_hint: string | null;
  os: string;
  secret_store: string;
}

export type AgentKind = 'claude_code' | 'codex' | 'mock';

export interface Detection {
  kind: AgentKind;
  installed: boolean;
  path: string | null;
  version: string | null;
}

export interface Settings {
  workspace_dir: string | null;
  max_concurrency: number;
  default_agent: string;
  default_provider_id: string | null;
  default_permission: string;
  auto_test: boolean;
  merge_mode: 'merge' | 'squash';
  delete_branch_after_merge: boolean;
  notifications: boolean;
  run_timeout_minutes: number;
}

export interface Commands {
  setup: string | null;
  lint: string | null;
  typecheck: string | null;
  test: string | null;
  build: string | null;
  dev: string | null;
}

export interface ProjectInput {
  name: string;
  kind: string;
  local_path: string;
  git_url: string | null;
  default_branch: string | null;
  context_md: string;
  commands: Commands;
  default_agent: string | null;
  default_provider_id: string | null;
}

export interface Project extends Omit<ProjectInput, 'default_branch'> {
  id: string;
  default_branch: string;
  created_at: string;
  updated_at: string;
}

export interface Pricing {
  input_per_mtok: number;
  output_per_mtok: number;
  cache_per_mtok: number;
}

export interface ProviderInput {
  name: string;
  protocol: 'native' | 'anthropic' | 'openai_responses' | 'openai_chat';
  base_url: string | null;
  models: string[];
  default_model: string | null;
  small_model: string | null;
  extra_env: [string, string][];
  pricing: Pricing | null;
  proxy: string | null;
}

export interface Provider extends ProviderInput {
  id: string;
  /** 凭据管理器里的条目名；有值表示已保存密钥。密钥本身不会回到界面。 */
  secret_ref: string | null;
  created_at: string;
  updated_at: string;
}

export type TicketType = 'feature_backend' | 'feature_frontend' | 'feature_mixed' | 'bug' | 'chore';
export type TicketStatus =
  | 'draft'
  | 'ai_review'
  | 'human_review'
  | 'finalized'
  | 'developing'
  | 'acceptance'
  | 'done'
  | 'cancelled';
export type Priority = 'urgent' | 'high' | 'medium' | 'low';

export interface TicketProject {
  project_id: string;
  role: string;
  subdir: string | null;
}

export interface TicketInput {
  title: string;
  type: TicketType;
  priority: Priority;
  labels: string[];
  due_date: string | null;
  content_md: string;
  projects: TicketProject[];
  acceptance_criteria: string[];
  bug_info?: unknown;
  source_kind?: 'manual' | 'file';
  default_agent: string | null;
  default_provider_id: string | null;
}

export interface Ticket {
  id: string;
  key: string;
  title: string;
  type: TicketType;
  status: TicketStatus;
  priority: Priority;
  labels: string[];
  due_date: string | null;
  source_kind: string;
  current_version_id: string | null;
  finalized_version_id: string | null;
  acceptance_criteria: string[];
  bug_info: unknown;
  default_agent: string | null;
  default_provider_id: string | null;
  cancelled_from: TicketStatus | null;
  created_at: string;
  updated_at: string;
  projects: TicketProject[];
}

export interface TicketSummary extends Ticket {
  tasks_total: number;
  tasks_done: number;
  tasks_running: number;
}

export interface PrdVersion {
  id: string;
  ticket_id: string;
  version: number;
  content_md: string;
  author: string;
  origin: string | null;
  note: string | null;
  created_at: string;
}

export interface TimelineEvent {
  id: string;
  ts: string;
  type: string;
  payload: Record<string, unknown>;
}

export type TaskStatus =
  | 'pending'
  | 'blocked'
  | 'queued'
  | 'running'
  | 'review'
  | 'changes_requested'
  | 'accepted'
  | 'merged'
  | 'failed'
  | 'stopped'
  | 'cancelled';

export interface TaskInput {
  ticket_id: string;
  kind: 'backend' | 'frontend' | 'bug' | 'doc';
  project_id: string;
  title: string;
  spec_md: string;
  acceptance_criteria: string[];
  agent: string | null;
  provider_id: string | null;
  model: string | null;
  permission_profile: string | null;
  depends_on: string[];
}

export interface Task extends TaskInput {
  id: string;
  status: TaskStatus;
  branch: string | null;
  worktree_path: string | null;
  base_commit: string | null;
  merged_commit: string | null;
  created_at: string;
  updated_at: string;
}

export interface FileStat {
  path: string;
  added: number;
  removed: number;
}

export type RunStatus =
  | 'queued'
  | 'starting'
  | 'running'
  | 'finalizing'
  | 'stopping'
  | 'succeeded'
  | 'failed'
  | 'stopped'
  | 'orphaned';

export interface Run {
  id: string;
  task_id: string | null;
  ticket_id: string | null;
  number: number;
  purpose: string;
  parent_run_id: string | null;
  agent: string;
  provider_id: string | null;
  model: string | null;
  permission: string | null;
  agent_session_id: string | null;
  resume_session_id: string | null;
  prompt: string;
  command: string | null;
  status: RunStatus;
  exit_code: number | null;
  queued_at: string | null;
  started_at: string | null;
  agent_finished_at: string | null;
  finished_at: string | null;
  input_tokens: number;
  output_tokens: number;
  cache_tokens: number;
  cost_usd: number | null;
  cost_source: 'reported' | 'estimated' | null;
  start_commit: string | null;
  end_commit: string | null;
  summary: string | null;
  raw_log_path: string | null;
  error: string | null;
  diff_stat: FileStat[];
}

export interface RunRow extends Run {
  ticket_key: string | null;
  ticket_title: string | null;
  task_title: string | null;
}

export interface RunEvent {
  run_id: string;
  seq: number;
  ts: string;
  /** spawned / worktree / session_started / text / thinking / tool_call / tool_result / command / file_change /
   *  usage / finished / error / raw / stderr / commit / check_started / check_finished / status */
  type: string;
  payload: Record<string, unknown>;
}

export interface TestReport {
  id: string;
  run_id: string;
  task_id: string | null;
  command: string;
  exit_code: number | null;
  output_path: string | null;
  duration_ms: number;
  created_at: string;
}

export interface StartOptions {
  instruction?: string | null;
  fresh_session?: boolean;
  prompt_override?: string | null;
  command_id?: string | null;
}

export interface RunPlan {
  agent: string;
  provider_id: string | null;
  model: string | null;
  permission: string;
  resume: [string, string] | null;
  prompt: string;
}

export interface TaskDiff {
  base: string;
  files: FileStat[];
  patch: string;
  truncated: boolean;
}

export interface TicketDetail {
  ticket: Ticket;
  versions: PrdVersion[];
  tasks: Task[];
  runs: Run[];
  timeline: TimelineEvent[];
}

export interface RunDetail {
  run: RunRow;
  events: RunEvent[];
  reports: TestReport[];
}

export interface AgentStat {
  agent: string;
  runs: number;
  succeeded: number;
  failed: number;
  stopped: number;
  input_tokens: number;
  output_tokens: number;
  cache_tokens: number;
  cost_usd: number;
  agent_seconds: number;
}

export interface Stats {
  tickets_total: number;
  tickets_done: number;
  avg_cycle_hours: number | null;
  runs_total: number;
  cost_usd: number;
  input_tokens: number;
  output_tokens: number;
  cache_tokens: number;
  agents: AgentStat[];
  days: { day: string; runs: number; cost_usd: number; tokens: number }[];
  providers: [string | null, number, number][];
}

export type EngineEvent =
  | { type: 'run_event'; event: RunEvent }
  | { type: 'changed'; ticket_id: string | null; task_id: string | null; run_id: string | null }
  | {
      type: 'run_finished';
      run_id: string;
      task_id: string;
      ticket_key: string;
      task_title: string;
      status: RunStatus;
      failed_checks: string[];
    };

export const api = {
  systemInfo: () => invoke<SystemInfo>('system_info'),
  detectAgents: () => invoke<Detection[]>('detect_agents'),

  settingsGet: () => invoke<Settings>('settings_get'),
  settingsSave: (settings: Settings) => invoke<Settings>('settings_save', { settings }),

  projectList: () => invoke<Project[]>('project_list'),
  projectSave: (id: string | null, input: ProjectInput) => invoke<Project>('project_save', { id, input }),
  projectDelete: (id: string) => invoke<void>('project_delete', { id }),
  projectAgentDocs: (path: string) => invoke<[string, string][]>('project_agent_docs', { path }),

  providerList: () => invoke<Provider[]>('provider_list'),
  /** secret：undefined 不动现有密钥，空串清除。 */
  providerSave: (id: string | null, input: ProviderInput, secret?: string) =>
    invoke<Provider>('provider_save', { id, input, secret: secret ?? null }),
  providerDelete: (id: string) => invoke<void>('provider_delete', { id }),

  ticketList: (query?: string) => invoke<TicketSummary[]>('ticket_list', { query: query ?? null }),
  ticketGet: (id: string) => invoke<TicketDetail>('ticket_get', { id }),
  ticketCreate: (input: TicketInput) => invoke<Ticket>('ticket_create', { input }),
  ticketUpdate: (id: string, input: TicketInput) => invoke<Ticket>('ticket_update', { id, input }),
  ticketSavePrd: (id: string, content: string, note?: string) =>
    invoke<PrdVersion>('ticket_save_prd', { id, content, note: note ?? null }),
  ticketTransition: (id: string, to: TicketStatus) => invoke<Ticket>('ticket_transition', { id, to }),
  ticketDraft: (markdown: string) =>
    invoke<{ title: string; type: TicketType; content_md: string }>('ticket_draft', { markdown }),
  readTextFile: (path: string) => invoke<string>('read_text_file', { path }),

  taskCreate: (input: TaskInput) => invoke<Task>('task_create', { input }),
  taskUpdate: (id: string, input: TaskInput) => invoke<Task>('task_update', { id, input }),
  taskPlan: (id: string, options: StartOptions = {}) => invoke<RunPlan>('task_plan', { id, options }),
  taskStart: (id: string, options: StartOptions = {}) => invoke<Run>('task_start', { id, options }),
  taskAccept: (id: string) => invoke<Task>('task_accept', { id }),
  taskReject: (id: string, note: string) => invoke<Task>('task_reject', { id, note }),
  taskMerge: (id: string) => invoke<Task>('task_merge', { id }),
  taskCancel: (id: string) => invoke<Task>('task_cancel', { id }),
  taskDiff: (id: string) => invoke<TaskDiff>('task_diff', { id }),

  runList: (limit?: number) => invoke<RunRow[]>('run_list', { limit: limit ?? null }),
  runGet: (id: string, after?: number) => invoke<RunDetail>('run_get', { id, after: after ?? null }),
  runStop: (id: string) => invoke<void>('run_stop', { id }),
  readLog: (path: string) => invoke<string>('read_log', { path }),

  statsGet: () => invoke<Stats>('stats_get'),
};

/** 订阅引擎事件；返回取消订阅的函数。 */
export function onEngine(handler: (ev: EngineEvent) => void): () => void {
  const pending = listen<EngineEvent>('tw://engine', (e) => handler(e.payload));
  return () => {
    void pending.then((off) => off());
  };
}
