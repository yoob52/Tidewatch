// 运行面板：左列表（进行中置顶），右日志时间线，底部状态条
import { useEffect, useRef, useState } from 'react';
import { api, onEngine, type RunDetail, type RunEvent, type RunRow } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { AgentTag, Empty, Inline, Ripple, RunStatusTag, fmtCost, fmtDur, fmtTok, parseTs, runActive, runSeconds, useNow, useUi } from '../lib/ui';

const str = (v: unknown) => (typeof v === 'string' ? v : v == null ? '' : JSON.stringify(v));

function LogItem({ e, start, raw }: { e: RunEvent; start: number | null; raw: boolean }) {
  const p = e.payload;
  const at = parseTs(e.ts);
  const time = <time>{start != null && at != null ? fmtDur((at - start) / 1000) : ''}</time>;
  const sys = (text: string, bad = false) => (
    <div className="logi sys" style={bad ? { color: 'var(--cinnabar)' } : undefined}>
      {time}
      <div>
        <Icon name={bad ? 'alert' : 'info'} size={12} />
        {text}
      </div>
    </div>
  );
  const tool = (name: string, arg: string, body: string, bad?: boolean, tag?: string) => (
    <div className="logi">
      {time}
      {body ? (
        <details className="toolc" open={bad}>
          <summary>
            <Icon name="right" size={12} />
            <b>{name}</b>
            <span className="arg mono">{arg}</span>
            {tag && <span className={`tag ${bad ? 'cin' : 'moss'}`}>{tag}</span>}
          </summary>
          <pre className="term">{body}</pre>
        </details>
      ) : (
        <div className="toolc flat">
          <Icon name="file" size={12} />
          <b>{name}</b>
          <span className="arg mono">{arg}</span>
          {tag && <span className={`tag ${bad ? 'cin' : 'moss'}`}>{tag}</span>}
        </div>
      )}
    </div>
  );
  if (raw) return tool(e.type, '', JSON.stringify(p, null, 2));
  switch (e.type) {
    case 'worktree':
      return sys(`worktree ${str(p.path)} · 分支 ${str(p.branch)}`);
    case 'spawned':
      return tool('启动 Agent', `pid ${str(p.pid)}`, str(p.command));
    case 'session_started':
      return sys(`会话 ${str(p.session_id).slice(0, 8)} · 模型 ${str(p.model) || '默认'}`);
    case 'text':
      return (
        <div className="logi msg">
          {time}
          <div className="bubble">
            <Inline text={str(p.text)} />
          </div>
        </div>
      );
    case 'thinking':
      return tool('思考', '', str(p.text));
    case 'tool_call':
      return tool(str(p.name) || '工具', str(p.input).slice(0, 120), JSON.stringify(p.input, null, 2));
    case 'tool_result':
      return tool('工具结果', '', str(p.output), p.is_error === true, p.is_error === true ? '出错' : undefined);
    case 'command':
      if (p.done !== true) return tool('运行', str(p.command), '');
      return tool('运行', str(p.command), str(p.output), p.exit_code !== 0 && p.exit_code != null, p.exit_code == null ? undefined : `退出码 ${str(p.exit_code)}`);
    case 'file_change':
      return tool({ add: '新建', delete: '删除' }[str(p.kind)] ?? '修改', str(p.path), '');
    case 'usage':
      return null;
    case 'finished':
      return p.summary ? (
        <div className="logi msg">
          {time}
          <div className="bubble">
            <Inline text={str(p.summary)} />
          </div>
        </div>
      ) : null;
    case 'error':
      return sys(str(p.message), true);
    case 'stderr':
      return tool('stderr', '', str(p.text), true);
    case 'commit':
      return sys(`已提交 ${str(p.commit).slice(0, 8)}：${str(p.message)}`);
    case 'check_started':
      return sys(`开始检查 ${str(p.name)}：${str(p.command)}`);
    case 'check_finished':
      return tool(
        `检查 ${str(p.name)}`,
        str(p.command),
        str(p.tail),
        p.exit_code !== 0,
        p.timed_out === true ? '超时' : `退出码 ${str(p.exit_code)}`,
      );
    case 'status':
      return sys(
        { finalizing: 'Agent 已结束，正在提交并运行检查', succeeded: '运行完成', failed: '运行失败', stopped: '已停止', orphaned: '观澜退出时这次运行被中断' }[
          str(p.status)
        ] ?? str(p.status),
        p.status === 'failed',
      );
    default:
      return tool(e.type, '', str(p.line ?? p));
  }
}

