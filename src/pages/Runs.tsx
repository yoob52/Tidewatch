import { tr } from '../lib/i18n';
// 运行面板：单栏（左列表 + 右时间线）或四格并行；每轮运行都能直接追加指令
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { api, onEngine, type FollowupMode, type RunDetail, type RunEvent, type RunRow } from '../lib/api';
import { useData } from '../lib/data';
import { useStartTask } from '../lib/start';
import { Icon } from '../lib/Icon';
import { AgentTag, Empty, Inline, Ripple, RunStatusTag, fmtCost, fmtDur, fmtTok, parseTs, runActive, runSeconds, useNow, useUi, runTitle } from '../lib/ui';

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
      return sys(tr('worktree {0} · 分支 {1}', str(p.path), str(p.branch)));
    case 'spawned':
      return tool(tr('启动 Agent'), `pid ${str(p.pid)}`, str(p.command));
    case 'session_started':
      return sys(tr('会话 {0} · 模型 {1}', str(p.session_id).slice(0, 8), str(p.model) || tr('默认')));
    case 'text':
      return (
        <div className="logi msg">
          {time}
          <div className="bubble">
            <Inline text={str(p.text)} />
          </div>
        </div>
      );
    case 'steer':
      return (
        <div className="logi msg me">
          {time}
          <div className="bubble">
            <small>{tr('追加指令')}</small>
            <Inline text={str(p.text)} />
          </div>
        </div>
      );
    case 'followup':
      return sys(p.mode === 'restart' ? tr('追加指令（打断后重开）：{0}', str(p.text)) : tr('追加指令已排队，这一轮结束后接着做：{0}', str(p.text)));
    case 'thinking':
      return tool(tr('思考'), '', str(p.text));
    case 'tool_call':
      return tool(str(p.name) || tr('工具'), str(p.input).slice(0, 120), JSON.stringify(p.input, null, 2), !!p.danger, p.danger ? tr('危险：{0}', str(p.danger)) : undefined);
    case 'guard':
      return sys(str(p.message), true);
    case 'tool_result':
      return tool(tr('工具结果'), '', str(p.output), p.is_error === true, p.is_error === true ? tr('出错') : undefined);
    case 'command':
      if (p.done !== true) return tool(tr('运行'), str(p.command), p.danger ? str(p.command) : '', !!p.danger, p.danger ? tr('危险：{0}', str(p.danger)) : undefined);
      return tool(tr('运行'), str(p.command), str(p.output), p.exit_code !== 0 && p.exit_code != null, p.exit_code == null ? undefined : tr('退出码 {0}', str(p.exit_code)));
    case 'file_change':
      return tool({ add: tr('新建'), delete: tr('删除') }[str(p.kind)] ?? tr('修改'), str(p.path), '');
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
      return sys(tr('已提交 {0}：{1}', str(p.commit).slice(0, 8), str(p.message)));
    case 'check_started':
      return sys(tr('开始检查 {0}：{1}', str(p.name), str(p.command)));
    case 'check_finished':
      return tool(
        tr('检查 {0}', str(p.name)),
        str(p.command),
        str(p.tail),
        p.exit_code !== 0,
        p.timed_out === true ? tr('超时') : tr('退出码 {0}', str(p.exit_code)),
      );
    case 'status':
      return sys(
        { finalizing: tr('Agent 已结束，正在提交并运行检查'), succeeded: tr('运行完成'), failed: tr('运行失败'), stopped: tr('已停止'), orphaned: tr('观澜退出时这次运行被中断') }[
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
        <span className="tag line">{tr('第')} {r.number} {tr('次')}</span>
        <span className="grow" />
        <span className={`mono ${active ? 't-live' : 'dim'}`}>{r.status === 'queued' ? tr('排队') : fmtDur(runSeconds(r, now))}</span>
      </div>
      <b className="ell">{runTitle(r)}</b>
      <div className="row sub">
        {active ? <Ripple /> : <span className={`dot st-${r.status === 'succeeded' ? 'done' : r.status}`} />}
        <span className="grow ell">{r.error ?? (r.task_id ? r.summary : null) ?? ''}</span>
        <span className="mono">{fmtTok(r.input_tokens + r.cache_tokens + r.output_tokens)}</span>
      </div>
    </a>
  );
}

