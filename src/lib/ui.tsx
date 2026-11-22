// 共享常量与小组件；类名沿用 prototype-v2 的样式表。
import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from 'react';
import { Icon } from './Icon';
import type { IconName } from './icons';
import { parseImages, PrdImage, type ImageActions } from './images';
import { onEngine, type EngineEvent, type Priority, type RunStatus, type TaskStatus, type TicketStatus, type TicketType } from './api';

export const STAGES: { id: TicketStatus; name: string; num: string; hint: string; mine?: boolean }[] = [
  { id: 'draft', name: '草稿', num: '一', hint: '随意编辑' },
  { id: 'ai_review', name: 'AI 预审', num: '二', hint: 'Agent 读 PRD' },
  { id: 'human_review', name: '人工审核', num: '三', hint: '等你处理问题', mine: true },
  { id: 'finalized', name: '已定稿', num: '四', hint: 'PRD 已锁定' },
  { id: 'developing', name: '开发中', num: '五', hint: 'Agent 执行' },
  { id: 'acceptance', name: '验收中', num: '六', hint: '等你看 Diff', mine: true },
  { id: 'done', name: '已完成', num: '七', hint: '已合并' },
];

export const STAGE_NAME: Record<TicketStatus, string> = {
  draft: '草稿',
  ai_review: 'AI 预审',
  human_review: '人工审核',
  finalized: '已定稿',
  developing: '开发中',
  acceptance: '验收中',
  done: '已完成',
  cancelled: '已取消',
};

export function stageTone(s: TicketStatus): string {
  if (s === 'human_review' || s === 'acceptance') return 'cin';
  if (s === 'done') return 'moss';
  if (s === 'developing' || s === 'ai_review') return 'tide';
  return '';
}

export const TYPES: Record<TicketType, { ch: string; cls: string; name: string }> = {
  feature_backend: { ch: '后', cls: 'be', name: '后端需求' },
  feature_frontend: { ch: '前', cls: 'fe', name: '前端需求' },
  feature_mixed: { ch: '混', cls: 'mix', name: '前后端混合' },
  bug: { ch: '虫', cls: 'bug', name: 'Bug' },
  chore: { ch: '杂', cls: 'chore', name: '杂项' },
};

export const PRIO: Record<Priority, string> = { urgent: '紧急', high: '高', medium: '中', low: '低' };

export const AGENTS: Record<string, { name: string; g: string; short: string }> = {
  claude_code: { name: 'Claude Code', g: 'cc', short: 'CC' },
  codex: { name: 'Codex', g: 'cx', short: 'CX' },
  mock: { name: '演示 Agent', g: 'ds', short: '演' },
};

export const TASK_KIND: Record<string, { ch: string; cls: string; name: string; short: string }> = {
  backend: { ch: '后', cls: 'be', name: '后端', short: 'BE' },
  frontend: { ch: '前', cls: 'fe', name: '前端', short: 'FE' },
  bug: { ch: '虫', cls: 'bug', name: 'Bug', short: 'BUG' },
  doc: { ch: '文', cls: 'chore', name: '文档', short: 'DOC' },
};

export const TASK_STATUS: Record<TaskStatus, [string, string]> = {
  pending: ['待开始', 'dim'],
  blocked: ['等依赖', 'dim'],
  queued: ['排队', 'dim'],
  running: ['运行中', 'tide'],
  review: ['待验收', 'cin'],
  changes_requested: ['已打回', 'amber'],
  accepted: ['已通过', 'moss'],
  merged: ['已合并', 'moss'],
  failed: ['失败', 'cin'],
  stopped: ['已停止', 'amber'],
  cancelled: ['已取消', 'dim'],
};

export const RUN_STATUS: Record<RunStatus, [string, string]> = {
  queued: ['排队', ''],
  starting: ['启动中', 'tide'],
  running: ['运行中', 'tide'],
  finalizing: ['收尾中', 'tide'],
  stopping: ['停止中', 'amber'],
  succeeded: ['完成', 'moss'],
  failed: ['失败', 'cin'],
  stopped: ['已停止', 'amber'],
  orphaned: ['中断', 'amber'],
};

