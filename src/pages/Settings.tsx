// 设置：工作区与并发、默认执行配置、自动化、Prompt 模板、通知与外观、本机环境
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useState } from 'react';
import { api, type UpdateProgress, type ApiStatus, type BackupFile, type Detection, type Member, type Settings, type SourceConfig, type SourceInput, type Stage, type SyncReport, type SyncSource, type SystemInfo, type TemplateInfo, type TicketType, type Workflow, type WorkflowInput } from '../lib/api';
import { installUpdate as runInstall, setUpdateInfo, useUpdate } from '../lib/update';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { AGENTS, Field, Modal, PERMISSIONS, Seg, Switch, TYPES, useUi } from '../lib/ui';

const SECTIONS = ['工作区与并发', '默认执行配置', '自动化', 'Prompt 模板', '通知与外观', '备份与导出', '接口与成员', '外部同步', '工作流', '本机环境'];

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
const ROLE_HINT: Record<Member['role'], string> = {
  owner: '全部操作，含合并和以 full_auto 开始',
  member: '建单、评论、开始 / 停止、验收、回答问题',
  viewer: '只读',
};

/** 本地 HTTP / MCP 接口的开关，以及能带令牌访问它的成员。 */
function ApiSection({ settings, save }: { settings: Settings; save: (patch: Partial<Settings>) => Promise<void> }) {
  const ui = useUi();
  const [status, setStatus] = useState<ApiStatus | null>(null);
  const [members, setMembers] = useState<Member[]>([]);
  const [port, setPort] = useState(String(settings.api_port));
  const [name, setName] = useState('');
  const [role, setRole] = useState<Member['role']>('member');
  const [shown, setShown] = useState<{ name: string; token: string } | null>(null);

  const refresh = () => {
    api.apiStatus().then(setStatus, () => undefined);
    api.memberList().then(setMembers, () => undefined);
  };
  useEffect(refresh, [settings.api_enabled, settings.api_port, settings.api_remote]);
  useEffect(() => setPort(String(settings.api_port)), [settings.api_port]);

  const base = status?.running ? `http://127.0.0.1:${status.port}` : null;
  const savePort = () => {
    const n = Number(port);
    if (n === settings.api_port) return;
    if (!Number.isInteger(n) || n < 1024 || n > 65535) {
      ui.toast('端口要在 1024 到 65535 之间');
      setPort(String(settings.api_port));
      return;
    }
    void save({ api_port: n });
  };
  const setRemote = async (on: boolean) => {
    if (
      on &&
      !(await ui.confirm(
        '允许局域网里的其他设备访问？',
        '打开后接口监听这台机器的所有网卡：同一网络里的人只要有成员令牌，就能读工单、开始运行。接口是明文 HTTP，令牌在网络上不加密，只在你信任的网络里打开。',
        '打开',
      ))
    )
      return;
    await save({ api_remote: on });
  };
  const create = async () => {
    const made = await ui.attempt(api.memberCreate(name, role));
    if (!made) return;
    setName('');
    setShown({ name: made.member.name, token: made.token });
    refresh();
  };
  const update = async (m: Member) => {
    await ui.attempt(api.memberUpdate(m));
    refresh();
  };
  const reset = async (m: Member) => {
    if (!(await ui.confirm(`重置「${m.name}」的令牌？`, '旧令牌立刻失效，正在用它的脚本和网页要换成新的。', '重置'))) return;
    const made = await ui.attempt(api.memberResetToken(m.id));
    if (made) setShown({ name: m.name, token: made.token });
    refresh();
  };
  const remove = async (m: Member) => {
    if (!(await ui.confirm(`删除成员「${m.name}」？`, '它的令牌立刻失效；指派给它的工单变成未指派。', '删除'))) return;
    await ui.attempt(api.memberDelete(m.id));
    refresh();
  };

  return (
    <div className="card">
      <div className="srow">
        <div>
          <b>本地接口（HTTP / MCP）</b>
          <p>
            打开后，脚本可以经 REST 读写工单；运行中的 Claude Code、Codex、OpenCode 会自动接上观澜的 MCP 服务，可以读当前工单、把只读任务委派给另一个 Agent、把拿不准的问题挂给你。默认关闭。
          </p>
          {settings.api_enabled && status?.running && (
            <p className="mono">
              {base}/api/v1 · {base}/mcp · 看板页 {base}/
            </p>
          )}
          {settings.api_enabled && status?.error && <p style={{ color: 'var(--cinnabar)' }}>{status.error}</p>}
        </div>
        <div className="sctl">
          <Switch on={settings.api_enabled} onChange={(v) => void save({ api_enabled: v })} label="本地接口" />
        </div>
      </div>
      <div className="srow">
        <div>
          <b>端口</b>
          <p>改端口后，已经在跑的 Agent 仍然连着旧地址，要到下一轮才用新的。</p>
        </div>
        <div className="sctl">
          <input className="input mono" style={{ width: 90 }} value={port} onChange={(e) => setPort(e.target.value)} onBlur={savePort} inputMode="numeric" aria-label="接口端口" />
        </div>
      </div>
      <div className="srow">
        <div>
          <b>允许局域网访问</b>
          <p>关着时只有这台机器能连。打开后同一网络里的人可以带成员令牌访问接口和看板页；明文 HTTP，不要在不信任的网络里打开。</p>
          {status?.remote && status.lan_urls.length > 0 && <p className="mono">{status.lan_urls.join(' · ')}</p>}
        </div>
        <div className="sctl">
          <Switch on={settings.api_remote} onChange={(v) => void setRemote(v)} label="允许局域网访问" />
        </div>
      </div>
      {members.map((m) => (
        <div className="srow" key={m.id}>
          <div>
            <b>{m.name}</b>
            <p>
              {ROLE_HINT[m.role]}
              {m.token_hint && <span className="mono"> · 令牌 …{m.token_hint}</span>}
              {m.last_seen_at ? ` · 最近访问 ${m.last_seen_at.slice(0, 16).replace('T', ' ')}` : ' · 还没用过'}
            </p>
          </div>
          <div className="sctl" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            {m.disabled && <span className="tag amber">已停用</span>}
            <select className="input" style={{ width: 100 }} value={m.role} onChange={(e) => void update({ ...m, role: e.target.value as Member['role'] })} aria-label={`${m.name} 的角色`}>
              <option value="owner">owner</option>
              <option value="member">member</option>
              <option value="viewer">viewer</option>
            </select>
            <button className="btn sm" onClick={() => void update({ ...m, disabled: !m.disabled })}>
              {m.disabled ? '启用' : '停用'}
            </button>
            <button className="btn sm" onClick={() => void reset(m)}>
              重置令牌
            </button>
            <button className="btn sm ghost" onClick={() => void remove(m)} aria-label={`删除 ${m.name}`}>
              <Icon name="x" size={13} />
            </button>
          </div>
        </div>
      ))}
      <div className="srow">
        <div>
          <b>添加成员</b>
          <p>每个成员一个令牌，用于 REST、MCP 和看板页。令牌只显示一次，数据库里只存它的摘要。运行中的 Agent 用的是每一轮单独签发的临时令牌，不需要在这里建成员。</p>
        </div>
        <div className="sctl" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
          <input className="input" style={{ width: 130 }} value={name} onChange={(e) => setName(e.target.value)} placeholder="名字" aria-label="成员名" />
          <select className="input" style={{ width: 100 }} value={role} onChange={(e) => setRole(e.target.value as Member['role'])} aria-label="角色">
            <option value="owner">owner</option>
            <option value="member">member</option>
            <option value="viewer">viewer</option>
          </select>
          <button className="btn sm" disabled={!name.trim()} onClick={() => void create()}>
            添加
          </button>
        </div>
      </div>
      {shown && (
        <Modal
          title={`「${shown.name}」的令牌`}
          hint="只显示这一次。关掉之后看不到了，丢了只能重置。"
          width={620}
          onClose={() => setShown(null)}
          footer={
            <>
              <span className="grow" />
              <button
                className="btn"
                onClick={() => {
                  void navigator.clipboard.writeText(shown.token).then(() => ui.toast('已复制'));
                }}
              >
                复制
              </button>
              <button className="btn primary" onClick={() => setShown(null)}>
                我已保存
              </button>
            </>
          }
        >
          <pre className="plain boxed mono" style={{ whiteSpace: 'pre-wrap', wordBreak: 'break-all', margin: 0 }} aria-label="令牌">
            {shown.token}
          </pre>
          <p className="dim" style={{ marginTop: 8 }}>
            用法：请求头带上 <span className="mono">Authorization: Bearer {'<令牌>'}</span>。
          </p>
        </Modal>
      )}
    </div>
  );
}

