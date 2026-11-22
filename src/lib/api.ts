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
  /** 自动检查没过时最多让 Agent 自己修几次；0 不自动修。 */
  auto_fix_attempts: number;
  /** 子任务进入待验收时自动发起 AI 交叉审查。 */
  auto_cross_review: boolean;
  /** 启动时联网检查新版本；默认关。 */
  check_updates: boolean;
  merge_mode: 'merge' | 'squash';
  delete_branch_after_merge: boolean;
  notifications: boolean;
  run_timeout_minutes: number;
  /** 确认拆分 / 影响分析后立刻开始开发。 */
  auto_develop: boolean;
  /** 依赖满足（比如接口文档出来）后，等它的子任务自动入队。 */
  auto_start_unblocked: boolean;
  /** 单次 Run 的花费上限（美元）；0 不限。 */
  budget_usd_per_run: number;
  /** 单次 Run 的 Token 上限（输入 + 输出）；0 不限。 */
  budget_tokens_per_run: number;
  /** Agent 连续多少分钟没输出算卡住；0 不检测。 */
  stuck_minutes: number;
  stuck_action: 'notify' | 'stop';
  stop_on_danger: boolean;
  /** 开发阶段也把已被取代的参考图发给 Agent。 */
  send_superseded_images: boolean;
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
  /** 模型只能处理文字：图片改发文字描述。 */
  text_only: boolean;
}

export interface Attachment {
  id: string;
  ticket_id: string | null;
  file_name: string;
  mime: string;
  description: string | null;
  /** prd：随原始 PRD 导入；user：需求方之后贴的。 */
  origin: 'prd' | 'user';
  sha256: string;
  width: number | null;
  height: number | null;
  created_at: string;
}

export interface Imported {
  file_name: string;
  markdown: string;
  attachments: Attachment[];
  warnings: string[];
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
  /** 变更进行中：发起变更时的定稿版本。 */
  change_from_version_id: string | null;
  change_reason: string | null;
  created_at: string;
  updated_at: string;
  projects: TicketProject[];
}

export interface TicketSummary extends Ticket {
  tasks_total: number;
  tasks_done: number;
  tasks_running: number;
  issues_open: number;
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
  /** depends_on 里只等接口文档的那些。 */
  api_doc_deps?: string[];
}

export interface Task extends TaskInput {
  id: string;
  status: TaskStatus;
  branch: string | null;
  worktree_path: string | null;
  base_commit: string | null;
  merged_commit: string | null;
  /** 用户手动推送到远程时分支的提交。 */
  pushed_commit: string | null;
  /** 用户手动开的 PR。 */
  pr_url: string | null;
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
  handoff: { from_run_id: string; summary_md: string } | null;
}

export type Severity = 'blocker' | 'major' | 'minor' | 'suggestion';
export type Resolution = 'open' | 'accepted' | 'manual' | 'ignored' | 'pending_confirm';

export interface Review {
  id: string;
  ticket_id: string;
  prd_version_id: string | null;
  run_id: string | null;
  summary: string;
  scope: { backend?: boolean; frontend?: boolean } | null;
  criteria_draft: string[];
  error: string | null;
  created_at: string;
}

export interface Issue {
  id: string;
  review_id: string;
  ticket_id: string;
  code: string;
  severity: Severity;
  category: string;
  location: string;
  problem: string;
  question: string | null;
  suggestion: string | null;
  resolution: Resolution;
  resolution_note: string | null;
  resolved_in_version_id: string | null;
  updated_at: string;
}

export interface TaskDraft {
  ref: string;
  kind: string;
  /** 项目名或项目 id。 */
  project: string;
  title: string;
  spec: string;
  acceptance_criteria: string[];
  depends_on: string[];
  dependency_kind: string | null;
  agent: string | null;
}

export interface TaskImpact {
  task_id: string;
  impact: 'none' | 'rework';
  instruction: string;
}

export interface Proposal {
  id: string;
  ticket_id: string;
  kind: 'split' | 'impact';
  prd_version_id: string | null;
  run_id: string | null;
  status: 'running' | 'pending' | 'applied' | 'discarded' | 'failed';
  content: { summary?: string; tasks?: unknown[]; new_tasks?: unknown[] } | null;
  error: string | null;
  created_at: string;
  updated_at: string;
}

export interface Artifact {
  id: string;
  task_id: string;
  kind: string;
  path: string;
  commit_sha: string | null;
  created_at: string;
}

