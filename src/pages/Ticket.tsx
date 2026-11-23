import { tr } from '../lib/i18n';
// 工单详情：概览 / PRD / 子任务 / 验收 / 运行记录
import { useMemo, useRef, useState } from 'react';
import { api, type TicketDetail, type TicketStatus, type TimelineEvent } from '../lib/api';
import { useData } from '../lib/data';
import { FLOW } from '../lib/flow';
import { useConfirmTicketRun } from '../lib/start';
import { useChooseDevelopment, useChooseTicketRun } from '../lib/runOptions';
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
  Modal,
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
          title={skipped.includes(s.id) ? tr('Bug 快速通道跳过了这一步') : undefined}
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
  const confirmRun = useConfirmTicketRun();
  const chooseRun = useChooseTicketRun();
  const chooseDevelopment = useChooseDevelopment();
  const data = useData();
  const { data: d, error, reload } = useLoad(() => api.ticketGet(ticketKey), [ticketKey]);
  const [finalizing, setFinalizing] = useState(false);
  const [binding, setBinding] = useState(false);

  if (error)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="alert" title={tr('打不开这张工单')} body={error}>
            <a className="btn" href="#/tickets">
              {tr('回到工单列表')}
            </a>
          </Empty>
        </div>
      </div>
    );
  if (!d) return <div className="page dim">{tr('读取中…')}</div>;

  const t = d.ticket;
  const canBind = !t.finalized_version_id && ['draft', 'human_review'].includes(t.status);
  const live = d.tasks.filter((k) => k.status !== 'cancelled');
  const toAccept = live.filter((k) => k.status === 'review').length;
  const openIssues = d.issues.filter((i) => i.resolution === 'open' || i.resolution === 'pending_confirm');
  const blockers = openIssues.filter((i) => i.severity === 'blocker').length;
  const reviewing = t.status === 'human_review' || t.status === 'ai_review';
  const current = tab || (reviewing ? 'review' : t.status === 'acceptance' ? 'accept' : 'overview');
  const tabs: [string, string, string | number, boolean?][] = [
    ['overview', tr('概览'), ''],
    ['prd', 'PRD', d.versions.length ? `v${d.versions[d.versions.length - 1].version}` : ''],
    ['review', tr('审核'), t.status === 'human_review' && openIssues.length ? openIssues.length : '', true],
    ['tasks', tr('子任务'), live.length || ''],
    ['accept', tr('验收'), toAccept || '', true],
    ['runs', tr('运行记录'), d.runs.length || ''],
  ];

  const move = async (to: TicketStatus, label: string) => {
    if (to === 'finalized') {
      // 定稿要过校验、选要不要让 AI 拆分
      setFinalizing(true);
      return;
    }
    if (to === 'cancelled' && !(await ui.confirm(tr('取消工单'), tr('{0} 会从看板移走，之后可以恢复到取消前的状态。', t.key), tr('取消工单'), true))) return;
    let job: Promise<unknown>;
    if (to === 'human_review' && (t.status === 'finalized' || t.status === 'developing')) {
      // 发起变更：要写原因；有子任务在跑时问要不要停
      const reason = await ui.prompt(
        tr('发起变更'),
        tr('{0} 回到人工审核，PRD 重新可编辑。改完再次定稿时，AI 会分析这次变更对已有子任务的影响。', t.key),
        tr('为什么要改需求'),
        tr('发起变更'),
        true,
      );
      if (!reason) return;
      const busy = d.runs.some((r) => r.task_id && (runActive(r.status) || r.status === 'queued'));
      const stop = busy && (await ui.confirm(tr('有子任务正在运行'), tr('要现在停掉它们吗？不停的话它们会按旧需求跑完，之后再按影响分析返工。'), tr('停掉正在运行的')));
      job = api.changeRequest(t.id, reason, stop);
    } else if (to === 'ai_review') {
      if (!t.projects.length && canBind) {
        setBinding(true);
        return;
      }
      const options = await chooseRun(t.id, 'review', tr('提交预审'));
      if (!options || !(await confirmRun())) return;
      job = api.reviewStart(t.id, options);
    } else if (t.status === 'finalized' && to === 'developing') {
      const options = await chooseDevelopment(t.id);
      if (!options) return;
      job = api.developmentStart(t.id, options);
    } else {
      job = api.ticketTransition(t.id, to);
    }
    if (await ui.attempt(job, tr('{0} → {1}（{2}）', t.key, STAGE_NAME[to], label))) {
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
              {t.due_date && <span className="dim">{tr('截止')} {t.due_date}</span>}
            </div>
            <h2>{t.title}</h2>
          </div>
          <div className="row">
            {canBind && !t.projects.length && (
              <button className="btn" onClick={() => setBinding(true)}>{tr('绑定项目')}</button>
            )}
            {t.status === 'cancelled' && t.cancelled_from && (
              <button className="btn" onClick={() => void move(t.cancelled_from!, tr('恢复'))}>
                <Icon name="undo" size={14} />
                {tr('恢复到「')}{STAGE_NAME[t.cancelled_from]}{tr('」')}
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
              <button className="btn ghost" onClick={() => void move('cancelled', tr('取消'))} title={tr('取消工单')}>
                <Icon name="x" size={14} />
                {tr('取消工单')}
              </button>
            )}
          </div>
        </div>
        <Rail
          status={t.status === 'cancelled' ? (t.cancelled_from ?? 'draft') : t.status}
          skipped={d.timeline.some((e) => e.type === 'status' && e.payload.from === 'draft' && e.payload.to === 'developing') ? ['ai_review', 'human_review', 'finalized'] : []}
        />
      </div>
      {binding && (
        <ProjectBindings key={t.id} d={d} reload={reload} onClose={() => setBinding(false)} />
      )}
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
          <ReviewTab d={d} reload={reload} onBind={canBind ? () => setBinding(true) : undefined} />
        ) : current === 'tasks' ? (
          <TasksTab d={d} reload={reload} />
        ) : current === 'accept' ? (
          <AcceptTab d={d} reload={reload} />
        ) : current === 'runs' ? (
          <RunsTab d={d} />
        ) : (
          <OverviewTab d={d} onBind={canBind ? () => setBinding(true) : undefined} />
        )}
      </div>
    </div>
  );
}