const GATE_NAME: Record<Stage['gate'], string> = { finalized: '定稿之前', done: '完成之前' };

/** 新建 / 编辑一个工作流。 */
function WorkflowEditor({ current, onClose, onSaved }: { current: Workflow | null; onClose: () => void; onSaved: () => void }) {
  const ui = useUi();
  const [f, setF] = useState<WorkflowInput>(
    current
      ? { name: current.name, config: current.config, enabled: current.enabled }
      : { name: '', config: { ticket_types: ['feature_mixed'], stages: [{ id: '', name: '', gate: 'finalized', hint: '' }], auto_done: true }, enabled: true },
  );
  const cfg = (patch: Partial<WorkflowInput['config']>) => setF((x) => ({ ...x, config: { ...x.config, ...patch } }));
  const stage = (i: number, patch: Partial<Stage>) => cfg({ stages: f.config.stages.map((s, k) => (k === i ? { ...s, ...patch } : s)) });
  const move = (i: number, by: number) => {
    const next = [...f.config.stages];
    const [s] = next.splice(i, 1);
    next.splice(i + by, 0, s);
    cfg({ stages: next });
  };
  const toggleType = (t: TicketType) =>
    cfg({ ticket_types: f.config.ticket_types.includes(t) ? f.config.ticket_types.filter((x) => x !== t) : [...f.config.ticket_types, t] });
  const submit = async () => {
    const saved = await ui.attempt(api.workflowSave(current?.id ?? null, f), '已保存');
    if (saved) onSaved();
  };
  const onlyBug = f.config.ticket_types.length > 0 && f.config.ticket_types.every((t) => t === 'bug');
  return (
    <Modal
      title={current ? `编辑「${current.name}」` : '添加工作流'}
      hint="内置的状态（草稿、预审、人工审核、已定稿、开发中、验收中、已完成）不变。这里给某几类工单加上自己的环节：每个环节要有人点「通过」，工单才能往下走。"
      width={720}
      onClose={onClose}
      footer={
        <>
          <span className="grow" />
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn primary" disabled={!f.name.trim() || !f.config.ticket_types.length} onClick={() => void submit()}>
            保存
          </button>
        </>
      }
    >
      <div className="form">
        <Field label="名称">
          <input className="input" value={f.name} onChange={(e) => setF((x) => ({ ...x, name: e.target.value }))} placeholder="例如：带设计评审的需求流程" />
        </Field>
        <Field label="适用的工单类型" hint="一种类型同一时间只能有一个启用的工作流">
          <div className="row" style={{ flexWrap: 'wrap', gap: 12 }}>
            {(Object.keys(TYPES) as TicketType[]).map((t) => (
              <label key={t} className="row">
                <input type="checkbox" checked={f.config.ticket_types.includes(t)} onChange={() => toggleType(t)} />
                {TYPES[t].name}
              </label>
            ))}
          </div>
        </Field>
        <Field label="环节" hint="按顺序逐个通过。工单退回前面的状态（比如发起变更、打回返工）后要重新过">
          {f.config.stages.map((s, i) => (
            <div key={i} className="row" style={{ gap: 6, marginBottom: 6, alignItems: 'center' }}>
              <input className="input" style={{ width: 150 }} value={s.name} onChange={(e) => stage(i, { name: e.target.value })} placeholder="环节名称" aria-label={`环节 ${i + 1} 名称`} />
              <select className="input" style={{ width: 110 }} value={s.gate} onChange={(e) => stage(i, { gate: e.target.value as Stage['gate'] })} aria-label={`环节 ${i + 1} 位置`}>
                <option value="finalized">{GATE_NAME.finalized}</option>
                <option value="done">{GATE_NAME.done}</option>
              </select>
              <input className="input" style={{ flex: 1 }} value={s.hint} onChange={(e) => stage(i, { hint: e.target.value })} placeholder="要检查什么（可选）" aria-label={`环节 ${i + 1} 说明`} />
              <button className="btn sm ghost" disabled={i === 0} onClick={() => move(i, -1)} aria-label="上移">
                ↑
              </button>
              <button className="btn sm ghost" disabled={i === f.config.stages.length - 1} onClick={() => move(i, 1)} aria-label="下移">
                ↓
              </button>
              <button className="btn sm ghost" onClick={() => cfg({ stages: f.config.stages.filter((_, k) => k !== i) })} aria-label={`删除环节 ${i + 1}`}>
                <Icon name="x" size={13} />
              </button>
            </div>
          ))}
          <button className="btn sm" onClick={() => cfg({ stages: [...f.config.stages, { id: '', name: '', gate: 'done', hint: '' }] })}>
            加一个环节
          </button>
          {onlyBug && f.config.stages.some((s) => s.gate === 'finalized') && (
            <p className="dim" style={{ marginTop: 6 }}>
              Bug 走快速通道、不经过定稿，「定稿之前」的环节对它不起作用。
            </p>
          )}
        </Field>
        <label className="row">
          <input type="checkbox" checked={f.config.auto_done} onChange={(e) => cfg({ auto_done: e.target.checked })} />
          子任务全部合并、环节都通过后，自动完成工单（关掉后要手动把工单拖到「已完成」）
        </label>
      </div>
    </Modal>
  );
}

