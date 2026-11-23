// 工单详情里的「子任务」与「验收」两页
import { Fragment, useEffect, useMemo, useState } from 'react';
import {
  api,
  type CodeFinding,
  type CompareOption,
  type InlineComment,
  type Proposal,
  type Run,
  type RunDetail,
  type Task,
  type TaskDiff,
  type TaskDraft,
  type TaskImpact,
  type TaskInput,
} from '../lib/api';
import { useData } from '../lib/data';
import { useConfirmTicketRun, useStartTask } from '../lib/start';
import { Icon } from '../lib/Icon';
import {
  AGENTS,
  AgentTag,
  Empty,
  Field,
  Inline,
  KindStamp,
  Modal,
  PERMISSIONS,
  Ripple,
  Seg,
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
    api_doc_deps: [],
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
                  onClick={() => {
                    const depends_on = f.depends_on.includes(k.id) ? f.depends_on.filter((x) => x !== k.id) : [...f.depends_on, k.id];
                    set({ depends_on, api_doc_deps: (f.api_doc_deps ?? []).filter((x) => depends_on.includes(x)) });
                  }}
                >
                  <KindStamp kind={k.kind} />
                  {k.title}
                </button>
              ))}
            </div>
            {f.depends_on.some((id) => deps.find((k) => k.id === id)?.kind === 'backend') && (
              <label className="row" style={{ marginTop: 8, cursor: 'pointer' }}>
                <input
                  type="checkbox"
                  checked={(f.api_doc_deps ?? []).length > 0}
                  onChange={(e) =>
                    set({ api_doc_deps: e.target.checked ? f.depends_on.filter((id) => deps.find((k) => k.id === id)?.kind === 'backend') : [] })
                  }
                />
                后端写出接口文档就可以开始，不用等它做完
              </label>
            )}
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

const DRAFT_KINDS = ['backend', 'frontend', 'bug', 'doc'];

/** 把 AI 给的建议整理成可以编辑的草稿；字段缺了或类型不对的用空值补上。 */
function toDrafts(raw: unknown, projects: { id: string; name: string }[]): TaskDraft[] {
  if (!Array.isArray(raw)) return [];
  return raw.map((x, i) => {
    const o = (x ?? {}) as Record<string, unknown>;
    const str = (k: string) => (typeof o[k] === 'string' ? (o[k] as string) : '');
    const list = (k: string) => (Array.isArray(o[k]) ? (o[k] as unknown[]).filter((v): v is string => typeof v === 'string') : []);
    const wanted = str('project').trim().toLowerCase();
    const project = projects.find((p) => p.id === str('project')) ?? projects.find((p) => p.name.toLowerCase() === wanted);
    return {
      ref: str('ref') || `T${i + 1}`,
      kind: DRAFT_KINDS.includes(str('kind')) ? str('kind') : 'backend',
      project: project?.id ?? '',
      title: str('title'),
      spec: str('spec'),
      acceptance_criteria: list('acceptance_criteria'),
      depends_on: list('depends_on'),
      dependency_kind: str('dependency_kind') || null,
      agent: str('agent') || null,
    };
  });
}

