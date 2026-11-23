// 检查英文词典：src 里每一条 tr('…') 文案都要在 src/i18n/en.ts 里有译文，词典里也不留用不到的条目。
// 用法：node scripts/i18n-check.mjs [--dump <file>]  （--dump 把缺译文的条目按文件写成 JSON）
import fs from 'node:fs';
import path from 'node:path';

const files = [];
(function walk(dir) {
  for (const f of fs.readdirSync(dir)) {
    const p = path.join(dir, f);
    if (fs.statSync(p).isDirectory()) walk(p);
    else if (/\.tsx?$/.test(f) && !p.includes('i18n')) files.push(p);
  }
})('src');

const literal = /'((?:[^'\\\n]|\\.)*)'/y;
const unquote = (s) => s.replace(/\\(.)/g, (_, c) => (c === 'n' ? '\n' : c));
const used = new Map();
for (const file of files.sort()) {
  const src = fs.readFileSync(file, 'utf8');
  for (const m of src.matchAll(/\btr\(\s*/g)) {
    literal.lastIndex = m.index + m[0].length;
    const lit = literal.exec(src);
    if (!lit) {
      const line = src.slice(0, m.index).split('\n').length;
      console.error(`${file}:${line} tr() 的第一个参数必须是单引号字符串字面量`);
      process.exitCode = 1;
      continue;
    }
    const key = unquote(lit[1]);
    if (!used.has(key)) used.set(key, file);
  }
}

const dict = fs.readFileSync('src/i18n/en.ts', 'utf8');
const have = new Set();
for (const m of dict.matchAll(/^\s*'((?:[^'\\\n]|\\.)*)':/gm)) have.add(unquote(m[1]));

const missing = [...used.keys()].filter((k) => !have.has(k));
const stale = [...have].filter((k) => !used.has(k));
const dump = process.argv.indexOf('--dump');
if (dump > 0) {
  const by = {};
  for (const k of missing) (by[used.get(k)] ??= []).push(k);
  fs.writeFileSync(process.argv[dump + 1], JSON.stringify(by, null, 1));
}
for (const k of missing) console.error(`缺英文译文（${used.get(k)}）：${JSON.stringify(k)}`);
for (const k of stale) console.error(`词典里有、界面上已不用：${JSON.stringify(k)}`);
if (missing.length || stale.length) process.exitCode = 1;
else console.log(`i18n: ${used.size} 条文案都有英文译文`);