/** 自定义工作流：在内置流程上加自己的环节。 */
function WorkflowSection() {
  const ui = useUi();
  const { reload } = useData();
  const [list, setList] = useState<Workflow[]>([]);
  const [editing, setEditing] = useState<Workflow | 'new' | null>(null);
  const refresh = () => {
    void api.workflowList().then(setList, () => undefined);
    // 看板上的「待：环节」跟着变
    void reload();
  };
  useEffect(() => void api.workflowList().then(setList, () => undefined), []);
  const toggle = async (w: Workflow) => {
    await ui.attempt(api.workflowSave(w.id, { name: w.name, config: w.config, enabled: !w.enabled }));
    refresh();
  };
  const remove = async (w: Workflow) => {
    if (!(await ui.confirm(`删除工作流「${w.name}」？`, '这几类工单回到内置流程；已经通过的环节记录一并删除，时间线里的记录留着。', '删除', true))) return;
    await ui.attempt(api.workflowDelete(w.id));
    refresh();
  };
  return (
    <div className="card">
      {list.length === 0 && (
        <div className="srow">
          <div>
            <b>现在用的是内置流程</b>
            <p>可以给某几类工单加上自己的环节，比如定稿前的「设计评审」、完成前的「QA 验证」。环节由人点通过，没通过之前工单不能定稿 / 完成。</p>
          </div>
        </div>
      )}
      {list.map((w) => (
        <div className="srow" key={w.id}>
          <div>
            <b>{w.name}</b>
            <p>
              {w.config.ticket_types.map((t) => TYPES[t]?.name ?? t).join('、')}
              {w.config.stages.length > 0 ? ` · ${w.config.stages.map((s) => `${s.name}（${GATE_NAME[s.gate]}）`).join(' → ')}` : ' · 没有环节'}
              {!w.config.auto_done && ' · 手动完成'}
            </p>
          </div>
          <div className="sctl" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            {!w.enabled && <span className="tag amber">已停用</span>}
            <button className="btn sm" onClick={() => setEditing(w)}>
              编辑
            </button>
            <button className="btn sm" onClick={() => void toggle(w)}>
              {w.enabled ? '停用' : '启用'}
            </button>
            <button className="btn sm ghost" onClick={() => void remove(w)} aria-label={`删除 ${w.name}`}>
              <Icon name="x" size={13} />
            </button>
          </div>
        </div>
      ))}
      <div className="srow">
        <div>
          <b>添加工作流</b>
          <p>改动立刻对进行中的工单生效：新加的环节要补过，删掉的环节不再卡。</p>
        </div>
        <div className="sctl">
          <button className="btn sm" onClick={() => setEditing('new')}>
            添加…
          </button>
        </div>
      </div>
      {editing && (
        <WorkflowEditor
          current={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            refresh();
          }}
        />
      )}
    </div>
  );
}

