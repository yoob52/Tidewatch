// 设置：工作区与并发、默认执行配置、自动化、Prompt 模板、通知与外观、本机环境
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useState } from 'react';
import { api, type Detection, type Settings, type SystemInfo, type TemplateInfo } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { AGENTS, Modal, PERMISSIONS, Seg, Switch, useUi } from '../lib/ui';

const SECTIONS = ['工作区与并发', '默认执行配置', '自动化', 'Prompt 模板', '通知与外观', '本机环境'];

/** 编辑一个 Prompt 模板：左边改，右边看用示例数据填出来的效果。 */
function TemplateEditor({ tpl, onClose, onSaved }: { tpl: TemplateInfo; onClose: () => void; onSaved: (list: TemplateInfo[]) => void }) {
  const ui = useUi();
  const [text, setText] = useState(tpl.content);
  const [preview, setPreview] = useState<{ text: string; unknown_variables: string[] } | null>(null);

  // 停止输入 300ms 后刷新预览
  useEffect(() => {
    let stale = false;
    const timer = setTimeout(() => {
      api.promptPreview(tpl.name, text).then(
        (p) => !stale && setPreview(p),
        () => undefined,
      );
    }, 300);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [tpl.name, text]);

  const save = async (content: string | null) => {
    const unknown = content === null ? [] : (preview?.unknown_variables ?? []);
    if (
      unknown.length &&
      !(await ui.confirm('模板里有不认识的变量', `${unknown.map((v) => `{{${v}}}`).join('、')} 不会被替换，会原样发给 Agent。仍然保存吗？`, '仍然保存'))
    )
      return;
    const list = await ui.attempt(api.promptSave(tpl.name, content), content === null ? '已恢复默认模板' : '模板已保存，下一次运行生效');
    if (list) onSaved(list);
  };

  return (
    <Modal
      title={`Prompt 模板 · ${tpl.title}`}
      hint={tpl.description}
      width={1040}
      onClose={onClose}
      footer={
        <>
          <button className="btn ghost" disabled={text === tpl.default} onClick={() => setText(tpl.default)} title="把编辑框里的内容换回内置模板（还要点保存）">
            填回默认内容
          </button>
          {tpl.customized && (
            <button className="btn" onClick={() => void save(null)}>
              恢复默认并保存
            </button>
          )}
          <span className="grow" />
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn primary" disabled={!text.trim() || text === tpl.content} onClick={() => void save(text)}>
            保存
          </button>
        </>
      }
    >
      <div className="row" style={{ flexWrap: 'wrap', marginBottom: 8 }}>
        <span className="dim">可用变量（点击追加到末尾）：</span>
        {tpl.variables.map((v) => (
          <button key={v} className="tag line mono" onClick={() => setText((x) => `${x}{{${v}}}`)}>
            {`{{${v}}}`}
          </button>
        ))}
      </div>
      <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 12 }}>
        <textarea className="textarea mono" style={{ minHeight: 420 }} value={text} onChange={(e) => setText(e.target.value)} aria-label="模板内容" />
        <div>
          <pre className="plain boxed" style={{ minHeight: 420, maxHeight: 520, overflow: 'auto', margin: 0, whiteSpace: 'pre-wrap' }} aria-label="预览">
            {preview?.text ?? '生成预览中…'}
          </pre>
        </div>
      </div>
      <p className="dim" style={{ marginTop: 8 }}>
        右边是用示例数据填出来的效果。
        {['review', 'split', 'impact'].includes(tpl.name) && '末尾的输出格式由系统固定追加，不在模板里，改模板不会让结果解析失败。'}
        {preview && preview.unknown_variables.length > 0 && (
          <span style={{ color: 'var(--cinnabar)' }}> 不认识的变量：{preview.unknown_variables.map((v) => `{{${v}}}`).join('、')}</span>
        )}
      </p>
    </Modal>
  );
}

function PromptTemplates() {
  const [list, setList] = useState<TemplateInfo[] | null>(null);
  const [editing, setEditing] = useState<TemplateInfo | null>(null);
  useEffect(() => {
    api.promptTemplates().then(setList, () => setList([]));
  }, []);
  return (
    <div className="card">
      {list === null && (
        <div className="srow">
          <div>
            <p>读取中…</p>
          </div>
        </div>
      )}
      {(list ?? []).map((tpl) => (
        <div className="srow" key={tpl.name}>
          <div>
            <b>{tpl.title}</b>
            <p>{tpl.description}</p>
          </div>
          <div className="sctl">
            {tpl.customized && <span className="tag tide">已自定义</span>}
            <button className="btn sm" onClick={() => setEditing(tpl)}>
              <Icon name="edit" size={13} />
              编辑
            </button>
          </div>
        </div>
      ))}
      {editing && (
        <TemplateEditor
          key={editing.name}
          tpl={editing}
          onClose={() => setEditing(null)}
          onSaved={(l) => {
            setList(l);
            setEditing(null);
          }}
        />
      )}
    </div>
  );
}
const PERMISSION_HINT: Record<string, string> = {
  read_only: '只读，不改文件',
  workspace_write: '可改 worktree 内的文件',
  full_auto: '不经确认执行任意命令',
};