export interface InlineComment {
  file_path: string;
  line: number | null;
  body: string;
}

export interface RejectInput {
  note: string;
  comments?: InlineComment[];
  rerun?: boolean;
}

export interface TemplateInfo {
  name: string;
  title: string;
  description: string;
  default: string;
  content: string;
  customized: boolean;
  variables: string[];
}

export interface TaskDiff {
  base: string;
  files: FileStat[];
  patch: string;
  truncated: boolean;
}

export interface UpdateInfo {
  current: string;
  /** 还没有任何发布时为 null。 */
  latest: string | null;
  newer: boolean;
  url: string | null;
  notes: string | null;
}

export interface ProviderCompat {
  agent: string;
  agent_name: string;
  compatible: boolean;
  /** 不兼容时的 LiteLLM 配置说明（Markdown）。 */
  bridge_guide: string | null;
}

export interface BackupFile {
  name: string;
  path: string;
  bytes: number;
}

export interface Exported {
  dir: string;
  tickets: number;
  attachments: number;
  /** 数据库里有记录、但文件已经不在的附件。 */
  missing: string[];
}

export interface PrInput {
  title: string;
  body: string;
  draft: boolean;
}

export interface CodeFinding {
  severity: 'blocker' | 'major' | 'minor';
  file: string | null;
  line: number | null;
  title: string;
  detail: string;
  suggestion: string;
}

/** AI 交叉审查：另一个 Agent 只读看完子任务的 Diff 后给的问题列表，只供参考。 */
export interface CodeReview {
  id: string;
  task_id: string;
  ticket_id: string;
  run_id: string;
  head_commit: string | null;
  agent: string;
  model: string | null;
  status: 'running' | 'done' | 'failed';
  summary: string | null;
  findings: CodeFinding[];
  error: string | null;
  created_at: string;
  updated_at: string;
}

export interface TicketDetail {
  ticket: Ticket;
  versions: PrdVersion[];
  tasks: Task[];
  runs: Run[];
  timeline: TimelineEvent[];
  reviews: Review[];
  issues: Issue[];
  split: Proposal | null;
  impact: Proposal | null;
  artifacts: Artifact[];
  code_reviews: CodeReview[];
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
  /** 最近 8 周每周完成的工单数，旧的在前；week 是那一周的周一。 */
  weeks: { week: string; done: number }[];
  rework_runs: number;
  auto_fix_runs: number;
  auto_fix_succeeded: number;
  code_reviews: number;
  code_review_findings: number;
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
    }
  | { type: 'review_finished'; ticket_id: string; ticket_key: string; ticket_title: string; issues: number; blockers: number; error: string | null }
  | { type: 'proposal_ready'; ticket_id: string; ticket_key: string; ticket_title: string; kind: 'split' | 'impact'; error: string | null }
  | { type: 'api_doc_ready'; ticket_key: string; task_title: string; path: string; unblocked: number };