function describe(e: TimelineEvent, d: TicketDetail): [IconName, string, string] {
  const p = e.payload as Record<string, string | number | string[] | null | undefined>;
  const task = d.tasks.find((k) => k.id === p.task_id);
  const name = task ? tr('{0}子任务「{1}」', TASK_KIND[task.kind]?.name ?? task.kind, task.title) : tr('子任务');
  switch (e.type) {
    case 'created':
      return ['plus', tr('你'), tr('创建了工单 {0}', p.key ?? '')];
    case 'prd_version':
      return ['file', tr('你'), tr('保存 PRD v{0}{1}', p.version, p.note ? tr('：{0}', p.note) : '')];
    case 'status':
      return ['arrow', tr('你'), tr('工单从「{0}」进入「{1}」', STAGE_NAME[p.from as TicketStatus] ?? p.from, STAGE_NAME[p.to as TicketStatus] ?? p.to)];
    case 'task_created':
      return ['branch', tr('你'), tr('创建{0}', name)];
    case 'run_queued':
      return p.purpose && !p.task_id
        ? ['runs', tr('系统'), tr('{0}已入队', PURPOSE[p.purpose as string] ?? p.purpose)]
        : ['runs', tr('系统'), tr('{0}第 {1} 次运行已入队', name, p.number)];
    case 'run_finished': {
      const failed = (p.failed_checks as string[] | undefined) ?? [];
      const st = { succeeded: tr('运行完成'), failed: tr('运行失败'), stopped: tr('已停止'), orphaned: tr('被中断') }[p.status as string] ?? String(p.status);
      return [p.status === 'succeeded' ? 'check' : 'alert', 'Agent', `${name}${st}${failed.length ? tr('，{0} 没通过', failed.join(tr('、'))) : ''}`];
    }
    case 'task_accepted':
      return ['check', tr('你'), tr('验收通过{0}', name)];
    case 'task_rejected':
      return p.reason === 'change'
        ? ['undo', tr('系统'), tr('需求变更，{0}需要追加工作：{1}', name, p.note ?? '')]
        : ['undo', tr('你'), tr('打回{0}：{1}{2}', name, p.note ?? '', p.comments ? tr('（{0} 条行内评论）', p.comments) : '')];
    case 'review_finished':
      return p.error
        ? ['alert', 'AI', tr('预审没有完成：{0}', p.error)]
        : ['spark', 'AI', tr('预审完成，发现 {0} 个问题{1}', p.issues, p.blockers ? tr('，其中 {0} 个阻塞', p.blockers) : '')];
    case 'code_review_finished':
      return p.error
        ? ['alert', 'AI', tr('{0}的交叉审查没有完成：{1}', name, p.error)]
        : ['spark', 'AI', tr('{0}的交叉审查完成，{1}', name, p.findings ? tr('发现 {0} 个问题{1}', p.findings, p.blockers ? tr('，其中 {0} 个阻塞', p.blockers) : '') : tr('没有发现问题'))];
    case 'task_pushed':
      return ['branch', tr('你'), tr('把{0}的分支 {1} 推到了 {2}（{3}）', name, p.branch, p.remote, String(p.commit ?? '').slice(0, 8))];
    case 'pr_created':
      return ['merge', tr('你'), tr('给{0}开了 PR：{1}', name, p.url)];
    case 'auto_fix':
      return ['undo', tr('系统'), tr('{0}的自动检查没过，交回 Agent 自动修复（第 {1} 次）', name, p.attempt)];
    case 'issue_resolved': {
      const how = { accepted: tr('采纳了'), manual: tr('手动处理了'), ignored: tr('忽略了'), pending_confirm: tr('把'), open: tr('重新打开了') }[p.resolution as string] ?? tr('处理了');
      return ['check', tr('你'), tr('{0}问题 {1}{2}{3}', how, p.code, p.resolution === 'pending_confirm' ? tr(' 标为待确认') : '', p.note ? tr('：{0}', p.note) : '')];
    }
    case 'finalized':
      return ['lock', tr('你'), tr('定稿，PRD 已锁定')];
    case 'proposal_ready': {
      const what = p.kind === 'impact' ? tr('变更影响分析') : tr('拆分建议');
      return p.error ? ['alert', 'AI', tr('{0}没有生成出来：{1}', what, p.error)] : ['spark', 'AI', tr('{0}出来了，等你确认', what)];
    }
    case 'proposal_discarded':
      return ['x', tr('你'), tr('放弃了{0}', p.kind === 'impact' ? tr('变更影响分析') : tr('拆分建议'))];
    case 'split_applied':
      return ['branch', tr('你'), tr('确认拆分，创建了 {0} 个子任务', (p.tasks as string[] | undefined)?.length ?? 0)];
    case 'impact_applied':
      return ['check', tr('你'), tr('确认影响分析：{0} 个子任务要追加工作，新增 {1} 个', (p.rework as string[] | undefined)?.length ?? 0, p.new_tasks ?? 0)];
    case 'change_requested':
      return ['undo', tr('你'), tr('发起变更：{0}', p.reason ?? '')];
    case 'api_doc_ready':
      return ['file', 'Agent', tr('{0}写出了接口文档 {1}{2}', name, p.path, (p.unblocked as string[] | undefined)?.length ? tr('，等它的子任务已解锁') : '')];
    case 'task_merged':
      return ['merge', tr('系统'), tr('{0}已合并（{1}）{2}', name, String(p.commit ?? '').slice(0, 8), p.note ? tr('；{0}', p.note) : '')];
    case 'compare_started':
      return ['branch', tr('你'), tr('发起多 Agent 对比：{0}另外交给 {1} 个 Agent 各做一遍', name, (p.candidates as string[] | undefined)?.length ?? 0)];
    case 'compare_start_failed':
      return ['alert', tr('系统'), tr('对比候选「{0}」没有入队：{1}', p.title ?? '', p.error ?? '')];
    case 'compare_adopted':
      return ['check', tr('你'), tr('采用了{0}的结果，其余 {1} 个候选已取消', name, (p.dropped as string[] | undefined)?.length ?? 0)];
    case 'task_cancelled':
      return ['x', tr('你'), tr('取消{0}', name)];
    case 'question_asked':
      return ['info', 'Agent', tr('提了一个问题：{0}', p.body ?? '')];
    case 'question_answered':
      return ['check', tr('你'), tr('回答了「{0}」：{1}', p.body ?? '', p.answer ?? '')];
    case 'delegate_finished':
      return [p.error ? 'alert' : 'spark', 'Agent', tr('委派给 {0} 的只读检查{1}', AGENTS[p.agent as string]?.name ?? p.agent, p.error ? tr('没有完成：{0}', p.error) : tr('完成'))];
    case 'comment':
      return ['edit', tr('你'), tr('评论：{0}', p.body ?? '')];
    case 'stage_passed':
      return ['check', tr('你'), tr('通过了环节「{0}」{1}', p.stage ?? '', p.note ? tr('：{0}', p.note) : '')];
    case 'source_linked':
      return ['upload', tr('系统'), tr('从{0}导入（{1}）', p.kind === 'github' ? ' GitHub Issue ' : tr('飞书文档'), p.source ?? '')];
    case 'source_comment':
      return ['edit', tr('{0}（GitHub）', p.author ?? tr('有人')), tr('评论：{0}', p.body ?? '')];
    case 'source_changed':
      return ['info', tr('系统'), tr('外部来源的内容变了，等你决定要不要导入')];
    case 'source_applied':
      return ['check', tr('你'), p.version ? tr('把外部来源的新内容导入为 PRD v{0}', p.version) : tr('同步了外部来源的新标题')];
    case 'source_dismissed':
      return ['x', tr('你'), tr('忽略了外部来源的这次变更')];
    case 'source_written_back':
      return ['check', tr('系统'), tr('已在 GitHub Issue #{0} 下评论并关闭', p.number ?? '')];
    case 'assigned':
      return ['info', tr('你'), p.assignee ? tr('指派给 {0}', p.assignee) : tr('取消了指派')];
    default:
      return ['info', tr('系统'), e.type];
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
    const done = await ui.attempt(api.ticketStagePass(d.ticket.id, cur.id, note), tr('「{0}」已通过', cur.name));
    if (done) setNote('');
  };
  return (
    <div className="callout tide">
      <Icon name="info" size={15} />
      <div style={{ flex: 1 }}>
        <b>
          {tr('等待环节：')}{cur.name}{tr('（')}{same.indexOf(cur) + 1}/{same.length}{tr('）')}
        </b>{' '}
        {tr('通过之后才能')}{cur.gate === 'finalized' ? tr('定稿') : tr('完成这张工单')}{tr('。')}
        {cur.hint && <div style={{ whiteSpace: 'pre-wrap', marginTop: 4 }}>{cur.hint}</div>}
        <div className="row" style={{ gap: 6, marginTop: 8 }}>
          <input className="input" style={{ flex: 1, minWidth: 180 }} value={note} onChange={(e) => setNote(e.target.value)} placeholder={tr('备注（可选）')} aria-label={tr('环节备注')} />
          <button className="btn sm primary" onClick={() => void pass()}>
            {tr('通过')}
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
  const what = src.kind === 'github' ? 'GitHub Issue' : tr('飞书文档');
  const locked = !(['draft', 'human_review'].includes(t.status) || (t.type === 'bug' && ['developing', 'acceptance'].includes(t.status)));
  const apply = async () => {
    if (
      locked &&
      !(await ui.confirm(
        tr('导入会先发起变更'),
        tr('这张工单的需求已经定稿。导入外部的新内容会让它回到人工审核，正在跑的子任务会跑完当前这一轮；之后要重新定稿。'),
        tr('发起变更并导入'),
      ))
    )
      return;
    await ui.attempt(api.syncApply(t.id), tr('已导入为新的 PRD 版本'));
  };
  const titleChanged = src.pending_title && src.pending_title !== t.title;
  return (
    <div className="callout cin">
      <Icon name="info" size={15} />
      <div style={{ flex: 1 }}>
        <b>{tr('源已变更：')}</b>
        {what}{tr('在导入之后有改动')}{titleChanged ? tr('（标题改成了「{0}」）', src.pending_title) : ''}{tr('。现在的 PRD 没有动。')}
        <div className="row" style={{ gap: 6, marginTop: 8 }}>
          <button className="btn sm primary" onClick={() => void apply()}>
            {tr('导入为新版本')}
          </button>
          <button className="btn sm" onClick={() => setOpen((x) => !x)}>
            {open ? tr('收起') : tr('看新内容')}
          </button>
          <button className="btn sm ghost" onClick={() => void ui.attempt(api.syncDismiss(t.id), tr('已忽略这次变更'))}>
            {tr('忽略')}
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
  const answer = (id: string, text: string) => void ui.attempt(api.questionAnswer(id, text), tr('已回答，下一轮运行会带上'));
  return (
    <>
      {open.map((q) => {
        const task = d.tasks.find((k) => k.id === q.task_id);
        const draft = drafts[q.id] ?? '';
        return (
          <div className="callout cin" key={q.id}>
            <Icon name="info" size={15} />
            <div style={{ flex: 1 }}>
              <b>{tr('Agent 有个问题要你回答')}{task ? tr('（{0}）', task.title) : ''}{tr('：')}</b>
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
                  placeholder={tr('写下回答…')}
                  aria-label={tr('回答')}
                />
                <button className="btn sm primary" disabled={!draft.trim()} onClick={() => answer(q.id, draft)}>
                  {tr('回答')}
                </button>
                <button className="btn sm ghost" onClick={() => void ui.attempt(api.questionDismiss(q.id))}>
                  {tr('忽略')}
                </button>
              </div>
            </div>
          </div>
        );
      })}
    </>
  );
}

function ProjectBindings({ d, reload, onClose }: TabProps & { onClose: () => void }) {
  const ui = useUi();
  const data = useData();
  const [picked, setPicked] = useState(() => d.ticket.projects.map((p) => p.project_id));
  const [busy, setBusy] = useState(false);
  const save = async () => {
    setBusy(true);
    const bindings = picked.map((project_id) => d.ticket.projects.find((p) => p.project_id === project_id) ?? ({
      project_id,
      role: data.projects.find((p) => p.id === project_id)?.kind === 'frontend' ? 'frontend' : 'backend',
      subdir: null,
    }));
    if (await ui.attempt(api.ticketBindProjects(d.ticket.id, bindings), tr('项目绑定已保存'))) {
      await Promise.all([reload(), data.reload()]);
      onClose();
    } else {
      setBusy(false);
    }
  };
  return (
    <Modal title={tr('绑定项目')} hint={tr('可多选')} onClose={onClose} footer={
      <>
        <button className="btn" onClick={onClose}>{tr('取消')}</button>
        <button className="btn primary" disabled={busy || !data.projects.length} onClick={() => void save()}>{tr('保存')}</button>
      </>
    }>
      {data.projects.length ? (
        <div className="pick">
          {data.projects.map((p) => (
            <button key={p.id} className={picked.includes(p.id) ? 'on' : ''} aria-pressed={picked.includes(p.id)}
              onClick={() => setPicked((prev) => prev.includes(p.id) ? prev.filter((id) => id !== p.id) : [...prev, p.id])}>
              <Icon name="folder" size={13} />{p.name}
            </button>
          ))}
        </div>
      ) : <p className="dim">{tr('还没有注册项目。')}<a className="lnk" href="#/projects" onClick={onClose}>{tr('去注册 →')}</a></p>}
    </Modal>
  );
}

function OverviewTab({ d, onBind }: { d: TicketDetail; onBind?: () => void }) {
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
              <b>{tr('AI 正在预审这份 PRD。')}</b> {tr('结束后工单进入人工审核。')}{' '}
              <a className="lnk" href={`#/ticket/${t.key}/review`}>
                {tr('看进度 →')}
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
                  ? tr('变更进行中：{0}', t.change_reason)
                  : openIssues.length
                    ? tr('预审提出的问题还有 {0} 个没处理{1}。', openIssues.length, blockers ? tr('，其中 {0} 个阻塞定稿', blockers) : '')
                    : tr('等你审核并定稿。')}
              </b>{' '}
              <a className="lnk" href={`#/ticket/${t.key}/review`}>
                {tr('去审核 →')}
              </a>
            </div>
          </div>
        )}
        {t.status === 'finalized' && proposal && (
          <div className="callout tide">
            <Icon name="spark" size={15} />
            <div>
              <b>
                {proposal.kind === 'impact' ? tr('变更影响分析') : tr('拆分建议')}
                {proposal.status === 'running' ? tr('正在生成。') : proposal.status === 'failed' ? tr('没有生成出来。') : tr('等你确认。')}
              </b>{' '}
              <a className="lnk" href={`#/ticket/${t.key}/tasks`}>
                {tr('去子任务页 →')}
              </a>
            </div>
          </div>
        )}
        {wait.length > 0 && (
          <div className="callout cin">
            <Icon name="check" size={15} />
            <div>
              <b>{wait.map((k) => TASK_KIND[k.kind]?.name ?? k.kind).join(tr('、'))}{tr('子任务已跑完，等你验收。')}</b>{' '}
              <a className="lnk" href={`#/ticket/${t.key}/accept`}>
                {tr('去验收 →')}
              </a>
            </div>
          </div>
        )}
        <div className="sec-title" style={{ marginTop: 18 }}>
          {tr('子任务')}
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
                  <span>{projects.find((p) => p.id === k.project_id)?.name ?? tr('已删除的项目')}</span>
                  {k.branch && <span className="mono">{k.branch}</span>}
                </div>
              </a>
            ))}
          </div>
        ) : (
          <div className="card">
            <Empty icon="branch" title={tr('还没有子任务')} body={tr('定稿时可以让 AI 拆分，也可以在「子任务」页自己添加。每个子任务有自己的分支和 worktree。')}>
              <a className="btn" href={`#/ticket/${t.key}/tasks`}>
                {tr('打开子任务')}
              </a>
            </Empty>
          </div>
        )}
        <div className="sec-title" style={{ marginTop: 22 }}>
          {tr('时间线')}
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
                  <b>{who === tr('你') && e.actor ? e.actor : who}</b> {what}
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
              <dt>{tr('负责人')}</dt>
              <dd>
                <select
                  className="input"
                  aria-label={tr('负责人')}
                  value={t.assignee_id ?? ''}
                  onChange={(e) => void ui.attempt(api.ticketAssign(t.id, e.target.value || null))}
                >
                  <option value="">{tr('未指派')}</option>
                  {assignable.map((m) => (
                    <option key={m.id} value={m.id}>
                      {m.name}
                    </option>
                  ))}
                </select>
              </dd>
            </>
          )}
          <dt>{tr('类型')}</dt>
          <dd>
            <Stamp type={t.type} /> {TYPES[t.type]?.name}
          </dd>
          {d.stages.length > 0 && (
            <>
              <dt>{tr('环节')}</dt>
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
              <dt>{tr('来源')}</dt>
              <dd>
                {d.source.kind === 'github' ? `GitHub Issue #${d.source.external_id}` : tr('飞书文档')}
                {d.source.remote_state === 'closed' && tr('（已关闭）')}
                <div className="mono dim" style={{ wordBreak: 'break-all', userSelect: 'text' }}>
                  {d.source.url}
                </div>
              </dd>
            </>
          )}
          <dt>{tr('绑定项目')}</dt>
          <dd>
            {t.projects.length ? (
              t.projects.map((p) => (
                <a key={p.project_id} className="tag line" href={`#/projects/${p.project_id}`} style={{ marginRight: 4 }}>
                  <Icon name="folder" size={11} />
                  {projects.find((x) => x.id === p.project_id)?.name ?? tr('已删除')}
                </a>
              ))
            ) : (
              <span className="dim">{tr('未绑定')}</span>
            )}
            {onBind && <button className="btn sm" onClick={onBind}>{tr('编辑项目绑定')}</button>}
          </dd>
          <dt>{tr('当前 PRD')}</dt>
          <dd>
            v{d.versions[d.versions.length - 1]?.version ?? 1}
            {t.finalized_version_id ? tr(' · 已定稿') : ''}
          </dd>
          <dt>{tr('默认 Agent')}</dt>
          <dd>
            <AgentTag agent={agent} />
          </dd>
          <dt>Provider</dt>
          <dd>{providers.find((p) => p.id === providerId)?.name ?? tr('本机登录态')}</dd>
          <dt>{tr('标签')}</dt>
          <dd>
            {t.labels.length ? (
              t.labels.map((l) => (
                <span key={l} className="tag" style={{ marginRight: 4 }}>
                  {l}
                </span>
              ))
            ) : (
              <span className="dim">{tr('无')}</span>
            )}
          </dd>
          <dt>{tr('累计费用')}</dt>
          <dd className="mono">{d.runs.length ? `${fmtCost(cost)} · ${fmtDur(secs)}` : '—'}</dd>
          <dt>{tr('创建')}</dt>
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
    if (await ui.attempt(api.ticketSavePrd(t.id, next, note, 'image'), tr('{0}，已保存为新版本', note))) {
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
      if (id) await saveImages(replaceImage(cur.content_md, target.replace, id, tr('需求方截图')), tr('替换了一张图片'));
      return;
    }
    const sel = selectionOf(area.current);
    const added: string[] = [];
    for (const f of files) {
      const id = await ui.attempt(uploadImage(t.id, f));
      if (!id) break;
      added.push(imageMd(tr('需求方截图'), id, 'authoritative'));
    }
    setDraft((d) => insertAt(d ?? cur.content_md, sel, added));
  };
  const imageActions = useMemo<ImageActions>(
    () => ({
      setAuthority: (id, authority) =>
        void saveImages(setAuthority(cur.content_md, id, authority), authority === 'authoritative' ? tr('把一张图设为权威') : tr('把一张图改为仅参考')),
      replace: (id) => {
        pending.current = { replace: id };
        fileInput.current?.click();
      },
      remove: async (id) => {
        if (await ui.confirm(tr('从 PRD 里移除这张图？'), tr('只是当前版本不再引用它；图片文件和旧版本都保留。'), tr('移除')))
          await saveImages(removeImage(cur.content_md, id), tr('移除了一张图片'));
      },
      describe: async (id) => {
        const a = (await ui.attempt(api.attachmentList(t.id)))?.find((x) => x.id === id);
        if (!a) return;
        const text = await ui.prompt(
          tr('图片的文字描述'),
          tr('模型看不了图时（Provider 标为"只能处理文字"），发给 Agent 的是这段描述。'),
          tr('这张图画了什么：有哪些区域、字段、按钮和文案'),
          tr('保存'),
          false,
          a.description ?? '',
        );
        if (text !== null) await ui.attempt(api.attachmentDescribe(id, text), tr('已保存描述'));
      },
    }),
    [cur?.id, cur?.content_md, t.id],
  );
  const canEditImages = idx === 0 && editable && draft === null;

  if (!cur)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="file" title={tr('这张工单还没有正文')} />
        </div>
      </div>
    );

  const save = async () => {
    if (draft === null) return;
    const note = await ui.prompt(tr('保存为新版本'), tr('会生成 v{0}，旧版本保留不变。', versions[0].version + 1), tr('这次改了什么（可选）'), tr('保存'));
    if (note === null) return;
    if (await ui.attempt(api.ticketSavePrd(t.id, draft, note || undefined), tr('已保存为新版本'))) {
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
            {idx ? '' : tr(' · 当前')}
          </span>
          <span className="dim">
            {fmtAgo(cur.created_at)} · {cur.author === 'human' ? tr('你') : cur.author}
            {cur.note ? ` · ${cur.note}` : ''}
          </span>
          {!editable && idx === 0 && (
            <span className="tag">
              <Icon name="lock" size={11} />
              {tr('已锁定，只读')}
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
                {tr('添加图片')}
              </button>
              <button className="btn sm" onClick={() => setDraft(null)}>
                {tr('放弃修改')}
              </button>
              <button className="btn primary sm" disabled={draft === cur.content_md} onClick={() => void save()}>
                {tr('保存为 v')}{versions[0].version + 1}
              </button>
            </>
          ) : (
            <>
              {idx === 0 && editable && (
                <button className="btn sm" onClick={() => setDraft(cur.content_md)}>
                  <Icon name="edit" size={13} />
                  {tr('编辑')}
                </button>
              )}
              {prev && (
                <button className={`btn sm ${diffing ? 'on' : ''}`} onClick={() => setDiffing(!diffing)}>
                  <Icon name="code" size={13} />
                  {diffing ? tr('关闭对比') : tr('与 v{0} 对比', prev.version)}
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
            aria-label={tr('PRD 正文')}
          />
        ) : (
          <div className={`prose ${diffing ? 'diffing' : ''}`}>
            {cur.content_md.trim() ? (
              <Md src={cur.content_md} prev={diffing && prev ? prev.content_md : null} images={canEditImages ? imageActions : undefined} />
            ) : (
              <p className="dim">{tr('正文是空的。')}</p>
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
          {tr('版本')}<small>{tr('点击查看')}</small>
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
                <span>{v.origin === 'edit' ? tr('编辑') : v.origin === 'ai_suggestion' ? tr('AI 采纳') : v.origin === 'file' ? tr('文件导入') : tr('创建')}</span>
                <small>{fmtAgo(v.created_at)}</small>
                {v.note && <small>{v.note}</small>}
              </div>
            </li>
          ))}
        </ol>
        <div className="callout tide" style={{ marginTop: 14 }}>
          <Icon name="info" size={15} />
          <div>{tr('每次保存都会生成一个不可变的新版本。定稿后 PRD 只读，改动需要「发起变更」。')}</div>
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
          <Empty icon="runs" title={tr('还没有运行记录')} body={tr('开始一个子任务后，每次 Agent 运行都会记录在这里。')} />
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
          <span>{tr('运行次数')}</span>
          <b>{d.runs.length}</b>
        </div>
        <div>
          <span>{tr('总耗时')}</span>
          <b>{fmtDur(tot.s)}</b>
        </div>
        <div>
          <span>Token</span>
          <b>{fmtTok(tot.t)}</b>
        </div>
        <div>
          <span>{tr('费用')}</span>
          <b>{fmtCost(tot.c)}</b>
        </div>
      </div>
      <div className="card" style={{ overflow: 'hidden', marginTop: 14 }}>
        <table className="tbl">
          <thead>
            <tr>
              <th>{tr('运行')}</th>
              <th>{tr('子任务')}</th>
              <th>Agent</th>
              <th>Provider</th>
              <th>{tr('状态')}</th>
              <th className="num">{tr('耗时')}</th>
              <th className="num">Token</th>
              <th className="num">{tr('费用')}</th>
            </tr>
          </thead>
          <tbody>
            {[...d.runs].reverse().map((r) => {
              const task = d.tasks.find((k) => k.id === r.task_id);
              return (
                <tr key={r.id} onClick={() => go(`#/runs/${r.id}`)} style={{ cursor: 'pointer' }}>
                  <td className="mono">
                    <a className="lnk" href={`#/runs/${r.id}`} onClick={(e) => e.stopPropagation()}>
                      {tr('第')} {r.number} {tr('次')}
                    </a>
                  </td>
                  <td>{task ? `${TASK_KIND[task.kind]?.short ?? task.kind} · ${task.title}` : r.task_id ? '—' : (PURPOSE[r.purpose] ?? r.purpose)}</td>
                  <td>
                    <AgentTag agent={r.agent} />
                  </td>
                  <td>{providers.find((p) => p.id === r.provider_id)?.name ?? tr('本机登录态')}</td>
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