export function SettingsPage({
  theme,
  setTheme,
  agents,
}: {
  theme: 'light' | 'dark';
  setTheme: (t: 'light' | 'dark') => void;
  agents: Detection[] | null;
}) {
  const { settings, providers, reload } = useData();
  const ui = useUi();
  const [info, setInfo] = useState<SystemInfo | null>(null);
  const [workspace, setWorkspace] = useState(settings.workspace_dir ?? '');

  useEffect(() => {
    api.systemInfo().then(setInfo, () => undefined);
  }, []);
  useEffect(() => setWorkspace(settings.workspace_dir ?? ''), [settings.workspace_dir]);

  const save = async (patch: Partial<Settings>) => {
    if ((await ui.attempt(api.settingsSave({ ...settings, ...patch }))) !== undefined) await reload();
  };

  const pickWorkspace = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === 'string') await save({ workspace_dir: dir });
  };

  const stepper = (value: number, min: number, max: number, step: number, apply: (n: number) => void) => (
    <div className="stepper">
      <button disabled={value <= min} onClick={() => apply(Math.max(min, value - step))} aria-label="减少">
        −
      </button>
      <b>{value}</b>
      <button disabled={value >= max} onClick={() => apply(Math.min(max, value + step))} aria-label="增加">
        +
      </button>
    </div>
  );

  return (
    <div className="page flush split">
      <nav className="snav">
        {SECTIONS.map((s, i) => (
          <a key={s} href={`#/settings`} onClick={() => document.getElementById(`set-${i}`)?.scrollIntoView({ behavior: 'smooth' })}>
            {s}
          </a>
        ))}
      </nav>
      <section className="sbody">
        <h3 id="set-0">{SECTIONS[0]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>工作区目录</b>
              <p>子任务的 worktree 建在这里；留空则放在数据目录下。Windows 上建议用短路径。</p>
            </div>
            <div className="sctl">
              <input
                className="input mono"
                style={{ width: 300 }}
                value={workspace}
                onChange={(e) => setWorkspace(e.target.value)}
                onBlur={() => workspace !== (settings.workspace_dir ?? '') && void save({ workspace_dir: workspace || null })}
                placeholder="默认：数据目录/workspace"
                aria-label="工作区目录"
              />
              <button className="btn sm" onClick={() => void pickWorkspace()}>
                选择
              </button>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>全局并发上限</b>
              <p>同时运行的 Agent 数；超出的进入队列，按工单优先级和入队时间出队。</p>
            </div>
            <div className="sctl">{stepper(settings.max_concurrency, 1, 8, 1, (n) => void save({ max_concurrency: n }))}</div>
          </div>
          <div className="srow">
            <div>
              <b>单次运行超时</b>
              <p>超过这个时长，观澜会停止 Agent 并把运行记为失败。单位：分钟。</p>
            </div>
            <div className="sctl">{stepper(settings.run_timeout_minutes, 10, 480, 10, (n) => void save({ run_timeout_minutes: n }))}</div>
          </div>
          <div className="srow">
            <div>
              <b>单次运行花费上限</b>
              <p>这一轮的花费（Agent 上报，或按 Provider 单价估算）超过它就停止并记为失败。单位：美元，0 表示不限。</p>
            </div>
            <div className="sctl">{stepper(settings.budget_usd_per_run, 0, 100, 1, (n) => void save({ budget_usd_per_run: n }))}</div>
          </div>
          <div className="srow">
            <div>
              <b>单次运行 Token 上限</b>
              <p>输入加输出 Token 超过它就停止。单位：万 Token，0 表示不限。</p>
            </div>
            <div className="sctl">{stepper(Math.round(settings.budget_tokens_per_run / 10000), 0, 5000, 50, (n) => void save({ budget_tokens_per_run: n * 10000 }))}</div>
          </div>
          <div className="srow">
            <div>
              <b>卡住检测</b>
              <p>Agent 连续这么多分钟没有任何输出就提醒你，或直接停止这一轮。单位：分钟，0 表示不检测。</p>
            </div>
            <div className="sctl" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
              {stepper(settings.stuck_minutes, 0, 120, 5, (n) => void save({ stuck_minutes: n }))}
              <select className="input" style={{ width: 110 }} value={settings.stuck_action} onChange={(e) => void save({ stuck_action: e.target.value as 'notify' | 'stop' })} aria-label="卡住时">
                <option value="notify">只提醒</option>
                <option value="stop">停止运行</option>
              </select>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>发现危险命令就停止</b>
              <p>rm -r、git push、git reset --hard、sudo 等命令在运行日志里总会标红并提醒；打开后还会停掉这一轮。只能在 Agent 报出命令之后停，已经开始执行的那一条拦不住。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.stop_on_danger} onChange={(v) => void save({ stop_on_danger: v })} label="发现危险命令就停止" />
            </div>
          </div>
        </div>

        <h3 id="set-1">{SECTIONS[1]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>默认 Agent</b>
              <p>优先级：子任务 → 工单 → 项目 → 这里。</p>
            </div>
            <div className="sctl">
              <select className="input" style={{ width: 220 }} value={settings.default_agent} onChange={(e) => void save({ default_agent: e.target.value })}>
                {Object.entries(AGENTS).map(([k, v]) => (
                  <option key={k} value={k}>
                    {v.name}
                  </option>
                ))}
              </select>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>默认 Provider</b>
              <p>不选则用 Agent 在本机登录的账号。</p>
            </div>
            <div className="sctl">
              <select
                className="input"
                style={{ width: 220 }}
                value={settings.default_provider_id ?? ''}
                onChange={(e) => void save({ default_provider_id: e.target.value || null })}
              >
                <option value="">本机登录态</option>
                {providers.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>默认权限档位</b>
              <p>{PERMISSION_HINT[settings.default_permission]}。full_auto 不能设为默认，只能在子任务上单独选，每次开始都要确认。</p>
            </div>
            <div className="sctl">
              <Seg
                value={settings.default_permission}
                onChange={(v) => void save({ default_permission: v })}
                options={PERMISSIONS.filter((p) => p !== 'full_auto').map((p) => [p, p] as const)}
              />
            </div>
          </div>
        </div>

        <h3 id="set-2">{SECTIONS[2]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>确认拆分后立刻开始开发</b>
              <p>确认拆分建议或变更影响分析后，能开始的子任务直接入队。关闭时要自己点「开始开发」。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.auto_develop} onChange={(v) => void save({ auto_develop: v })} label="确认拆分后立刻开始开发" />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>依赖满足后自动开始</b>
              <p>后端写出接口文档、或依赖的子任务跑完后，等它的子任务自动入队。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.auto_start_unblocked} onChange={(v) => void save({ auto_start_unblocked: v })} label="依赖满足后自动开始" />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>运行结束后自动检查</b>
              <p>依次执行项目配置的 Lint、类型检查、测试和构建命令，结果显示在验收页。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.auto_test} onChange={(v) => void save({ auto_test: v })} label="运行结束后自动检查" />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>合并方式</b>
              <p>merge 保留子任务分支上的每个提交；squash 压成一个提交。观澜不会自动推送到远程。</p>
            </div>
            <div className="sctl">
              <Seg
                value={settings.merge_mode}
                onChange={(v) => void save({ merge_mode: v })}
                options={[
                  ['merge', 'merge'],
                  ['squash', 'squash'],
                ]}
              />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>合并后删除子任务分支</b>
              <p>worktree 总会清理。关闭后分支保留，需要时自己删除。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.delete_branch_after_merge} onChange={(v) => void save({ delete_branch_after_merge: v })} label="合并后删除子任务分支" />
            </div>
          </div>
        </div>

        <h3 id="set-3">{SECTIONS[3]}</h3>
        <PromptTemplates />

        <h3 id="set-4">{SECTIONS[4]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>系统通知</b>
              <p>运行完成、失败或被中断时弹出桌面通知。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.notifications} onChange={(v) => void save({ notifications: v })} label="系统通知" />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>外观</b>
              <p>只保存在这台机器上。</p>
            </div>
            <div className="sctl">
              <Seg
                value={theme}
                onChange={setTheme}
                options={[
                  ['light', '浅色'],
                  ['dark', '深色'],
                ]}
              />
            </div>
          </div>
        </div>

        <h3 id="set-5">{SECTIONS[5]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>观澜</b>
              <p>{info ? `${info.version} · ${info.os}` : '读取中…'}</p>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>数据库</b>
              <p className="mono">{info?.db_path ?? '—'}</p>
              {info && (
                <p>
                  结构第 {info.schema_version} 版 · SQLite {info.sqlite_version}
                </p>
              )}
            </div>
          </div>
          <div className="srow">
            <div>
              <b>密钥存放</b>
              <p>{info?.secret_store ?? '—'}</p>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>Git</b>
              <p>{info ? (info.git_version ?? '未安装：worktree 隔离需要 git') : '—'}</p>
              {info?.long_paths_hint && <p>{info.long_paths_hint}</p>}
            </div>
          </div>
          {(agents ?? []).map((a) => (
            <div className="srow" key={a.kind}>
              <div>
                <b>{AGENTS[a.kind]?.name ?? a.kind}</b>
                <p className="mono">{a.installed ? (a.path ?? '内置') : '未检测到'}</p>
              </div>
              <div className="sctl">
                <span className={`tag ${a.installed ? 'moss' : 'amber'}`}>
                  <Icon name={a.installed ? 'check' : 'alert'} size={11} />
                  {a.installed ? (a.version ?? '已安装') : '未安装'}
                </span>
              </div>
            </div>
          ))}
          {agents === null && (
            <div className="srow">
              <div>
                <b>Agent</b>
                <p>检测中…</p>
              </div>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
