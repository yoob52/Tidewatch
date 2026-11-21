// 工单详情：概览 / PRD / 子任务 / 验收 / 运行记录
import { useState } from 'react';
import { api, type TicketDetail, type TicketStatus, type TimelineEvent } from '../lib/api';
import { useData } from '../lib/data';
import { FLOW } from '../lib/flow';
import { Icon } from '../lib/Icon';
import type { IconName } from '../lib/icons';
import {
  AgentTag,
  Empty,
  KindStamp,
  Md,
  Prio,
  RunStatusTag,
  STAGES,
  STAGE_NAME,
  Stamp,
  TASK_KIND,
  TYPES,
  TaskStatusTag,
  fmtAgo,
  fmtCost,
  fmtDur,
  fmtTok,
  go,
  runSeconds,
  stageTone,
  useLoad,
  useUi,
} from '../lib/ui';
import { AcceptTab, TasksTab } from './TicketTasks';

export interface TabProps {
  d: TicketDetail;
  reload: () => Promise<void>;
}

function Rail({ status, skipped }: { status: TicketStatus; skipped: TicketStatus[] }) {
  const idx = STAGES.findIndex((s) => s.id === status);
  return (
    <div className="rail">
      {STAGES.map((s, i) => (
        <div
          key={s.id}
          className={`rail-s ${skipped.includes(s.id) ? '' : i < idx ? 'past' : i === idx ? 'now' : ''}`}
          title={skipped.includes(s.id) ? 'Bug 快速通道跳过了这一步' : undefined}
        >
          <i>{i < idx && !skipped.includes(s.id) ? <Icon name="check" size={10} /> : s.num}</i>
          <span>{s.name}</span>
        </div>
      ))}
    </div>
  );
}