const BLANK_SOURCE: SourceConfig = {
  repo: '',
  labels: [],
  write_back: false,
  app_id: '',
  api_base: null,
  project_id: null,
  ticket_type: 'feature_mixed',
};

function reportText(r: SyncReport): string {
  const parts = [
    r.created.length ? `新建 ${r.created.length} 张工单（${r.created.join('、')}）` : '',
    r.changed.length ? `${r.changed.length} 张工单的来源有更新（${r.changed.join('、')}）` : '',
    r.comments ? `${r.comments} 条新评论` : '',
    r.written_back.length ? `回写并关闭 ${r.written_back.length} 个 Issue` : '',
  ].filter(Boolean);
  const head = parts.length ? parts.join('；') : '没有新的内容';
  return r.warnings.length ? `${head}。注意：${r.warnings.join('；')}` : head;
}

/** 新建 / 编辑一个同步来源。 */
function SourceEditor({ current, onClose, onSaved }: { current: SyncSource | null; onClose: () => void; onSaved: () => void }) {
  const ui = useUi();
  const { projects } = useData();
  const [f, setF] = useState<SourceInput>(
    current ? { kind: current.kind, name: current.name, config: current.config, enabled: current.enabled } : { kind: 'github', name: '', config: BLANK_SOURCE, enabled: true },
  );
  const [secret, setSecret] = useState('');
  const [clear, setClear] = useState(false);
  const cfg = (patch: Partial<SourceConfig>) => setF((x) => ({ ...x, config: { ...x.config, ...patch } }));
  const github = f.kind === 'github';
  const ready = f.name.trim() && (github ? f.config.repo.trim() : f.config.app_id.trim());
  const submit = async () => {
    const saved = await ui.attempt(api.syncSourceSave(current?.id ?? null, f, secret.trim() ? secret.trim() : clear ? '' : undefined), '已保存');
    if (saved) onSaved();
  };
  return (
    <Modal
      title={current ? `编辑「${current.name}」` : '添加同步来源'}
      hint={
        github
          ? '把一个 GitHub 仓库的 Issue 拉成草稿工单。外部改了内容只会提示「源已变更」，要不要导入由你决定。'
          : '用飞书自建应用按链接导入新版文档。这部分按开放平台文档写成，还没有对着真实的飞书验证过。'
      }
      onClose={onClose}
      footer={
        <>
          <span className="grow" />
          <button className="btn" onClick={onClose}>
            取消
          </button>
          <button className="btn primary" disabled={!ready} onClick={() => void submit()}>
            保存
          </button>
        </>
      }
    >
      <div className="form">
        {!current && (
          <Field label="类型">
            <Seg
              value={f.kind}
              onChange={(k) => setF((x) => ({ ...x, kind: k }))}
              options={[
                ['github', 'GitHub Issues'],
                ['feishu', '飞书文档'],
              ]}
            />
          </Field>
        )}
        <Field label="名称">
          <input className="input" value={f.name} onChange={(e) => setF((x) => ({ ...x, name: e.target.value }))} placeholder={github ? '例如：商城仓库' : '例如：产品文档'} />
        </Field>
        {github ? (
          <>
            <Field label="仓库" hint="owner/name，或者仓库地址">
              <input className="input mono" value={f.config.repo} onChange={(e) => cfg({ repo: e.target.value })} placeholder="acme/shop" />
            </Field>
            <Field label="只拉带这些 label 的 Issue" hint="逗号分隔，全部都要有；留空拉全部">
              <input
                className="input mono"
                value={f.config.labels.join(', ')}
                onChange={(e) => cfg({ labels: e.target.value.split(/[,，]+/).map((l) => l.trimStart()) })}
                placeholder="tidewatch"
              />
            </Field>
            <Field
              label="访问令牌"
              hint={current?.has_secret ? '已保存；留空不改' : '留空时用本机 gh 登录的账号；公开仓库不填也能拉，但回写需要令牌'}
            >
              <input className="input mono" type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="off" aria-label="访问令牌" />
            </Field>
          </>
        ) : (
          <>
            <Field label="App ID" hint="飞书开放平台里自建应用的凭证">
              <input className="input mono" value={f.config.app_id} onChange={(e) => cfg({ app_id: e.target.value })} placeholder="cli_xxxxxxxx" />
            </Field>
            <Field label="App Secret" hint={current?.has_secret ? '已保存；留空不改' : '只存进系统凭据管理器'}>
              <input className="input mono" type="password" value={secret} onChange={(e) => setSecret(e.target.value)} autoComplete="off" aria-label="App Secret" />
            </Field>
            <p className="dim" style={{ margin: 0 }}>
              应用要开通「查看新版文档」权限（知识库里的文档还要「查看知识库」），并把要导入的文档分享给这个应用。
            </p>
          </>
        )}
        {current?.has_secret && (
          <label className="row">
            <input type="checkbox" checked={clear} onChange={(e) => setClear(e.target.checked)} />
            清除已保存的{github ? '令牌' : ' App Secret'}
          </label>
        )}
        <div className="form-grid">
          <Field label="新工单的类型" hint={github ? '带 bug label 的 Issue 总是建成 Bug' : undefined}>
            <select className="input" value={f.config.ticket_type} onChange={(e) => cfg({ ticket_type: e.target.value as TicketType })}>
              {Object.entries(TYPES).map(([k, t]) => (
                <option key={k} value={k}>
                  {t.name}
                </option>
              ))}
            </select>
          </Field>
          <Field label="新工单绑定的项目" hint="可选">
            <select className="input" value={f.config.project_id ?? ''} onChange={(e) => cfg({ project_id: e.target.value || null })}>
              <option value="">不绑定</option>
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
          </Field>
        </div>
        <Field label="接口地址" hint={github ? '可选；GitHub Enterprise 填 https://<域名>/api/v3' : '可选；私有化部署或 Lark 国际版（https://open.larksuite.com）时填'}>
          <input className="input mono" value={f.config.api_base ?? ''} onChange={(e) => cfg({ api_base: e.target.value || null })} placeholder={github ? 'https://api.github.com' : 'https://open.feishu.cn'} />
        </Field>
        {github && (
          <label className="row">
            <input type="checkbox" checked={f.config.write_back} onChange={(e) => cfg({ write_back: e.target.checked })} />
            工单完成后，在 Issue 下评论一条并关闭它（下次同步时执行）
          </label>
        )}
      </div>
    </Modal>
  );
}

