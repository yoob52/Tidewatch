// 项目：左边列表，右边是可编辑的项目卡片（路径、默认执行配置、上下文卡片、命令）
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useState } from 'react';
import { api, type Commands, type Project, type ProjectInput } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { AGENTS, Empty, Field, go, useUi } from '../lib/ui';

const KINDS: [string, string][] = [
  ['backend', '后端'],
  ['frontend', '前端'],
  ['fullstack', '全栈'],
  ['other', '其它'],
];

const COMMANDS: [keyof Commands, string, string][] = [
  ['setup', '准备', 'worktree 首次创建后'],
  ['lint', 'Lint', '运行结束后'],
  ['typecheck', '类型检查', '运行结束后'],
  ['test', '测试', '运行结束后'],
  ['build', '构建', '运行结束后'],
  ['dev', '本地启动', '只作记录，不自动执行'],
];

const blank: ProjectInput = {
  name: '',
  kind: 'backend',
  local_path: '',
  git_url: null,
  default_branch: null,
  context_md: '',
  commands: { setup: null, lint: null, typecheck: null, test: null, build: null, dev: null },
  default_agent: null,
  default_provider_id: null,
};

const toInput = (p: Project): ProjectInput => ({ ...p, commands: { ...p.commands } });

export function ProjectsPage({ id }: { id?: string }) {
  const { projects, providers, tickets, reload } = useData();
  const ui = useUi();
  const isNew = id === 'new' || projects.length === 0;
  const current = isNew ? undefined : (projects.find((p) => p.id === id) ?? projects[0]);
  const [f, setF] = useState<ProjectInput>(blank);
  const [dirty, setDirty] = useState(false);

  useEffect(() => {
    setF(current ? toInput(current) : blank);
    setDirty(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.id, current?.updated_at, isNew]);

  const set = (patch: Partial<ProjectInput>) => {
    setF((x) => ({ ...x, ...patch }));
    setDirty(true);
  };

  const pickDir = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir !== 'string') return;
    set({ local_path: dir, name: f.name || (dir.split(/[\\/]/).filter(Boolean).pop() ?? '') });
  };

  const importDocs = async () => {
    if (!f.local_path.trim()) {
      ui.toast('请先选择项目目录');
      return;
    }
    const docs = await ui.attempt(api.projectAgentDocs(f.local_path));
    if (!docs) return;
    if (!docs.length) {
      ui.toast('这个目录里没有 AGENTS.md 或 CLAUDE.md');
      return;
    }
    set({ context_md: docs.map(([name, text]) => `<!-- 来自 ${name} -->\n${text.trim()}`).join('\n\n') });
    ui.toast(`已填入 ${docs.map(([n]) => n).join('、')}，保存后生效`);
  };

  const save = async () => {
    const p = await ui.attempt(api.projectSave(current?.id ?? null, f), current ? '已保存' : '已注册项目');
    if (!p) return;
    await reload();
    setDirty(false);
    go(`#/projects/${p.id}`);
  };

  const remove = async () => {
    if (!current) return;
    if (!(await ui.confirm('删除项目', `只从观澜里移除「${current.name}」，不会动磁盘上的目录。有工单或子任务在用时不能删除。`, '删除', true))) return;
    if ((await ui.attempt(api.projectDelete(current.id), '已删除')) !== undefined) {
      await reload();
      go('#/projects');
    }
  };

  return (
    <div className="page flush split">
      <aside className="plist">
        <div className="row" style={{ padding: '14px 14px 8px' }}>
          <b className="grow">{projects.length} 个项目</b>
          <a className="btn sm" href="#/projects/new">
            <Icon name="plus" size={13} />
            注册项目
          </a>
        </div>
        {projects.map((x) => (
          <a key={x.id} className={`pi ${x.id === current?.id ? 'on' : ''}`} href={`#/projects/${x.id}`}>
            <div className="row">
              <b className="mono">{x.name}</b>
              <span className="tag line">{KINDS.find(([k]) => k === x.kind)?.[1] ?? x.kind}</span>
            </div>
            <small className="mono ell">{x.local_path}</small>
            <small>
              {tickets.filter((t) => !['done', 'cancelled'].includes(t.status) && t.projects.some((p) => p.project_id === x.id)).length} 张进行中的工单
            </small>
          </a>
        ))}
        {projects.length === 0 && <Empty icon="folder" title="还没有项目" body="注册一个本地 git 仓库，工单才能交给 Agent 去改代码。" />}
      </aside>
      <section className="pdetail">
        <div className="row" style={{ marginBottom: 4 }}>
          <h2 className="mono">{current ? current.name : '注册项目'}</h2>
          <span className="grow" />
          {current && (
            <button className="btn danger sm" onClick={() => void remove()}>
              删除
            </button>
          )}
          <button className="btn primary sm" disabled={!dirty} onClick={() => void save()}>
            {current ? '保存' : '注册'}
          </button>
        </div>
        <p className="dim">{current ? `默认分支 ${current.default_branch}` : '选择一个已经是 git 仓库的本地目录。观澜在工作区里为每个子任务另建 worktree，不直接改这个目录。'}</p>
        <div className="pgrid">
          <div className="card span2">
            <div className="card-h">
              <h3>基本信息</h3>
            </div>
            <div className="card-b form">
              <Field label="本地目录">
                <div className="row">
                  <input className="input mono grow" value={f.local_path} onChange={(e) => set({ local_path: e.target.value })} placeholder="D:\code\shop-api" />
                  <button className="btn" onClick={() => void pickDir()}>
                    <Icon name="folder" size={13} />
                    选择目录
                  </button>
                </div>
              </Field>
              <div className="form-grid">
                <Field label="名称">
                  <input className="input" value={f.name} onChange={(e) => set({ name: e.target.value })} />
                </Field>
                <Field label="类型">
                  <select className="input" value={f.kind} onChange={(e) => set({ kind: e.target.value })}>
                    {KINDS.map(([k, n]) => (
                      <option key={k} value={k}>
                        {n}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label="默认分支" hint="留空则自动识别">
                  <input className="input mono" value={f.default_branch ?? ''} onChange={(e) => set({ default_branch: e.target.value || null })} placeholder="main" />
                </Field>
                <Field label="Git 地址" hint="可选，只作记录">
                  <input className="input mono" value={f.git_url ?? ''} onChange={(e) => set({ git_url: e.target.value || null })} />
                </Field>
                <Field label="默认 Agent">
                  <select className="input" value={f.default_agent ?? ''} onChange={(e) => set({ default_agent: e.target.value || null })}>
                    <option value="">跟随全局默认</option>
                    {Object.entries(AGENTS).map(([k, v]) => (
                      <option key={k} value={k}>
                        {v.name}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field label="默认 Provider">
                  <select className="input" value={f.default_provider_id ?? ''} onChange={(e) => set({ default_provider_id: e.target.value || null })}>
                    <option value="">跟随全局默认</option>
                    {providers.map((p) => (
                      <option key={p.id} value={p.id}>
                        {p.name}
                      </option>
                    ))}
                  </select>
                </Field>
              </div>
            </div>
          </div>
          <div className="card span2">
            <div className="card-h">
              <h3>上下文卡片</h3>
              <span className="dim">每次运行自动拼入 Prompt，不改写仓库里的 CLAUDE.md / AGENTS.md</span>
              <span className="grow" />
              <button className="btn sm" onClick={() => void importDocs()}>
                <Icon name="upload" size={13} />从 AGENTS.md / CLAUDE.md 导入
              </button>
            </div>
            <div className="card-b">
              <textarea
                className="textarea mono"
                rows={9}
                value={f.context_md}
                onChange={(e) => set({ context_md: e.target.value })}
                placeholder={'技术栈、目录结构、编码约定、不能碰的地方…'}
                aria-label="上下文卡片"
              />
            </div>
          </div>
          <div className="card span2">
            <div className="card-h">
              <h3>命令配置</h3>
              <span className="dim">在子任务的 worktree 里执行；留空表示不执行</span>
            </div>
            <div className="card-b cmds">
              {COMMANDS.map(([k, name, hint]) => (
                <div key={k}>
                  <label>
                    {name}
                    <small>{hint}</small>
                  </label>
                  <input
                    className="input mono"
                    value={f.commands[k] ?? ''}
                    onChange={(e) => set({ commands: { ...f.commands, [k]: e.target.value || null } })}
                    aria-label={`${name}命令`}
                  />
                </div>
              ))}
            </div>
          </div>
        </div>
      </section>
    </div>
  );
}