export function TicketPage({ ticketKey, tab }: { ticketKey: string; tab?: string }) {
  const ui = useUi();
  const data = useData();
  const { data: d, error, reload } = useLoad(() => api.ticketGet(ticketKey), [ticketKey]);

  if (error)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="alert" title="打不开这张工单" body={error}>
            <a className="btn" href="#/tickets">
              回到工单列表
            </a>
          </Empty>
        </div>
      </div>
    );
  if (!d) return <div className="page dim">读取中…</div>;

  const t = d.ticket;
  const live = d.tasks.filter((k) => k.status !== 'cancelled');
  const toAccept = live.filter((k) => k.status === 'review').length;
  const current = tab || (t.status === 'acceptance' ? 'accept' : 'overview');
  const tabs: [string, string, string | number, boolean?][] = [
    ['overview', '概览', ''],
    ['prd', 'PRD', d.versions.length ? `v${d.versions[d.versions.length - 1].version}` : ''],
    ['tasks', '子任务', live.length || ''],
    ['accept', '验收', toAccept || '', true],
    ['runs', '运行记录', d.runs.length || ''],
  ];

  const move = async (to: TicketStatus, label: string) => {
    if (to === 'finalized' && !(await ui.confirm('定稿', `定稿后 ${t.key} 的 PRD 将锁定，再修改需要「发起变更」。`, '定稿'))) return;
    if (to === 'cancelled' && !(await ui.confirm('取消工单', `${t.key} 会从看板移走，之后可以恢复到取消前的状态。`, '取消工单', true))) return;
    if (await ui.attempt(api.ticketTransition(t.id, to), `${t.key} → ${STAGE_NAME[to]}（${label}）`)) {
      await Promise.all([reload(), data.reload()]);
    }
  };

  // 主按钮：每个阶段最常用的那一步；其余流转放在旁边
  const primary: Partial<Record<TicketStatus, TicketStatus>> = {
    draft: t.type === 'bug' ? 'developing' : 'ai_review',
    ai_review: 'human_review',
    human_review: 'finalized',
    finalized: 'developing',
    acceptance: 'done',
  };
  const flow = Object.entries(FLOW[t.status] ?? {}).filter(
    ([to]) => !(t.status === 'draft' && to === 'developing' && t.type !== 'bug') && !(t.status === 'developing' && to === 'acceptance'),
  ) as [TicketStatus, string][];

  return (
    <div className="page flush">
      <div className="thead">
        <div className="thead-1">
          <Stamp type={t.type} lg />
          <div className="grow">
            <div className="row">
              <span className="key">{t.key}</span>
              <span className={`tag ${stageTone(t.status)}`}>{STAGE_NAME[t.status]}</span>
              <Prio p={t.priority} />
              {t.due_date && <span className="dim">截止 {t.due_date}</span>}
            </div>
            <h2>{t.title}</h2>
          </div>
          <div className="row">
            {t.status === 'cancelled' && t.cancelled_from && (
              <button className="btn" onClick={() => void move(t.cancelled_from!, '恢复')}>
                <Icon name="undo" size={14} />
                恢复到「{STAGE_NAME[t.cancelled_from]}」
              </button>
            )}
            {flow
              .filter(([to]) => to !== primary[t.status])
              .map(([to, label]) => (
                <button key={to} className="btn" onClick={() => void move(to, label)}>
                  {label}
                </button>
              ))}
            {flow
              .filter(([to]) => to === primary[t.status])
              .map(([to, label]) => (
                <button key={to} className="btn primary" onClick={() => void move(to, label)}>
                  {label}
                </button>
              ))}
            {['draft', 'human_review', 'developing'].includes(t.status) && (
              <button className="btn ghost" onClick={() => void move('cancelled', '取消')} title="取消工单">
                <Icon name="x" size={14} />
                取消工单
              </button>
            )}
          </div>
        </div>
        <Rail
          status={t.status === 'cancelled' ? (t.cancelled_from ?? 'draft') : t.status}
          skipped={d.timeline.some((e) => e.type === 'status' && e.payload.from === 'draft' && e.payload.to === 'developing') ? ['ai_review', 'human_review', 'finalized'] : []}
        />
      </div>
      <nav className="tabs">
        {tabs.map(([id, name, badge, hot]) => (
          <a key={id} href={`#/ticket/${t.key}/${id}`} className={current === id ? 'on' : ''}>
            {name}
            {badge !== '' && <em className={hot ? 'hot' : ''}>{badge}</em>}
          </a>
        ))}
      </nav>
      <div className="tbody" id="tbody">
        {current === 'prd' ? (
          <PrdTab d={d} reload={reload} />
        ) : current === 'tasks' ? (
          <TasksTab d={d} reload={reload} />
        ) : current === 'accept' ? (
          <AcceptTab d={d} reload={reload} />
        ) : current === 'runs' ? (
          <RunsTab d={d} />
        ) : (
          <OverviewTab d={d} />
        )}
      </div>
    </div>
  );
}

function describe(e: TimelineEvent, d: TicketDetail): [IconName, string, string] {
  const p = e.payload as Record<string, string | number | string[] | null | undefined>;
  const task = d.tasks.find((k) => k.id === p.task_id);
  const name = task ? `${TASK_KIND[task.kind]?.name ?? task.kind}子任务「${task.title}」` : '子任务';
  switch (e.type) {
    case 'created':
      return ['plus', '你', `创建了工单 ${p.key ?? ''}`];
    case 'prd_version':
      return ['file', '你', `保存 PRD v${p.version}${p.note ? `：${p.note}` : ''}`];
    case 'status':
      return ['arrow', '你', `工单从「${STAGE_NAME[p.from as TicketStatus] ?? p.from}」进入「${STAGE_NAME[p.to as TicketStatus] ?? p.to}」`];
    case 'task_created':
      return ['branch', '你', `创建${name}`];
    case 'run_queued':
      return ['runs', '系统', `${name}第 ${p.number} 次运行已入队`];
    case 'run_finished': {
      const failed = (p.failed_checks as string[] | undefined) ?? [];
      const st = { succeeded: '运行完成', failed: '运行失败', stopped: '已停止', orphaned: '被中断' }[p.status as string] ?? String(p.status);
      return [p.status === 'succeeded' ? 'check' : 'alert', 'Agent', `${name}${st}${failed.length ? `，${failed.join('、')} 没通过` : ''}`];
    }
    case 'task_accepted':
      return ['check', '你', `验收通过${name}`];
    case 'task_rejected':
      return ['undo', '你', `打回${name}：${p.note ?? ''}`];
    case 'task_merged':
      return ['merge', '系统', `${name}已合并（${String(p.commit ?? '').slice(0, 8)}）${p.note ? `；${p.note}` : ''}`];
    case 'task_cancelled':
      return ['x', '你', `取消${name}`];
    default:
      return ['info', '系统', e.type];
  }
}