const MODES: { id: FollowupMode; label: string; hint: string }[] = [
  { id: 'steer', label: tr('引导'), hint: tr('直接送进运行中的会话，Agent 在当前这一轮里接着处理。') },
  { id: 'restart', label: tr('打断重开'), hint: tr('停掉这一轮，再续接会话、带着这条指令重新开始。') },
  { id: 'queue', label: tr('排队'), hint: tr('等这一轮结束，再续接会话、带着这条指令开始下一轮。') },
];

const OUTCOMES: Record<string, string> = {
  steered: tr('已送达运行中的 Agent'),
  queued: tr('已排队，这一轮结束后接着做'),
  restarting: tr('正在停止，结束后带着指令重新开始'),
  started: tr('已开始新的一轮'),
};

/** 追加指令输入框：运行中可以引导 / 打断重开 / 排队；已结束时续接会话开始新的一轮。 */
function Composer({ r, compact, onStarted }: { r: RunRow; compact?: boolean; onStarted: (id: string) => void }) {
  const ui = useUi();
  const { reload } = useData();
  const startTask = useStartTask();
  const live = runActive(r.status) || r.status === 'queued';
  const [text, setText] = useState('');
  const [mode, setMode] = useState<FollowupMode>('steer');
  const [canSteer, setCanSteer] = useState(false);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!live) return setCanSteer(false);
    let stale = false;
    const check = () => api.runCanSteer(r.id).then((v) => !stale && setCanSteer(v), () => {});
    void check();
    const t = window.setInterval(check, 3000);
    return () => {
      stale = true;
      window.clearInterval(t);
    };
  }, [r.id, live]);

  const effective: FollowupMode = mode === 'steer' && !canSteer ? 'queue' : mode;
  const hint = !live
    ? tr('以续接原会话 + 新指令的方式开始一次新的运行。')
    : mode === 'steer' && !canSteer
      ? tr('这轮运行此刻不能直接引导，将排到这一轮之后。')
      : MODES.find((m) => m.id === effective)?.hint;

  const send = async () => {
    const v = text.trim();
    if (!v || busy || !r.task_id) return;
    setBusy(true);
    try {
      if (!live) {
        // 和子任务页走同一个入口：full_auto 每次都要重新确认
        const run = await startTask({ id: r.task_id }, v);
        if (run) {
          setText('');
          onStarted(run.id);
        }
        return;
      }
      const res = await ui.attempt(api.runFollowup(r.id, v, effective));
      if (!res) return;
      setText('');
      ui.toast(OUTCOMES[res.outcome] ?? res.outcome);
      if (res.outcome === 'started' && res.run_id) onStarted(res.run_id);
      void reload();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className={`composer ${compact ? 'compact' : ''}`}>
      <div className="cm-top">
        {live && (
          <div className="seg">
            {MODES.map((m) => (
              <button key={m.id} className={effective === m.id ? 'on' : ''} disabled={m.id === 'steer' && !canSteer} title={m.hint} onClick={() => setMode(m.id)}>
                {m.label}
              </button>
            ))}
          </div>
        )}
        <span className="dim ell">{hint}</span>
      </div>
      <div className="cm-row">
        <textarea
          rows={compact ? 1 : 2}
          value={text}
          placeholder={live ? tr('给 Agent 追加指令（Ctrl/⌘ + Enter 发送）') : tr('例如：把即将过期的角标改成橙色')}
          onChange={(e) => setText(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter' && (e.metaKey || e.ctrlKey)) {
              e.preventDefault();
              void send();
            }
          }}
        />
        <button className="btn primary" disabled={!text.trim() || busy} onClick={() => void send()}>
          <Icon name="send" size={14} />
          {tr('发送')}
        </button>
      </div>
    </div>
  );
}

