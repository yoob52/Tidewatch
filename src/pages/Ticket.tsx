// 工单详情：概览 / PRD / 子任务 / 验收 / 运行记录
import { useMemo, useRef, useState } from 'react';
import { api, type TicketDetail, type TicketStatus, type TimelineEvent } from '../lib/api';
import { useData } from '../lib/data';
import { FLOW } from '../lib/flow';
import { Icon } from '../lib/Icon';
import {
  imageFiles,
  imageMd,
  insertAt,
  selectionOf,
  removeImage,
  replaceImage,
  setAuthority,
  uploadImage,
  type ImageActions,
} from '../lib/images';
import type { IconName } from '../lib/icons';
import {
  AGENTS,
  AgentTag,
  Empty,
  KindStamp,
  Md,
  PURPOSE,
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
  runActive,
  runSeconds,
  stageTone,
  useLoad,
  useUi,
} from '../lib/ui';
import { FinalizeDialog, ReviewTab } from './TicketReview';
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
  const [finalizing, setFinalizing] = useState(false);

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
  const openIssues = d.issues.filter((i) => i.resolution === 'open' || i.resolution === 'pending_confirm');
  const blockers = openIssues.filter((i) => i.severity === 'blocker').length;
  const reviewing = t.status === 'human_review' || t.status === 'ai_review';
  const current = tab || (reviewing ? 'review' : t.status === 'acceptance' ? 'accept' : 'overview');
  const tabs: [string, string, string | number, boolean?][] = [
    ['overview', '概览', ''],
    ['prd', 'PRD', d.versions.length ? `v${d.versions[d.versions.length - 1].version}` : ''],
    ['review', '审核', t.status === 'human_review' && openIssues.length ? openIssues.length : '', true],
    ['tasks', '子任务', live.length || ''],
    ['accept', '验收', toAccept || '', true],
    ['runs', '运行记录', d.runs.length || ''],
  ];

  const move = async (to: TicketStatus, label: string) => {
    if (to === 'finalized') {
      // 定稿要过校验、选要不要让 AI 拆分
      setFinalizing(true);
      return;
    }
    if (to === 'cancelled' && !(await ui.confirm('取消工单', `${t.key} 会从看板移走，之后可以恢复到取消前的状态。`, '取消工单', true))) return;
    let job: Promise<unknown>;
    if (to === 'human_review' && (t.status === 'finalized' || t.status === 'developing')) {
      // 发起变更：要写原因；有子任务在跑时问要不要停
      const reason = await ui.prompt(
        '发起变更',
        `${t.key} 回到人工审核，PRD 重新可编辑。改完再次定稿时，AI 会分析这次变更对已有子任务的影响。`,
        '为什么要改需求',
        '发起变更',
        true,
      );
      if (!reason) return;
      const busy = d.runs.some((r) => r.task_id && (runActive(r.status) || r.status === 'queued'));
      const stop = busy && (await ui.confirm('有子任务正在运行', '要现在停掉它们吗？不停的话它们会按旧需求跑完，之后再按影响分析返工。', '停掉正在运行的'));
      job = api.changeRequest(t.id, reason, stop);
    } else {
      job = api.ticketTransition(t.id, to);
    }
    if (await ui.attempt(job, `${t.key} → ${STAGE_NAME[to]}（${label}）`)) {
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
      {finalizing && (
        <FinalizeDialog d={d} blockers={blockers} onClose={() => setFinalizing(false)}
          onDone={async () => {
            setFinalizing(false);
            await Promise.all([reload(), data.reload()]);
          }}
        />
      )}
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
        ) : current === 'review' ? (
          <ReviewTab d={d} reload={reload} />
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
      return p.purpose && !p.task_id
        ? ['runs', '系统', `${PURPOSE[p.purpose as string] ?? p.purpose}已入队`]
        : ['runs', '系统', `${name}第 ${p.number} 次运行已入队`];
    case 'run_finished': {
      const failed = (p.failed_checks as string[] | undefined) ?? [];
      const st = { succeeded: '运行完成', failed: '运行失败', stopped: '已停止', orphaned: '被中断' }[p.status as string] ?? String(p.status);
      return [p.status === 'succeeded' ? 'check' : 'alert', 'Agent', `${name}${st}${failed.length ? `，${failed.join('、')} 没通过` : ''}`];
    }
    case 'task_accepted':
      return ['check', '你', `验收通过${name}`];
    case 'task_rejected':
      return p.reason === 'change'
        ? ['undo', '系统', `需求变更，${name}需要追加工作：${p.note ?? ''}`]
        : ['undo', '你', `打回${name}：${p.note ?? ''}${p.comments ? `（${p.comments} 条行内评论）` : ''}`];
    case 'review_finished':
      return p.error
        ? ['alert', 'AI', `预审没有完成：${p.error}`]
        : ['spark', 'AI', `预审完成，发现 ${p.issues} 个问题${p.blockers ? `，其中 ${p.blockers} 个阻塞` : ''}`];
    case 'code_review_finished':
      return p.error
        ? ['alert', 'AI', `${name}的交叉审查没有完成：${p.error}`]
        : ['spark', 'AI', `${name}的交叉审查完成，${p.findings ? `发现 ${p.findings} 个问题${p.blockers ? `，其中 ${p.blockers} 个阻塞` : ''}` : '没有发现问题'}`];
    case 'task_pushed':
      return ['branch', '你', `把${name}的分支 ${p.branch} 推到了 ${p.remote}（${String(p.commit ?? '').slice(0, 8)}）`];
    case 'pr_created':
      return ['merge', '你', `给${name}开了 PR：${p.url}`];
    case 'auto_fix':
      return ['undo', '系统', `${name}的自动检查没过，交回 Agent 自动修复（第 ${p.attempt} 次）`];
    case 'issue_resolved': {
      const how = { accepted: '采纳了', manual: '手动处理了', ignored: '忽略了', pending_confirm: '把', open: '重新打开了' }[p.resolution as string] ?? '处理了';
      return ['check', '你', `${how}问题 ${p.code}${p.resolution === 'pending_confirm' ? ' 标为待确认' : ''}${p.note ? `：${p.note}` : ''}`];
    }
    case 'finalized':
      return ['lock', '你', '定稿，PRD 已锁定'];
    case 'proposal_ready': {
      const what = p.kind === 'impact' ? '变更影响分析' : '拆分建议';
      return p.error ? ['alert', 'AI', `${what}没有生成出来：${p.error}`] : ['spark', 'AI', `${what}出来了，等你确认`];
    }
    case 'proposal_discarded':
      return ['x', '你', `放弃了${p.kind === 'impact' ? '变更影响分析' : '拆分建议'}`];
    case 'split_applied':
      return ['branch', '你', `确认拆分，创建了 ${(p.tasks as string[] | undefined)?.length ?? 0} 个子任务`];
    case 'impact_applied':
      return ['check', '你', `确认影响分析：${(p.rework as string[] | undefined)?.length ?? 0} 个子任务要追加工作，新增 ${p.new_tasks ?? 0} 个`];
    case 'change_requested':
      return ['undo', '你', `发起变更：${p.reason ?? ''}`];
    case 'api_doc_ready':
      return ['file', 'Agent', `${name}写出了接口文档 ${p.path}${(p.unblocked as string[] | undefined)?.length ? '，等它的子任务已解锁' : ''}`];
    case 'task_merged':
      return ['merge', '系统', `${name}已合并（${String(p.commit ?? '').slice(0, 8)}）${p.note ? `；${p.note}` : ''}`];
    case 'compare_started':
      return ['branch', '你', `发起多 Agent 对比：${name}另外交给 ${(p.candidates as string[] | undefined)?.length ?? 0} 个 Agent 各做一遍`];
    case 'compare_start_failed':
      return ['alert', '系统', `对比候选「${p.title ?? ''}」没有入队：${p.error ?? ''}`];
    case 'compare_adopted':
      return ['check', '你', `采用了${name}的结果，其余 ${(p.dropped as string[] | undefined)?.length ?? 0} 个候选已取消`];
    case 'task_cancelled':
      return ['x', '你', `取消${name}`];
    case 'question_asked':
      return ['info', 'Agent', `提了一个问题：${p.body ?? ''}`];
    case 'question_answered':
      return ['check', '你', `回答了「${p.body ?? ''}」：${p.answer ?? ''}`];
    case 'delegate_finished':
      return [p.error ? 'alert' : 'spark', 'Agent', `委派给 ${AGENTS[p.agent as string]?.name ?? p.agent} 的只读检查${p.error ? `没有完成：${p.error}` : '完成'}`];
    case 'comment':
      return ['edit', '你', `评论：${p.body ?? ''}`];
    case 'stage_passed':
      return ['check', '你', `通过了环节「${p.stage ?? ''}」${p.note ? `：${p.note}` : ''}`];
    case 'source_linked':
      return ['upload', '系统', `从${p.kind === 'github' ? ' GitHub Issue ' : '飞书文档'}导入（${p.source ?? ''}）`];
    case 'source_comment':
      return ['edit', `${p.author ?? '有人'}（GitHub）`, `评论：${p.body ?? ''}`];
    case 'source_changed':
      return ['info', '系统', '外部来源的内容变了，等你决定要不要导入'];
    case 'source_applied':
      return ['check', '你', p.version ? `把外部来源的新内容导入为 PRD v${p.version}` : '同步了外部来源的新标题'];
    case 'source_dismissed':
      return ['x', '你', '忽略了外部来源的这次变更'];
    case 'source_written_back':
      return ['check', '系统', `已在 GitHub Issue #${p.number ?? ''} 下评论并关闭`];
    case 'assigned':
      return ['info', '你', p.assignee ? `指派给 ${p.assignee}` : '取消了指派'];
    default:
      return ['info', '系统', e.type];
  }
}