function OverviewTab({ d }: { d: TicketDetail }) {
  const { projects, providers, settings } = useData();
  const t = d.ticket;
  const live = d.tasks.filter((k) => k.status !== 'cancelled');
  const wait = live.filter((k) => k.status === 'review');
  const cost = d.runs.reduce((a, r) => a + (r.cost_usd ?? 0), 0);
  const secs = d.runs.reduce((a, r) => a + runSeconds(r), 0);
  const first = projects.find((p) => p.id === t.projects[0]?.project_id);
  const agent = t.default_agent ?? first?.default_agent ?? settings.default_agent;
  const providerId = t.default_provider_id ?? first?.default_provider_id ?? settings.default_provider_id;
  return (
    <div className="page ov">
      <div className="ov-main">
        {t.status === 'ai_review' && (
          <div className="callout tide">
            <Icon name="info" size={15} />
            <div>
              <b>这一版还没有 AI 预审。</b> 自动预审在下一阶段（M2）实现；现在请自己读一遍 PRD，然后点右上角「预审完成」。
            </div>
          </div>
        )}
        {wait.length > 0 && (
          <div className="callout cin">
            <Icon name="check" size={15} />
            <div>
              <b>{wait.map((k) => TASK_KIND[k.kind]?.name ?? k.kind).join('、')}子任务已跑完，等你验收。</b>{' '}
              <a className="lnk" href={`#/ticket/${t.key}/accept`}>
                去验收 →
              </a>
            </div>
          </div>
        )}
        <div className="sec-title" style={{ marginTop: 18 }}>
          子任务
        </div>
        {live.length ? (
          <div className="tgrid">
            {live.map((k) => (
              <a key={k.id} className="tk" href={`#/ticket/${t.key}/tasks`}>
                <div className="row">
                  <KindStamp kind={k.kind} />
                  <b className="grow ell">{k.title}</b>
                  <TaskStatusTag status={k.status} />
                </div>
                <div className="row dim" style={{ marginTop: 8, fontSize: 12 }}>
                  <span>{projects.find((p) => p.id === k.project_id)?.name ?? '已删除的项目'}</span>
                  {k.branch && <span className="mono">{k.branch}</span>}
                </div>
              </a>
            ))}
          </div>
        ) : (
          <div className="card">
            <Empty icon="branch" title="还没有子任务" body="工单进入「开发中」后，在「子任务」页创建，每个子任务有自己的分支和 worktree。">
              <a className="btn" href={`#/ticket/${t.key}/tasks`}>
                打开子任务
              </a>
            </Empty>
          </div>
        )}
        <div className="sec-title" style={{ marginTop: 22 }}>
          时间线
        </div>
        <ol className="tl">
          {[...d.timeline].reverse().map((e) => {
            const [icon, who, what] = describe(e, d);
            return (
              <li key={e.id}>
                <i>
                  <Icon name={icon} size={12} />
                </i>
                <div>
                  <b>{who}</b> {what}
                </div>
                <time>{fmtAgo(e.ts)}</time>
              </li>
            );
          })}
        </ol>
      </div>
      <aside className="ov-side">
        <dl className="props">
          <dt>类型</dt>
          <dd>
            <Stamp type={t.type} /> {TYPES[t.type]?.name}
          </dd>
          <dt>绑定项目</dt>
          <dd>
            {t.projects.length ? (
              t.projects.map((p) => (
                <a key={p.project_id} className="tag line" href={`#/projects/${p.project_id}`} style={{ marginRight: 4 }}>
                  <Icon name="folder" size={11} />
                  {projects.find((x) => x.id === p.project_id)?.name ?? '已删除'}
                </a>
              ))
            ) : (
              <span className="dim">未绑定</span>
            )}
          </dd>
          <dt>当前 PRD</dt>
          <dd>
            v{d.versions[d.versions.length - 1]?.version ?? 1}
            {t.finalized_version_id ? ' · 已定稿' : ''}
          </dd>
          <dt>默认 Agent</dt>
          <dd>
            <AgentTag agent={agent} />
          </dd>
          <dt>Provider</dt>
          <dd>{providers.find((p) => p.id === providerId)?.name ?? '本机登录态'}</dd>
          <dt>标签</dt>
          <dd>
            {t.labels.length ? (
              t.labels.map((l) => (
                <span key={l} className="tag" style={{ marginRight: 4 }}>
                  {l}
                </span>
              ))
            ) : (
              <span className="dim">无</span>
            )}
          </dd>
          <dt>累计费用</dt>
          <dd className="mono">{d.runs.length ? `${fmtCost(cost)} · ${fmtDur(secs)}` : '—'}</dd>
          <dt>创建</dt>
          <dd>{fmtAgo(t.created_at)}</dd>
        </dl>
      </aside>
    </div>
  );
}