/** 一轮运行的头部、时间线、追加指令和状态条；四格模式下用紧凑版。 */
function RunView({ runId, compact, picker, onOpen }: { runId: string; compact?: boolean; picker?: ReactNode; onOpen?: (id: string) => void }) {
  const { runs, providers, reload } = useData();
  const ui = useUi();
  const startTask = useStartTask();
  const [detail, setDetail] = useState<RunDetail | null>(null);
  // 列表只装最近的运行；链接指向更早的一轮时，用单独取回的那一轮，不拿别的运行顶替
  const current: RunRow | undefined = runs.find((r) => r.id === runId) ?? (detail?.run.id === runId ? detail.run : undefined);
  const [raw, setRaw] = useState(false);
  const logRef = useRef<HTMLDivElement>(null);
  const stick = useRef(true);
  const now = useNow(!!current && runActive(current.status));
  const open = (id: string) => (onOpen ? onOpen(id) : (window.location.hash = `#/runs/${id}`));

  // 先取全量，再把引擎推来的事件按序号接上；序号不连续就重新取一次
  useEffect(() => {
    setDetail(null);
    stick.current = true;
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
      <section className={`rmain ${compact ? 'compact' : ''}`}>
        {picker && <header className="rhead">{picker}</header>}
        <Empty icon="runs" title={tr('正在读取这轮运行')} body={tr('如果一直没有出现，这轮运行可能已经随工单删除。')} />
      </section>
    );

  const r = { ...current, ...(detail && detail.run.id === current.id ? { prompt: detail.run.prompt } : {}) };
  const active = runActive(r.status);
  const start = parseTs(r.started_at ?? r.queued_at);

  const stop = async () => {
    if (r.status !== 'queued' && !(await ui.confirm(tr('停止运行'), tr('会结束 Agent 的整个进程树。已做的改动留在 worktree 里，之后可以恢复原会话继续。'), tr('停止'), true))) return;
    if ((await ui.attempt(api.runStop(r.id), r.status === 'queued' ? tr('已移出队列') : tr('正在停止…'))) !== undefined) void reload();
  };
  const resume = async () => {
    if (!r.task_id) return;
    const run = await startTask({ id: r.task_id });
    if (run) open(run.id);
  };

  return (
    <section className={`rmain ${compact ? 'compact' : ''}`}>
      <header className="rhead">
        <div className="grow">
          <div className="row">
            {picker}
            {r.ticket_key && (
              <a className="key lnk" href={`#/ticket/${r.ticket_key}/tasks`}>
                {r.ticket_key}
              </a>
            )}
            {compact && <AgentTag agent={r.agent} glyphOnly />}
            <span className="tag line">{tr('第')} {r.number} {tr('次')}</span>
            <RunStatusTag status={r.status} />
            {!compact && r.permission && (
              <span className="tag line">
                <Icon name="shield" size={11} />
                {r.permission}
              </span>
            )}
            {!compact && r.resume_session_id && <span className="dim">{tr('续接会话')} {r.resume_session_id.slice(0, 8)}</span>}
          </div>
          <h2 className="ell">{runTitle(r)}</h2>
        </div>
        {active || r.status === 'queued' ? (
          <button className={`btn ${active ? 'danger' : ''} ${compact ? 'sm' : ''}`} disabled={r.status === 'stopping'} onClick={() => void stop()} title={tr('停止')}>
            <Icon name="stop" size={14} />
            {!compact && (r.status === 'queued' ? tr('取消排队') : tr('停止'))}
          </button>
        ) : !r.task_id ? (
          // 预审、拆分、智能创建这类工单级运行没有会话可续接，只能回工单重新发起
          r.ticket_key &&
          ['stopped', 'failed', 'orphaned'].includes(r.status) && (
            <a className="btn" href={`#/ticket/${r.ticket_key}`}>
              <Icon name="resume" size={14} />
              {!compact && tr('回工单重新发起')}
            </a>
          )
        ) : (
          ['stopped', 'failed', 'orphaned'].includes(r.status) && (
            <button className={`btn ${compact ? 'sm' : ''}`} onClick={() => void resume()} title={tr('恢复')}>
              <Icon name="resume" size={14} />
              {!compact && tr('恢复')}
            </button>
          )
        )}
        {compact ? (
          <a className="btn ghost sm" href={`#/runs/${r.id}`} title={tr('在单栏中打开')} onClick={() => localStorage.setItem(LAYOUT_KEY, 'single')}>
            <Icon name="ext" size={13} />
          </a>
        ) : (
          <button className={`btn ghost ${raw ? 'on' : ''}`} onClick={() => setRaw(!raw)}>
            <Icon name="code" size={14} />
            {raw ? tr('时间线') : tr('原始事件')}
          </button>
        )}
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
            <div className="bubble">{tr('等待空闲的并发名额。')}</div>
          </div>
        )}
        {detail?.events
          // Agent 的最后一句话通常就是总结，不重复显示
          .filter((e, i, all) => raw || e.type !== 'finished' || !all.slice(0, i).some((x) => x.type === 'text' && x.payload.text === e.payload.summary))
          .map((e) => <LogItem key={e.seq} e={e} start={start} raw={raw} />)}
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
              <span>{r.status === 'finalizing' ? tr('提交并运行检查') : r.status === 'stopping' ? tr('正在停止') : tr('Agent 工作中')}</span>
            </div>
          </div>
        )}
      </div>
      {r.task_id && <Composer key={r.id} r={r} compact={compact} onStarted={open} />}
      <footer className="rfoot mono">
        <span>
          <AgentTag agent={r.agent} />
        </span>
        <span>
          <i>{tr('模型')}</i>
          {r.model ?? tr('默认')}
        </span>
        {!compact && (
          <span>
            <i>Provider</i>
            {providers.find((p) => p.id === r.provider_id)?.name ?? tr('本机登录态')}
          </span>
        )}
        {!compact && (
          <span title={r.agent_session_id ?? undefined}>
            <i>session</i>
            {r.agent_session_id ? `${r.agent_session_id.slice(0, 8)}…` : '—'}
          </span>
        )}
        <span className="grow" />
        <span>
          <i>{tr('耗时')}</i>
          <b>{fmtDur(runSeconds(r, now))}</b>
        </span>
        <span>
          <i>{tr('输入')}</i>
          <b>{fmtTok(r.input_tokens)}</b>
        </span>
        {!compact && r.cache_tokens > 0 && (
          <span title={tr('从缓存读取的输入，不计入「输入」')}>
            <i>{tr('缓存读取')}</i>
            <b>{fmtTok(r.cache_tokens)}</b>
          </span>
        )}
        <span>
          <i>{tr('输出')}</i>
          <b>{fmtTok(r.output_tokens)}</b>
        </span>
        <span title={r.cost_source === 'estimated' ? tr('按 Provider 价格估算') : r.cost_source === 'reported' ? tr('Agent 自报') : undefined}>
          <i>{tr('费用')}{r.cost_source === 'estimated' ? tr('（估）') : ''}</i>
          <b>{fmtCost(r.cost_usd)}</b>
        </span>
      </footer>
    </section>
  );
}