/** 自定义工作流：现在轮到的环节，由人点通过。 */
function StageGate({ d }: { d: TicketDetail }) {
  const ui = useUi();
  const [note, setNote] = useState('');
  const cur = d.stages.find((s) => s.current);
  if (!cur) return null;
  const same = d.stages.filter((s) => s.gate === cur.gate);
  const pass = async () => {
    const done = await ui.attempt(api.ticketStagePass(d.ticket.id, cur.id, note), `「${cur.name}」已通过`);
    if (done) setNote('');
  };
  return (
    <div className="callout tide">
      <Icon name="info" size={15} />
      <div style={{ flex: 1 }}>
        <b>
          等待环节：{cur.name}（{same.indexOf(cur) + 1}/{same.length}）
        </b>{' '}
        通过之后才能{cur.gate === 'finalized' ? '定稿' : '完成这张工单'}。
        {cur.hint && <div style={{ whiteSpace: 'pre-wrap', marginTop: 4 }}>{cur.hint}</div>}
        <div className="row" style={{ gap: 6, marginTop: 8 }}>
          <input className="input" style={{ flex: 1, minWidth: 180 }} value={note} onChange={(e) => setNote(e.target.value)} placeholder="备注（可选）" aria-label="环节备注" />
          <button className="btn sm primary" onClick={() => void pass()}>
            通过
          </button>
        </div>
      </div>
    </div>
  );
}