function PrdTab({ d, reload }: TabProps) {
  const ui = useUi();
  const t = d.ticket;
  const versions = [...d.versions].reverse();
  const [shown, setShown] = useState<number | null>(null);
  const [diffing, setDiffing] = useState(false);
  const [draft, setDraft] = useState<string | null>(null);
  const idx = Math.max(0, versions.findIndex((v) => v.version === shown));
  const cur = versions[idx];
  const prev = versions[idx + 1];
  const editable =
    ['draft', 'human_review'].includes(t.status) || (t.type === 'bug' && ['developing', 'acceptance'].includes(t.status));

  if (!cur)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="file" title="这张工单还没有正文" />
        </div>
      </div>
    );

  const save = async () => {
    if (draft === null) return;
    const note = await ui.prompt('保存为新版本', `会生成 v${versions[0].version + 1}，旧版本保留不变。`, '这次改了什么（可选）', '保存');
    if (note === null) return;
    if (await ui.attempt(api.ticketSavePrd(t.id, draft, note || undefined), '已保存为新版本')) {
      setDraft(null);
      setShown(null);
      await reload();
    }
  };

  return (
    <div className="page prd2">
      <article className="doc card">
        <div className="doc-bar">
          <span className="tag tide">
            v{cur.version}
            {idx ? '' : ' · 当前'}
          </span>
          <span className="dim">
            {fmtAgo(cur.created_at)} · {cur.author === 'human' ? '你' : cur.author}
            {cur.note ? ` · ${cur.note}` : ''}
          </span>
          {!editable && idx === 0 && (
            <span className="tag">
              <Icon name="lock" size={11} />
              已锁定，只读
            </span>
          )}
          <span className="grow" />
          {draft !== null ? (
            <>
              <button className="btn sm" onClick={() => setDraft(null)}>
                放弃修改
              </button>
              <button className="btn primary sm" disabled={draft === cur.content_md} onClick={() => void save()}>
                保存为 v{versions[0].version + 1}
              </button>
            </>
          ) : (
            <>
              {idx === 0 && editable && (
                <button className="btn sm" onClick={() => setDraft(cur.content_md)}>
                  <Icon name="edit" size={13} />
                  编辑
                </button>
              )}
              {prev && (
                <button className={`btn sm ${diffing ? 'on' : ''}`} onClick={() => setDiffing(!diffing)}>
                  <Icon name="code" size={13} />
                  {diffing ? '关闭对比' : `与 v${prev.version} 对比`}
                </button>
              )}
            </>
          )}
        </div>
        {draft !== null ? (
          <textarea
            className="textarea mono"
            style={{ minHeight: 420, border: 0, borderRadius: 0 }}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            aria-label="PRD 正文"
          />
        ) : (
          <div className={`prose ${diffing ? 'diffing' : ''}`}>
            {cur.content_md.trim() ? <Md src={cur.content_md} prev={diffing && prev ? prev.content_md : null} /> : <p className="dim">正文是空的。</p>}
          </div>
        )}
      </article>
      <aside>
        <div className="sec-title">
          版本<small>点击查看</small>
        </div>
        <ol className="vers">
          {versions.map((v) => (
            <li
              key={v.id}
              className={v.version === cur.version ? 'on' : ''}
              style={{ cursor: 'pointer' }}
              onClick={() => {
                setDraft(null);
                setShown(v.version);
              }}
            >
              <b>v{v.version}</b>
              <div>
                <span>{v.origin === 'edit' ? '编辑' : v.origin === 'file' ? '文件导入' : '创建'}</span>
                <small>{fmtAgo(v.created_at)}</small>
                {v.note && <small>{v.note}</small>}
              </div>
            </li>
          ))}
        </ol>
        <div className="callout tide" style={{ marginTop: 14 }}>
          <Icon name="info" size={15} />
          <div>每次保存都会生成一个不可变的新版本。定稿后 PRD 只读，改动需要「发起变更」。</div>
        </div>
      </aside>
    </div>
  );
}

