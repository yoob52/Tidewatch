import { useData } from '../lib/data';
import { AgentTag, Empty, fmtCost, fmtDur, fmtTok } from '../lib/ui';

export function StatsPage() {
  const { stats, providers } = useData();
  if (!stats.runs_total && !stats.tickets_total)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="chart" title="还没有数据" body="创建工单并运行子任务后，这里会统计费用、Token 和各 Agent 的表现。" />
        </div>
      </div>
    );

  const days = stats.days.slice(-14);
  const W = 640;
  const H = 180;
  const maxCost = Math.max(0.01, ...days.map((d) => d.cost_usd));
  const maxTok = Math.max(1, ...days.map((d) => d.tokens));
  const x = (i: number) => (days.length > 1 ? 30 + (i * (W - 50)) / (days.length - 1) : W / 2);
  const y = (v: number) => H - 24 - v * (H - 44);
  const area = days.length
    ? `M${x(0)} ${y(days[0].tokens / maxTok)} ${days.map((d, i) => `L${x(i)} ${y(d.tokens / maxTok)}`).join(' ')} L${x(days.length - 1)} ${H - 24} L${x(0)} ${H - 24}Z`
    : '';
  const maxDone = Math.max(1, ...stats.weeks.map((w) => w.done));
  const line = days.map((d, i) => `${i ? 'L' : 'M'}${x(i)} ${y(d.cost_usd / maxCost)}`).join(' ');

  return (
    <div className="page">
      <div className="kpis big">
        <div>
          <span>完成工单</span>
          <b>{stats.tickets_done}</b>
          <small>共 {stats.tickets_total} 张</small>
        </div>
        <div>
          <span>
            平均周期 <i>创建 → 完成</i>
          </span>
          <b>
            {stats.avg_cycle_hours == null ? '—' : stats.avg_cycle_hours < 48 ? stats.avg_cycle_hours.toFixed(1) : (stats.avg_cycle_hours / 24).toFixed(1)}
            {stats.avg_cycle_hours != null && <em>{stats.avg_cycle_hours < 48 ? '小时' : '天'}</em>}
          </b>
        </div>
        <div>
          <span>运行次数</span>
          <b>{stats.runs_total}</b>
        </div>
        <div>
          <span>Token</span>
          <b>{fmtTok(stats.input_tokens + stats.cache_tokens + stats.output_tokens)}</b>
          <small>
            输入 {fmtTok(stats.input_tokens)} · 缓存读取 {fmtTok(stats.cache_tokens)} · 输出 {fmtTok(stats.output_tokens)}
          </small>
        </div>
        <div>
          <span>累计费用</span>
          <b>{fmtCost(stats.cost_usd)}</b>
          <small>Agent 自报，或按 Provider 价格估算</small>
        </div>
      </div>
      <div className="sgrid">
        <div className="card span2">
          <div className="card-h">
            <h3>Token 与费用趋势</h3>
            <span className="grow" />
            <span className="lgd">
              <i style={{ background: 'var(--tide-wash)', border: '1px solid var(--tide)' }} />
              Token（峰值 {fmtTok(maxTok)}）
            </span>
            <span className="lgd">
              <i style={{ background: 'var(--cinnabar)', height: 2 }} />
              费用（峰值 {fmtCost(maxCost)}）
            </span>
          </div>
          <div className="card-b">
            {days.length ? (
              <svg viewBox={`0 0 ${W} ${H}`} className="chart" role="img" aria-label="最近 14 天的 Token 与费用">
                {[0, 0.25, 0.5, 0.75, 1].map((v) => (
                  <line key={v} x1="30" x2={W - 20} y1={y(v)} y2={y(v)} className="grid" />
                ))}
                {days.length > 1 && <path d={area} className="area" />}
                <path d={line} className="line" />
                {days.map((d, i) => (
                  <circle key={d.day} cx={x(i)} cy={y(d.cost_usd / maxCost)} r="2.5" className="pt">
                    <title>
                      {d.day}：{d.runs} 次运行 · {fmtTok(d.tokens)} · {fmtCost(d.cost_usd)}
                    </title>
                  </circle>
                ))}
                {days.map((d, i) =>
                  days.length <= 7 || i % 2 === (days.length - 1) % 2 ? (
                    <text key={d.day} x={x(i)} y={H - 8} textAnchor="middle">
                      {d.day.slice(5).replace('-', '/')}
                    </text>
                  ) : null,
                )}
              </svg>
            ) : (
              <p className="dim">还没有运行记录。</p>
            )}
          </div>
        </div>
        <div className="card">
          <div className="card-h">
            <h3>每周完成工单</h3>
            <span className="grow" />
            <span className="lgd">最近 {stats.weeks.length} 周</span>
          </div>
          <div className="card-b">
            <svg viewBox="0 0 320 150" className="chart" role="img" aria-label="最近几周每周完成的工单数">
              <line x1="10" x2="310" y1="122" y2="122" className="grid" />
              {stats.weeks.map((w, i) => {
                const bw = 300 / stats.weeks.length;
                const h = (w.done / maxDone) * 96;
                return (
                  <g key={w.week}>
                    <rect x={10 + i * bw + bw * 0.2} y={122 - h} width={bw * 0.6} height={h} className="bar">
                      <title>
                        {w.week} 起的一周：完成 {w.done} 张
                      </title>
                    </rect>
                    {w.done > 0 && (
                      <text x={10 + i * bw + bw / 2} y={116 - h} textAnchor="middle">
                        {w.done}
                      </text>
                    )}
                    <text x={10 + i * bw + bw / 2} y={138} textAnchor="middle">
                      {w.week.slice(5).replace('-', '/')}
                    </text>
                  </g>
                );
              })}
            </svg>
          </div>
        </div>
        <div className="card">
          <div className="card-h">
            <h3>返工与自动化</h3>
          </div>
          <table className="tbl">
            <tbody>
              <tr>
                <td>打回返工</td>
                <td className="num">{stats.rework_runs} 次</td>
              </tr>
              <tr>
                <td>检查没过后自动修复</td>
                <td className="num">
                  {stats.auto_fix_runs} 次{stats.auto_fix_runs > 0 && `，${stats.auto_fix_succeeded} 次修好`}
                </td>
              </tr>
              <tr>
                <td>AI 交叉审查</td>
                <td className="num">
                  {stats.code_reviews} 次{stats.code_reviews > 0 && `，报出 ${stats.code_review_findings} 个问题`}
                </td>
              </tr>
            </tbody>
          </table>
        </div>
        <div className="card span2">
          <div className="card-h">
            <h3>各 Agent 表现</h3>
          </div>
          <table className="tbl">
            <thead>
              <tr>
                <th>Agent</th>
                <th className="num">运行次数</th>
                <th style={{ width: '26%' }}>成功率</th>
                <th className="num">失败</th>
                <th className="num">停止</th>
                <th className="num">平均耗时</th>
                <th className="num">Token</th>
                <th className="num">费用</th>
              </tr>
            </thead>
            <tbody>
              {stats.agents.map((a) => {
                const ended = a.succeeded + a.failed + a.stopped;
                const rate = ended ? Math.round((a.succeeded / ended) * 100) : 0;
                return (
                  <tr key={a.agent}>
                    <td>
                      <AgentTag agent={a.agent} />
                    </td>
                    <td className="num">{a.runs}</td>
                    <td>
                      <div className="row">
                        <div className="bar grow">
                          <i style={{ width: `${rate}%`, background: 'var(--moss)' }} />
                        </div>
                        <span className="mono" style={{ width: 34, textAlign: 'right' }}>
                          {ended ? `${rate}%` : '—'}
                        </span>
                      </div>
                    </td>
                    <td className="num">{a.failed}</td>
                    <td className="num">{a.stopped}</td>
                    <td className="num">{a.runs ? fmtDur(a.agent_seconds / a.runs) : '—'}</td>
                    <td className="num">{fmtTok(a.input_tokens + a.cache_tokens + a.output_tokens)}</td>
                    <td className="num">{fmtCost(a.cost_usd)}</td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
        <div className="card span2">
          <div className="card-h">
            <h3>按 Provider</h3>
          </div>
          <table className="tbl">
            <thead>
              <tr>
                <th>Provider</th>
                <th className="num">运行次数</th>
                <th className="num">费用</th>
              </tr>
            </thead>
            <tbody>
              {stats.providers.map(([id, runs, cost]) => (
                <tr key={id ?? 'native'}>
                  <td>{id ? (providers.find((p) => p.id === id)?.name ?? '已删除的 Provider') : '本机登录态'}</td>
                  <td className="num">{runs}</td>
                  <td className="num">{fmtCost(cost)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>
    </div>
  );
}
