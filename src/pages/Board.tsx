import { lang, tr } from '../lib/i18n';
// 看板：七个阶段是一条从左到右的水流；「人工审核」「验收中」两列是你的关口
import { useState } from 'react';
import { api, type TicketStatus, type TicketSummary } from '../lib/api';
import { useData } from '../lib/data';
import { canMove } from '../lib/flow';
import { useConfirmTicketRun } from '../lib/start';
import { Icon } from '../lib/Icon';
import { Ripple, STAGES, STAGE_NAME, Seg, Stamp, runActive, useUi } from '../lib/ui';

export function TaskChips({ t }: { t: TicketSummary }) {
  if (!t.tasks_total) return null;
  return (
    <>
      {t.tasks_running > 0 && (
        <span className="subchip tide">
          <Ripple />
          <span>{t.tasks_running} {tr('个在跑')}</span>
        </span>
      )}
      <span className={`subchip ${t.tasks_done === t.tasks_total ? 'moss' : 'dim'}`} title={tr('已跑完的子任务 / 全部子任务')}>
        <b>
          {t.tasks_done}/{t.tasks_total}
        </b>
        <span>{tr('子任务完成')}</span>
      </span>
    </>
  );
}

export function BoardPage() {
  const { tickets, projects, runs, reload } = useData();
  const ui = useUi();
  const confirmRun = useConfirmTicketRun();
  const [filter, setFilter] = useState('all');
  const [drag, setDrag] = useState<TicketSummary | null>(null);
  const [over, setOver] = useState<TicketStatus | null>(null);

  const projectName = (id: string) => projects.find((p) => p.id === id)?.name ?? tr('已删除的项目');
  const list = tickets.filter(
    (t) => filter === 'all' || (filter === 'bug' ? t.type === 'bug' : t.projects.some((p) => p.project_id === filter)),
  );
  const review = tickets.filter((t) => t.status === 'human_review');
  const acc = tickets.filter((t) => t.status === 'acceptance');
  const running = runs.filter((r) => runActive(r.status)).length;
  const d = new Date();
  const dateText =
    lang === 'en'
      ? d.toLocaleDateString('en-US', { weekday: 'short', month: 'short', day: 'numeric' })
      : `${d.getMonth() + 1} 月 ${d.getDate()} 日 · 周${'日一二三四五六'[d.getDay()]}`;

  const move = async (t: TicketSummary, to: TicketStatus) => {
    if (t.status === to) return;
    const r = canMove(t, to);
    if (!r.ok) {
      ui.toast(tr('{0} 无法流转：{1}', t.key, r.why));
      return;
    }
    if (to === 'finalized' && !(await ui.confirm(tr('定稿'), tr('定稿后 {0} 的 PRD 将锁定，再修改需要「发起变更」。要让 AI 拆分子任务，请在工单的「审核」页定稿。', t.key), tr('定稿')))) return;
    if (to === 'finalized' && !(await confirmRun())) return;
    let job: Promise<unknown>;
    if (to === 'human_review' && (t.status === 'finalized' || t.status === 'developing')) {
      const reason = await ui.prompt(tr('发起变更'), tr('{0} 回到人工审核，PRD 重新可编辑；正在运行的子任务会继续跑完。', t.key), tr('为什么要改需求'), tr('发起变更'), true);
      if (!reason) return;
      job = api.changeRequest(t.id, reason, false);
    } else {
      if (to === 'ai_review' && !(await confirmRun())) return;
      job = api.ticketTransition(t.id, to);
    }
    const done = await ui.attempt(job);
    if (done) {
      ui.toast(tr('{0} → {1}（{2}）', t.key, STAGE_NAME[to], r.label));
      void reload();
    }
  };

  return (
    <div className="page flush board-page">
      <div className="today">
        <div className="today-l">
          <span className="today-date">{dateText}</span>
          <p>
            {tr('有')} <b>{review.length + acc.length}</b> {tr('张工单在等你：')}
            <a href={review[0] ? `#/ticket/${review[0].key}` : '#/tickets'}>{review.length} {tr('张待审核')}</a>{tr('，')}
            <a href={acc[0] ? `#/ticket/${acc[0].key}/accept` : '#/tickets'}>{acc.length} {tr('张待验收')}</a>{tr('。')}
            {running ? (
              <>
                {tr('另有')} <a href="#/runs">{running} {tr('个 Agent 正在运行')}</a>{tr('。')}
              </>
            ) : (
              tr('当前没有 Agent 在运行。')
            )}
          </p>
        </div>
        <div className="row">
          <Seg
            value={filter}
            onChange={setFilter}
            options={[['all', tr('全部')], ...projects.slice(0, 4).map((p) => [p.id, p.name] as const), ['bug', tr('只看 Bug')]]}
          />
        </div>
      </div>
      <div className="board">
        {STAGES.map((s) => {
          const items = list.filter((t) => t.status === s.id);
          const hint = drag && drag.status !== s.id ? (canMove(drag, s.id).ok ? 'can' : 'cannot') : '';
          return (
            <section
              key={s.id}
              className={`col ${s.mine ? 'mine' : ''} ${hint} ${over === s.id ? 'over' : ''}`}
              onDragOver={(e) => {
                e.preventDefault();
                setOver(s.id);
              }}
              onDragLeave={() => setOver((o) => (o === s.id ? null : o))}
              onDrop={(e) => {
                e.preventDefault();
                setOver(null);
                if (drag) void move(drag, s.id);
                setDrag(null);
              }}
            >
              <header className="col-h">
                <span className="col-num">{s.num}</span>
                <b>{s.name}</b>
                <span className="col-n">{items.length}</span>
                {s.mine && items.length > 0 && <span className="col-flag">{tr('等你')}</span>}
              </header>
              <div className="col-b">
                {items.length === 0 && <div className="col-empty">{s.hint}</div>}
                {items.map((t) => (
                  <a
                    key={t.id}
                    className={`tcard p-${t.priority} ${drag?.id === t.id ? 'dragging' : ''}`}
                    draggable
                    href={`#/ticket/${t.key}`}
                    onDragStart={(e) => {
                      e.dataTransfer.effectAllowed = 'move';
                      setDrag(t);
                    }}
                    onDragEnd={() => {
                      setDrag(null);
                      setOver(null);
                    }}
                  >
                    <div className="tcard-top">
                      <Stamp type={t.type} />
                      <span className="key">{t.key}</span>
                      {t.tasks_running > 0 && (
                        <span className="live">
                          <Ripple />
                        </span>
                      )}
                      <span className="grow" />
                      {t.due_date && (
                        <span className="due">
                          <Icon name="clock" size={11} />
                          {t.due_date.slice(5)}
                        </span>
                      )}
                    </div>
                    <div className="tcard-title">{t.title}</div>
                    <div className="tcard-meta">
                      {t.projects.map((p) => (
                        <span key={p.project_id} className="tag line">
                          {projectName(p.project_id)}
                        </span>
                      ))}
                    </div>
                    {t.waiting_stage && (
                      <span className="subchip dim" title={tr('自定义工作流：这个环节通过后才能往下走')}>
                        {tr('待：')}{t.waiting_stage}
                      </span>
                    )}
                    {(t.tasks_total > 0 || (t.status === 'human_review' && t.issues_open > 0) || t.status === 'ai_review') && (
                      <div className="tcard-foot">
                        {t.tasks_total > 0 && <TaskChips t={t} />}
                        {t.status === 'human_review' && t.issues_open > 0 && (
                          <span className="subchip cin">
                            <b>{t.issues_open}</b>
                            <span>{tr('个问题待处理')}</span>
                          </span>
                        )}
                        {t.status === 'ai_review' && (
                          <span className="subchip tide">
                            <span>{tr('AI 预审中')}</span>
                          </span>
                        )}
                      </div>
                    )}
                  </a>
                ))}
              </div>
            </section>
          );
        })}
      </div>
      <div className="board-hint">
        <Icon name="drag" size={12} />
        {tr('拖动卡片到另一列即尝试流转；不合法的流转会说明原因。Bug 可从「草稿」直接拖到「开发中」。')}
      </div>
    </div>
  );
}