type Layout = 'single' | 'grid';
const LAYOUT_KEY = 'tw-runs-layout';
const PANES_KEY = 'tw-runs-panes';

function readPanes(): (string | null)[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(PANES_KEY) ?? '[]');
    return Array.isArray(v) ? v.slice(0, 4).map((x) => (typeof x === 'string' ? x : null)) : [];
  } catch {
    return [];
  }
}

function LayoutSwitch({ layout, set }: { layout: Layout; set: (l: Layout) => void }) {
  return (
    <div className="seg">
      <button className={layout === 'single' ? 'on' : ''} title={tr('单栏')} onClick={() => set('single')}>
        <Icon name="pane" size={13} />
        {tr('单栏')}
      </button>
      <button className={layout === 'grid' ? 'on' : ''} title={tr('四格并行')} onClick={() => set('grid')}>
        <Icon name="grid" size={13} />
        {tr('四格')}
      </button>
    </div>
  );
}

/** 四格并行：没手动选的窗格自动填进行中的运行，其次是最近的。 */
function RunGrid({ focus, switcher }: { focus?: string; switcher: ReactNode }) {
  const { runs } = useData();
  const [picked, setPicked] = useState(readPanes);
  const live = runs.filter((r) => runActive(r.status) || r.status === 'queued');
  const panes = useMemo(() => {
    const out = [0, 1, 2, 3].map((i) => picked[i] ?? null);
    if (focus && !out.includes(focus)) {
      const k = out.indexOf(null);
      out[k >= 0 ? k : 0] = focus;
    }
    const pool = [...live, ...runs.filter((r) => !live.includes(r))].map((r) => r.id);
    for (let i = 0; i < 4; i++) if (!out[i]) out[i] = pool.find((x) => !out.includes(x)) ?? null;
    return out;
  }, [picked, runs, live, focus]);
  const pick = (i: number, v: string | null) => {
    const next = [0, 1, 2, 3].map((k) => picked[k] ?? null);
    next[i] = v;
    localStorage.setItem(PANES_KEY, JSON.stringify(next));
    setPicked(next);
  };
  const picker = (i: number) => (
    <select className="g4-pick" value={picked[i] ?? ''} title={tr('选择这一格显示的运行')} onChange={(e) => pick(i, e.target.value || null)}>
      <option value="">{tr('自动')}</option>
      {runs.map((r) => (
        <option key={r.id} value={r.id}>
          {`${r.ticket_key ?? '—'} · ${tr('第')}${r.number}${tr('次')} · ${runTitle(r)}`}
        </option>
      ))}
    </select>
  );
  return (
    <div className="page flush runs grid4">
      <div className="g4-bar">
        {switcher}
        <b>{tr('并行运行')}</b>
        <span className="tag line">
          {live.length} {tr('进行中')}
        </span>
        <span className="grow" />
        <span className="dim">{tr('每一格可以单独选择一轮运行，并直接给它追加指令。')}</span>
      </div>
      <div className="g4">
        {panes.map((pid, i) => (
          <div className="g4-pane" key={i}>
            {pid ? (
              <RunView key={pid} runId={pid} compact picker={picker(i)} onOpen={(id) => pick(i, id)} />
            ) : (
              <section className="rmain compact">
                <header className="rhead">{picker(i)}</header>
                <Empty icon="runs" title={tr('空闲窗格')} body={tr('在工单的「子任务」页开始一个子任务，运行会出现在这里。')} />
              </section>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}

export function RunsPage({ id }: { id?: string }) {
  const { runs } = useData();
  const [layout, setLayoutState] = useState<Layout>(() => (localStorage.getItem(LAYOUT_KEY) === 'grid' ? 'grid' : 'single'));
  const setLayout = (l: Layout) => {
    localStorage.setItem(LAYOUT_KEY, l);
    setLayoutState(l);
  };
  const now = useNow(runs.some((r) => runActive(r.status)));
  const switcher = <LayoutSwitch layout={layout} set={setLayout} />;
  if (layout === 'grid') return <RunGrid focus={id} switcher={switcher} />;

  const runId = id ?? runs[0]?.id;
  if (!runId)
    return (
      <div className="page">
        <div className="card">
          <Empty icon="runs" title={tr('还没有运行')} body={tr('在工单的「子任务」页开始一个子任务，运行会出现在这里。')} />
        </div>
      </div>
    );

  const act = runs.filter((r) => runActive(r.status) || r.status === 'queued');
  const hist = runs.filter((r) => !act.includes(r));
  return (
    <div className="page flush runs">
      <aside className="rlist">
        <div className="rl-top">{switcher}</div>
        <div className="rl-h">
          <span>{tr('进行中')}</span>
          <i>{act.length}</i>
        </div>
        {act.map((x) => (
          <RunItem key={x.id} r={x} on={x.id === runId} now={now} />
        ))}
        <div className="rl-h">
          <span>{tr('历史')}</span>
          <i>{hist.length}</i>
        </div>
        {hist.map((x) => (
          <RunItem key={x.id} r={x} on={x.id === runId} now={now} />
        ))}
      </aside>
      <RunView key={runId} runId={runId} />
    </div>
  );
}
