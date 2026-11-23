import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import { Icon } from './lib/Icon';
import type { IconName } from './lib/icons';
import { api, type Detection, type SystemInfo, type TicketSummary } from './lib/api';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { lang, tr } from './lib/i18n';
import { DataProvider, useData } from './lib/data';
import { STAGE_NAME, UiProvider, fmtAgo, fmtCost, fmtDur, go, runActive, runSeconds, runTitle, switchLang, useNow, useUi } from './lib/ui';
import { BoardPage } from './pages/Board';
import { TicketsPage } from './pages/Tickets';
import { NewTicketPage } from './pages/NewTicket';
import { TicketPage } from './pages/Ticket';
import { RunsPage } from './pages/Runs';
import { ProjectsPage } from './pages/Projects';
import { ProvidersPage } from './pages/Providers';
import { StatsPage } from './pages/Stats';
import { SkillsPage } from './pages/Skills';
import { SettingsPage } from './pages/Settings';

type Theme = 'light' | 'dark';

interface Route {
  page: string;
  a?: string;
  b?: string;
}

function parseHash(): Route {
  const [path] = (window.location.hash.slice(2) || 'board').split('?');
  const [page, a, b] = path.split('/').map(decodeURIComponent);
  return { page: page || 'board', a, b };
}

const TITLES: Record<string, string> = {
  board: tr('看板'),
  tickets: tr('工单'),
  new: tr('新建工单'),
  runs: tr('运行'),
  projects: tr('项目'),
  providers: 'Provider',
  skills: tr('Skill 市场'),
  stats: tr('统计'),
  settings: tr('设置'),
};

const PAGES: { id: string; icon: IconName }[] = [
  { id: 'board', icon: 'board' },
  { id: 'tickets', icon: 'list' },
  { id: 'runs', icon: 'zap' },
  { id: 'projects', icon: 'folder' },
  { id: 'providers', icon: 'plug' },
  { id: 'skills', icon: 'puzzle' },
  { id: 'stats', icon: 'chart' },
  { id: 'settings', icon: 'gear' },
];

/** 还在流程里、没走完的工单：资源栏的「进行中」。 */
const ACTIVE_STAGES = ['ai_review', 'human_review', 'developing', 'acceptance'];
const MINE_STAGES = ['human_review', 'acceptance'];

function Wave({ level }: { level: number }) {
  const y = 30 - level * 22;
  const w = 'q15 -5 30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0';
  return (
    <svg viewBox="0 0 230 34" preserveAspectRatio="none">
      <line className="cap" x1="0" x2="230" y1="6" y2="6" />
      <path className="w1" d={`M0 ${y + 2} ${w} V34 H0Z`} />
      <path className="w2" d={`M-20 ${y + 5} ${w} V34 H-20Z`} />
    </svg>
  );
}

