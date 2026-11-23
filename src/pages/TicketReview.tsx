import { tr } from '../lib/i18n';
// 工单详情里的「审核」页：AI 预审结果、问题清单、验收标准、定稿
import { useState } from 'react';
import { api, type Issue, type Resolution, type Severity, type TicketRunOptions } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { AgentTag, Empty, Inline, Md, Modal, Ripple, fmtCost, fmtDur, go, runActive, runSeconds, useLoad, useUi } from '../lib/ui';
import { useConfirmTicketRun } from '../lib/start';
import { RunOptionsFields, useChooseTicketRun } from '../lib/runOptions';
import type { TabProps } from './Ticket';

const SEV: Record<Severity, [string, string]> = {
  blocker: [tr('阻塞'), 'cin'],
  major: [tr('重要'), 'amber'],
  minor: [tr('一般'), 'tide'],
  suggestion: [tr('建议'), ''],
};
const RES: Record<Resolution, string> = {
  open: tr('待处理'),
  accepted: tr('已采纳'),
  manual: tr('已手动修改'),
  ignored: tr('已忽略'),
  pending_confirm: tr('待确认'),
};
const unresolved = (i: Issue) => i.resolution === 'open' || i.resolution === 'pending_confirm';

/** 改正文的弹层：采纳建议前的预览（可以再改），或手动修改。 */
function PrdEditDialog({
  title,
  hint,
  current,
  proposed,
  ok,
  onClose,
  onSave,
}: {
  title: string;
  hint: string;
  current: string;
  proposed: string;
  ok: string;
  onClose: () => void;
  onSave: (content: string) => Promise<void>;
}) {
  const [text, setText] = useState(proposed);
  const [editing, setEditing] = useState(proposed === current);
  const [busy, setBusy] = useState(false);
  return (
    <Modal
      title={title}
      hint={hint}
      width={760}
      onClose={onClose}
      footer={
        <>
          {!editing && (
            <button className="btn" onClick={() => setEditing(true)}>
              <Icon name="edit" size={13} />
              {tr('改一下再保存')}
            </button>
          )}
          <span className="grow" />
          <button className="btn" onClick={onClose}>
            {tr('取消')}
          </button>
          <button
            className="btn primary"
            disabled={busy}
            onClick={async () => {
              setBusy(true);
              await onSave(text);
              setBusy(false);
            }}
          >
            {ok}
          </button>
        </>
      }
    >
      {editing ? (
        <textarea className="textarea mono" style={{ minHeight: 380 }} value={text} onChange={(e) => setText(e.target.value)} aria-label={tr('PRD 正文')} autoFocus />
      ) : (
        <div className="prose diffing">
          <Md src={text} prev={current} />
        </div>
      )}
    </Modal>
  );
}