/** 源已变更：外部的 Issue / 文档有一版还没导入。 */
function SourceChanged({ d }: { d: TicketDetail }) {
  const ui = useUi();
  const [open, setOpen] = useState(false);
  const src = d.source;
  if (!src?.pending_rev) return null;
  const t = d.ticket;
  const what = src.kind === 'github' ? 'GitHub Issue' : '飞书文档';
  const locked = !(['draft', 'human_review'].includes(t.status) || (t.type === 'bug' && ['developing', 'acceptance'].includes(t.status)));
  const apply = async () => {
    if (
      locked &&
      !(await ui.confirm(
        '导入会先发起变更',
        '这张工单的需求已经定稿。导入外部的新内容会让它回到人工审核，正在跑的子任务会跑完当前这一轮；之后要重新定稿。',
        '发起变更并导入',
      ))
    )
      return;
    await ui.attempt(api.syncApply(t.id), '已导入为新的 PRD 版本');
  };
  const titleChanged = src.pending_title && src.pending_title !== t.title;
  return (
    <div className="callout cin">
      <Icon name="info" size={15} />
      <div style={{ flex: 1 }}>
        <b>源已变更：</b>
        {what}在导入之后有改动{titleChanged ? `（标题改成了「${src.pending_title}」）` : ''}。现在的 PRD 没有动。
        <div className="row" style={{ gap: 6, marginTop: 8 }}>
          <button className="btn sm primary" onClick={() => void apply()}>
            导入为新版本
          </button>
          <button className="btn sm" onClick={() => setOpen((x) => !x)}>
            {open ? '收起' : '看新内容'}
          </button>
          <button className="btn sm ghost" onClick={() => void ui.attempt(api.syncDismiss(t.id), '已忽略这次变更')}>
            忽略
          </button>
        </div>
        {open && (
          <pre className="plain boxed" style={{ whiteSpace: 'pre-wrap', marginTop: 8, maxHeight: 320, overflow: 'auto' }}>
            {src.pending_md}
          </pre>
        )}
      </div>
    </div>
  );
}

