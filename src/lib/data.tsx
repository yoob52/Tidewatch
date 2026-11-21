// 全局数据：列表类的数据整个应用共用一份，引擎报告变化时整体刷新。
import { createContext, useContext, type ReactNode } from 'react';
import { api, type Project, type Provider, type RunRow, type Settings, type Stats, type TicketSummary } from './api';
import { useLoad } from './ui';

export interface AppData {
  tickets: TicketSummary[];
  runs: RunRow[];
  projects: Project[];
  providers: Provider[];
  settings: Settings;
  stats: Stats;
  reload: () => Promise<void>;
}

const DataContext = createContext<AppData | null>(null);

export function useData(): AppData {
  const d = useContext(DataContext);
  if (!d) throw new Error('DataProvider 未挂载');
  return d;
}

export function DataProvider({ children }: { children: ReactNode }) {
  const { data, error, reload } = useLoad(async () => {
    const [tickets, runs, projects, providers, settings, stats] = await Promise.all([
      api.ticketList(),
      api.runList(),
      api.projectList(),
      api.providerList(),
      api.settingsGet(),
      api.statsGet(),
    ]);
    return { tickets, runs, projects, providers, settings, stats };
  });
  if (!data) {
    return (
      <div className="empty" style={{ height: '100vh' }}>
        {error ? (
          <>
            <b>读取本地数据失败</b>
            <p>{error}</p>
            <button className="btn" onClick={() => void reload()}>
              重试
            </button>
          </>
        ) : (
          <p className="dim">读取中…</p>
        )}
      </div>
    );
  }
  return <DataContext.Provider value={{ ...data, reload }}>{children}</DataContext.Provider>;
}
