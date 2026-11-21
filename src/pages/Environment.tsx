import { useCallback, useEffect, useState } from 'react';
import { api, type Detection, type SystemInfo } from '../lib/api';
import { Icon } from '../lib/Icon';

const AGENT_NAME: Record<Detection['kind'], string> = { claude_code: 'Claude Code', codex: 'Codex', mock: '演示 Agent' };

export function EnvironmentPage() {
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [agents, setAgents] = useState<Detection[] | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    setError(null);
    setAgents(null);
    api.systemInfo().then(setInfo).catch((e) => setError(String(e)));
    api.detectAgents().then(setAgents).catch((e) => setError(String(e)));
  }, []);
  useEffect(load, [load]);

  return (
    <div style={{ maxWidth: 760, display: 'grid', gap: 16 }}>
      {error && <div className="card" style={{ padding: 14, color: 'var(--red)' }}>读取环境失败：{error}</div>}
      <section className="card" style={{ padding: 18 }}>
        <h3 style={{ marginBottom: 12 }}>本机</h3>
        {info ? (
          <dl className="props">
            <dt>版本</dt>
            <dd>观澜 {info.version} · {info.os}</dd>
            <dt>数据库</dt>
            <dd className="mono">{info.db_path}</dd>
            <dt>结构</dt>
            <dd>第 {info.schema_version} 版 · SQLite {info.sqlite_version}</dd>
            <dt>Git</dt>
            <dd>{info.git_version ?? '未安装：worktree 隔离需要 git'}</dd>
            {info.long_paths_hint && (
              <>
                <dt>长路径</dt>
                <dd>{info.long_paths_hint}</dd>
              </>
            )}
          </dl>
        ) : (
          !error && <p className="dim">读取中…</p>
        )}
      </section>
      <section className="card" style={{ padding: 18 }}>
        <div style={{ display: 'flex', alignItems: 'center', marginBottom: 12 }}>
          <h3>Agent</h3>
          <span className="spacer" style={{ flex: 1 }} />
          <button className="btn" onClick={load}>
            重新检测
          </button>
        </div>
        {agents ? (
          <dl className="props">
            {agents.map((a) => (
              <div key={a.kind} style={{ display: 'contents' }}>
                <dt>{AGENT_NAME[a.kind]}</dt>
                <dd>
                  {a.installed ? (
                    <>
                      <Icon name="check" size={13} /> {a.version ?? '已安装'} <span className="mono dim">{a.path}</span>
                    </>
                  ) : (
                    <span className="dim">未安装，或不在 PATH 里</span>
                  )}
                </dd>
              </div>
            ))}
          </dl>
        ) : (
          !error && <p className="dim">检测中…</p>
        )}
      </section>
    </div>
  );
}