/** 命令面板：按编号或标题找工单，或者跳到某个页面。 */
function Palette({ tickets, onClose }: { tickets: TicketSummary[]; onClose: () => void }) {
  const [q, setQ] = useState('');
  const [at, setAt] = useState(0);
  const list = useRef<HTMLDivElement>(null);
  const items = useMemo(() => {
    const k = q.trim().toLowerCase();
    const pages = [{ id: 'new', icon: 'plus' as IconName }, ...PAGES]
      .map((p) => ({ group: tr('跳转'), icon: p.icon, key: '', title: TITLES[p.id], meta: '', hash: `#/${p.id}` }))
      .filter((p) => !k || p.title.toLowerCase().includes(k));
    const found = tickets
      .filter((x) => !k || x.key.toLowerCase().includes(k) || x.title.toLowerCase().includes(k))
      .sort((x, y) => Number(y.key.toLowerCase() === k) - Number(x.key.toLowerCase() === k) || y.updated_at.localeCompare(x.updated_at))
      .slice(0, 8)
      .map((x) => ({ group: tr('工单'), icon: 'form' as IconName, key: x.key, title: x.title, meta: STAGE_NAME[x.status], hash: `#/ticket/${x.key}` }));
    return k ? [...found, ...pages] : [...pages, ...found];
  }, [q, tickets]);
  const open = (hash: string) => {
    go(hash);
    onClose();
  };
  useEffect(() => {
    list.current?.querySelector('.on')?.scrollIntoView({ block: 'nearest' });
  }, [at]);
  return (
    <div className="palette-bg" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="palette" role="dialog" aria-modal="true" aria-label={tr('搜索工单或跳转页面')}>
        <div className="palette-in">
          <Icon name="search" size={15} />
          <input
            autoFocus
            value={q}
            placeholder={tr('搜索工单编号、标题，或输入页面名称')}
            onChange={(e) => {
              setQ(e.target.value);
              setAt(0);
            }}
            onKeyDown={(e) => {
              if (e.nativeEvent.isComposing || e.keyCode === 229) return;
              if (e.key === 'Escape') onClose();
              else if (e.key === 'ArrowDown') {
                e.preventDefault();
                setAt((i) => Math.min(items.length - 1, i + 1));
              } else if (e.key === 'ArrowUp') {
                e.preventDefault();
                setAt((i) => Math.max(0, i - 1));
              } else if (e.key === 'Enter' && items[at]) open(items[at].hash);
            }}
          />
          <kbd>Esc</kbd>
        </div>
        <div className="palette-list" ref={list}>
          {items.length === 0 && <div className="palette-none">{tr('没有匹配的工单或页面')}</div>}
          {items.map((it, i) => (
            <div key={it.hash}>
              {(i === 0 || items[i - 1].group !== it.group) && <div className="palette-group">{it.group}</div>}
              <button className={`palette-item ${i === at ? 'on' : ''}`} onMouseEnter={() => setAt(i)} onClick={() => open(it.hash)}>
                <Icon name={it.icon} size={14} />
                {it.key && <span className="k">{it.key}</span>}
                <span className="t">{it.title}</span>
                {it.meta && <span className="m">{it.meta}</span>}
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function Shell({ route, theme, setTheme }: { route: Route; theme: Theme; setTheme: (t: Theme) => void }) {
  const { tickets, runs, projects, providers, settings, stats } = useData();
  const [agents, setAgents] = useState<Detection[]>([]);
  const [sys, setSys] = useState<SystemInfo | null>(null);
  const [narrow, setNarrow] = useState(() => localStorage.getItem('tw-explorer') === 'off');
  const [palette, setPalette] = useState(false);
  // 打开设置页和窗口重新获得焦点时都重新检测：用户可能刚装完 Agent
  const onSettings = route.page === 'settings';
  useEffect(() => {
    const detect = () => void api.detectAgents().then(setAgents, () => setAgents([]));
    detect();
    window.addEventListener('focus', detect);
    return () => window.removeEventListener('focus', detect);
  }, [onSettings]);
  useEffect(() => {
    void api.systemInfo().then(setSys, () => setSys(null));
  }, []);
  useEffect(() => {
    void api
      .trayLabels(
        tr('显示观澜'),
        tr('退出观澜'),
        tr('观澜'),
        tr('还有正在运行的 Agent，退出会中断它们。确定退出观澜吗？'),
        tr('取消'),
      )
      .catch(() => {});
  }, []);
  useEffect(() => {
    localStorage.setItem('tw-explorer', narrow ? 'off' : 'on');
  }, [narrow]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'k') {
        e.preventDefault();
        setPalette((v) => !v);
      }
    };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  // 开了「启动时检查更新」才联网；失败不打扰
  const ui = useUi();
  const checkUpdates = settings.check_updates;
  useEffect(() => {
    if (!checkUpdates) return;
    api.updateCheck().then(
      (u) => u.newer && ui.toast(tr('观澜有新版本 {0}（当前 {1}），到 设置 › 本机环境 查看。', u.latest, u.current)),
      () => undefined,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [checkUpdates]);

  const live = runs.filter((r) => runActive(r.status) || r.status === 'queued');
  const running = live.filter((r) => r.status !== 'queued').length;
  const queued = live.length - running;
  const now = useNow(running > 0);
  const mine = tickets.filter((x) => MINE_STAGES.includes(x.status)).length;
  const today = new Date().toISOString().slice(0, 10);
  const todayCost = stats.days.find((d) => d.day === today)?.cost_usd ?? 0;
  const limit = settings.max_concurrency;

  const { page, a, b } = route;
  const active = page === 'ticket' ? 'tickets' : page === 'new' ? 'board' : TITLES[page] ? page : 'board';
  const ticket = page === 'ticket' ? tickets.find((x) => x.key === a) : undefined;
  const title = page === 'ticket' ? (a ?? tr('工单')) : (TITLES[page] ?? tr('看板'));

  const byRecent = [...tickets].sort((x, y) => y.updated_at.localeCompare(x.updated_at));
  const liveTickets = new Set(live.map((r) => r.ticket_id));
  const activeTickets = byRecent.filter((x) => ACTIVE_STAGES.includes(x.status) || liveTickets.has(x.id));
  const recentTickets = byRecent.filter((x) => !activeTickets.includes(x)).slice(0, 6);
  const badges: Record<string, ReactNode> = {
    board: mine > 0 && <span className="badge hot">{mine}</span>,
    runs: running > 0 && <span className="badge">{running}</span>,
  };

  let view: ReactNode;
  switch (page) {
    case 'tickets':
      view = <TicketsPage />;
      break;
    case 'new':
      view = <NewTicketPage />;
      break;
    case 'ticket':
      view = <TicketPage key={a} ticketKey={a ?? ''} tab={b} />;
      break;
    case 'runs':
      view = <RunsPage id={a} />;
      break;
    case 'projects':
      view = <ProjectsPage id={a} />;
      break;
    case 'providers':
      view = <ProvidersPage id={a} />;
      break;
    case 'skills':
      view = <SkillsPage />;
      break;
    case 'stats':
      view = <StatsPage />;
      break;
    case 'settings':
      view = <SettingsPage theme={theme} setTheme={setTheme} agents={agents} />;
      break;
    default:
      view = <BoardPage />;
  }

  return (
    <div className={`app ${narrow ? 'narrow' : ''}`}>
      <header className="titlebar" data-tauri-drag-region>
        <a className="brand" href="#/board">
          <span className="mark">
            <Icon name="wave" size={18} />
          </span>
          <b>{tr('观澜')}</b>
          {lang === 'zh' && <small>Tidewatch</small>}
        </a>
        <button className="omni" onClick={() => setPalette(true)}>
          <Icon name="search" size={14} />
          <span>{tr('搜索工单，或跳转到页面')}</span>
          <kbd>Ctrl K</kbd>
        </button>
        <div className="tools">
          <a className="btn primary" href="#/new">
            <Icon name="plus" size={14} />
            {tr('新建工单')}
          </a>
          <button className="langbtn" onClick={() => void switchLang(ui, lang === 'zh' ? 'en' : 'zh')} title={tr('切换语言 / Switch language')}>
            {lang === 'zh' ? 'EN' : '中文'}
          </button>
          <button className="iconbtn" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')} title={tr('切换亮色 / 暗色')}>
            <Icon name={theme === 'dark' ? 'sun' : 'moon'} />
          </button>
          <WinControls />
        </div>
      </header>

      <nav className="actbar">
        <button onClick={() => setNarrow((v) => !v)} title={narrow ? tr('展开资源栏') : tr('收起资源栏')}>
          <Icon name="panel" />
        </button>
        {PAGES.slice(0, 5).map((p) => (
          <a key={p.id} href={`#/${p.id}`} className={active === p.id ? 'on' : ''} title={TITLES[p.id]}>
            <Icon name={p.icon} />
            {badges[p.id]}
          </a>
        ))}
        <span className="sep" />
        {PAGES.slice(5).map((p) => (
          <a key={p.id} href={`#/${p.id}`} className={active === p.id ? 'on' : ''} title={TITLES[p.id]}>
            <Icon name={p.icon} />
          </a>
        ))}
      </nav>

      <aside className="explorer">
        <div className="ex-head">
          <b>{tr('工单与会话')}</b>
          <a className="iconbtn" href="#/new" title={tr('新建工单')}>
            <Icon name="plus" size={14} />
          </a>
        </div>
        <div className="ex-scroll">
          <div className="ex-group">
            <span>{tr('进行中')}</span>
            <i>Active · {String(activeTickets.length).padStart(2, '0')}</i>
          </div>
          {activeTickets.length === 0 && <div className="ex-empty">{tr('没有进行中的工单')}</div>}
          {activeTickets.map((x) => {
            const sessions = live.filter((r) => r.ticket_id === x.id);
            return (
              <div key={x.id}>
                <a href={`#/ticket/${x.key}`} className={`ex-row ${ticket?.id === x.id ? 'on' : ''} ${sessions.length ? 'live' : ''}`} title={x.title}>
                  <Icon name="form" size={14} />
                  <span className="k">{x.key}</span>
                  <span className="t">{x.title}</span>
                  {sessions.some((r) => r.status !== 'queued') ? (
                    <span className="pulse" />
                  ) : (
                    <span className={`m ${MINE_STAGES.includes(x.status) ? 'need' : ''}`}>{MINE_STAGES.includes(x.status) ? tr('待你') : STAGE_NAME[x.status]}</span>
                  )}
                </a>
                {sessions.map((r) => (
                  <a key={r.id} href={`#/runs/${r.id}`} className={`ex-row sub ${page === 'runs' && a === r.id ? 'on' : ''}`} title={runTitle(r)}>
                    <Icon name="zap" size={13} />
                    <span className="t">{runTitle(r)}</span>
                    <span className="m">{r.status === 'queued' ? tr('排队') : fmtDur(runSeconds(r, now))}</span>
                  </a>
                ))}
              </div>
            );
          })}
          <div className="ex-group">
            <span>{tr('最近工单')}</span>
            <i>Recent</i>
          </div>
          {recentTickets.length === 0 && <div className="ex-empty">{tr('还没有其他工单')}</div>}
          {recentTickets.map((x) => (
            <a key={x.id} href={`#/ticket/${x.key}`} className={`ex-row ${ticket?.id === x.id ? 'on' : ''}`} title={x.title}>
              <Icon name={x.status === 'done' ? 'check' : 'form'} size={14} />
              <span className="k">{x.key}</span>
              <span className="t">{x.title}</span>
              <span className="m">{fmtAgo(x.updated_at, now)}</span>
            </a>
          ))}
          <div className="ex-group">
            <span>{tr('项目')}</span>
            <i>Projects</i>
          </div>
          {projects.length === 0 && (
            <a className="ex-row" href="#/projects">
              <Icon name="plus" size={14} />
              <span className="t">{tr('添加第一个项目')}</span>
            </a>
          )}
          {projects.map((p) => (
            <a key={p.id} href={`#/projects/${p.id}`} className={`ex-row ${page === 'projects' && a === p.id ? 'on' : ''}`} title={p.local_path}>
              <Icon name="branch" size={14} />
              <span className="t">{p.name}</span>
              <span className="m">{p.default_branch}</span>
            </a>
          ))}
        </div>
        <div className="ex-foot">
          <a className="gauge" href="#/runs" title={tr('打开运行面板')}>
            <div className="gauge-head">
              <span>{tr('运行队列')}</span>
              <b>
                {running}
                <i> / {limit}</i>
              </b>
            </div>
            <Wave level={Math.min(1, running / limit)} />
            <div className="gauge-rows">
              <div>
                <span>{tr('排队')}</span>
                <b>{queued}</b>
              </div>
              <div>
                <span>{tr('今日费用')}</span>
                <b>{fmtCost(todayCost)}</b>
              </div>
            </div>
          </a>
        </div>
      </aside>

      <div className="main">
        <header className="top">
          {page === 'ticket' && (
            <span className="crumb">
              <a href="#/tickets">{tr('工单')}</a>
              <Icon name="right" size={11} />
            </span>
          )}
          {page === 'new' && (
            <span className="crumb">
              <a href="#/board">{tr('看板')}</a>
              <Icon name="right" size={11} />
            </span>
          )}
          <h1>{title}</h1>
          {ticket && <span className="sub">{ticket.title}</span>}
          <span className="spacer" />
          <a className="runchip" href="#/runs" title={tr('打开运行面板')}>
            {running > 0 && <span className="pulse" />}
            {tr('运行中')}{' '}
            <b>
              {running}/{limit}
            </b>
          </a>
        </header>
        <main className="view" id="view">
          {view}
        </main>
      </div>

      <footer className="statusbar">
        <a href="#/settings" className="lead" title={tr('数据都存在这台电脑上')}>
          <Icon name="shield" size={12} />
          {tr('本机存储')}
        </a>
        {sys && (
          <span title={sys.data_dir}>
            <Icon name="folder" size={12} />
            <span className="path">{settings.workspace_dir ?? sys.data_dir}</span>
          </span>
        )}
        <a className="env" href="#/settings" title={tr('环境检测，点击查看详情')}>
          {agents
            .filter((d) => d.kind !== 'mock')
            .map((d) => (
              <span key={d.kind} className={d.installed ? '' : 'off'} title={d.installed ? (d.version ?? tr('已安装')) : tr('未安装，或不在 PATH 里')}>
                <i />
                {d.kind === 'claude_code' ? 'claude' : d.kind}
              </span>
            ))}
        </a>
        <span className="spacer" />
        <a href="#/runs">
          {running > 0 && <span className="pulse" />}
          {tr('运行中')}{' '}
          <b>
            {running}/{limit}
          </b>
        </a>
        <span>
          Provider <b>{providers.length}</b>
        </span>
        {sys && <span>v{sys.version}</span>}
      </footer>
      {palette && <Palette tickets={tickets} onClose={() => setPalette(false)} />}
    </div>
  );
}

export function App() {
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem('tw-theme-v2') as Theme) || 'dark');
  const [route, setRoute] = useState<Route>(parseHash);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('tw-theme-v2', theme);
  }, [theme]);
  useEffect(() => {
    const on = () => setRoute(parseHash());
    window.addEventListener('hashchange', on);
    return () => window.removeEventListener('hashchange', on);
  }, []);

  return (
    <UiProvider>
      <DataProvider>
        <Shell route={route} theme={theme} setTheme={setTheme} />
      </DataProvider>
    </UiProvider>
  );
}

function WinControls() {
  const win = getCurrentWindow();
  const [max, setMax] = useState(false);
  useEffect(() => {
    const sync = () => void win.isMaximized().then(setMax).catch(() => {});
    sync();
    const off = win.onResized(sync);
    return () => void off.then((f) => f());
  }, [win]);
  return (
    <div className="winctl">
      <button onClick={() => void win.minimize()} title={tr('最小化')}>
        <Icon name="min" size={14} />
      </button>
      <button onClick={() => void win.toggleMaximize()} title={max ? tr('还原') : tr('最大化')}>
        <Icon name={max ? 'restore' : 'max'} size={14} />
      </button>
      <button className="close" onClick={() => void win.close()} title={tr('关闭')}>
        <Icon name="x" size={14} />
      </button>
    </div>
  );
}
