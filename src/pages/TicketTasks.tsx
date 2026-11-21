// 工单详情里的「子任务」与「验收」两页
import { useEffect, useMemo, useState } from 'react';
import { api, type RunDetail, type Task, type TaskDiff, type TaskInput } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import {
  AGENTS,
  AgentTag,
  Empty,
  Field,
  Inline,
  KindStamp,
  PERMISSIONS,
  TASK_KIND,
  TaskStatusTag,
  fmtCost,
  fmtDur,
  runActive,
  runSeconds,
  useUi,
} from '../lib/ui';
import type { TabProps } from './Ticket';

function TaskForm({ d, onDone }: { d: TabProps['d']; onDone: (created: boolean) => void }) {
  const { projects, providers } = useData();
  const ui = useUi();
  const t = d.ticket;
  const bound = projects.filter((p) => t.projects.some((x) => x.project_id === p.id));
  const choices = bound.length ? bound : projects;
  const [f, setF] = useState<TaskInput>({
    ticket_id: t.id,
    kind: t.type === 'bug' ? 'bug' : t.type === 'feature_frontend' ? 'frontend' : 'backend',
    project_id: choices[0]?.id ?? '',
    title: t.title,
    spec_md: d.versions[d.versions.length - 1]?.content_md ?? '',
    acceptance_criteria: t.acceptance_criteria,
    agent: null,
    provider_id: null,
    model: null,
    permission_profile: null,
    depends_on: [],
  });
  const set = (patch: Partial<TaskInput>) => setF((x) => ({ ...x, ...patch }));
  const deps = d.tasks.filter((k) => k.status !== 'cancelled');

  const submit = async () => {
    if (await ui.attempt(api.taskCreate(f), '已创建子任务')) onDone(true);
  };

  return (
    <section className="task card" style={{ padding: 16 }}>
      <div className="form">
        <div className="form-grid">
          <Field label="类型">
            <div className="pick">
              {Object.entries(TASK_KIND).map(([k, v]) => (
                <button key={k} className={f.kind === k ? 'on' : ''} onClick={() => set({ kind: k as TaskInput['kind'] })}>
                  <KindStamp kind={k} />
                  {v.name}
                </button>
              ))}
            </div>
          </Field>
          <Field label="项目">
            <select className="input" value={f.project_id} onChange={(e) => set({ project_id: e.target.value })}>
              {choices.length === 0 && <option value="">还没有注册项目</option>}
              {choices.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </Field>
        </div>
        <Field label="标题">
          <input className="input" value={f.title} onChange={(e) => set({ title: e.target.value })} />
        </Field>
        <Field label="任务说明" hint="会拼进给 Agent 的 Prompt；默认是当前 PRD">
          <textarea className="textarea mono" rows={8} value={f.spec_md} onChange={(e) => set({ spec_md: e.target.value })} />
        </Field>
        <Field label="验收标准" hint="每行一条">
          <textarea
            className="textarea"
            rows={3}
            value={f.acceptance_criteria.join('\n')}
            onChange={(e) => set({ acceptance_criteria: e.target.value.split('\n') })}
          />
        </Field>
        <div className="form-grid">
          <Field label="Agent" hint="不选则用工单 / 项目 / 全局默认">
            <select className="input" value={f.agent ?? ''} onChange={(e) => set({ agent: e.target.value || null })}>
              <option value="">跟随默认</option>
              {Object.entries(AGENTS).map(([k, v]) => (
                <option key={k} value={k}>
                  {v.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label="Provider">
            <select className="input" value={f.provider_id ?? ''} onChange={(e) => set({ provider_id: e.target.value || null })}>
              <option value="">跟随默认</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label="模型" hint="可选">
            <input className="input mono" value={f.model ?? ''} onChange={(e) => set({ model: e.target.value || null })} placeholder="跟随 Provider 默认" />
          </Field>
          <Field label="权限档位">
            <select className="input" value={f.permission_profile ?? ''} onChange={(e) => set({ permission_profile: e.target.value || null })}>
              <option value="">跟随全局默认</option>
              {PERMISSIONS.map((p) => (
                <option key={p} value={p}>
                  {p}
                </option>
              ))}
            </select>
          </Field>
        </div>
        {deps.length > 0 && (
          <Field label="依赖" hint="被依赖的子任务跑完后才能开始">
            <div className="pick">
              {deps.map((k) => (
                <button
                  key={k.id}
                  className={f.depends_on.includes(k.id) ? 'on' : ''}
                  onClick={() => set({ depends_on: f.depends_on.includes(k.id) ? f.depends_on.filter((x) => x !== k.id) : [...f.depends_on, k.id] })}
                >
                  <KindStamp kind={k.kind} />
                  {k.title}
                </button>
              ))}
            </div>
          </Field>
        )}
        <div className="row">
          <span className="grow" />
          <button className="btn" onClick={() => onDone(false)}>
            取消
          </button>
          <button className="btn primary" disabled={!f.project_id || !f.title.trim()} onClick={() => void submit()}>
            创建子任务
          </button>
        </div>
      </div>
    </section>
  );
}

/** 开始 / 恢复一个子任务。full_auto 每次都要二次确认。 */
export function useStartTask(reload: () => Promise<void>) {
  const ui = useUi();
  const data = useData();
  return async (task: Task, instruction?: string) => {
    const plan = await ui.attempt(api.taskPlan(task.id, { instruction }));
    if (!plan) return;
    if (
      plan.permission === 'full_auto' &&
      !(await ui.confirm('以 full_auto 运行', 'Agent 将不经确认执行任意命令。建议只在可丢弃的 worktree 中使用。', '我了解风险，开始', true))
    )
      return;
    const run = await ui.attempt(api.taskStart(task.id, { instruction, command_id: crypto.randomUUID() }));
    if (run) {
      ui.toast(plan.resume ? `已入队，将续接会话 ${plan.resume[1].slice(0, 8)}` : '已入队，有空闲并发时自动开始');
      await Promise.all([reload(), data.reload()]);
    }
  };
}

export function TasksTab({ d, reload }: TabProps) {
  const { projects, providers } = useData();
  const ui = useUi();
  const data = useData();
  const start = useStartTask(reload);
  const [adding, setAdding] = useState(false);
  const t = d.ticket;
  const canAdd = ['developing', 'finalized', 'acceptance'].includes(t.status);
  const refresh = () => Promise.all([reload(), data.reload()]);

  const act = async (p: Promise<unknown>, ok: string) => {
    if ((await ui.attempt(p, ok)) !== undefined) await refresh();
  };

  return (
    <div className="page">
      <div className="tasks">
        {d.tasks.length === 0 && !adding && (
          <div className="card">
            <Empty
              icon="branch"
              title="还没有子任务"
              body={canAdd ? '每个子任务在自己的分支和 worktree 里交给一个 Agent 执行。' : '工单进入「开发中」后才能创建子任务。'}
            />
          </div>
        )}
        {d.tasks.map((k) => {
          const runs = d.runs.filter((r) => r.task_id === k.id);
          const last = runs[runs.length - 1];
          const activeRun = runs.find((r) => runActive(r.status) || r.status === 'queued');
          const cost = runs.reduce((a, r) => a + (r.cost_usd ?? 0), 0);
          const secs = runs.reduce((a, r) => a + runSeconds(r), 0);
          const depNames = k.depends_on.map((id) => d.tasks.find((x) => x.id === id)?.title ?? '已删除的子任务');
          const provider = providers.find((p) => p.id === (k.provider_id ?? last?.provider_id));
          return (
            <div key={k.id}>
              {depNames.length > 0 && (
                <div className="dep-line">
                  <span>
                    <Icon name="link" size={12} />
                    依赖：{depNames.join('、')}
                  </span>
                </div>
              )}
              <section className="task card">
                <header>
                  <KindStamp kind={k.kind} lg />
                  <div className="grow">
                    <b>{k.title}</b>
                    <div className="dim">
                      {TASK_KIND[k.kind]?.name ?? k.kind}子任务 · {projects.find((p) => p.id === k.project_id)?.name ?? '已删除的项目'}
                    </div>
                  </div>
                  <TaskStatusTag status={k.status} />
                </header>
                <div className="task-grid">
                  <div>
                    <label>Agent</label>
                    {k.agent || last ? <AgentTag agent={k.agent ?? last!.agent} /> : <span className="dim">跟随默认</span>}
                  </div>
                  <div>
                    <label>Provider · 模型</label>
                    {provider?.name ?? '跟随默认'} · <span className="mono">{k.model ?? last?.model ?? '默认模型'}</span>
                  </div>
                  <div>
                    <label>权限档位</label>
                    <span className="tag line">
                      <Icon name="shield" size={11} />
                      {k.permission_profile ?? last?.permission ?? '跟随默认'}
                    </span>
                  </div>
                  <div>
                    <label>分支</label>
                    <span className="mono">{k.branch ?? '开始后创建'}</span>
                  </div>
                  <div style={{ gridColumn: 'span 2' }}>
                    <label>worktree</label>
                    <span className="mono">{k.worktree_path ?? (k.status === 'merged' ? '合并后已清理' : '开始后创建')}</span>
                  </div>
                  <div>
                    <label>最近运行</label>
                    {last ? (
                      <>
                        <a className="lnk mono" href={`#/runs/${last.id}`}>
                          第 {last.number} 次
                        </a>{' '}
                        · 共 {runs.length} 次
                      </>
                    ) : (
                      <span className="dim">尚未运行</span>
                    )}
                  </div>
                  <div>
                    <label>耗时 · 费用</label>
                    <span className="mono">{runs.length ? `${fmtDur(secs)} · ${fmtCost(cost)}` : '—'}</span>
                  </div>
                  {last && last.diff_stat.length > 0 && (
                    <div style={{ gridColumn: 'span 4' }}>
                      <label>改动</label>
                      <span className="mono dim">
                        {last.diff_stat.length} 个文件 +{last.diff_stat.reduce((a, x) => a + x.added, 0)} −
                        {last.diff_stat.reduce((a, x) => a + x.removed, 0)}
                      </span>
                      {k.merged_commit && <span className="tag moss" style={{ marginLeft: 8 }}>已合并 {k.merged_commit.slice(0, 8)}</span>}
                    </div>
                  )}
                  {last?.error && (
                    <div style={{ gridColumn: 'span 4', color: 'var(--cinnabar)' }}>
                      <label>上次运行的错误</label>
                      {last.error}
                    </div>
                  )}
                </div>
                <footer>
                  {activeRun && (
                    <>
                      <a className="btn sm" href={`#/runs/${activeRun.id}`}>
                        <Icon name="term" size={13} />
                        看实时日志
                      </a>
                      <button
                        className="btn danger sm"
                        onClick={() => void act(api.runStop(activeRun.id), activeRun.status === 'queued' ? '已移出队列' : '正在停止，可稍后恢复原会话')}
                      >
                        <Icon name="stop" size={13} />
                        {activeRun.status === 'queued' ? '取消排队' : '停止'}
                      </button>
                    </>
                  )}
                  {k.status === 'pending' && (
                    <button className="btn primary sm" onClick={() => void start(k)}>
                      <Icon name="play" size={13} />
                      开始
                    </button>
                  )}
                  {k.status === 'blocked' && (
                    <button className="btn sm" disabled title="等待依赖的子任务跑完">
                      <Icon name="play" size={13} />
                      等待依赖
                    </button>
                  )}
                  {['stopped', 'failed'].includes(k.status) && (
                    <button className="btn sm" onClick={() => void start(k)}>
                      <Icon name="resume" size={13} />
                      {k.status === 'stopped' ? '恢复' : '重试'}
                    </button>
                  )}
                  {k.status === 'changes_requested' && (
                    <button className="btn primary sm" onClick={() => void start(k)}>
                      <Icon name="resume" size={13} />
                      按打回意见返工
                    </button>
                  )}
                  {k.status === 'review' && (
                    <a className="btn primary sm" href={`#/ticket/${t.key}/accept`}>
                      去验收
                    </a>
                  )}
                  {k.status === 'accepted' && (
                    <button className="btn primary sm" onClick={() => void act(api.taskMerge(k.id), '已合并，worktree 已清理')}>
                      <Icon name="merge" size={13} />
                      合并到主分支
                    </button>
                  )}
                  {!['merged', 'cancelled', 'running', 'queued'].includes(k.status) && (
                    <button
                      className="btn ghost sm"
                      onClick={async () => {
                        if (await ui.confirm('取消子任务', '会删除它的 worktree；分支保留，改动不会丢。', '取消子任务', true))
                          await act(api.taskCancel(k.id), '已取消子任务');
                      }}
                    >
                      取消子任务
                    </button>
                  )}
                </footer>
              </section>
            </div>
          );
        })}
        {adding ? (
          <TaskForm
            d={d}
            onDone={(created) => {
              setAdding(false);
              if (created) void refresh();
            }}
          />
        ) : (
          canAdd && (
            <div className="row" style={{ marginTop: 12 }}>
              <button className="btn" onClick={() => setAdding(true)}>
                <Icon name="plus" size={13} />
                添加子任务
              </button>
            </div>
          )
        )}
      </div>
    </div>
  );
}

interface FilePatch {
  path: string;
  status: 'A' | 'M' | 'D';
  rows: [number | null, number | null, 'ctx' | 'add' | 'del' | 'hunk', string][];
}

/** 把 `git diff` 的输出按文件拆开，并算出左右行号。 */
export function parsePatch(patch: string): FilePatch[] {
  const files: FilePatch[] = [];
  let cur: FilePatch | null = null;
  let a = 0;
  let b = 0;
  let inHunk = false;
  for (const line of patch.split('\n')) {
    if (line.startsWith('diff --git ')) {
      const m = / b\/(.+)$/.exec(line);
      cur = { path: m ? m[1] : line.slice(11), status: 'M', rows: [] };
      files.push(cur);
      inHunk = false;
    } else if (!cur) {
      continue;
    } else if (!inHunk && line.startsWith('new file')) {
      cur.status = 'A';
    } else if (!inHunk && line.startsWith('deleted file')) {
      cur.status = 'D';
    } else if (line.startsWith('@@')) {
      const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)/.exec(line);
      a = m ? Number(m[1]) : 0;
      b = m ? Number(m[2]) : 0;
      inHunk = true;
      cur.rows.push([null, null, 'hunk', line]);
    } else if (!inHunk || line.startsWith('\\')) {
      continue;
    } else if (line.startsWith('+')) {
      cur.rows.push([null, b++, 'add', line.slice(1)]);
    } else if (line.startsWith('-')) {
      cur.rows.push([a++, null, 'del', line.slice(1)]);
    } else {
      cur.rows.push([a++, b++, 'ctx', line.slice(1)]);
    }
  }
  return files;
}

export function AcceptTab({ d, reload }: TabProps) {
  const ui = useUi();
  const data = useData();
  const start = useStartTask(reload);
  const t = d.ticket;
  const viewable = d.tasks.filter((k) => ['review', 'accepted', 'merged'].includes(k.status));
  const [sel, setSel] = useState<string | null>(null);
  const task = viewable.find((k) => k.id === sel) ?? viewable.find((k) => k.status === 'review') ?? viewable[0];
  const [diff, setDiff] = useState<TaskDiff | null>(null);
  const [diffError, setDiffError] = useState<string | null>(null);
  const [run, setRun] = useState<RunDetail | null>(null);
  const [file, setFile] = useState<string | null>(null);
  const [checked, setChecked] = useState<Record<string, boolean>>({});
  const last = task ? d.runs.filter((r) => r.task_id === task.id).pop() : undefined;

  useEffect(() => {
    setDiff(null);
    setDiffError(null);
    setRun(null);
    if (!task) return;
    let stale = false;
    api.taskDiff(task.id).then(
      (v) => !stale && setDiff(v),
      (e) => !stale && setDiffError(String(e)),
    );
    if (last) api.runGet(last.id, Number.MAX_SAFE_INTEGER).then((v) => !stale && setRun(v), () => undefined);
    return () => {
      stale = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [task?.id, task?.status, last?.id, last?.status]);

  const files = useMemo(() => (diff ? parsePatch(diff.patch) : []), [diff]);

  if (!task)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="check" title="还没有可验收的改动" body="子任务跑完后，这里会出现 Diff、检查结果和 Agent 总结。">
            <a className="btn" href={`#/ticket/${t.key}/tasks`}>
              查看子任务
            </a>
          </Empty>
        </div>
      </div>
    );

  const shown = files.find((f) => f.path === file) ?? files[0];
  const criteria = task.acceptance_criteria.length ? task.acceptance_criteria : t.acceptance_criteria;
  const unchecked = criteria.filter((_, i) => !checked[`${task.id}:${i}`]).length;
  const add = diff?.files.reduce((n, f) => n + f.added, 0) ?? 0;
  const del = diff?.files.reduce((n, f) => n + f.removed, 0) ?? 0;
  const refresh = () => Promise.all([reload(), data.reload()]);

  const pass = async () => {
    if (unchecked) {
      ui.toast(`还有 ${unchecked} 条验收标准未确认`);
      return;
    }
    const failed = run?.reports.filter((r) => r.exit_code !== 0) ?? [];
    const body = failed.length
      ? `注意：${failed.map((r) => r.command).join('、')} 没有通过。通过后还需要点「合并」才会进入主分支。`
      : '子任务标记为已通过。之后点「合并」才会进入主分支。';
    if (!(await ui.confirm('通过这个子任务', body, '通过'))) return;
    if (await ui.attempt(api.taskAccept(task.id), '已通过')) await refresh();
  };

  const reject = async () => {
    const note = await ui.prompt('打回子任务', '意见会交给 Agent，优先续接原会话返工。', '写明需要修改的地方', '打回并开始返工', true);
    if (!note) return;
    const rejected = await ui.attempt(api.taskReject(task.id, note));
    if (!rejected) return;
    await start(rejected);
    await refresh();
  };

  const merge = async () => {
    if (!(await ui.confirm('合并到主分支', '把这个子任务的分支合并进项目的默认分支，然后清理 worktree。不会推送到远程。', '合并'))) return;
    if (await ui.attempt(api.taskMerge(task.id), '已合并，worktree 已清理')) await refresh();
  };

  return (
    <div className="acc">
      <div className="acc-bar">
        <div className="seg">
          {viewable.map((k) => (
            <button key={k.id} className={k.id === task.id ? 'on' : ''} onClick={() => setSel(k.id)}>
              <span style={{ marginRight: 6, display: 'inline-grid' }}>
                <KindStamp kind={k.kind} />
              </span>
              {k.title.slice(0, 16)}
            </button>
          ))}
        </div>
        <TaskStatusTag status={task.status} />
        <span className="mono dim">
          {diff?.files.length ?? 0} 个文件 <span style={{ color: 'var(--moss)' }}>+{add}</span>{' '}
          <span style={{ color: 'var(--cinnabar)' }}>−{del}</span>
        </span>
        <span className="grow" />
        {task.status === 'review' && (
          <>
            <button className="btn danger sm" onClick={() => void reject()}>
              <Icon name="undo" size={13} />
              打回
            </button>
            <button className="btn primary sm" onClick={() => void pass()}>
              <Icon name="check" size={13} />
              通过
            </button>
          </>
        )}
        {task.status === 'accepted' && (
          <>
            <button className="btn sm" onClick={() => void reject()}>
              <Icon name="undo" size={13} />
              打回
            </button>
            <button className="btn primary sm" onClick={() => void merge()}>
              <Icon name="merge" size={13} />
              合并
            </button>
          </>
        )}
        {task.status === 'merged' && (
          <span className="tag moss">
            <Icon name="check" size={11} />
            已合并 {task.merged_commit?.slice(0, 8)}
          </span>
        )}
      </div>
      <div className="acc-cols">
        <nav className="ftree">
          <div className="ftree-h">改动文件</div>
          {files.map((f) => {
            const parts = f.path.split('/');
            const stat = diff?.files.find((x) => x.path === f.path);
            return (
              <a key={f.path} className={f === shown ? 'on' : ''} title={f.path} onClick={() => setFile(f.path)}>
                <span className={`fs ${f.status}`}>{f.status}</span>
                <span className="grow ell">
                  <b>{parts.pop()}</b>
                  <small>{parts.slice(-2).join('/')}</small>
                </span>
                <span className="mono dim">
                  {stat?.added ? `+${stat.added}` : ''}
                  {stat?.removed ? ` −${stat.removed}` : ''}
                </span>
              </a>
            );
          })}
        </nav>
        <section className="diff">
          <div className="diff-h">
            <span className="mono">
              <b>{shown?.path ?? '—'}</b>
            </span>
            <span className="grow" />
            <span className="dim">{diff?.truncated ? 'Diff 太大，只显示了前一部分' : '并排对比'}</span>
          </div>
          <div className="diff-b">
            {diffError ? (
              <Empty icon="alert" title="读不到 Diff" body={diffError} />
            ) : !diff ? (
              <p className="dim" style={{ padding: 16 }}>
                读取中…
              </p>
            ) : !shown ? (
              <Empty icon="file" title="这个子任务没有改动文件" />
            ) : (
              <table>
                <tbody>
                  {shown.rows.map(([ol, nl, kind, text], i) =>
                    kind === 'hunk' ? (
                      <tr key={i}>
                        <td className="ln" />
                        <td className="code l dim" colSpan={3}>
                          {text}
                        </td>
                      </tr>
                    ) : (
                      <tr key={i} className={kind === 'ctx' ? '' : kind}>
                        <td className="ln">{ol ?? ''}</td>
                        <td className="code l">{kind === 'add' ? '' : text}</td>
                        <td className="ln">{nl ?? ''}</td>
                        <td className="code r">{kind === 'del' ? '' : text}</td>
                      </tr>
                    ),
                  )}
                </tbody>
              </table>
            )}
          </div>
        </section>
        <aside className="acc-side">
          <div className="sec-title">
            验收标准
            <small>
              {criteria.length - unchecked} / {criteria.length} 已确认
            </small>
          </div>
          {criteria.length ? (
            <ul className="ac">
              {criteria.map((c, i) => {
                const k = `${task.id}:${i}`;
                return (
                  <li key={k} onClick={() => task.status === 'review' && setChecked((x) => ({ ...x, [k]: !x[k] }))}>
                    <span className={`check ${checked[k] || task.status !== 'review' ? 'on' : ''}`}>
                      <Icon name="check" size={11} />
                    </span>
                    {c}
                  </li>
                );
              })}
            </ul>
          ) : (
            <p className="dim">这张工单没有写验收标准。</p>
          )}
          <div className="sec-title" style={{ marginTop: 18 }}>
            自动检查
          </div>
          <div className="tests">
            {run?.reports.length ? (
              run.reports.map((r) => (
                <div key={r.id}>
                  <span className={`tag ${r.exit_code === 0 ? 'moss' : 'cin'}`}>
                    <Icon name={r.exit_code === 0 ? 'check' : 'x'} size={11} />
                    {r.exit_code === 0 ? '通过' : r.exit_code == null ? '超时' : `退出码 ${r.exit_code}`}
                  </span>
                  <span className="mono ell">{r.command}</span>
                  <span className="dim">{fmtDur(r.duration_ms / 1000)}</span>
                </div>
              ))
            ) : (
              <p className="dim">这次运行没有执行检查命令（项目没有配置，或在设置里关掉了）。</p>
            )}
          </div>
          <div className="sec-title" style={{ marginTop: 18 }}>
            Agent 总结
          </div>
          <p className="sum">{last?.summary ? <Inline text={last.summary} /> : <span className="dim">Agent 没有留下总结。</span>}</p>
          {last && (
            <div className="cost mono">
              <Icon name="clock" size={12} />
              <a className="lnk" href={`#/runs/${last.id}`}>
                第 {last.number} 次运行
              </a>{' '}
              · {fmtDur(runSeconds(last))} · {fmtCost(last.cost_usd)}
            </div>
          )}
        </aside>
      </div>
    </div>
  );
}