export const PERMISSIONS = ['read_only', 'workspace_write', 'full_auto'] as const;

/** 运行的用途。预审 / 拆分 / 影响分析是工单级的，不属于某个子任务。 */
export const PURPOSE: Record<string, string> = {
  dev: '开发',
  rework: '返工',
  review: 'AI 预审',
  split: '拆分子任务',
  impact: '变更影响分析',
};

/** 运行在列表里显示的名字：子任务标题；工单级运行显示「用途 · 工单标题」。 */
export function runTitle(r: { task_id: string | null; purpose: string; task_title?: string | null; ticket_title?: string | null }): string {
  if (r.task_id) return r.task_title ?? r.ticket_title ?? '已删除的子任务';
  return `${PURPOSE[r.purpose] ?? r.purpose} · ${r.ticket_title ?? '已删除的工单'}`;
}

export const runActive = (s: RunStatus) => ['starting', 'running', 'finalizing', 'stopping'].includes(s);

export function Stamp({ type, lg }: { type: TicketType; lg?: boolean }) {
  const t = TYPES[type] ?? TYPES.chore;
  return (
    <span className={`stamp ${t.cls}${lg ? ' lg' : ''}`} title={t.name}>
      {t.ch}
    </span>
  );
}

export function KindStamp({ kind, lg }: { kind: string; lg?: boolean }) {
  const k = TASK_KIND[kind] ?? TASK_KIND.doc;
  return (
    <span className={`stamp ${k.cls}${lg ? ' lg' : ''}`} title={`${k.name}子任务`}>
      {k.ch}
    </span>
  );
}

export function Prio({ p }: { p: Priority }) {
  return (
    <span className={`prio ${p}`}>
      <i />
      {PRIO[p] ?? p}
    </span>
  );
}

export function AgentTag({ agent, glyphOnly }: { agent: string; glyphOnly?: boolean }) {
  const g = AGENTS[agent] ?? { name: agent, g: 'ds', short: agent.slice(0, 2).toUpperCase() };
  return (
    <span className="agent" title={g.name}>
      <span className={`glyph ${g.g}`}>{g.short}</span>
      {!glyphOnly && g.name}
    </span>
  );
}

export function Ripple() {
  return (
    <span className="ripple">
      <i />
      <i />
      <i />
      <i />
    </span>
  );
}

export function Tag({ tone = '', icon, children }: { tone?: string; icon?: IconName; children: ReactNode }) {
  return (
    <span className={`tag ${tone}`}>
      {icon && <Icon name={icon} size={11} />}
      {children}
    </span>
  );
}

export function TaskStatusTag({ status }: { status: TaskStatus }) {
  const [name, tone] = TASK_STATUS[status] ?? [status, ''];
  return (
    <span className={`tag ${tone === 'dim' ? '' : tone}`}>
      {status === 'running' && <Ripple />}
      {name}
    </span>
  );
}

export function RunStatusTag({ status }: { status: RunStatus }) {
  const [name, tone] = RUN_STATUS[status] ?? [status, ''];
  return (
    <span className={`tag ${tone}`}>
      {runActive(status) && <Ripple />}
      {name}
    </span>
  );
}

export function Empty({ icon, title, body, children }: { icon: IconName; title: string; body?: string; children?: ReactNode }) {
  return (
    <div className="empty">
      <Icon name={icon} size={28} />
      <b>{title}</b>
      {body && <p>{body}</p>}
      {children}
    </div>
  );
}

export function Seg<T extends string>({
  value,
  options,
  onChange,
  className = '',
}: {
  value: T;
  options: readonly (readonly [T, ReactNode])[];
  onChange: (v: T) => void;
  className?: string;
}) {
  return (
    <div className={`seg ${className}`}>
      {options.map(([k, label]) => (
        <button key={k} type="button" className={value === k ? 'on' : ''} onClick={() => onChange(k)}>
          {label}
        </button>
      ))}
    </div>
  );
}

