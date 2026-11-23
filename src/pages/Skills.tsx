import { tr } from '../lib/i18n';
// Skill 市场：左边是本机各 Agent 已装的 skill，右边从 skills.sh 搜索、预览并一键安装
import { useMemo, useState } from 'react';
import { api, type InstalledSkill, type MarketSkill, type SkillPreview, type SkillTarget } from '../lib/api';
import { Icon } from '../lib/Icon';
import { Empty, Field, Md, Modal, useLoad, useUi } from '../lib/ui';

const TARGETS: { id: SkillTarget; name: string; dir: string }[] = [
  { id: 'claude_code', name: 'Claude Code', dir: '~/.claude/skills' },
  { id: 'codex', name: 'Codex', dir: '~/.codex/skills' },
  { id: 'shared', name: tr('通用'), dir: '~/.agents/skills' },
];
const TARGET_NAME = (t: SkillTarget) => TARGETS.find((x) => x.id === t)?.name ?? t;
const PAGE = 24;
const fmtInstalls = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(n));

function TargetPicker({ value, onChange }: { value: SkillTarget[]; onChange: (v: SkillTarget[]) => void }) {
  return (
    <div className="sk-targets">
      {TARGETS.map((t) => (
        <label key={t.id} className={`sk-target ${value.includes(t.id) ? 'on' : ''}`}>
          <input type="checkbox" checked={value.includes(t.id)} onChange={(e) => onChange(e.target.checked ? [...value, t.id] : value.filter((x) => x !== t.id))} />
          <b>{t.name}</b>
          <span className="mono dim">{t.dir}</span>
        </label>
      ))}
    </div>
  );
}

function PreviewModal({ skill, installedIn, onClose, onInstalled }: { skill: MarketSkill; installedIn: SkillTarget[]; onClose: () => void; onInstalled: () => void }) {
  const ui = useUi();
  const [targets, setTargets] = useState<SkillTarget[]>(installedIn.length ? installedIn : ['claude_code']);
  const [busy, setBusy] = useState(false);
  const { data: preview, error } = useLoad<SkillPreview>(() => api.skillsPreview(skill.source, skill.skillId), [skill.id], () => false);
  const install = async () => {
    setBusy(true);
    const dirs = await ui.attempt(api.skillsInstall(skill.source, skill.skillId, targets));
    setBusy(false);
    if (dirs) {
      ui.toast(tr('已安装到 {0} 个位置', dirs.length));
      onInstalled();
      onClose();
    }
  };
  return (
    <Modal
      title={skill.name}
      hint={
        <a href={`https://skills.sh/${skill.id}`} target="_blank" rel="noreferrer" className="mono">
          {skill.source}
        </a>
      }
      width={760}
      onClose={onClose}
      footer={
        <>
          <span className="grow dim">{tr('skill 会作为指令交给 Agent 执行，安装前请先看一遍内容。')}</span>
          <button className="btn" onClick={onClose}>
            {tr('取消')}
          </button>
          <button className="btn primary" disabled={!preview || !targets.length || busy} onClick={() => void install()}>
            <Icon name="download" size={14} />
            {installedIn.length ? tr('更新') : tr('安装')}
          </button>
        </>
      }
    >
      {error ? (
        <div className="sk-err">
          <Icon name="alert" size={14} />
          {error}
        </div>
      ) : !preview ? (
        <div className="dim">{tr('正在从 GitHub 读取 SKILL.md…')}</div>
      ) : (
        <>
          {preview.description && <p className="sk-desc">{preview.description}</p>}
          <Field label={tr('安装到')}>
            <TargetPicker value={targets} onChange={setTargets} />
          </Field>
          <div className="sk-files mono dim">
            {tr('{0} 个文件', preview.files.length)} · {preview.path || '/'}
          </div>
          <div className="sk-md">
            <Md src={preview.content.replace(/^---[\s\S]*?\n---\n?/, '')} />
          </div>
        </>
      )}
    </Modal>
  );
}

