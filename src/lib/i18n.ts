// 多语言：界面文案以中文原文为键，英文在 src/i18n/en.ts 里按原文查表。
// 语言在启动时定下来（切换时整页重载），所以模块顶层的常量表也可以直接用 t()。
import { EN } from '../i18n/en';

export type Lang = 'zh' | 'en';

const KEY = 'tw-lang';

function initial(): Lang {
  const saved = localStorage.getItem(KEY);
  if (saved === 'zh' || saved === 'en') return saved;
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en';
}

export const lang: Lang = initial();
document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en';

export function setLang(next: Lang) {
  if (next === lang) return;
  localStorage.setItem(KEY, next);
  window.location.reload();
}

/** 翻译一条文案；`{0}`、`{1}` 依次换成后面的参数。英文表里没有的原样返回中文。 */
export function t(text: string, ...args: unknown[]): string {
  const s = lang === 'en' ? (EN[text] ?? text) : text;
  return args.length ? s.replace(/\{(\d+)\}/g, (_, i) => String(args[Number(i)] ?? '')) : s;
}
