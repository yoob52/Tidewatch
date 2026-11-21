import { useEffect, useState } from 'react';
import { api, type TicketSummary } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { Empty, Prio, Ripple, STAGE_NAME, Seg, Stamp, fmtAgo, go, stageTone } from '../lib/ui';
import { TaskChips } from './Board';

const SOURCE: Record<string, string> = { manual: '手动', file: '文件导入' };

export function TicketsPage() {
  const { tickets } = useData();
  const [filter, setFilter] = useState<'all' | 'mine' | 'running' | 'done' | 'cancelled'>('all');
  const [query, setQuery] = useState('');
  const [found, setFound] = useState<TicketSummary[] | null>(null);

  // 搜索走后端的全文索引；`tickets` 变了也要重搜，结果才不会过期
  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setFound(null);
      return;
    }
    let stale = false;
    const timer = setTimeout(() => {
      api.ticketList(q).then(
        (r) => !stale && setFound(r),
        () => !stale && setFound([]),
      );
    }, 200);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [query, tickets]);

  const rows = (found ?? tickets).filter(
    (t) =>
      (filter === 'all' && t.status !== 'cancelled') ||
      (filter === 'mine' && (t.status === 'human_review' || t.status === 'acceptance')) ||
      (filter === 'running' && t.tasks_running > 0) ||
      filter === t.status,
  );

  return (
    <div className="page">
      <div className="row" style={{ marginBottom: 14 }}>
        <Seg
          value={filter}
          onChange={setFilter}
          options={[
            ['all', '全部'],
            ['mine', '等我处理'],
            ['running', '运行中'],
            ['done', '已完成'],
            ['cancelled', '已取消'],
          ]}
        />
        <label className="search" style={{ marginLeft: 8, cursor: 'text' }}>
          <Icon name="search" size={14} />
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="搜索标题、正文或编号"
            aria-label="搜索工单"
            style={{ border: 0, outline: 0, background: 'transparent', color: 'inherit', font: 'inherit', width: 220 }}
          />
        </label>
        <span className="grow" />
        <span className="dim">{rows.length} 张工单</span>
      </div>
      <div className="card" style={{ overflow: 'hidden' }}>
        {rows.length === 0 ? (
          <Empty
            icon="list"
            title={query.trim() ? '没有匹配的工单' : tickets.length ? '这个筛选下没有工单' : '还没有工单'}
            body={tickets.length ? undefined : '把需求或 Bug 描述粘贴进来，就能创建第一张工单。'}
          >
            {!tickets.length && (
              <a className="btn primary" href="#/new">
                新建工单
              </a>
            )}
          </Empty>
        ) : (
          <table className="tbl">
            <thead>
              <tr>
                <th style={{ width: 74 }}>编号</th>
                <th>标题</th>
                <th>状态</th>
                <th>优先级</th>
                <th>子任务</th>
                <th>来源</th>
                <th style={{ textAlign: 'right' }}>更新</th>
              </tr>
            </thead>
            <tbody>
              {rows.map((t) => (
                <tr key={t.id} onClick={() => go(`#/ticket/${t.key}`)} style={{ cursor: 'pointer' }}>
                  <td>
                    <a className="key" href={`#/ticket/${t.key}`} onClick={(e) => e.stopPropagation()}>
                      {t.key}
                    </a>
                  </td>
                  <td>
                    <span className="row">
                      <Stamp type={t.type} />
                      <b style={{ fontWeight: 500 }}>{t.title}</b>
                      {t.tasks_running > 0 && <Ripple />}
                    </span>
                  </td>
                  <td>
                    <span className={`tag ${stageTone(t.status)}`}>{STAGE_NAME[t.status]}</span>
                  </td>
                  <td>
                    <Prio p={t.priority} />
                  </td>
                  <td>
                    <span className="row" style={{ gap: 4 }}>
                      {t.tasks_total ? <TaskChips t={t} /> : <span className="dim">—</span>}
                    </span>
                  </td>
                  <td className="dim">{SOURCE[t.source_kind] ?? t.source_kind}</td>
                  <td className="dim" style={{ textAlign: 'right', whiteSpace: 'nowrap' }}>
                    {fmtAgo(t.updated_at)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </div>
    </div>
  );
}