function CreateModal({ onClose, onCreated }: { onClose: () => void; onCreated: () => void }) {
  const ui = useUi();
  const [name, setName] = useState('');
  const [desc, setDesc] = useState('');
  const [body, setBody] = useState('');
  const [targets, setTargets] = useState<SkillTarget[]>(['claude_code']);
  const ok = /^[A-Za-z0-9][A-Za-z0-9._-]*$/.test(name) && desc.trim() && targets.length;
  const save = async () => {
    if ((await ui.attempt(api.skillsCreate(targets, name, desc, body), tr('已创建'))) !== undefined) {
      onCreated();
      onClose();
    }
  };
  return (
    <Modal
      title={tr('新建 skill')}
      hint={tr('写一份 SKILL.md：Agent 会根据描述判断什么时候使用它。')}
      onClose={onClose}
      footer={
        <>
          <span className="grow" />
          <button className="btn" onClick={onClose}>
            {tr('取消')}
          </button>
          <button className="btn primary" disabled={!ok} onClick={() => void save()}>
            {tr('创建 skill')}
          </button>
        </>
      }
    >
      <Field label={tr('名称')} hint={tr('字母、数字、-、_、.')}>
        <input className="input mono" value={name} placeholder="my-skill" onChange={(e) => setName(e.target.value)} />
      </Field>
      <Field label={tr('描述')}>
        <input className="input" value={desc} placeholder={tr('例如：按团队规范写 React 组件和测试')} onChange={(e) => setDesc(e.target.value)} />
      </Field>
      <Field label={tr('内容')} hint="Markdown">
        <textarea className="textarea mono" rows={8} value={body} onChange={(e) => setBody(e.target.value)} />
      </Field>
      <Field label={tr('安装到')}>
        <TargetPicker value={targets} onChange={setTargets} />
      </Field>
    </Modal>
  );
}