export function Switch({ on, onChange, label }: { on: boolean; onChange: (v: boolean) => void; label?: string }) {
  return (
    <span
      className={`switch ${on ? 'on' : ''}`}
      role="switch"
      aria-checked={on}
      aria-label={label}
      tabIndex={0}
      onClick={() => onChange(!on)}
      onKeyDown={(e) => {
        if (e.key === ' ' || e.key === 'Enter') {
          e.preventDefault();
          onChange(!on);
        }
      }}
    />
  );
}

export function Field({ label, hint, children }: { label: string; hint?: ReactNode; children: ReactNode }) {
  return (
    <div className="field">
      <label>
        {label}
        {hint && <small> {hint}</small>}
      </label>
      {children}
    </div>
  );
}

// ---- 格式化 ----

export function fmtDur(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const x = s % 60;
  const mm = String(m).padStart(2, '0');
  const ss = String(x).padStart(2, '0');
  return h ? `${h}:${mm}:${ss}` : `${mm}:${ss}`;
}

export function fmtTok(n: number): string {
  return n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}k` : String(n);
}

export function fmtCost(v: number | null | undefined): string {
  return v == null ? '—' : `$ ${v.toFixed(v > 0 && v < 0.01 ? 4 : 2)}`;
}

/** 库里的时间是 UTC（`YYYY-MM-DD HH:MM:SS` 或 ISO）。 */
export function parseTs(ts: string | null | undefined): number | null {
  if (!ts) return null;
  const iso = ts.includes('T') ? ts : `${ts.replace(' ', 'T')}Z`;
  const n = Date.parse(iso);
  return Number.isNaN(n) ? null : n;
}

export function fmtAgo(ts: string | null | undefined, now = Date.now()): string {
  const t = parseTs(ts);
  if (t == null) return '—';
  const s = Math.max(0, (now - t) / 1000);
  if (s < 60) return '刚刚';
  if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
  if (s < 86400) return `${Math.floor(s / 3600)} 小时前`;
  if (s < 86400 * 7) return `${Math.floor(s / 86400)} 天前`;
  const d = new Date(t);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, '0')}-${String(d.getDate()).padStart(2, '0')}`;
}

export function runSeconds(r: { started_at: string | null; finished_at: string | null }, now = Date.now()): number {
  const a = parseTs(r.started_at);
  if (a == null) return 0;
  return ((parseTs(r.finished_at) ?? now) - a) / 1000;
}

// ---- 极简 Markdown：标题、列表、段落、行内 code / 加粗 ----

function inline(s: string, key: number): ReactNode {
  const parts = s.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).filter(Boolean);
  return (
    <span key={key}>
      {parts.map((p, i) =>
        p.startsWith('`') && p.endsWith('`') && p.length > 2 ? (
          <code key={i}>{p.slice(1, -1)}</code>
        ) : p.startsWith('**') && p.endsWith('**') && p.length > 4 ? (
          <strong key={i}>{p.slice(2, -2)}</strong>
        ) : (
          p
        ),
      )}
    </span>
  );
}

export function Inline({ text }: { text: string }) {
  return <>{inline(text, 0)}</>;
}

/** `prev`：上一版正文，用来标出本版新增的行。 */
const cells = (l: string) =>
  l
    .trim()
    .replace(/^\||\|$/g, '')
    .split(/(?<!\\)\|/)
    .map((c) => c.trim().replace(/\\\|/g, '|'));

