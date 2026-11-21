import { useEffect, useState, type ReactNode } from 'react';
import { Icon } from './lib/Icon';
import type { IconName } from './lib/icons';
import { api, type Detection } from './lib/api';
import { DataProvider, useData } from './lib/data';
import { UiProvider, fmtCost, runActive } from './lib/ui';
import { BoardPage } from './pages/Board';
import { TicketsPage } from './pages/Tickets';
import { NewTicketPage } from './pages/NewTicket';
import { TicketPage } from './pages/Ticket';
import { RunsPage } from './pages/Runs';
import { ProjectsPage } from './pages/Projects';
import { ProvidersPage } from './pages/Providers';
import { StatsPage } from './pages/Stats';
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
  board: '看板',
  tickets: '工单',
  new: '新建工单',
  runs: '运行',
  projects: '项目',
  providers: 'Provider',
  stats: '统计',
  settings: '设置',
};

function Wave({ level }: { level: number }) {
  const y = 40 - level * 30;
  const w = 'q15 -5 30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0 t30 0';
  return (
    <svg viewBox="0 0 186 46" preserveAspectRatio="none">
      <line className="cap" x1="0" x2="186" y1="8" y2="8" />
      <path className="w1" d={`M0 ${y + 2} ${w} V46 H0Z`} />
      <path className="w2" d={`M-20 ${y + 5} ${w} V46 H-20Z`} />
    </svg>
  );
}

function Shell({ route, theme, setTheme }: { route: Route; theme: Theme; setTheme: (t: Theme) => void }) {
  const { tickets, runs, projects, providers, settings, stats } = useData();
  const [agents, setAgents] = useState<Detection[]>([]);
  useEffect(() => {
    api.detectAgents().then(setAgents, () => setAgents([]));
  }, []);

  const running = runs.filter((r) => runActive(r.status)).length;
  const queued = runs.filter((r) => r.status === 'queued').length;
  const mine = tickets.filter((t) => t.status === 'human_review' || t.status === 'acceptance').length;
  const today = new Date().toISOString().slice(0, 10);
  const todayCost = stats.days.find((d) => d.day === today)?.cost_usd ?? 0;
  const limit = settings.max_concurrency;

  const { page, a, b } = route;
  const active = page === 'ticket' ? 'tickets' : page === 'new' ? 'board' : TITLES[page] ? page : 'board';
  const ticket = page === 'ticket' ? tickets.find((t) => t.key === a) : undefined;
  const title = page === 'ticket' ? (a ?? '工单') : (TITLES[page] ?? '看板');

  const nav = (id: string, icon: IconName, name: string, n?: ReactNode, live?: boolean) => (
    <a href={`#/${id}`} className={active === id ? 'on' : ''}>
      <Icon name={icon} />
      <span>{name}</span>
      {n !== undefined && (
        <span className={`n ${live ? 'live' : ''}`}>
          {live && <span className="pulse" />}
          {n}
        </span>
      )}
    </a>
  );

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
    <div className="app">
      <aside className="side">
        <a className="brand" href="#/board">
          <span className="seal">观澜</span>
          <span>
            <b>观澜</b>
            <small>Tidewatch</small>
          </span>
        </a>
        <nav className="nav">
          <div className="nav-group">工作</div>
          {nav('board', 'board', '看板', `${mine} 待办`)}
          {nav('tickets', 'list', '工单', tickets.length)}
          {nav('runs', 'runs', '运行', running, running > 0)}
          <div className="nav-group">配置</div>
          {nav('projects', 'folder', '项目', projects.length)}
          {nav('providers', 'plug', 'Provider', providers.length)}
          {nav('stats', 'chart', '统计')}
          {nav('settings', 'gear', '设置')}
        </nav>
        <div className="side-foot">
          <a className="gauge" href="#/runs" title="运行队列">
            <div className="gauge-head">
              <span>潮位 · 并发</span>
              <b>
                {running}
                <i> / {limit}</i>
              </b>
            </div>
            <Wave level={Math.min(1, running / limit)} />
            <div className="gauge-rows">
              <div>
                <span>排队</span>
                <b>{queued}</b>
              </div>
              <div>
                <span>今日费用</span>
                <b>{fmtCost(todayCost)}</b>
              </div>
            </div>
          </a>
          <a className="env" href="#/settings" title="环境检测，点击查看详情">
            {agents
              .filter((d) => d.kind !== 'mock')
              .map((d) => (
                <span key={d.kind} className={d.installed ? '' : 'off'} title={d.installed ? (d.version ?? '已安装') : '未安装，或不在 PATH 里'}>
                  <i style={d.installed ? undefined : { background: 'var(--ink-3)' }} />
                  {d.kind === 'claude_code' ? 'claude' : d.kind}
                </span>
              ))}
          </a>
        </div>
      </aside>
      <div className="main">
        <header className="top">
          {page === 'ticket' && (
            <span className="crumb">
              <a href="#/tickets">工单</a>
              <Icon name="right" size={11} />
            </span>
          )}
          {page === 'new' && (
            <span className="crumb">
              <a href="#/board">看板</a>
              <Icon name="right" size={11} />
            </span>
          )}
          <h1 title={ticket?.title}>{title}</h1>
          <span className="spacer" />
          <a className="btn primary" href="#/new">
            <Icon name="plus" size={14} />
            新建工单
          </a>
          <a className="runchip" href="#/runs" title="打开运行面板">
            {running > 0 && <span className="pulse" />}
            运行中{' '}
            <b>
              {running}/{limit}
            </b>
          </a>
          <button className="iconbtn" onClick={() => setTheme(theme === 'dark' ? 'light' : 'dark')} title="切换亮色 / 暗色">
            <Icon name={theme === 'dark' ? 'sun' : 'moon'} />
          </button>
        </header>
        <main className="view" id="view">
          {view}
        </main>
      </div>
    </div>
  );
}

export function App() {
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem('tw-theme') as Theme) || 'light');
  const [route, setRoute] = useState<Route>(parseHash);
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('tw-theme', theme);
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