/** Agent 经 MCP 提的、还没回答的问题。回答会写进对应子任务下一轮的 Prompt。 */
function Questions({ d }: { d: TicketDetail }) {
  const ui = useUi();
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const open = d.questions.filter((q) => q.status === 'open');
  if (!open.length) return null;
  // 回答 / 忽略之后，引擎的变更事件会让工单重新加载
  const answer = (id: string, text: string) => void ui.attempt(api.questionAnswer(id, text), '已回答，下一轮运行会带上');
  return (
    <>
      {open.map((q) => {
        const task = d.tasks.find((k) => k.id === q.task_id);
        const draft = drafts[q.id] ?? '';
        return (
          <div className="callout cin" key={q.id}>
            <Icon name="info" size={15} />
            <div style={{ flex: 1 }}>
              <b>Agent 有个问题要你回答{task ? `（${task.title}）` : ''}：</b>
              <div style={{ whiteSpace: 'pre-wrap', margin: '4px 0 8px' }}>{q.body}</div>
              <div className="row" style={{ flexWrap: 'wrap', gap: 6 }}>
                {q.options.map((o) => (
                  <button key={o} className="btn sm" onClick={() => answer(q.id, o)}>
                    {o}
                  </button>
                ))}
                <input
                  className="input"
                  style={{ flex: 1, minWidth: 180 }}
                  value={draft}
                  onChange={(e) => setDrafts((x) => ({ ...x, [q.id]: e.target.value }))}
                  placeholder="写下回答…"
                  aria-label="回答"
                />
                <button className="btn sm primary" disabled={!draft.trim()} onClick={() => answer(q.id, draft)}>
                  回答
                </button>
                <button className="btn sm ghost" onClick={() => void ui.attempt(api.questionDismiss(q.id))}>
                  忽略
                </button>
              </div>
            </div>
          </div>
        );
      })}
    </>
  );
}