export function FinalizeDialog({ d, blockers, onClose, onDone }: { d: TabProps['d']; blockers: number; onClose: () => void; onDone: () => Promise<void> }) {
  const ui = useUi();
  const confirmRun = useConfirmTicketRun();
  const { providers } = useData();
  const t = d.ticket;
  const hasTasks = d.tasks.some((k) => k.status !== 'cancelled');
  const impact = !!t.change_from_version_id && hasTasks;
  const [split, setSplit] = useState(!hasTasks);
  const [options, setOptions] = useState<TicketRunOptions>({});
  const [busy, setBusy] = useState(false);
  const version = d.versions[d.versions.length - 1]?.version ?? 1;
  const criteria = t.acceptance_criteria.length;
  const needsProject = (split || impact) && t.projects.length === 0;
  const runAi = split || impact;
  const { data: config, error } = useLoad(
    () => runAi && !needsProject ? api.ticketRunConfig(t.id, impact ? 'impact' : 'split') : Promise.resolve(null),
    [t.id, runAi, needsProject, impact],
  );
  const blocked = blockers > 0 || criteria === 0 || needsProject || (runAi && !config);

  const confirm = async () => {
    if ((split || impact) && !(await confirmRun())) return;
    setBusy(true);
    const res = await ui.attempt(api.ticketFinalize(t.id, split, runAi ? options : undefined));
    setBusy(false);
    if (!res) return;
    ui.toast(
      res.run
        ? tr('{0} 已定稿（v{1}），{2}', t.key, version, res.run.purpose === 'impact' ? tr('正在分析变更的影响') : tr('AI 正在拆分子任务'))
        : tr('{0} 已定稿（v{1}）', t.key, version),
    );
    await onDone();
    go(`#/ticket/${t.key}/tasks`);
  };

  return (
    <Modal
      title={impact ? tr('定稿这次变更') : tr('定稿')}
      hint={tr('定稿会锁定 PRD v{0}。之后再改需求，需要「发起变更」并重新审核。', version)}
      onClose={onClose}
      footer={
        <>
          <button className="btn" onClick={onClose}>
            {tr('再看看')}
          </button>
          <button className="btn primary" disabled={blocked || busy} onClick={() => void confirm()}>
            <Icon name="lock" size={14} />
            {tr('确认定稿')}
          </button>
        </>
      }
    >
      <div className="gate">
        <div className={blockers ? 'bad' : 'ok'}>
          <Icon name={blockers ? 'x' : 'check'} size={13} />
          {blockers ? tr('还有 {0} 个阻塞问题未处理', blockers) : tr('阻塞问题已全部处理')}
        </div>
        <div className={criteria ? 'ok' : 'bad'}>
          <Icon name={criteria ? 'check' : 'x'} size={13} />
          {criteria ? tr('验收标准 {0} 条', criteria) : tr('验收标准不能为空')}
        </div>
        <div className={needsProject ? 'bad' : 'ok'}>
          <Icon name={needsProject ? 'x' : 'check'} size={13} />
          {t.projects.length ? tr('已绑定 {0} 个项目', t.projects.length) : needsProject ? tr('AI 要在项目目录里看代码，请先绑定项目') : tr('没有绑定项目')}
        </div>
      </div>
      {impact ? (
        <div className="callout tide" style={{ marginTop: 14 }}>
          <Icon name="spark" size={15} />
          <div>
            {tr('这是一次变更（')}{t.change_reason}{tr('）。定稿后 AI 会对比变更前后的 PRD，分析每个已有子任务要不要追加工作；结果在「子任务」页等你确认。')}
          </div>
        </div>
      ) : (
        <label className="row" style={{ marginTop: 14, cursor: 'pointer' }} onClick={() => setSplit(!split)}>
          <span className={`check ${split ? 'on' : ''}`}>
            <Icon name="check" size={11} />
          </span>
          <span>
            {tr('让 AI 拆分子任务')}
            <small className="dim" style={{ display: 'block' }}>
              {tr('给出后端 / 前端子任务和依赖关系的建议，在「子任务」页确认或修改后才会创建。不勾选则自己添加子任务。')}
            </small>
          </span>
        </label>
      )}
      {runAi && config && <RunOptionsFields config={config} providers={providers} value={options} onChange={setOptions} />}
      {runAi && error && <p className="dim" role="alert">{error}</p>}
    </Modal>
  );
}

