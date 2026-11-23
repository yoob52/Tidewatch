import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import './styles/tokens.css';
import './styles/shell.css';
import './styles/components.css';
import './styles/pages.css';
import { App } from './App';
import { api } from './lib/api';
import { lang } from './lib/i18n';

void api.uiLangSet(lang).catch(() => {});

// 原生拖放已关（看板要用 HTML5 拖放）：拖进窗口的文件不能让 WebView 直接打开它，把表单换掉
for (const type of ['dragover', 'drop'] as const)
  window.addEventListener(type, (e) => {
    if (e.dataTransfer?.types.includes('Files')) e.preventDefault();
  });

createRoot(document.getElementById('app')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
);