export function SkillsPage() {
  const ui = useUi();
  const installed = useLoad(() => api.skillsInstalled(), [], () => false);
  const [filter, setFilter] = useState('');
  const [input, setInput] = useState('');
  const [query, setQuery] = useState('');
  const [page, setPage] = useState(0);
  const market = useLoad(() => api.skillsMarket(query), [query], () => false);
  const [preview, setPreview] = useState<MarketSkill | null>(null);
  const [creating, setCreating] = useState(false);

  const mine = installed.data ?? [];
  const byName = useMemo(() => {
    const m = new Map<string, SkillTarget[]>();
    for (const s of mine) m.set(s.name, [...(m.get(s.name) ?? []), s.target]);
    return m;
  }, [mine]);
  const shown = mine.filter((s) => !filter || `${s.name} ${s.description}`.toLowerCase().includes(filter.toLowerCase()));
  const results = market.data ?? [];
  const pages = Math.max(1, Math.ceil(results.length / PAGE));
  const slice = results.slice(page * PAGE, page * PAGE + PAGE);

  const remove = async (s: InstalledSkill) => {
    if (!(await ui.confirm(tr('删除 skill'), tr('会删除 {0}，Agent 之后不再使用它。', s.dir), tr('删除'), true))) return;
    if ((await ui.attempt(api.skillsRemove(s.target, s.dir.split(/[\\/]/).pop() ?? s.name), tr('已删除'))) !== undefined) void installed.reload();
  };
  const search = () => {
    setPage(0);
    setQuery(input.trim());
  };

  return (
    <div className="page flush skills">
      <aside className="sk-side">
        <div className="sk-h">
          <b>{tr('已安装')}</b>
          <i>{mine.length}</i>
          <span className="grow" />
          <button className="btn ghost sm" title={tr('刷新')} onClick={() => void installed.reload()}>
            <Icon name="resume" size={13} />
          </button>
          <button className="btn sm" onClick={() => setCreating(true)}>
            <Icon name="plus" size={13} />
            {tr('新建 skill')}
          </button>
        </div>
        <input className="input" value={filter} placeholder={tr('筛选已安装的 skill')} onChange={(e) => setFilter(e.target.value)} />
        {installed.error && <div className="sk-err">{installed.error}</div>}
        {shown.length === 0 && !installed.error ? (
          <Empty icon="puzzle" title={tr('还没有 skill')} body={tr('从右边的市场安装，或新建一个。')} />
        ) : (
          shown.map((s) => (
            <div key={s.dir} className="sk-item" title={s.dir}>
              <div className="row">
                <Icon name="puzzle" size={13} />
                <b className="ell grow">{s.name}</b>
                <span className="tag line">{TARGET_NAME(s.target)}</span>
                <button className="btn ghost sm" title={tr('删除')} onClick={() => void remove(s)}>
                  <Icon name="trash" size={12} />
                </button>
              </div>
              {s.description && <p className="ell2">{s.description}</p>}
            </div>
          ))
        )}
      </aside>
      <section className="sk-main">
        <header className="sk-top">
          <div>
            <h2>{tr('Skill 市场')}</h2>
            <p className="dim">{tr('来自 skills.sh 的开源 Agent Skill，安装到 Claude Code、Codex 等 Agent 的 skills 目录。')}</p>
          </div>
          <span className="grow" />
          <div className="sk-search">
            <Icon name="search" size={14} />
            <input
              value={input}
              placeholder={tr('搜索 skill，例如 react、pdf、testing')}
              onChange={(e) => setInput(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && search()}
            />
            <button className="btn primary sm" onClick={search}>
              {tr('搜索')}
            </button>
          </div>
        </header>
        <div className="sk-sub">
          <b>{query ? tr('「{0}」的搜索结果', query) : tr('热门推荐')}</b>
          <span className="dim">{tr('{0} 个', results.length)}</span>
          {query && (
            <button
              className="btn ghost sm"
              onClick={() => {
                setInput('');
                setQuery('');
                setPage(0);
              }}
            >
              <Icon name="x" size={12} />
              {tr('清除')}
            </button>
          )}
        </div>
        <div className="sk-grid">
          {market.error ? (
            <div className="sk-err">
              <Icon name="alert" size={14} />
              {market.error}
            </div>
          ) : !market.data ? (
            <div className="dim">{tr('正在加载市场…')}</div>
          ) : slice.length === 0 ? (
            <Empty icon="search" title={tr('没有找到匹配的 skill')} body={tr('换个关键词试试。')} />
          ) : (
            slice.map((s) => {
              const there = byName.get(s.skillId) ?? byName.get(s.name) ?? [];
              return (
                <div key={s.id} className="sk-card" onClick={() => setPreview(s)}>
                  <div className="row">
                    <span className="sk-glyph">{s.name.slice(0, 1).toUpperCase()}</span>
                    <div className="grow">
                      <b className="ell">{s.name}</b>
                      <span className="mono dim ell">{s.source}</span>
                    </div>
                  </div>
                  <div className="row sk-meta">
                    <span className="dim">
                      <Icon name="download" size={12} /> {fmtInstalls(s.installs)}
                    </span>
                    <span className="grow" />
                    {there.length > 0 ? (
                      <span className="tag moss">
                        <Icon name="check" size={11} />
                        {tr('已安装')}
                      </span>
                    ) : (
                      <button className="btn sm">{tr('安装')}</button>
                    )}
                  </div>
                </div>
              );
            })
          )}
        </div>
        {pages > 1 && (
          <footer className="sk-pager">
            <button className="btn ghost sm" disabled={page === 0} onClick={() => setPage(page - 1)}>
              {tr('上一页')}
            </button>
            <span className="mono">
              {page + 1} / {pages}
            </span>
            <button className="btn ghost sm" disabled={page >= pages - 1} onClick={() => setPage(page + 1)}>
              {tr('下一页')}
            </button>
          </footer>
        )}
      </section>
      {preview && <PreviewModal skill={preview} installedIn={byName.get(preview.skillId) ?? []} onClose={() => setPreview(null)} onInstalled={() => void installed.reload()} />}
      {creating && <CreateModal onClose={() => setCreating(false)} onCreated={() => void installed.reload()} />}
    </div>
  );
}