/** 外部同步：GitHub Issues 拉成工单，飞书文档按链接导入。都只在点「同步」/「导入」时联网。 */
function SyncSection() {
  const ui = useUi();
  const [sources, setSources] = useState<SyncSource[]>([]);
  const [editing, setEditing] = useState<SyncSource | 'new' | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [importing, setImporting] = useState<SyncSource | null>(null);
  const [url, setUrl] = useState('');

  const refresh = () => void api.syncSourceList().then(setSources, () => undefined);
  useEffect(refresh, []);

  const run = async (s: SyncSource) => {
    setBusy(s.id);
    const r = await ui.attempt(api.syncRun(s.id));
    setBusy(null);
    if (r) ui.toast(reportText(r));
    refresh();
  };
  const toggle = async (s: SyncSource) => {
    await ui.attempt(api.syncSourceSave(s.id, { kind: s.kind, name: s.name, config: s.config, enabled: !s.enabled }));
    refresh();
  };
  const remove = async (s: SyncSource) => {
    if (!(await ui.confirm(`删除同步来源「${s.name}」？`, '已经建好的工单会留着，只是不再和外部关联；保存的令牌一并清除。', '删除', true))) return;
    await ui.attempt(api.syncSourceDelete(s.id));
    refresh();
  };
  const importDoc = async () => {
    if (!importing) return;
    setBusy(importing.id);
    const done = await ui.attempt(api.syncFeishuImport(importing.id, url));
    setBusy(null);
    if (!done) return;
    setImporting(null);
    setUrl('');
    ui.toast(done.warnings.length ? `已导入为 ${done.ticket.key}。注意：${done.warnings.join('；')}` : `已导入为 ${done.ticket.key}`);
    window.location.hash = `#/ticket/${done.ticket.key}`;
  };

  return (
    <div className="card">
      {sources.length === 0 && (
        <div className="srow">
          <div>
            <b>还没有同步来源</b>
            <p>可以把 GitHub 仓库的 Issue 拉成草稿工单，或者按链接导入飞书文档。只在你点「同步」或「导入」时联网，不会在后台自动拉。</p>
          </div>
        </div>
      )}
      {sources.map((s) => (
        <div className="srow" key={s.id}>
          <div>
            <b>{s.name}</b>
            <p>
              {s.kind === 'github' ? (
                <>
                  GitHub Issues · <span className="mono">{s.config.repo}</span>
                  {s.config.labels.length > 0 && ` · label：${s.config.labels.join('、')}`}
                  {s.config.write_back && ' · 完成后回写'}
                </>
              ) : (
                <>
                  飞书文档 · <span className="mono">{s.config.app_id}</span> · 未对真实飞书验证
                </>
              )}
              {s.last_synced_at ? ` · 上次同步 ${s.last_synced_at.slice(0, 16).replace('T', ' ')}` : ' · 还没同步过'}
            </p>
            {s.last_error && <p style={{ color: 'var(--cinnabar)' }}>上次失败：{s.last_error}</p>}
            {s.kind === 'feishu' && !s.has_secret && <p style={{ color: 'var(--cinnabar)' }}>还没有填 App Secret。</p>}
          </div>
          <div className="sctl" style={{ display: 'flex', gap: 8, alignItems: 'center' }}>
            {!s.enabled && <span className="tag amber">已停用</span>}
            {s.kind === 'feishu' && (
              <button className="btn sm" disabled={!s.enabled || busy === s.id} onClick={() => setImporting(s)}>
                导入文档…
              </button>
            )}
            <button className="btn sm" disabled={!s.enabled || busy === s.id} onClick={() => void run(s)}>
              {busy === s.id ? '同步中…' : s.kind === 'feishu' ? '检查更新' : '同步'}
            </button>
            <button className="btn sm" onClick={() => setEditing(s)}>
              编辑
            </button>
            <button className="btn sm" onClick={() => void toggle(s)}>
              {s.enabled ? '停用' : '启用'}
            </button>
            <button className="btn sm ghost" onClick={() => void remove(s)} aria-label={`删除 ${s.name}`}>
              <Icon name="x" size={13} />
            </button>
          </div>
        </div>
      ))}
      <div className="srow">
        <div>
          <b>添加来源</b>
          <p>令牌和 App Secret 只存进系统凭据管理器，不进数据库。</p>
        </div>
        <div className="sctl">
          <button className="btn sm" onClick={() => setEditing('new')}>
            添加…
          </button>
        </div>
      </div>
      {editing && (
        <SourceEditor
          current={editing === 'new' ? null : editing}
          onClose={() => setEditing(null)}
          onSaved={() => {
            setEditing(null);
            refresh();
          }}
        />
      )}
      {importing && (
        <Modal
          title="按链接导入飞书文档"
          hint="支持新版文档（/docx/）和知识库（/wiki/）里的新版文档。导入后是一张草稿工单，图片作为参考图附上。"
          width={620}
          onClose={() => setImporting(null)}
          footer={
            <>
              <span className="grow" />
              <button className="btn" onClick={() => setImporting(null)}>
                取消
              </button>
              <button className="btn primary" disabled={!url.trim() || busy === importing.id} onClick={() => void importDoc()}>
                {busy === importing.id ? '导入中…' : '导入'}
              </button>
            </>
          }
        >
          <input className="input mono" style={{ width: '100%' }} value={url} onChange={(e) => setUrl(e.target.value)} placeholder="https://<企业>.feishu.cn/docx/…" aria-label="飞书文档链接" autoFocus />
        </Modal>
      )}
    </div>
  );
}