function RunItem({ r, on, now }: { r: RunRow; on: boolean; now: number }) {
  const active = runActive(r.status);
  return (
    <a className={`ri ${on ? 'on' : ''}`} href={`#/runs/${r.id}`}>
      <div className="row">
        <AgentTag agent={r.agent} glyphOnly />
        <span className="key">{r.ticket_key ?? '—'}</span>
        <span className="tag line">第 {r.number} 次</span>
        <span className="grow" />
        <span className={`mono ${active ? 't-live' : 'dim'}`}>{r.status === 'queued' ? '排队' : fmtDur(runSeconds(r, now))}</span>
      </div>
      <b className="ell">{r.task_title ?? r.ticket_title ?? '已删除的子任务'}</b>
      <div className="row sub">
        {active ? <Ripple /> : <span className={`dot st-${r.status === 'succeeded' ? 'done' : r.status}`} />}
        <span className="grow ell">{r.error ?? r.summary ?? ''}</span>
        <span className="mono">{fmtTok(r.input_tokens + r.output_tokens)}</span>
      </div>
    </a>
  );
}

export function RunsPage({ id }: { id?: string }) {
  const { runs, providers, reload } = useData();
  const ui = useUi();
  const current = runs.find((r) => r.id === id) ?? runs[0];
  const runId = current?.id;
  const [detail, setDetail] = useState<RunDetail | null>(null);
  const [raw, setRaw] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const anyActive = runs.some((r) => runActive(r.status));
  const now = useNow(anyActive);

  // 先取全量，再把引擎推来的事件按序号接上；序号不连续就重新取一次
  useEffect(() => {
    setDetail(null);
    stick.current = true;
    if (!runId) return;
    let stale = false;
    const fetchAll = () =>
      api.runGet(runId).then(
        (d) => !stale && setDetail(d),
        (e) => !stale && ui.toast(String(e)),
      );
    void fetchAll();
    const off = onEngine((ev) => {
      if (ev.type === 'run_event' && ev.event.run_id === runId) {
        setDetail((d) => {
          if (!d) return d;
          const lastSeq = d.events.length ? d.events[d.events.length - 1].seq : 0;
          if (ev.event.seq <= lastSeq) return d;
          if (ev.event.seq !== lastSeq + 1) {
            void fetchAll();
            return d;
          }
          return { ...d, events: [...d.events, ev.event] };
        });
      } else if (ev.type === 'run_finished' && ev.run_id === runId) {
        void fetchAll();
      }
    });
    return () => {
      stale = true;
      off();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [runId]);

  useEffect(() => {
    const el = logRef.current;
    if (el && stick.current) el.scrollTop = el.scrollHeight;
  }, [detail?.events.length, raw]);

  if (!current)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="runs" title="还没有运行" body="在工单的「子任务」页开始一个子任务，运行会出现在这里。" />
        </div>
      </div>
    );

  const act = runs.filter((r) => runActive(r.status) || r.status === 'queued');
  const hist = runs.filter((r) => !act.includes(r));
  const r = { ...current, ...(detail?.run.id === current.id ? { prompt: detail.run.prompt } : {}) };
  const active = runActive(r.status);
  const start = parseTs(r.started_at ?? r.queued_at);

  const stop = async () => {
    if (r.status !== 'queued' && !(await ui.confirm('停止运行', '会结束 Agent 的整个进程树。已做的改动留在 worktree 里，之后可以恢复原会话继续。', '停止', true))) return;
    if ((await ui.attempt(api.runStop(r.id), r.status === 'queued' ? '已移出队列' : '正在停止…')) !== undefined) void reload();
  };
  const resume = async (ask: boolean) => {
    if (!r.task_id) return;
    let instruction: string | undefined;
    if (ask) {
      const v = await ui.prompt('追加指令', '以续接原会话 + 新指令的方式开始一次新的运行。', '例如：把即将过期的角标改成橙色', '发送', true);
      if (!v) return;
      instruction = v;
    }
    const run = await ui.attempt(api.taskStart(r.task_id, { instruction, command_id: crypto.randomUUID() }), '已入队');
    if (run) {
      await reload();
      window.location.hash = `#/runs/${run.id}`;
    }
  };

  return (
    <div className="page flush runs">
      <aside className="rlist">
        <div className="rl-h">
          <span>进行中</span>
          <i>{act.length}</i>
        </div>
        {act.map((x) => (
          <RunItem key={x.id} r={x} on={x.id === r.id} now={now} />
        ))}
        <div className="rl-h">
          <span>历史</span>
          <i>{hist.length}</i>
        </div>
        {hist.map((x) => (
          <RunItem key={x.id} r={x} on={x.id === r.id} now={now} />
        ))}
      </aside>
      <section className="rmain">
        <header className="rhead">
          <div className="grow">
            <div className="row">
              {r.ticket_key && (
                <a className="key lnk" href={`#/ticket/${r.ticket_key}/tasks`}>
                  {r.ticket_key}
                </a>
              )}
              <span className="tag line">第 {r.number} 次</span>
              <RunStatusTag status={r.status} />
              {r.permission && (
                <span className="tag line">
                  <Icon name="shield" size={11} />
                  {r.permission}
                </span>
              )}
              {r.resume_session_id && <span className="dim">续接会话 {r.resume_session_id.slice(0, 8)}</span>}
            </div>
            <h2>{r.task_title ?? r.ticket_title ?? '已删除的子任务'}</h2>
          </div>
          {active || r.status === 'queued' ? (
            <button className={`btn ${active ? 'danger' : ''}`} disabled={r.status === 'stopping'} onClick={() => void stop()}>
              <Icon name="stop" size={14} />
              {r.status === 'queued' ? '取消排队' : '停止'}
            </button>
          ) : (
            ['stopped', 'failed', 'orphaned'].includes(r.status) && (
              <button className="btn" onClick={() => void resume(false)}>
                <Icon name="resume" size={14} />
                恢复
              </button>
            )
          )}
          {!active && r.status !== 'queued' && (
            <button className="btn" onClick={() => void resume(true)}>
              <Icon name="msg" size={14} />
              追加指令
            </button>
          )}
          <button className={`btn ghost ${raw ? 'on' : ''}`} onClick={() => setRaw(!raw)}>
            <Icon name="code" size={14} />
            {raw ? '时间线' : '原始事件'}
          </button>
        </header>
        <div
          className="rlog"
          ref={logRef}
          onScroll={(e) => {
            const el = e.currentTarget;
            stick.current = el.scrollHeight - el.scrollTop - el.clientHeight < 80;
          }}
        >
          {r.prompt && (
            <div className="logi">
              <time>00:00</time>
              <details className="toolc">
                <summary>
                  <Icon name="right" size={12} />
                  <b>Prompt</b>
                  <span className="arg">{r.prompt.split('\n')[0].slice(0, 80)}</span>
                </summary>
                <pre className="plain">{r.prompt}</pre>
              </details>
            </div>
          )}
          {r.status === 'queued' && (
            <div className="logi msg">
              <time />
              <div className="bubble">等待空闲的并发名额。</div>
            </div>
          )}
          {detail?.events.map((e) => <LogItem key={e.seq} e={e} start={start} raw={raw} />)}
          {r.error && !active && (
            <div className="logi sys" style={{ color: 'var(--cinnabar)' }}>
              <time />
              <div>
                <Icon name="alert" size={12} />
                {r.error}
              </div>
            </div>
          )}
          {active && (
            <div className="logi typing">
              <time />
              <div>
                <Ripple />
                <span>{r.status === 'finalizing' ? '提交并运行检查' : r.status === 'stopping' ? '正在停止' : 'Agent 工作中'}</span>
              </div>
            </div>
          )}
        </div>
        <footer className="rfoot mono">
          <span>
            <AgentTag agent={r.agent} />
          </span>
          <span>
            <i>模型</i>
            {r.model ?? '默认'}
          </span>
          <span>
            <i>Provider</i>
            {providers.find((p) => p.id === r.provider_id)?.name ?? '本机登录态'}
          </span>
          <span title={r.agent_session_id ?? undefined}>
            <i>session</i>
            {r.agent_session_id ? `${r.agent_session_id.slice(0, 8)}…` : '—'}
          </span>
          <span className="grow" />
          <span>
            <i>耗时</i>
            <b>{fmtDur(runSeconds(r, now))}</b>
          </span>
          <span>
            <i>输入</i>
            <b>{fmtTok(r.input_tokens)}</b>
          </span>
          <span>
            <i>输出</i>
            <b>{fmtTok(r.output_tokens)}</b>
          </span>
          <span title={r.cost_source === 'estimated' ? '按 Provider 价格估算' : r.cost_source === 'reported' ? 'Agent 自报' : undefined}>
            <i>费用{r.cost_source === 'estimated' ? '（估）' : ''}</i>
            <b>{fmtCost(r.cost_usd)}</b>
          </span>
        </footer>
      </section>
    </div>
  );
}