export const api = {
  systemInfo: () => invoke<SystemInfo>('system_info'),
  /** 这个协议的 Provider 哪个 Agent 能直接用；不能的给出 LiteLLM 桥接说明。 */
  providerCompat: (name: string, protocol: string, baseUrl: string | null, defaultModel: string | null) =>
    invoke<ProviderCompat[]>('provider_compat', { name, protocol, baseUrl, defaultModel }),
  /** 联网读 GitHub 上最新发布的版本号。 */
  updateCheck: () => invoke<UpdateInfo>('update_check'),
  backupList: () => invoke<BackupFile[]>('backup_list'),
  backupNow: () => invoke<BackupFile[]>('backup_now'),
  /** 导出到 dest 下新建的一个目录。 */
  exportTickets: (dest: string) => invoke<Exported>('export_tickets', { dest }),
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
  /** origin：image 表示这一版只动了图片（贴图、替换、改效力、移除）。 */
  ticketSavePrd: (id: string, content: string, note?: string, origin?: 'edit' | 'image') =>
    invoke<PrdVersion>('ticket_save_prd', { id, content, note: note ?? null, origin: origin ?? null }),
  ticketTransition: (id: string, to: TicketStatus) => invoke<Ticket>('ticket_transition', { id, to }),
  ticketDraft: (markdown: string) =>
    invoke<{ title: string; type: TicketType; content_md: string }>('ticket_draft', { markdown }),
  readTextFile: (path: string) => invoke<string>('read_text_file', { path }),
  importDocument: (path: string) => invoke<Imported>('import_document', { path }),

  /** ticketId 为空：工单还没建，建单时认领。 */
  attachmentAdd: (ticketId: string | null, fileName: string, dataBase64: string, origin: 'prd' | 'user' = 'user') =>
    invoke<Attachment>('attachment_add', { ticketId, fileName, dataBase64, origin }),
  attachmentList: (ticketId: string) => invoke<Attachment[]>('attachment_list', { ticketId }),
  attachmentData: (id: string) => invoke<string>('attachment_data', { id }),
  attachmentDescribe: (id: string, description: string) =>
    invoke<Attachment>('attachment_describe', { id, description }),

  reviewStart: (id: string) => invoke<Run>('review_start', { id }),
  reviewSkip: (id: string) => invoke<Ticket>('review_skip', { id }),
  issuePreview: (id: string, action: 'accept' | 'ignore', note?: string) =>
    invoke<string>('issue_preview', { id, action, note: note ?? null }),
  /** action：accept 采纳 / manual 手动修改 / ignore 忽略（要理由）/ pending 待确认 / reopen 重新打开。 */
  issueResolve: (id: string, action: 'accept' | 'manual' | 'ignore' | 'pending' | 'reopen', note?: string, content?: string) =>
    invoke<Issue>('issue_resolve', { id, action, note: note ?? null, content: content ?? null }),
  questionsExport: (id: string) => invoke<string>('questions_export', { id }),
  criteriaSet: (id: string, criteria: string[]) => invoke<Ticket>('criteria_set', { id, criteria }),
  ticketFinalize: (id: string, split: boolean) => invoke<{ ticket: Ticket; run: Run | null }>('ticket_finalize', { id, split }),
  proposalRegenerate: (ticketId: string) => invoke<Run>('proposal_regenerate', { id: ticketId }),
  proposalDiscard: (proposalId: string) => invoke<void>('proposal_discard', { id: proposalId }),
  splitApply: (ticketId: string, tasks: TaskDraft[]) => invoke<Task[]>('split_apply', { id: ticketId, tasks }),
  impactApply: (ticketId: string, impacts: TaskImpact[], newTasks: TaskDraft[]) =>
    invoke<Task[]>('impact_apply', { id: ticketId, impacts, newTasks }),
  developmentStart: (ticketId: string) => invoke<Run[]>('development_start', { id: ticketId }),
  changeRequest: (ticketId: string, reason: string, stopRunning: boolean) =>
    invoke<Ticket>('change_request', { id: ticketId, reason, stopRunning }),

  promptTemplates: () => invoke<TemplateInfo[]>('prompt_templates'),
  promptPreview: (name: string, content: string) =>
    invoke<{ text: string; unknown_variables: string[] }>('prompt_preview', { name, content }),
  /** content 为 null 恢复默认。 */
  promptSave: (name: string, content: string | null) => invoke<TemplateInfo[]>('prompt_save', { name, content }),

  taskCreate: (input: TaskInput) => invoke<Task>('task_create', { input }),
  taskUpdate: (id: string, input: TaskInput) => invoke<Task>('task_update', { id, input }),
  taskPlan: (id: string, options: StartOptions = {}) => invoke<RunPlan>('task_plan', { id, options }),
  taskStart: (id: string, options: StartOptions = {}) => invoke<Run>('task_start', { id, options }),
  taskAccept: (id: string) => invoke<Task>('task_accept', { id }),
  taskReject: (id: string, input: RejectInput) => invoke<Task>('task_reject', { id, input }),
  taskMerge: (id: string) => invoke<Task>('task_merge', { id }),
  taskCancel: (id: string) => invoke<Task>('task_cancel', { id }),
  taskDiff: (id: string) => invoke<TaskDiff>('task_diff', { id }),
  /** 把子任务的分支推到远程。只在用户点了之后调用。 */
  taskPush: (id: string) => invoke<Task>('task_push', { id }),
  taskPrDraft: (id: string) => invoke<PrInput>('task_pr_draft', { id }),
  /** 分支没推过（或推过后又有新提交）会先推送，再用 gh pr create 开 PR。 */
  taskPrCreate: (id: string, input: PrInput) => invoke<Task>('task_pr_create', { id, input }),
  /** 不指定 agent 时换一个和写代码的不同的 Agent。 */
  codeReviewStart: (taskId: string, agent?: string | null, providerId?: string | null) =>
    invoke<Run>('code_review_start', { taskId, agent: agent ?? null, providerId: providerId ?? null }),

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