function DraftList({ drafts, setDrafts, choices }: { drafts: TaskDraft[]; setDrafts: (d: TaskDraft[]) => void; choices: { id: string; name: string }[] }) {
  const [open, setOpen] = useState<string | null>(null);
  const patch = (ref: string, p: Partial<TaskDraft>) => setDrafts(drafts.map((x) => (x.ref === ref ? { ...x, ...p } : x)));
  const remove = (ref: string) =>
    setDrafts(drafts.filter((x) => x.ref !== ref).map((x) => ({ ...x, depends_on: x.depends_on.filter((r) => r !== ref) })));
  return (
    <div className="splits">
      {drafts.map((s) => {
        const deps = s.depends_on.map((r) => drafts.find((x) => x.ref === r)?.title || r);
        return (
          <div key={s.ref}>
            {deps.length > 0 && (
              <div className="dep-line">
                <span>
                  <Icon name="link" size={12} />
                  {s.dependency_kind === 'api_doc' ? `等「${deps.join('、')}」写出接口文档后解锁，不必等它全部实现` : `等「${deps.join('、')}」完成后解锁`}
                </span>
              </div>
            )}
            <div className="sp-item">
              <KindStamp kind={s.kind} lg />
              <div className="grow">
                <input className="input" value={s.title} placeholder="子任务标题" onChange={(e) => patch(s.ref, { title: e.target.value })} aria-label="子任务标题" />
                <div className="row" style={{ marginTop: 8, flexWrap: 'wrap' }}>
                  <select className="input" style={{ width: 'auto' }} value={s.kind} onChange={(e) => patch(s.ref, { kind: e.target.value })} aria-label="类型">
                    {DRAFT_KINDS.map((k) => (
                      <option key={k} value={k}>
                        {TASK_KIND[k]?.name ?? k}
                      </option>
                    ))}
                  </select>
                  <select className="input" style={{ width: 'auto' }} value={s.project} onChange={(e) => patch(s.ref, { project: e.target.value })} aria-label="项目">
                    <option value="">选择项目</option>
                    {choices.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                  <select className="input" style={{ width: 'auto' }} value={s.agent ?? ''} onChange={(e) => patch(s.ref, { agent: e.target.value || null })} aria-label="Agent">
                    <option value="">Agent 跟随默认</option>
                    {Object.entries(AGENTS).map(([k, v]) => (
                      <option key={k} value={k}>
                        {v.name}
                      </option>
                    ))}
                  </select>
                  <span className="tag">{s.acceptance_criteria.filter((c) => c.trim()).length} 条验收标准</span>
                  <button className="btn ghost sm" onClick={() => setOpen(open === s.ref ? null : s.ref)}>
                    {open === s.ref ? '收起' : '改说明和验收标准'}
                  </button>
                </div>
                {open === s.ref && (
                  <div className="form" style={{ marginTop: 10 }}>
                    <Field label="任务说明" hint="会拼进给 Agent 的 Prompt">
                      <textarea className="textarea mono" rows={5} value={s.spec} onChange={(e) => patch(s.ref, { spec: e.target.value })} />
                    </Field>
                    <Field label="验收标准" hint="每行一条">
                      <textarea
                        className="textarea"
                        rows={3}
                        value={s.acceptance_criteria.join('\n')}
                        onChange={(e) => patch(s.ref, { acceptance_criteria: e.target.value.split('\n') })}
                      />
                    </Field>
                  </div>
                )}
              </div>
              <button className="iconbtn" title="去掉这个子任务" aria-label="去掉这个子任务" onClick={() => remove(s.ref)}>
                <Icon name="x" />
              </button>
            </div>
          </div>
        );
      })}
    </div>
  );
}

/** 定稿后 AI 给出的拆分建议 / 变更影响分析：生成中、失败、等确认三种状态。 */
function ProposalPanel({ d, p, refresh }: { d: TabProps['d']; p: Proposal; refresh: () => Promise<unknown> }) {
  const ui = useUi();
  const confirmRun = useConfirmTicketRun();
  const { projects } = useData();
  const t = d.ticket;
  const bound = projects.filter((x) => t.projects.some((b) => b.project_id === x.id));
  const choices = bound.length ? bound : projects;
  const what = p.kind === 'impact' ? '变更影响分析' : '拆分建议';
  const run = d.runs.find((r) => r.id === p.run_id);
  const [drafts, setDrafts] = useState<TaskDraft[]>(() => toDrafts(p.kind === 'impact' ? p.content?.new_tasks : p.content?.tasks, projects));
  const [impacts, setImpacts] = useState<TaskImpact[]>(() =>
    d.tasks
      .filter((k) => k.status !== 'cancelled')
      .map((k) => {
        const raw = (Array.isArray(p.content?.tasks) ? p.content.tasks : []) as Record<string, unknown>[];
        const hit = raw.find((x) => x && x.task_id === k.id);
        return {
          task_id: k.id,
          impact: hit?.impact === 'rework' ? 'rework' : 'none',
          instruction: typeof hit?.instruction === 'string' ? hit.instruction : '',
        };
      }),
  );
  const [busy, setBusy] = useState(false);

  const act = async (job: Promise<unknown>, ok: string) => {
    setBusy(true);
    const res = await ui.attempt(job, ok);
    setBusy(false);
    if (res !== undefined) await refresh();
  };
  const regenerate = async () => {
    if (await confirmRun()) await act(api.proposalRegenerate(t.id), `已重新提交，AI 正在生成${what}`);
  };
  const discard = async () => {
    const body =
      p.kind === 'impact' ? '已有的子任务保持原样，不追加任何工作。确定这次变更不影响它们吗？' : '之后可以自己添加子任务，或再让 AI 拆一次。';
    if (await ui.confirm(`放弃这份${what}`, body, '放弃', true)) await act(api.proposalDiscard(p.id), '已放弃');
  };

  if (p.status === 'running')
    return (
      <section className="task card" style={{ padding: 16 }}>
        <div className="row">
          <span className="live">
            <Ripple />
          </span>
          <b className="grow">{p.kind === 'impact' ? 'AI 正在分析这次变更对已有子任务的影响…' : 'AI 正在拆分子任务…'}</b>
          {run && (
            <>
              <a className="btn sm" href={`#/runs/${run.id}`}>
                <Icon name="term" size={13} />
                看实时日志
              </a>
              <button className="btn danger sm" disabled={busy} onClick={() => void act(api.runStop(run.id), '已停止')}>
                <Icon name="stop" size={13} />
                停止
              </button>
            </>
          )}
        </div>
        <p className="dim" style={{ marginTop: 6 }}>
          只读模式，不改项目文件。出来之后在这里确认或修改，确认前不会创建子任务，也不能开始开发。
        </p>
      </section>
    );

  if (p.status === 'failed')
    return (
      <div className="callout cin">
        <Icon name="alert" size={15} />
        <div className="grow">
          <b>{what}没有生成出来：</b>
          {p.error}{' '}
          {run && (
            <a className="lnk" href={`#/runs/${run.id}`}>
              看那次运行 →
            </a>
          )}
          <div className="row" style={{ marginTop: 8 }}>
            <button className="btn sm" disabled={busy} onClick={() => void regenerate()}>
              <Icon name="resume" size={13} />
              重新生成
            </button>
            <button className="btn ghost sm" disabled={busy} onClick={() => void discard()}>
              {p.kind === 'impact' ? '不分析了，子任务保持原样' : '不用 AI 拆，自己添加'}
            </button>
          </div>
        </div>
      </div>
    );

  const badDraft = drafts.find((x) => !x.title.trim() || !x.project);
  const clean = (list: TaskDraft[]) => list.map((x) => ({ ...x, acceptance_criteria: x.acceptance_criteria.filter((c) => c.trim()) }));

  if (p.kind === 'split')
    return (
      <section className="task card" style={{ padding: 16 }}>
        <div className="sec-title">
          <Icon name="spark" size={15} />
          AI 拆分建议<small>可以改标题、类型、项目、Agent，或去掉不要的</small>
        </div>
        {drafts.length ? <DraftList drafts={drafts} setDrafts={setDrafts} choices={choices} /> : <p className="dim">建议里没有子任务了。可以重新生成，或放弃后自己添加。</p>}
        <div className="row" style={{ marginTop: 14 }}>
          <button className="btn ghost sm" disabled={busy} onClick={() => void discard()}>
            放弃建议，自己添加
          </button>
          <button className="btn sm" disabled={busy} onClick={() => void regenerate()}>
            <Icon name="resume" size={13} />
            重新生成
          </button>
          <span className="grow" />
          <button
            className="btn primary"
            disabled={busy || !drafts.length || !!badDraft}
            title={badDraft ? '每个子任务都要有标题和项目' : undefined}
            onClick={() => void act(api.splitApply(t.id, clean(drafts)), `已创建 ${drafts.length} 个子任务`)}
          >
            <Icon name="check" size={13} />
            确认拆分，创建 {drafts.length} 个子任务
          </button>
        </div>
      </section>
    );

  const reworkMissing = impacts.find((i) => i.impact === 'rework' && !i.instruction.trim());
  return (
    <section className="task card" style={{ padding: 16 }}>
      <div className="sec-title">
        <Icon name="spark" size={15} />
        变更影响分析<small>{t.change_reason}</small>
      </div>
      {p.content?.summary && <p style={{ marginBottom: 10 }}>{p.content.summary}</p>}
      <div className="splits">
        {impacts.map((i) => {
          const k = d.tasks.find((x) => x.id === i.task_id);
          if (!k) return null;
          const set = (patch: Partial<TaskImpact>) => setImpacts(impacts.map((x) => (x.task_id === i.task_id ? { ...x, ...patch } : x)));
          return (
            <div key={i.task_id} className="sp-item">
              <KindStamp kind={k.kind} lg />
              <div className="grow">
                <div className="row">
                  <b className="grow ell">{k.title}</b>
                  <TaskStatusTag status={k.status} />
                  <Seg
                    value={i.impact}
                    onChange={(v) => set({ impact: v })}
                    options={[
                      ['none', '不受影响'],
                      ['rework', '要追加工作'],
                    ]}
                  />
                </div>
                {i.impact === 'rework' && (
                  <>
                    <textarea
                      className="textarea"
                      rows={3}
                      style={{ marginTop: 8 }}
                      placeholder="要追加做什么（会交给 Agent）"
                      value={i.instruction}
                      onChange={(e) => set({ instruction: e.target.value })}
                      aria-label="追加的工作"
                    />
                    <small className="dim">
                      {['pending', 'blocked'].includes(k.status)
                        ? '还没开始：补进任务说明，首轮就按新需求做。'
                        : k.status === 'merged'
                          ? '已经合并：会另开一个子任务做增量。'
                          : '已经做过：标为「已打回」，在原来的 worktree 里增量返工。'}
                    </small>
                  </>
                )}
              </div>
            </div>
          );
        })}
      </div>
      {drafts.length > 0 && (
        <>
          <div className="sec-title" style={{ marginTop: 16 }}>
            需要新增的子任务
          </div>
          <DraftList drafts={drafts} setDrafts={setDrafts} choices={choices} />
        </>
      )}
      <div className="row" style={{ marginTop: 14 }}>
        <button className="btn ghost sm" disabled={busy} onClick={() => void discard()}>
          这次变更不影响子任务
        </button>
        <button className="btn sm" disabled={busy} onClick={() => void regenerate()}>
          <Icon name="resume" size={13} />
          重新分析
        </button>
        <span className="grow" />
        <button
          className="btn primary"
          disabled={busy || !!reworkMissing || !!badDraft}
          title={reworkMissing ? '标了要追加工作的子任务，要写明做什么' : badDraft ? '新增的子任务都要有标题和项目' : undefined}
          onClick={() => void act(api.impactApply(t.id, impacts, clean(drafts)), '已确认，受影响的子任务等待返工')}
        >
          <Icon name="check" size={13} />
          确认影响分析
        </button>
      </div>
    </section>
  );
}

/** 同题对比里还没取消的子任务（含自己）；不在对比里时只有自己。 */
export function compareGroup(tasks: Task[], k: Task): Task[] {
  const root = k.compare_of ?? k.id;
  return tasks.filter((x) => (x.id === root || x.compare_of === root) && x.status !== 'cancelled');
}

/** 发起多 Agent 对比：另选 1 到 3 个 Agent，和这个子任务各做一遍。 */
function CompareModal({ task, onClose, onDone }: { task: Task; onClose: () => void; onDone: () => void }) {
  const { providers } = useData();
  const ui = useUi();
  const blank: CompareOption = { agent: Object.keys(AGENTS).find((a) => a !== task.agent && a !== 'mock') ?? 'codex', provider_id: null, model: null };
  const [picks, setPicks] = useState<CompareOption[]>([blank]);
  const patch = (i: number, p: Partial<CompareOption>) => setPicks((x) => x.map((o, k) => (k === i ? { ...o, ...p } : o)));
  const submit = async () => {
    const group = await ui.attempt(api.taskCompare(task.id, picks), `已入队，${picks.length + 1} 个 Agent 各做一遍`);
    if (group) onDone();
  };
  return (
    <Modal
      title="多 Agent 对比"
      hint={`「${task.title}」原样交给下面的 Agent 各做一遍：说明、验收标准、权限相同，各在自己的 worktree 和分支里，互不影响。跑完后比较结果，采用一个，其余的取消。费用按 Agent 个数成倍增加。`}
      width={680}
      onClose={onClose}
      footer={
        <>
          <span className="grow" />
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn primary" onClick={() => void submit()}>
            开始对比
          </button>
        </>
      }
    >
      <div className="form">
        {picks.map((o, i) => (
          <div key={i} className="row" style={{ gap: 6, marginBottom: 6, alignItems: 'center' }}>
            <select className="input" style={{ width: 150 }} value={o.agent} onChange={(e) => patch(i, { agent: e.target.value })} aria-label={`候选 ${i + 1} 的 Agent`}>
              {Object.entries(AGENTS).map(([k, v]) => (
                <option key={k} value={k}>
                  {v.name}
                </option>
              ))}
            </select>
            <select className="input" style={{ flex: 1 }} value={o.provider_id ?? ''} onChange={(e) => patch(i, { provider_id: e.target.value || null })} aria-label={`候选 ${i + 1} 的 Provider`}>
              <option value="">Provider 跟随默认</option>
              {providers.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
            <input className="input mono" style={{ width: 170 }} value={o.model ?? ''} onChange={(e) => patch(i, { model: e.target.value || null })} placeholder="模型（默认）" aria-label={`候选 ${i + 1} 的模型`} />
            <button className="btn sm ghost" disabled={picks.length === 1} onClick={() => setPicks((x) => x.filter((_, k) => k !== i))} aria-label={`去掉候选 ${i + 1}`}>
              <Icon name="x" size={13} />
            </button>
          </div>
        ))}
        {picks.length < 3 && (
          <button className="btn sm" onClick={() => setPicks((x) => [...x, blank])}>
            再加一个
          </button>
        )}
      </div>
    </Modal>
  );
}

/** 一轮运行的自动检查结果：几条通过、几条没通过。 */
function CheckCell({ runId }: { runId: string }) {
  const [text, setText] = useState('…');
  useEffect(() => {
    let stale = false;
    api.runGet(runId).then(
      (r) => {
        if (stale) return;
        const failed = r.reports.filter((x) => x.exit_code !== 0).length;
        setText(!r.reports.length ? '没有检查' : failed ? `${failed} 条没通过` : '全部通过');
      },
      () => !stale && setText('—'),
    );
    return () => {
      stale = true;
    };
  }, [runId]);
  return <span>{text}</span>;
}

/** 对比中的一组：各个 Agent 的结果并排，选一个采用。 */
function ComparePanel({ group, runs, adopt }: { group: Task[]; runs: Run[]; adopt: (k: Task) => void }) {
  const { providers } = useData();
  return (
    <div className="card" style={{ marginBottom: 12 }}>
      <div className="srow">
        <div>
          <b>多 Agent 对比：{group[0].title.replace(/（对比 · .*）$/, '')}</b>
          <p>各在自己的 worktree 里做同一个子任务。到「验收」页看每个的 Diff，采用一个之后其余的取消（分支留着）；没采用之前不能通过、不能合并。</p>
        </div>
      </div>
      <table className="tbl">
        <thead>
          <tr>
            <th>Agent</th>
            <th>Provider · 模型</th>
            <th>状态</th>
            <th>改动</th>
            <th>检查</th>
            <th>耗时 · 费用</th>
            <th />
          </tr>
        </thead>
        <tbody>
          {group.map((k) => {
            const mine = runs.filter((r) => r.task_id === k.id);
            const last = mine[mine.length - 1];
            const agent = k.agent ?? last?.agent;
            const provider = providers.find((p) => p.id === (k.provider_id ?? last?.provider_id));
            return (
              <tr key={k.id}>
                <td>{agent ? <AgentTag agent={agent} /> : <span className="dim">跟随默认</span>}</td>
                <td>
                  {provider?.name ?? '跟随默认'} · <span className="mono">{k.model ?? last?.model ?? '默认模型'}</span>
                </td>
                <td>
                  <TaskStatusTag status={k.status} />
                </td>
                <td className="mono">
                  {last && last.diff_stat.length > 0
                    ? `${last.diff_stat.length} 个文件 +${last.diff_stat.reduce((a, x) => a + x.added, 0)} −${last.diff_stat.reduce((a, x) => a + x.removed, 0)}`
                    : '—'}
                </td>
                <td>{last && !runActive(last.status) && last.status !== 'queued' ? <CheckCell key={`${last.id}:${last.status}`} runId={last.id} /> : '—'}</td>
                <td className="mono">{mine.length ? `${fmtDur(mine.reduce((a, r) => a + runSeconds(r), 0))} · ${fmtCost(mine.reduce((a, r) => a + (r.cost_usd ?? 0), 0))}` : '—'}</td>
                <td style={{ textAlign: 'right' }}>
                  {['review', 'accepted'].includes(k.status) && (
                    <button className="btn sm primary" onClick={() => adopt(k)}>
                      采用这个结果
                    </button>
                  )}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

export function TasksTab({ d, reload }: TabProps) {
  const { projects, providers } = useData();
  const ui = useUi();
  const confirmRun = useConfirmTicketRun();
  const data = useData();
  const start = useStartTask(reload);
  const [adding, setAdding] = useState(false);
  const t = d.ticket;
  const refresh = () => Promise.all([reload(), data.reload()]);
  // 还没处理的建议：生成中、失败、等确认。只在定稿后、开始开发前有意义
  const proposal =
    t.status === 'finalized' ? ([d.impact, d.split].find((x) => x && ['running', 'pending', 'failed'].includes(x.status)) ?? null) : null;
  const canAdd = ['developing', 'finalized', 'acceptance'].includes(t.status) && proposal?.status !== 'running';
  const live = d.tasks.filter((k) => k.status !== 'cancelled');
  const waitingStart = live.filter((k) => ['pending', 'changes_requested'].includes(k.status));
  // full_auto 每次开始都要单独确认，"开始开发"不会带上它们
  const needConfirm = waitingStart.filter((k) => k.permission_profile === 'full_auto').length;
  const startable = waitingStart.length - needConfirm;

  const act = async (p: Promise<unknown>, ok: string) => {
    if ((await ui.attempt(p, ok)) !== undefined) await refresh();
  };
  const [comparing, setComparing] = useState<Task | null>(null);
  // 还在对比的各组，按原任务
  const groups = d.tasks.filter((k) => !k.compare_of).map((k) => compareGroup(d.tasks, k)).filter((g) => g.length > 1);
  const adopt = async (k: Task) => {
    const others = compareGroup(d.tasks, k).filter((x) => x.id !== k.id);
    if (
      await ui.confirm(
        '采用这个结果',
        `其余 ${others.length} 个候选会取消：worktree 删掉，分支留着。依赖它们的子任务改为依赖这一个。之后照常验收、合并。`,
        '采用',
      )
    )
      await act(api.taskAdopt(k.id), '已采用，其余候选已取消');
  };

  return (
    <div className="page">
      <div className="tasks">
        {proposal && <ProposalPanel key={`${proposal.id}:${proposal.status}`} d={d} p={proposal} refresh={refresh} />}
        {groups.map((g) => (
          <ComparePanel key={g[0].id} group={g} runs={d.runs} adopt={(k) => void adopt(k)} />
        ))}
        {comparing && (
          <CompareModal
            task={comparing}
            onClose={() => setComparing(null)}
            onDone={() => {
              setComparing(null);
              void refresh();
            }}
          />
        )}
        {t.status === 'finalized' && !proposal && live.length > 0 && (
          <div className="callout tide">
            <Icon name="info" size={15} />
            <div className="grow">
              <b>PRD 已定稿，子任务已就绪。</b> 开始开发后，{startable} 个可以开始的子任务一起入队；等依赖的在依赖满足后自动开始。
              {needConfirm > 0 && ` 另有 ${needConfirm} 个 full_auto 子任务不会自动开始，需要逐个点「开始」确认。`}
            </div>
            <button
              className="btn primary sm"
              onClick={async () => {
                const runs = await ui.attempt(api.developmentStart(t.id));
                if (runs) {
                  const confirmHint = needConfirm ? `；${needConfirm} 个 full_auto 子任务需要逐个点「开始」确认` : '';
                  ui.toast(
                    runs.length
                      ? `已入队 ${runs.length} 个子任务${confirmHint}`
                      : needConfirm
                        ? `没有自动入队的子任务${confirmHint}`
                        : '没有可以马上开始的子任务',
                  );
                  await refresh();
                }
              }}
            >
              <Icon name="play" size={13} />
              开始开发
            </button>
          </div>
        )}
        {d.tasks.length === 0 && !adding && !proposal && (
          <div className="card">
            <Empty
              icon="branch"
              title="还没有子任务"
              body={canAdd ? '每个子任务在自己的分支和 worktree 里交给一个 Agent 执行。' : '工单定稿（Bug 是开始修复）后才能创建子任务。'}
            >
              {t.status === 'finalized' && (
                <button
                  className="btn"
                  onClick={async () => {
                    if ((await confirmRun()) && (await ui.attempt(api.proposalRegenerate(t.id), 'AI 正在拆分子任务'))) await refresh();
                  }}
                >
                  <Icon name="spark" size={13} />
                  让 AI 拆分
                </button>
              )}
            </Empty>
          </div>
        )}
        {d.tasks.map((k) => {
          const runs = d.runs.filter((r) => r.task_id === k.id);
          const last = runs[runs.length - 1];
          const activeRun = runs.find((r) => runActive(r.status) || r.status === 'queued');
          const cost = runs.reduce((a, r) => a + (r.cost_usd ?? 0), 0);
          const secs = runs.reduce((a, r) => a + runSeconds(r), 0);
          const docDeps = k.api_doc_deps ?? [];
          const depNames = k.depends_on.map(
            (id) => `${d.tasks.find((x) => x.id === id)?.title ?? '已删除的子任务'}${docDeps.includes(id) ? '（出接口文档即可）' : ''}`,
          );
          const apiDoc = d.artifacts.find((a) => a.task_id === k.id && a.kind === 'api_doc');
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
                  {compareGroup(d.tasks, k).length > 1 && <span className="tag tide">对比中</span>}
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
                  {apiDoc && (
                    <div style={{ gridColumn: 'span 4' }}>
                      <label>接口文档</label>
                      <span className="tag tide">
                        <Icon name="file" size={11} />
                        已产出
                      </span>{' '}
                      <span className="mono">{apiDoc.path}</span>
                      {apiDoc.commit_sha ? <span className="mono dim"> · {apiDoc.commit_sha.slice(0, 8)}</span> : <span className="dim"> · 还在写，运行结束后提交</span>}
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
                  {k.status === 'pending' && runs.length === 0 && !k.compare_of && compareGroup(d.tasks, k).length === 1 && (
                    <button className="btn sm" onClick={() => setComparing(k)} title="同一个子任务交给几个 Agent 各做一遍，比较后采用一个">
                      <Icon name="branch" size={13} />
                      多 Agent 对比…
                    </button>
                  )}
                  {k.status === 'blocked' && (
                    <button className="btn sm" disabled title={docDeps.length ? '等后端子任务写出接口文档' : '等待依赖的子任务跑完'}>
                      <Icon name="play" size={13} />
                      {docDeps.length ? '等接口文档' : '等待依赖'}
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
  // 还没随打回发出去的行内评论，按子任务存
  const [drafts, setDrafts] = useState<Record<string, InlineComment[]>>({});
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

  const rivals = compareGroup(d.tasks, task).filter((x) => x.id !== task.id);
  const adopt = async () => {
    if (
      !(await ui.confirm(
        '采用这个结果',
        `其余 ${rivals.length} 个候选会取消：worktree 删掉，分支留着。依赖它们的子任务改为依赖这一个。之后照常通过、合并。`,
        '采用',
      ))
    )
      return;
    if (await ui.attempt(api.taskAdopt(task.id), '已采用，其余候选已取消')) await refresh();
  };

  const comments = drafts[task.id] ?? [];
  const canComment = ['review', 'accepted'].includes(task.status);
  const addComment = async (path: string, line: number) => {
    const body = await ui.prompt('行内评论', `${path} 第 ${line} 行。评论会在打回时一起交给 Agent。`, '这一行要怎么改', '添加', true);
    if (body) setDrafts((x) => ({ ...x, [task.id]: [...(x[task.id] ?? []), { file_path: path, line, body }] }));
  };
  const removeComment = (c: InlineComment) => setDrafts((x) => ({ ...x, [task.id]: (x[task.id] ?? []).filter((y) => y !== c) }));

  const reject = async (initial?: string) => {
    const note = await ui.prompt(
      '打回子任务',
      comments.length ? `会带上 ${comments.length} 条行内评论。意见交给 Agent，优先续接原会话返工。` : '意见会交给 Agent，优先续接原会话返工。也可以先在 Diff 的行号旁加行内评论。',
      comments.length ? '总体意见（可选）' : '写明需要修改的地方',
      '打回并开始返工',
      comments.length === 0,
      initial,
    );
    if (note === null) return;
    const rejected = await ui.attempt(api.taskReject(task.id, { note, comments }));
    if (!rejected) return;
    setDrafts((x) => ({ ...x, [task.id]: [] }));
    await start(rejected);
    await refresh();
  };

  // AI 交叉审查：最新的一次；之后又跑过开发就算过期
  const cr = d.code_reviews.find((c) => c.task_id === task.id);
  const crStale = !!cr && cr.status === 'done' && !!last?.end_commit && !!cr.head_commit && cr.head_commit !== last.end_commit;
  const crossReview = async () => {
    if (await ui.attempt(api.codeReviewStart(task.id), '已发起交叉审查，结果出来后显示在这里')) await refresh();
  };
  const findingsNote = (list: CodeFinding[]) =>
    list
      .map((f, i) => {
        const at = f.file ? `（${f.file}${f.line ? `:${f.line}` : ''}）` : '';
        return `${i + 1}. ${f.title}${at}${f.detail && f.detail !== f.title ? `\n   ${f.detail}` : ''}${f.suggestion ? `\n   建议：${f.suggestion}` : ''}`;
      })
      .join('\n');

  const push = async () => {
    if (!(await ui.confirm('推送分支到远程', `把分支 ${task.branch} 推到项目仓库的远程。观澜不会自己推送，只有你点了才推；不会强推。`, '推送'))) return;
    if (await ui.attempt(api.taskPush(task.id), '已推送到远程')) await refresh();
  };
  const openPr = async () => {
    const draft = await ui.attempt(api.taskPrDraft(task.id));
    if (!draft) return;
    const title = await ui.prompt(
      '开 Pull Request',
      `用 GitHub CLI（gh pr create）给分支 ${task.branch} 开 PR；分支还没推到远程的话会先推送。正文按工单、Agent 总结和检查结果自动生成，开好后可以在 GitHub 上改。`,
      'PR 标题',
      '推送并开 PR',
      true,
      draft.title,
    );
    if (!title) return;
    const done = await ui.attempt(api.taskPrCreate(task.id, { ...draft, title }));
    if (done) {
      ui.toast(`PR 已创建：${done.pr_url}`);
      await refresh();
    }
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
              打回{comments.length ? `（${comments.length} 条评论）` : ''}
            </button>
            {rivals.length > 0 ? (
              <button className="btn primary sm" onClick={() => void adopt()} title="这个子任务还在和别的 Agent 对比，采用之后才能通过">
                <Icon name="check" size={13} />
                采用这个结果
              </button>
            ) : (
              <button className="btn primary sm" onClick={() => void pass()}>
                <Icon name="check" size={13} />
                通过
              </button>
            )}
          </>
        )}
        {task.status === 'accepted' && (
          <>
            <button className="btn sm" onClick={() => void reject()}>
              <Icon name="undo" size={13} />
              打回{comments.length ? `（${comments.length} 条评论）` : ''}
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
                      <Fragment key={i}>
                        <tr className={kind === 'ctx' ? '' : kind}>
                          <td className="ln">{ol ?? ''}</td>
                          <td className="code l">{kind === 'add' ? '' : text}</td>
                          <td className="ln">{nl ?? ''}</td>
                          <td className="code r">
                            {kind === 'del' ? '' : text}
                            {nl != null && canComment && (
                              <button className="addc" title="添加行内评论" aria-label={`在第 ${nl} 行添加评论`} onClick={() => void addComment(shown.path, nl)}>
                                <Icon name="plus" size={11} />
                              </button>
                            )}
                          </td>
                        </tr>
                        {nl != null &&
                          comments
                            .filter((c) => c.file_path === shown.path && c.line === nl)
                            .map((c, n) => (
                              <tr key={n} className="cmt">
                                <td colSpan={2} />
                                <td colSpan={2}>
                                  <div className="cbox">
                                    <Icon name="msg" size={13} />
                                    <div className="grow">
                                      <b>你</b> · 第 {c.line} 行
                                      <p>{c.body}</p>
                                    </div>
                                    <button className="btn ghost sm" onClick={() => removeComment(c)}>
                                      删除
                                    </button>
                                  </div>
                                </td>
                              </tr>
                            ))}
                      </Fragment>
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
            AI 交叉审查
            {cr && cr.status !== 'running' && <small>{AGENTS[cr.agent]?.name ?? cr.agent}{cr.model ? ` · ${cr.model}` : ''}</small>}
          </div>
          <div className="xrev">
            {!cr ? (
              <p className="dim">让另一个 Agent 只读地看一遍这份 Diff，列出它发现的问题。只供参考，不影响验收。</p>
            ) : cr.status === 'running' ? (
              <p className="dim">
                <Icon name="spark" size={12} /> {AGENTS[cr.agent]?.name ?? cr.agent} 正在审查…{' '}
                <a className="lnk" href={`#/runs/${cr.run_id}`}>
                  看实时日志
                </a>
              </p>
            ) : cr.status === 'failed' ? (
              <p style={{ color: 'var(--cinnabar)' }}>
                审查没有完成：{cr.error}{' '}
                <a className="lnk" href={`#/runs/${cr.run_id}`}>
                  看日志
                </a>
              </p>
            ) : (
              <>
                {crStale && <p className="tag amber">审查之后又有新的改动，结果可能已经过期</p>}
                {cr.summary && <p className="sum">{cr.summary}</p>}
                {cr.findings.length === 0 ? (
                  <p className="tag moss">
                    <Icon name="check" size={11} />
                    没有发现问题
                  </p>
                ) : (
                  <ul className="xrev-list">
                    {cr.findings.map((f, i) => (
                      <li key={i} className={`s-${f.severity}`}>
                        <div>
                          <span className={`tag ${f.severity === 'blocker' ? 'cin' : f.severity === 'major' ? 'amber' : 'line'}`}>
                            {{ blocker: '阻塞', major: '重要', minor: '次要' }[f.severity]}
                          </span>{' '}
                          <b>{f.title}</b>
                        </div>
                        {f.file && (
                          <a className="lnk mono" title="在 Diff 里打开这个文件" onClick={() => setFile(f.file)}>
                            {f.file}
                            {f.line ? `:${f.line}` : ''}
                          </a>
                        )}
                        {f.detail && f.detail !== f.title && <p>{f.detail}</p>}
                        {f.suggestion && <p className="dim">建议：{f.suggestion}</p>}
                      </li>
                    ))}
                  </ul>
                )}
              </>
            )}
            {cr?.status !== 'running' && ['review', 'accepted'].includes(task.status) && (
              <div style={{ display: 'flex', gap: 8, marginTop: 8, flexWrap: 'wrap' }}>
                <button className="btn sm" onClick={() => void crossReview()}>
                  <Icon name="spark" size={13} />
                  {cr ? '重新审查' : '让另一个 Agent 审查'}
                </button>
                {cr?.status === 'done' && cr.findings.length > 0 && (
                  <button className="btn sm" onClick={() => void reject(findingsNote(cr.findings))}>
                    <Icon name="undo" size={13} />
                    带着这些问题打回
                  </button>
                )}
              </div>
            )}
          </div>
          {['review', 'accepted'].includes(task.status) && (
            <>
              <div className="sec-title" style={{ marginTop: 18 }}>
                远程
              </div>
              <div className="xrev">
                <p className="dim">
                  {task.pr_url ? (
                    <>
                      PR：<span className="mono">{task.pr_url}</span>
                    </>
                  ) : task.pushed_commit ? (
                    <>
                      分支已推送（<span className="mono">{task.pushed_commit.slice(0, 8)}</span>）
                      {last?.end_commit && last.end_commit !== task.pushed_commit ? '，之后又有新提交' : ''}
                    </>
                  ) : (
                    '分支只在本机。观澜不会自己推送。'
                  )}
                </p>
                <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  <button className="btn sm" onClick={() => void push()}>
                    <Icon name="branch" size={13} />
                    推送分支
                  </button>
                  <button className="btn sm" onClick={() => void openPr()}>
                    <Icon name="merge" size={13} />
                    {task.pr_url ? '重新开 PR' : '开 PR'}
                  </button>
                </div>
              </div>
            </>
          )}
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