function RunsTab({ d }: { d: TicketDetail }) {
  const { providers } = useData();
  if (!d.runs.length)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="runs" title="还没有运行记录" body="开始一个子任务后，每次 Agent 运行都会记录在这里。" />
        </div>
      </div>
    );
  const tot = d.runs.reduce(
    (a, r) => ({ c: a.c + (r.cost_usd ?? 0), t: a.t + r.input_tokens + r.cache_tokens + r.output_tokens, s: a.s + runSeconds(r) }),
    { c: 0, t: 0, s: 0 },
  );
  return (
    <div className="page">
      <div className="kpis">
        <div>
          <span>运行次数</span>
          <b>{d.runs.length}</b>
        </div>
        <div>
          <span>总耗时</span>
          <b>{fmtDur(tot.s)}</b>
        </div>
        <div>
          <span>Token</span>
          <b>{fmtTok(tot.t)}</b>
        </div>
        <div>
          <span>费用</span>
          <b>{fmtCost(tot.c)}</b>
        </div>
      </div>
      <div className="card" style={{ overflow: 'hidden', marginTop: 14 }}>
        <table className="tbl">
          <thead>
            <tr>
              <th>运行</th>
              <th>子任务</th>
              <th>Agent</th>
              <th>Provider</th>
              <th>状态</th>
              <th className="num">耗时</th>
              <th className="num">Token</th>
              <th className="num">费用</th>
            </tr>
          </thead>
          <tbody>
            {[...d.runs].reverse().map((r) => {
              const task = d.tasks.find((k) => k.id === r.task_id);
              return (
                <tr key={r.id} onClick={() => go(`#/runs/${r.id}`)} style={{ cursor: 'pointer' }}>
                  <td className="mono">
                    <a className="lnk" href={`#/runs/${r.id}`} onClick={(e) => e.stopPropagation()}>
                      第 {r.number} 次
                    </a>
                  </td>
                  <td>{task ? `${TASK_KIND[task.kind]?.short ?? task.kind} · ${task.title}` : '—'}</td>
                  <td>
                    <AgentTag agent={r.agent} />
                  </td>
                  <td>{providers.find((p) => p.id === r.provider_id)?.name ?? '本机登录态'}</td>
                  <td>
                    <RunStatusTag status={r.status} />
                  </td>
                  <td className="num">{fmtDur(runSeconds(r))}</td>
                  <td className="num">{fmtTok(r.input_tokens + r.cache_tokens + r.output_tokens)}</td>
                  <td className="num">{fmtCost(r.cost_usd)}</td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
    </div>
  );
}