/** `images`：传了就在每张图上显示改效力 / 替换 / 移除的按钮。 */
export function Md({ src, prev, images }: { src: string; prev?: string | null; images?: ImageActions }) {
  const blocks = useMemo(() => {
    const old = prev == null ? null : new Set(prev.split('\n'));
    const out: ReactNode[] = [];
    let list: ReactNode[] = [];
    let table: string[] = [];
    let code: string[] | null = null;
    const flush = () => {
      if (list.length) {
        out.push(<ul key={`u${out.length}`}>{list}</ul>);
        list = [];
      }
      if (table.length) {
        const rows = table.filter((r) => !/^\s*\|?[\s:|-]+\|?\s*$/.test(r)).map(cells);
        out.push(
          <table key={`t${out.length}`} className="mdtable">
            <thead>
              <tr>{rows[0]?.map((c, i) => <th key={i}>{inline(c, 0)}</th>)}</tr>
            </thead>
            <tbody>
              {rows.slice(1).map((r, i) => (
                <tr key={i}>
                  {r.map((c, j) => (
                    <td key={j}>{c.split('<br>').map((x, k) => <div key={k}>{inline(x, 0)}</div>)}</td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>,
        );
        table = [];
      }
    };
    src.split('\n').forEach((l, i) => {
      if (l.trimStart().startsWith('```')) {
        if (code) {
          out.push(
            <pre key={i} className="plain boxed">
              {code.join('\n')}
            </pre>,
          );
          code = null;
        } else {
          flush();
          code = [];
        }
        return;
      }
      if (code) {
        code.push(l);
        return;
      }
      const cls = old && l.trim() && !old.has(l) ? 'ins' : undefined;
      if (l.trimStart().startsWith('|')) {
        if (list.length) flush();
        table.push(l);
        return;
      }
      if (table.length) flush();
      const imgs = parseImages(l);
      if (imgs.length && !l.replace(/!\[[^\]\n]*\]\(attachment:[^)]*\)/g, '').trim()) {
        flush();
        imgs.forEach((img, k) => out.push(<PrdImage key={`${i}-${k}`} img={img} actions={images} added={!!cls} />));
        return;
      }
      const item = /^\s*(?:[-*]|\d+[.)])\s+(.*)$/.exec(l);
      if (item) {
        list.push(
          <li key={i} className={cls}>
            {inline(item[1], 0)}
          </li>,
        );
        return;
      }
      flush();
      if (l.startsWith('### ')) out.push(<h4 key={i} className={cls}>{inline(l.slice(4), 0)}</h4>);
      else if (l.startsWith('## ')) out.push(<h3 key={i} className={cls}>{inline(l.slice(3), 0)}</h3>);
      else if (l.startsWith('# ')) out.push(<h2 key={i} className={cls}>{inline(l.slice(2), 0)}</h2>);
      else if (l.trim()) out.push(<p key={i} className={cls}>{inline(l, 0)}</p>);
    });
    if (code) out.push(<pre key="tail" className="plain boxed">{(code as string[]).join('\n')}</pre>);
    flush();
    return out;
  }, [src, prev, images]);
  return <>{blocks}</>;
}

// ---- 提示、确认框、输入框 ----

interface Dialog {
  title: string;
  body?: ReactNode;
  ok: string;
  danger?: boolean;
  prompt?: { placeholder: string; required?: boolean; initial?: string };
  resolve: (v: string | null) => void;
}

interface UiApi {
  toast: (msg: string) => void;
  /** 出错时提示并返回 undefined。 */
  attempt: <T>(p: Promise<T>, okMsg?: string) => Promise<T | undefined>;
  confirm: (title: string, body: ReactNode, ok: string, danger?: boolean) => Promise<boolean>;
  prompt: (title: string, body: ReactNode, placeholder: string, ok?: string, required?: boolean, initial?: string) => Promise<string | null>;
}

const UiContext = createContext<UiApi | null>(null);

export function useUi(): UiApi {
  const ui = useContext(UiContext);
  if (!ui) throw new Error('UiProvider 未挂载');
  return ui;
}

export function UiProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<{ id: number; msg: string }[]>([]);
  const [dialog, setDialog] = useState<Dialog | null>(null);
  const [text, setText] = useState('');
  const seq = useRef(0);

  const toast = useCallback((msg: string) => {
    const id = ++seq.current;
    setToasts((t) => [...t.slice(-1), { id, msg }]);
    setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 4200);
  }, []);

  const api = useMemo<UiApi>(
    () => ({
      toast,
      attempt: async (p, okMsg) => {
        try {
          const v = await p;
          if (okMsg) toast(okMsg);
          return v;
        } catch (e) {
          toast(String(e));
          return undefined;
        }
      },
      confirm: (title, body, ok, danger) =>
        new Promise((resolve) => setDialog({ title, body, ok, danger, resolve: (v) => resolve(v !== null) })),
      prompt: (title, body, placeholder, ok = '确定', required = false, initial = '') =>
        new Promise((resolve) => {
          setText(initial);
          setDialog({ title, body, ok, prompt: { placeholder, required }, resolve });
        }),
    }),
    [toast],
  );

  const close = (v: string | null) => {
    dialog?.resolve(v);
    setDialog(null);
  };

  useEffect(() => {
    if (!dialog) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') close(null);
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  return (
    <UiContext.Provider value={api}>
      {children}
      <div id="overlay" onMouseDown={(e) => e.target === e.currentTarget && close(null)}>
        {dialog && (
          <div className="pop" style={{ width: dialog.prompt ? 480 : 440 }} role="dialog" aria-modal="true" aria-label={dialog.title}>
            <div className="pop-h">
              <h2>{dialog.title}</h2>
              {dialog.body && <p>{dialog.body}</p>}
            </div>
            <div className="pop-b">
              {dialog.prompt && (
                <textarea
                  className="textarea"
                  rows={4}
                  autoFocus
                  placeholder={dialog.prompt.placeholder}
                  value={text}
                  onChange={(e) => setText(e.target.value)}
                />
              )}
            </div>
            <div className="pop-f">
              <button className="btn" onClick={() => close(null)}>
                取消
              </button>
              <button
                className={`btn ${dialog.danger ? 'danger' : 'primary'}`}
                autoFocus={!dialog.prompt}
                disabled={!!dialog.prompt?.required && !text.trim()}
                onClick={() => close(dialog.prompt ? text.trim() : '')}
              >
                {dialog.ok}
              </button>
            </div>
          </div>
        )}
      </div>
      <div id="toasts">
        {toasts.map((t) => (
          <div key={t.id} className="toast">
            {t.msg}
          </div>
        ))}
      </div>
    </UiContext.Provider>
  );
}

/** 自定义内容的弹层（定稿、手动修改正文等）。点遮罩或按 Esc 关闭。 */
export function Modal({
  title,
  hint,
  width = 640,
  onClose,
  children,
  footer,
}: {
  title: string;
  hint?: ReactNode;
  width?: number;
  onClose: () => void;
  children: ReactNode;
  footer: ReactNode;
}) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') onClose();
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
  return (
    <div className="modal-bg" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="pop" style={{ width }} role="dialog" aria-modal="true" aria-label={title}>
        <div className="pop-h">
          <h2>{title}</h2>
          {hint && <p>{hint}</p>}
        </div>
        <div className="pop-b">{children}</div>
        <div className="pop-f">{footer}</div>
      </div>
    </div>
  );
}

// ---- 数据加载 ----

/** 加载数据，并在引擎说「有变化」时重新加载。`deps` 变化时也重载。 */
export function useLoad<T>(load: () => Promise<T>, deps: unknown[] = [], refreshOn: (ev: EngineEvent) => boolean = (ev) => ev.type === 'changed') {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const loadRef = useRef(load);
  loadRef.current = load;
  const refreshRef = useRef(refreshOn);
  refreshRef.current = refreshOn;
  const gen = useRef(0);

  const reload = useCallback(() => {
    const mine = ++gen.current;
    return loadRef.current().then(
      (v) => {
        if (mine === gen.current) {
          setData(v);
          setError(null);
        }
      },
      (e) => {
        if (mine === gen.current) setError(String(e));
      },
    );
  }, []);

  useEffect(() => {
    void reload();
    let timer: ReturnType<typeof setTimeout> | null = null;
    const off = onEngine((ev) => {
      if (!refreshRef.current(ev) || timer) return;
      // 一次运行会连发很多「有变化」，合并成每 150ms 至多刷新一次
      timer = setTimeout(() => {
        timer = null;
        void reload();
      }, 150);
    });
    return () => {
      off();
      if (timer) clearTimeout(timer);
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, deps);

  return { data, error, reload, setData };
}

/** 每秒跳一次，给「运行中」的计时用。 */
export function useNow(active: boolean): number {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, [active]);
  return now;
}

export function go(hash: string) {
  window.location.hash = hash;
}