const SHELL_NAME: Record<Settings['agent_shell'], string> = {
  auto: '自动',
  pwsh: 'PowerShell 7',
  powershell: 'Windows PowerShell',
  git_bash: 'Git Bash',
};

const SHELL_HINT: Record<Settings['agent_shell'], string> = {
  auto: '按 Agent 自己的默认（Codex 用 PowerShell，Claude Code 用 Git Bash），只是避开 Microsoft Store 版 pwsh 的别名，Agent 启动它会被拒绝访问',
  pwsh: '要求用 MSI 装的 PowerShell 7（Store 版不行）。Claude Code 额外开启 PowerShell 工具',
  powershell: '系统自带的 powershell.exe。Claude Code 额外开启 PowerShell 工具',
  git_bash: 'Claude Code 用 Git for Windows 的 bash。Codex 只能用 PowerShell，不受影响',
};

const PERMISSION_HINT: Record<string, string> = {
  read_only: '只读，不改文件',
  workspace_write: '可改 worktree 内的文件',
  full_auto: '不经确认执行任意命令',
};

function UpdateProgressLine({ p }: { p: UpdateProgress }) {
  if (p.phase !== 'downloading') {
    return <p>{p.phase === 'checking' ? '正在查找安装包…' : p.phase === 'installing' ? '签名校验通过，正在安装…' : '安装完成，正在重启…'}</p>;
  }
  const mb = (n: number) => (n / 1048576).toFixed(1);
  const pct = p.total ? Math.min(100, Math.round((p.downloaded / p.total) * 100)) : null;
  return (
    <>
      <p>
        正在下载 {mb(p.downloaded)} MB{p.total ? ` / ${mb(p.total)} MB（${pct}%）` : ''}
      </p>
      <div className="bar" style={{ maxWidth: 320 }}>
        <i style={{ width: `${pct ?? 0}%` }} />
      </div>
    </>
  );
}

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

  const [backups, setBackups] = useState<BackupFile[]>([]);

  useEffect(() => {
    api.systemInfo().then(setInfo, () => undefined);
    api.backupList().then(setBackups, () => undefined);
  }, []);
  useEffect(() => setWorkspace(settings.workspace_dir ?? ''), [settings.workspace_dir]);

  const save = async (patch: Partial<Settings>) => {
    if ((await ui.attempt(api.settingsSave({ ...settings, ...patch }))) !== undefined) await reload();
  };

  const pickWorkspace = async () => {
    const dir = await open({ directory: true, multiple: false });
    if (typeof dir === 'string') await save({ workspace_dir: dir });
  };

  const { info: update, progress, error: updateError } = useUpdate();
  const checkUpdate = async () => {
    const u = await ui.attempt(api.updateCheck());
    if (u) setUpdateInfo(u);
  };
  const installUpdate = async () => {
    if (!update?.latest) return;
    const ok = await ui.confirm(
      `安装 ${update.latest}`,
      <>
        <p>观澜会下载安装包，校验签名通过后安装，然后自动重启。校验不过不会安装。</p>
        <p>有运行在进行时不能安装；排队中的运行会在重启后继续。</p>
      </>,
      '下载并安装',
    );
    if (ok) await runInstall();
  };
  const backupNow = async () => {
    const list = await ui.attempt(api.backupNow(), '已备份数据库');
    if (list) setBackups(list);
  };
  const exportAll = async () => {
    const dir = await open({ directory: true, multiple: false, title: '选择导出到哪个目录' });
    if (typeof dir !== 'string') return;
    const done = await ui.attempt(api.exportTickets(dir));
    if (!done) return;
    ui.toast(
      `已导出 ${done.tickets} 个工单、${done.attachments} 个附件到 ${done.dir}` +
        (done.missing.length ? `；${done.missing.length} 个附件的文件已经不在，没有导出` : ''),
    );
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
              <p>这一轮的花费（Agent 上报，或按 Provider 单价估算）超过它就停止并记为失败。单位：美元，0 表示不限。Claude Code 运行中途按 Provider 单价估算（没填单价就只能在结束时判定）；Codex 只在一轮结束时上报用量，超限只能事后记为失败。</p>
            </div>
            <div className="sctl">{stepper(settings.budget_usd_per_run, 0, 100, 1, (n) => void save({ budget_usd_per_run: n }))}</div>
          </div>
          <div className="srow">
            <div>
              <b>单次运行 Token 上限</b>
              <p>输入加输出 Token 超过它就停止。单位：万 Token，0 表示不限。Claude Code 每次请求后累计一次；Codex 只在一轮结束时上报。</p>
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
          <div className="srow">
            <div>
              <b>开发时也发送已被取代的图</b>
              <p>PRD 里被你替换掉的原图默认只在预审时发给 Agent；开发阶段不发，免得它照着旧图做。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.send_superseded_images} onChange={(v) => void save({ send_superseded_images: v })} label="开发时也发送已被取代的图" />
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
              <p>
                {PERMISSION_HINT[settings.default_permission]}。对所有 Agent 生效，子任务上可以单独改。选 full_auto 时，每次开始都要确认；AI 预审、拆分和影响分析也以 full_auto 运行，其他档位下它们只读。
              </p>
            </div>
            <div className="sctl">
              <Seg
                value={settings.default_permission}
                onChange={async (v) => {
                  if (
                    v === 'full_auto' &&
                    !(await ui.confirm('把 full_auto 设为默认', 'Codex 用 danger-full-access，Claude Code 用 bypassPermissions，Gemini 用 yolo：Agent 不经确认执行任意命令。每次开始仍会再确认一次。', '设为默认', true))
                  )
                    return;
                  void save({ default_permission: v });
                }}
                options={PERMISSIONS.map((p) => [p, p] as const)}
              />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>Agent 使用的 Shell（Windows）</b>
              <p>{SHELL_HINT[settings.agent_shell]}</p>
            </div>
            <div className="sctl">
              <select
                className="input"
                style={{ width: 220 }}
                value={settings.agent_shell}
                onChange={(e) => void save({ agent_shell: e.target.value as Settings['agent_shell'] })}
              >
                {Object.entries(SHELL_NAME).map(([k, v]) => (
                  <option key={k} value={k}>
                    {v}
                  </option>
                ))}
              </select>
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
              <b>检查失败自动修复</b>
              <p>自动检查没过时，把失败的命令和输出交回给同一个 Agent 接着修，最多修这么多次；还不过再交给你验收。0 表示不自动修。</p>
            </div>
            <div className="sctl">{stepper(settings.auto_fix_attempts, 0, 5, 1, (n) => void save({ auto_fix_attempts: n }))}</div>
          </div>
          <div className="srow">
            <div>
              <b>自动 AI 交叉审查</b>
              <p>子任务进入待验收时，自动让另一个 Agent 只读审查 Diff，问题列表显示在验收页供你参考。会多花一轮费用；关掉时可以在验收页手动发起。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.auto_cross_review} onChange={(v) => void save({ auto_cross_review: v })} label="自动 AI 交叉审查" />
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
              <b>数据库备份</b>
              <p>每天自动备份一次，保留最近 7 份。密钥存在系统凭据管理器里，不在数据库中，备份不含密钥。</p>
              {backups.length ? (
                <p className="mono">
                  最近一份：{backups[0].name}（{(backups[0].bytes / 1024).toFixed(0)} KB），共 {backups.length} 份
                </p>
              ) : (
                <p>还没有备份。</p>
              )}
              {backups[0] && <p className="mono">{backups[0].path.slice(0, -backups[0].name.length)}</p>}
            </div>
            <div className="sctl">
              <button className="btn sm" onClick={() => void backupNow()}>
                立即备份
              </button>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>导出工单</b>
              <p>把全部工单导出成 Markdown 归档：每个工单一个文件夹（工单、各版本 PRD、附件），另带一份数据库快照。</p>
            </div>
            <div className="sctl">
              <button className="btn sm" onClick={() => void exportAll()}>
                导出…
              </button>
            </div>
          </div>
        </div>

        <h3 id="set-6">{SECTIONS[6]}</h3>
        <ApiSection settings={settings} save={save} />

        <h3 id="set-7">{SECTIONS[7]}</h3>
        <SyncSection />

        <h3 id="set-8">{SECTIONS[8]}</h3>
        <WorkflowSection />

        <h3 id="set-9">{SECTIONS[9]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>观澜</b>
              <p>{info ? `${info.version} · ${info.os}` : '读取中…'}</p>
              {update &&
                (update.newer ? (
                  <>
                    <p>
                      有新版本 {update.latest}。
                      {update.install_blocked
                        ? `${update.install_blocked}请到发布页下载：`
                        : '可以直接在这里安装，也可以到发布页下载：'}
                      {update.url && (
                        <a
                          className="mono"
                          href={update.url}
                          onClick={(e) => {
                            e.preventDefault();
                            void ui.attempt(api.openUrl(update.url ?? ''));
                          }}
                        >
                          {update.url}
                        </a>
                      )}
                    </p>
                    {update.notes && (
                      <details className="rec-why">
                        <summary>这一版的更新说明</summary>
                        <p style={{ whiteSpace: 'pre-wrap' }}>{update.notes}</p>
                      </details>
                    )}
                    {progress && <UpdateProgressLine p={progress} />}
                    {updateError && <p style={{ color: 'var(--cinnabar)' }}>{updateError}</p>}
                  </>
                ) : (
                  <p>{update.latest ? `已经是最新版本（最新发布 ${update.latest}）。` : '还没有发布过正式版本。'}</p>
                ))}
            </div>
            <div className="sctl">
              {update?.newer && !update.install_blocked && (
                <button className="btn sm primary" disabled={!!progress} onClick={() => void installUpdate()}>
                  {progress ? '正在更新…' : '下载并安装'}
                </button>
              )}
              <button className="btn sm" disabled={!!progress} onClick={() => void checkUpdate()}>
                检查更新
              </button>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>启动时检查更新</b>
              <p>打开后，每次启动联网读一次 GitHub 上的最新版本号，不发送任何本机数据，也不会自动下载或安装（安装要你点「下载并安装」并确认）。默认关闭。</p>
            </div>
            <div className="sctl">
              <Switch on={settings.check_updates} onChange={(v) => void save({ check_updates: v })} label="启动时检查更新" />
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
                <p className="mono">
                  {a.installed ? (a.path ?? '内置') : AGENTS[a.kind]?.install ? `未检测到命令行版，安装：${AGENTS[a.kind].install}` : '未检测到'}
                </p>
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
