import { useEffect, useState } from 'react';
import { Icon } from './lib/Icon';
import { EnvironmentPage } from './pages/Environment';

type Theme = 'light' | 'dark';

export function App() {
  const [theme, setTheme] = useState<Theme>(() => (localStorage.getItem('tw-theme') as Theme) || 'light');
  useEffect(() => {
    document.documentElement.dataset.theme = theme;
    localStorage.setItem('tw-theme', theme);
  }, [theme]);

  return (
    <div className="app">
      <aside className="side">
        <a className="brand" href="#/">
          <span className="seal">观澜</span>
          <span>
            <b>观澜</b>
            <small>Tidewatch</small>
          </span>
        </a>
        <nav className="nav">
          <div className="nav-group">配置</div>
          <a href="#/" className="on">
            <Icon name="gear" />
            <span>环境</span>
          </a>
        </nav>
      </aside>
      <div className="main">
        <header className="top">
          <h1>环境</h1>
          <span className="spacer" />
          <button className="btn ghost" onClick={() => setTheme(theme === 'light' ? 'dark' : 'light')} title="切换主题">
            <Icon name={theme === 'light' ? 'moon' : 'sun'} size={14} />
            {theme === 'light' ? '深色' : '浅色'}
          </button>
        </header>
        <div className="page">
          <EnvironmentPage />
        </div>
      </div>
    </div>
  );
}