function OverviewTab({ d }: { d: TicketDetail }) {
  const ui = useUi();
  const { projects, providers, settings } = useData();
  const t = d.ticket;
  const { data: members } = useLoad(() => api.memberList(), []);
  // 停用的成员不能再指派，但已经是负责人的还要显示出来
  const assignable = (members ?? []).filter((m) => !m.disabled || m.id === t.assignee_id);
  const live = d.tasks.filter((k) => k.status !== 'cancelled');
  const wait = live.filter((k) => k.status === 'review');
  const openIssues = d.issues.filter((i) => i.resolution === 'open' || i.resolution === 'pending_confirm');
  const blockers = openIssues.filter((i) => i.severity === 'blocker').length;
  const proposal = [d.impact, d.split].find((x) => x && ['running', 'pending', 'failed'].includes(x.status));
  const cost = d.runs.reduce((a, r) => a + (r.cost_usd ?? 0), 0);
  const secs = d.runs.reduce((a, r) => a + runSeconds(r), 0);
  const first = projects.find((p) => p.id === t.projects[0]?.project_id);
  const agent = t.default_agent ?? first?.default_agent ?? settings.default_agent;
  const providerId = t.default_provider_id ?? first?.default_provider_id ?? settings.default_provider_id;
  return (
    <div className="page ov">
      <div className="ov-main">
        <Questions d={d} />
        <SourceChanged d={d} />
        <StageGate d={d} />
        {t.status === 'ai_review' && (
          <div className="callout tide">
            <Icon name="spark" size={15} />
            <div>
              <b>AI 正在预审这份 PRD。</b> 结束后工单进入人工审核。{' '}
              <a className="lnk" href={`#/ticket/${t.key}/review`}>
                看进度 →
              </a>
            </div>
          </div>
        )}
        {t.status === 'human_review' && (
          <div className="callout cin">
            <Icon name="info" size={15} />
            <div>
              <b>
                {t.change_from_version_id
                  ? `变更进行中：${t.change_reason}`
                  : openIssues.length
                    ? `预审提出的问题还有 ${openIssues.length} 个没处理${blockers ? `，其中 ${blockers} 个阻塞定稿` : ''}。`
                    : '等你审核并定稿。'}
              </b>{' '}
              <a className="lnk" href={`#/ticket/${t.key}/review`}>
                去审核 →
              </a>
            </div>
          </div>
        )}
        {t.status === 'finalized' && proposal && (
          <div className="callout tide">
            <Icon name="spark" size={15} />
            <div>
              <b>
                {proposal.kind === 'impact' ? '变更影响分析' : '拆分建议'}
                {proposal.status === 'running' ? '正在生成。' : proposal.status === 'failed' ? '没有生成出来。' : '等你确认。'}
              </b>{' '}
              <a className="lnk" href={`#/ticket/${t.key}/tasks`}>
                去子任务页 →
              </a>
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
            <Empty icon="branch" title="还没有子任务" body="定稿时可以让 AI 拆分，也可以在「子任务」页自己添加。每个子任务有自己的分支和 worktree。">
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
                  {/* 经接口操作的显示成员名 */}
                  <b>{who === '你' && e.actor ? e.actor : who}</b> {what}
                </div>
                <time>{fmtAgo(e.ts)}</time>
              </li>
            );
          })}
        </ol>
      </div>
      <aside className="ov-side">
        <dl className="props">
          {(d.assignee || assignable.length > 0) && (
            <>
              <dt>负责人</dt>
              <dd>
                <select
                  className="input"
                  aria-label="负责人"
                  value={t.assignee_id ?? ''}
                  onChange={(e) => void ui.attempt(api.ticketAssign(t.id, e.target.value || null))}
                >
                  <option value="">未指派</option>
                  {assignable.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </select>
              </dd>
            </>
          )}
          <dt>类型</dt>
          <dd>
            <Stamp type={t.type} /> {TYPES[t.type]?.name}
          </dd>
          {d.stages.length > 0 && (
            <>
              <dt>环节</dt>
              <dd>
                {d.stages.map((s) => (
                  <div key={s.id} className={s.passed_at ? '' : 'dim'} title={s.note ?? undefined}>
                    {s.passed_at ? '✓' : s.current ? '→' : '·'} {s.name}
                  </div>
                ))}
              </dd>
            </>
          )}
          {d.source && (
            <>
              <dt>来源</dt>
              <dd>
                {d.source.kind === 'github' ? `GitHub Issue #${d.source.external_id}` : '飞书文档'}
                {d.source.remote_state === 'closed' && '（已关闭）'}
                <div className="mono dim" style={{ wordBreak: 'break-all', userSelect: 'text' }}>
                  {d.source.url}
                </div>
              </dd>
            </>
          )}
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

  const area = useRef<HTMLTextAreaElement | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);
  /** 选好文件后要做的事：插入到编辑框，或替换某张图。 */
  const pending = useRef<{ replace?: string }>({});

  // 只动图片的修改直接存成一个新版本（来源记为 image）
  const saveImages = async (next: string, note: string) => {
    if (next === cur.content_md) return;
    if (await ui.attempt(api.ticketSavePrd(t.id, next, note, 'image'), `${note}，已保存为新版本`)) {
      setShown(null);
      await reload();
    }
  };
  const onFiles = async (files: File[]) => {
    const target = pending.current;
    pending.current = {};
    if (!files.length) return;
    if (target.replace) {
      const id = await ui.attempt(uploadImage(t.id, files[0]));
      if (id) await saveImages(replaceImage(cur.content_md, target.replace, id, '需求方截图'), '替换了一张图片');
      return;
    }
    const sel = selectionOf(area.current);
    const added: string[] = [];
    for (const f of files) {
      const id = await ui.attempt(uploadImage(t.id, f));
      if (!id) break;
      added.push(imageMd('需求方截图', id, 'authoritative'));
    }
    setDraft((d) => insertAt(d ?? cur.content_md, sel, added));
  };
  const imageActions = useMemo<ImageActions>(
    () => ({
      setAuthority: (id, authority) =>
        void saveImages(setAuthority(cur.content_md, id, authority), authority === 'authoritative' ? '把一张图设为权威' : '把一张图改为仅参考'),
      replace: (id) => {
        pending.current = { replace: id };
        fileInput.current?.click();
      },
      remove: async (id) => {
        if (await ui.confirm('从 PRD 里移除这张图？', '只是当前版本不再引用它；图片文件和旧版本都保留。', '移除'))
          await saveImages(removeImage(cur.content_md, id), '移除了一张图片');
      },
      describe: async (id) => {
        const a = (await ui.attempt(api.attachmentList(t.id)))?.find((x) => x.id === id);
        if (!a) return;
        const text = await ui.prompt(
          '图片的文字描述',
          '模型看不了图时（Provider 标为"只能处理文字"），发给 Agent 的是这段描述。',
          '这张图画了什么：有哪些区域、字段、按钮和文案',
          '保存',
          false,
          a.description ?? '',
        );
        if (text !== null) await ui.attempt(api.attachmentDescribe(id, text), '已保存描述');
      },
    }),
    [cur?.id, cur?.content_md, t.id],
  );
  const canEditImages = idx === 0 && editable && draft === null;

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
              <button
                className="btn sm"
                onClick={() => {
                  pending.current = {};
                  fileInput.current?.click();
                }}
              >
                <Icon name="upload" size={13} />
                添加图片
              </button>
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
            ref={area}
            className="textarea mono"
            style={{ minHeight: 420, border: 0, borderRadius: 0 }}
            value={draft}
            onChange={(e) => setDraft(e.target.value)}
            onPaste={(e) => {
              const files = imageFiles(e.clipboardData);
              if (!files.length) return;
              e.preventDefault();
              pending.current = {};
              void onFiles(files);
            }}
            aria-label="PRD 正文"
          />
        ) : (
          <div className={`prose ${diffing ? 'diffing' : ''}`}>
            {cur.content_md.trim() ? (
              <Md src={cur.content_md} prev={diffing && prev ? prev.content_md : null} images={canEditImages ? imageActions : undefined} />
            ) : (
              <p className="dim">正文是空的。</p>
            )}
          </div>
        )}
        <input
          ref={fileInput}
          type="file"
          accept="image/png,image/jpeg,image/gif,image/webp"
          multiple
          hidden
          onChange={(e) => {
            const files = Array.from(e.target.files ?? []);
            e.target.value = '';
            void onFiles(files);
          }}
        />
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
                <span>{v.origin === 'edit' ? '编辑' : v.origin === 'ai_suggestion' ? 'AI 采纳' : v.origin === 'file' ? '文件导入' : '创建'}</span>
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
                  <td>{task ? `${TASK_KIND[task.kind]?.short ?? task.kind} · ${task.title}` : r.task_id ? '—' : (PURPOSE[r.purpose] ?? r.purpose)}</td>
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