export function ReviewTab({ d, reload, onBind }: TabProps & { onBind?: () => void }) {
  const ui = useUi();
  const confirmRun = useConfirmTicketRun();
  const chooseRun = useChooseTicketRun();
  const data = useData();
  const t = d.ticket;
  const [dialog, setDialog] = useState<
    | { kind: 'accept' | 'manual'; issue: Issue; proposed: string }
    | { kind: 'finalize' }
    | { kind: 'questions'; text: string }
    | null
  >(null);
  const [newCriterion, setNewCriterion] = useState('');

  const refresh = () => Promise.all([reload(), data.reload()]);
  const prd = d.versions[d.versions.length - 1];
  const review = d.reviews[d.reviews.length - 1];
  const lastGood = [...d.reviews].reverse().find((r) => !r.error);
  const reviewRun = d.runs.find((r) => r.id === review?.run_id);
  const activeRun = d.runs.find((r) => r.task_id === null && r.purpose === 'review' && (runActive(r.status) || r.status === 'queued'));
  const editable = t.status === 'human_review';
  const blockers = d.issues.filter((i) => i.severity === 'blocker' && unresolved(i)).length;
  const pending = d.issues.filter((i) => i.resolution === 'pending_confirm').length;
  const canEditCriteria = ['draft', 'ai_review', 'human_review'].includes(t.status);
  // 已采用的在前，AI 起草但还没勾选的在后
  const criteria = [...t.acceptance_criteria, ...(lastGood?.criteria_draft ?? []).filter((c) => !t.acceptance_criteria.includes(c))];

  const startReview = async () => {
    if (!t.projects.length && onBind) return onBind();
    const options = await chooseRun(t.id, 'review', tr('提交预审'));
    if (!options || !(await confirmRun())) return;
    if (await ui.attempt(api.reviewStart(t.id, options), tr('已提交预审，有空闲并发时自动开始'))) await refresh();
  };
  const skipReview = async () => {
    if (!(await ui.confirm(tr('跳过 AI 预审'), tr('工单直接进入人工审核，由你自己检查 PRD。之后仍然可以点「重新预审」。'), tr('跳过预审')))) return;
    if (await ui.attempt(api.reviewSkip(t.id), tr('已进入人工审核'))) await refresh();
  };

  if (t.status === 'draft')
    return (
      <div className="page">
        <div className="card">
          <Empty
            icon="spark"
            title={t.type === 'bug' ? tr('Bug 可以走快速通道，不必审核') : tr('这份 PRD 还没有预审')}
            body={
              t.type === 'bug'
                ? tr('描述太模糊时，也可以先送去 AI 预审，让它指出缺了什么。')
                : tr('AI 会只读地查看项目代码，指出 PRD 里的缺口和歧义，并起草验收标准。')
            }
          >
            <button className="btn primary" onClick={() => void startReview()}>
              <Icon name="spark" size={13} />
              {tr('提交预审')}
            </button>
            {t.type !== 'bug' && (
              <button className="btn" onClick={() => void skipReview()}>
                {tr('跳过预审')}
              </button>
            )}
          </Empty>
        </div>
      </div>
    );

  if (t.status === 'ai_review')
    return (
      <div className="page">
        <div className="card">
          <Empty
            icon="spark"
            title={activeRun?.status === 'queued' ? tr('预审在排队') : tr('AI 正在预审这份 PRD')}
            body={
              activeRun
                ? tr('PRD v{0} · 只读模式，不会改动项目文件。结束后工单会进入人工审核。', prd?.version ?? 1)
                : tr('没有找到正在进行的预审运行。可以直接进入人工审核，或重新提交预审。')
            }
          >
            {activeRun ? (
              <>
                <span className="live">
                  <Ripple />
                </span>
                <AgentTag agent={activeRun.agent} />
                <a className="btn" href={`#/runs/${activeRun.id}`}>
                  <Icon name="term" size={13} />
                  {tr('看实时日志')}
                </a>
                <button
                  className="btn danger"
                  onClick={async () => {
                    if ((await ui.attempt(api.runStop(activeRun.id), tr('已停止预审，工单回到人工审核'))) !== undefined) await refresh();
                  }}
                >
                  <Icon name="stop" size={13} />
                  {tr('停止预审')}
                </button>
              </>
            ) : (
              <button
                className="btn"
                onClick={async () => {
                  if (await ui.attempt(api.ticketTransition(t.id, 'human_review'))) await refresh();
                }}
              >
                {tr('进入人工审核')}
              </button>
            )}
          </Empty>
        </div>
      </div>
    );

  const resolve = async (issue: Issue, action: 'pending' | 'reopen') => {
    if (await ui.attempt(api.issueResolve(issue.id, action))) await refresh();
  };
  const ignore = async (issue: Issue) => {
    const reason = await ui.prompt(tr('忽略 {0}', issue.code), tr('理由会记到 PRD 的「不做范围」里，开发的 Agent 也能看到。'), tr('为什么不处理'), tr('忽略'), true);
    if (!reason) return;
    if (await ui.attempt(api.issueResolve(issue.id, 'ignore', reason), tr('已忽略 {0}，PRD 已更新', issue.code))) await refresh();
  };
  const accept = async (issue: Issue) => {
    const proposed = await ui.attempt(api.issuePreview(issue.id, 'accept'));
    if (proposed !== undefined) setDialog({ kind: 'accept', issue, proposed });
  };
  const saveCriteria = async (list: string[]) => {
    if (await ui.attempt(api.criteriaSet(t.id, list))) await refresh();
  };
  const exportQuestions = async () => {
    const text = await ui.attempt(api.questionsExport(t.id));
    if (text !== undefined) setDialog({ kind: 'questions', text });
  };

  const card = (i: Issue) => {
    const [name, tone] = SEV[i.severity];
    const closed = !unresolved(i);
    const isPending = i.resolution === 'pending_confirm';
    return (
      <article key={i.id} className={`iss ${closed ? 'closed' : ''} s-${i.severity}`}>
        <header>
          <span className={`tag ${tone}`}>{name}</span>
          <span className="key">{i.code}</span>
          <span className="dim">{i.category}</span>
          <span className="grow" />
          {closed ? (
            <>
              <span className={`tag ${i.resolution === 'ignored' ? '' : 'moss'}`}>{RES[i.resolution]}</span>
              {editable && (
                <button className="btn ghost sm" title={tr('重新打开这个问题；已经写进正文的内容不会自动改回去')} onClick={() => void resolve(i, 'reopen')}>
                  {tr('撤销')}
                </button>
              )}
            </>
          ) : (
            isPending && <span className="tag amber">{tr('待确认')}{i.severity === 'blocker' ? tr(' · 仍阻塞定稿') : ''}</span>
          )}
        </header>
        {i.location && <blockquote>{i.location}</blockquote>}
        <p>
          <Inline text={i.problem} />
        </p>
        {closed
          ? i.resolution_note && <p className="dim">{i.resolution === 'ignored' ? tr('忽略理由：') : tr('备注：')}{i.resolution_note}</p>
          : (
              <>
                {i.question && (
                  <p className="q">
                    <Icon name="msg" size={13} />
                    <span>{tr('需确认：')}{i.question}</span>
                  </p>
                )}
                {i.suggestion && (
                  <div className="fix">
                    <span>{tr('建议改为')}</span>
                    <div>
                      <Inline text={i.suggestion} />
                    </div>
                  </div>
                )}
                {editable && (
                  <footer>
                    {i.suggestion && (
                      <button className="btn primary sm" onClick={() => void accept(i)}>
                        <Icon name="check" size={13} />
                        {tr('采纳建议')}
                      </button>
                    )}
                    <button className="btn sm" onClick={() => setDialog({ kind: 'manual', issue: i, proposed: prd?.content_md ?? '' })}>
                      {tr('手动修改')}
                    </button>
                    {isPending ? (
                      <button className="btn sm" onClick={() => void resolve(i, 'reopen')}>
                        {tr('取消待确认')}
                      </button>
                    ) : (
                      <button className="btn sm" title={tr('要先问需求方；阻塞级问题在确认前仍然不能定稿')} onClick={() => void resolve(i, 'pending')}>
                        {tr('待确认')}
                      </button>
                    )}
                    <button className="btn ghost sm" onClick={() => void ignore(i)}>
                      {tr('忽略…')}
                    </button>
                  </footer>
                )}
              </>
            )}
      </article>
    );
  };

  const groups = (['blocker', 'major', 'minor', 'suggestion'] as Severity[]).map((s) => {
    const list = d.issues.filter((i) => i.severity === s);
    if (!list.length) return null;
    return (
      <div key={s}>
        <div className="iss-g">
          <span>{SEV[s][0]}</span>
          <i>
            {list.filter(unresolved).length} / {list.length} {tr('待处理')}
          </i>
        </div>
        {list.map(card)}
      </div>
    );
  });

  return (
    <div className="rv">
      <div className="rv-bar">
        <div className="rv-sum">
          <b>{tr('AI 预审')}</b>
          {reviewRun ? (
            <span className="dim">
              <a className="lnk" href={`#/runs/${reviewRun.id}`}>
                {reviewRun.agent}
              </a>{' '}
              {tr('· 用时')} {fmtDur(runSeconds(reviewRun))} · {fmtCost(reviewRun.cost_usd)}
            </span>
          ) : (
            !review && <span className="dim">{tr('这张工单没有经过 AI 预审')}</span>
          )}
          {review?.summary && <span className="q1">{tr('「')}{review.summary}{tr('」')}</span>}
        </div>
        <span className="grow" />
        {editable && (
          <>
            <button className="btn sm" onClick={() => void startReview()} title={tr('重新审一遍当前的 PRD；已经提过、还没处理的问题不会重复提')}>
              <Icon name="resume" size={13} />
              {review ? tr('重新预审') : tr('提交预审')}
            </button>
            <button className="btn sm" disabled={!pending} onClick={() => void exportQuestions()}>
              <Icon name="msg" size={13} />
              {tr('导出待确认问题')}{pending ? ` (${pending})` : ''}
            </button>
            <button className="btn primary sm" onClick={() => setDialog({ kind: 'finalize' })} title={blockers ? tr('还有 {0} 个阻塞问题未处理', blockers) : tr('锁定 PRD')}>
              <Icon name="lock" size={13} />
              {tr('定稿')}{blockers ? tr(' · 还差 {0} 个阻塞', blockers) : '…'}
            </button>
          </>
        )}
      </div>
      <div className="rv-cols">
        <section className="rv-doc">
          <div className="doc-bar">
            <span className="tag tide">PRD v{prd?.version ?? 1}</span>
            {!editable && (
              <span className="tag">
                <Icon name="lock" size={11} />
                {t.finalized_version_id ? tr('已定稿') : tr('只读')}
              </span>
            )}
            <span className="grow" />
            <a className="btn ghost sm" href={`#/ticket/${t.key}/prd`}>
              <Icon name="edit" size={13} />
              {editable ? tr('编辑 / 看版本') : tr('看版本')}
            </a>
          </div>
          <div className="prose">
            {prd?.content_md.trim() ? <Md src={prd.content_md} /> : <p className="dim">{tr('正文是空的。')}</p>}
            <h3>
              {tr('验收标准')}{' '}
              {canEditCriteria && (
                <span className="tag tide" style={{ verticalAlign: 2 }}>
                  {tr('勾选的纳入定稿')}
                </span>
              )}
            </h3>
            {criteria.length ? (
              <ul className="ac">
                {criteria.map((c) => {
                  const on = t.acceptance_criteria.includes(c);
                  return (
                    <li
                      key={c}
                      style={canEditCriteria ? undefined : { cursor: 'default' }}
                      onClick={() => canEditCriteria && void saveCriteria(on ? t.acceptance_criteria.filter((x) => x !== c) : [...t.acceptance_criteria, c])}
                    >
                      <span className={`check ${on ? 'on' : ''}`}>
                        <Icon name="check" size={11} />
                      </span>
                      {c}
                    </li>
                  );
                })}
              </ul>
            ) : (
              <p className="dim">{tr('还没有验收标准。定稿前至少要有一条。')}</p>
            )}
            {canEditCriteria && (
              <div className="row" style={{ marginTop: 8 }}>
                <input
                  className="input grow"
                  placeholder={tr('添加一条可验证的验收标准')}
                  value={newCriterion}
                  onChange={(e) => setNewCriterion(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === 'Enter' && newCriterion.trim()) {
                      void saveCriteria([...t.acceptance_criteria, newCriterion.trim()]);
                      setNewCriterion('');
                    }
                  }}
                />
                <button
                  className="btn sm"
                  disabled={!newCriterion.trim()}
                  onClick={() => {
                    void saveCriteria([...t.acceptance_criteria, newCriterion.trim()]);
                    setNewCriterion('');
                  }}
                >
                  {tr('添加')}
                </button>
              </div>
            )}
          </div>
        </section>
        <section className="rv-iss">
          {t.change_from_version_id && editable && (
            <div className="callout tide" style={{ marginTop: 14 }}>
              <Icon name="info" size={15} />
              <div>
                <b>{tr('变更进行中：')}</b>
                {t.change_reason}{tr('。在「PRD」页改完后重新定稿，系统会分析这次变更对已有子任务的影响。')}
              </div>
            </div>
          )}
          {review?.error && (
            <div className="callout cin" style={{ marginTop: 14 }}>
              <Icon name="alert" size={15} />
              <div>
                <b>{tr('上一次预审没有完成：')}</b>
                {review.error}{' '}
                {reviewRun && (
                  <a className="lnk" href={`#/runs/${reviewRun.id}`}>
                    {tr('看那次运行 →')}
                  </a>
                )}
              </div>
            </div>
          )}
          {groups}
          {d.issues.length === 0 && (
            <Empty
              icon={review && !review.error ? 'check' : 'spark'}
              title={review && !review.error ? tr('预审没有发现问题') : tr('还没有问题清单')}
              body={
                review && !review.error
                  ? tr('确认验收标准后就可以定稿。')
                  : editable
                    ? tr('可以提交 AI 预审，也可以自己检查后直接定稿。')
                    : undefined
              }
            />
          )}
        </section>
      </div>

      {dialog?.kind === 'finalize' && <FinalizeDialog d={d} blockers={blockers} onClose={() => setDialog(null)} onDone={async () => void (await refresh())} />}
      {(dialog?.kind === 'accept' || dialog?.kind === 'manual') && (
        <PrdEditDialog
          title={dialog.kind === 'accept' ? tr('采纳 {0} 的建议', dialog.issue.code) : tr('手动修改正文（{0}）', dialog.issue.code)}
          hint={
            dialog.kind === 'accept'
              ? tr('绿色是会加进正文的内容。保存后生成一个新的 PRD 版本，来源记为「AI 采纳」。')
              : tr('按你的想法改正文，保存后生成新版本并把这个问题标为已处理。正文不用改的话直接保存。')
          }
          current={prd?.content_md ?? ''}
          proposed={dialog.proposed}
          ok={dialog.kind === 'accept' ? tr('采纳并保存') : tr('保存并标为已处理')}
          onClose={() => setDialog(null)}
          onSave={async (content) => {
            const issue = dialog.issue;
            const changed = content !== (prd?.content_md ?? '');
            const done = await ui.attempt(
              api.issueResolve(issue.id, dialog.kind, undefined, dialog.kind === 'accept' || changed ? content : undefined),
              `${issue.code} ${dialog.kind === 'accept' ? tr('已采纳') : tr('已处理')}${changed ? tr('，PRD 已更新') : ''}`,
            );
            if (done) {
              setDialog(null);
              await refresh();
            }
          }}
        />
      )}
      {dialog?.kind === 'questions' && (
        <Modal
          title={tr('待确认的问题')}
          hint={tr('可以直接发给需求方。得到答复后，回到对应的问题点「采纳建议」或「手动修改」。')}
          onClose={() => setDialog(null)}
          footer={
            <>
              <button className="btn" onClick={() => setDialog(null)}>
                {tr('关闭')}
              </button>
              <button
                className="btn primary"
                onClick={async () => {
                  await ui.attempt(navigator.clipboard.writeText(dialog.text), tr('已复制到剪贴板'));
                }}
              >
                {tr('复制')}
              </button>
            </>
          }
        >
          <textarea className="textarea mono" style={{ minHeight: 280 }} readOnly value={dialog.text} aria-label={tr('待确认的问题')} />
        </Modal>
      )}
    </div>
  );
}
