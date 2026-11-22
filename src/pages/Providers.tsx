// Provider：协议、Base URL、密钥（只进系统凭据管理器）、模型与价格
import { useEffect, useState } from 'react';
import { api, type Provider, type ProviderCompat, type ProviderInput } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { Empty, Field, Md, Modal, Switch, go, useUi } from '../lib/ui';

const PROTOCOLS: [ProviderInput['protocol'], string, string][] = [
  ['native', '本机登录态', '不注入任何变量，用 Agent 自己登录的账号'],
  ['anthropic', 'Anthropic 兼容', '给 Claude Code 用：注入 ANTHROPIC_BASE_URL 和密钥'],
  ['openai_responses', 'OpenAI Responses', '给 Codex 用：Responses 接口'],
  ['openai_chat', 'OpenAI Chat', '给 Codex 用：Chat Completions 接口'],
];

const TEMPLATES: [string, Partial<ProviderInput>][] = [
  ['Anthropic 官方', { protocol: 'anthropic', base_url: 'https://api.anthropic.com' }],
  ['DeepSeek', { protocol: 'anthropic', base_url: 'https://api.deepseek.com/anthropic', models: ['deepseek-chat'], default_model: 'deepseek-chat' }],
  ['OpenAI 官方', { protocol: 'openai_responses', base_url: 'https://api.openai.com/v1' }],
  ['自定义', {}],
];

const blank: ProviderInput = {
  name: '',
  protocol: 'anthropic',
  base_url: null,
  models: [],
  default_model: null,
  small_model: null,
  extra_env: [],
  pricing: null,
  proxy: null,
  text_only: false,
};

const toInput = (p: Provider): ProviderInput => ({
  name: p.name,
  protocol: p.protocol,
  base_url: p.base_url,
  models: p.models,
  default_model: p.default_model,
  small_model: p.small_model,
  extra_env: p.extra_env,
  pricing: p.pricing,
  proxy: p.proxy,
  text_only: p.text_only,
});

const num = (v: string) => (Number.isFinite(Number(v)) && v.trim() !== '' ? Number(v) : 0);

export function ProvidersPage({ id }: { id?: string }) {
  const { providers, runs, reload } = useData();
  const ui = useUi();
  const isNew = id === 'new' || providers.length === 0;
  const current = isNew ? undefined : (providers.find((p) => p.id === id) ?? providers[0]);
  const [f, setF] = useState<ProviderInput>(blank);
  const [secret, setSecret] = useState('');
  const [env, setEnv] = useState('');
  const [dirty, setDirty] = useState(false);
  const [compat, setCompat] = useState<ProviderCompat[]>([]);
  const [guide, setGuide] = useState<ProviderCompat | null>(null);

  useEffect(() => {
    let live = true;
    api.providerCompat(f.name || '这个 Provider', f.protocol, f.base_url, f.default_model).then(
      (c) => live && setCompat(c),
      () => live && setCompat([]),
    );
    return () => {
      live = false;
    };
  }, [f.name, f.protocol, f.base_url, f.default_model]);

  useEffect(() => {
    const next = current ? toInput(current) : blank;
    setF(next);
    setEnv(next.extra_env.map(([k, v]) => `${k}=${v}`).join('\n'));
    setSecret('');
    setDirty(false);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [current?.id, current?.updated_at, isNew]);

  const set = (patch: Partial<ProviderInput>) => {
    setF((x) => ({ ...x, ...patch }));
    setDirty(true);
  };

  const save = async () => {
    const extra_env = env
      .split('\n')
      .map((l) => l.trim())
      .filter((l) => l.includes('='))
      .map((l) => [l.slice(0, l.indexOf('=')).trim(), l.slice(l.indexOf('=') + 1)] as [string, string]);
    const p = await ui.attempt(api.providerSave(current?.id ?? null, { ...f, extra_env }, secret ? secret : undefined), '已保存');
    if (!p) return;
    setSecret('');
    await reload();
    setDirty(false);
    go(`#/providers/${p.id}`);
  };

  const clearSecret = async () => {
    if (!current) return;
    if (!(await ui.confirm('清除密钥', `从系统凭据管理器里删除「${current.name}」的密钥。之后用它的运行会因为没有密钥而失败。`, '清除', true))) return;
    if (await ui.attempt(api.providerSave(current.id, toInput(current), ''), '已清除密钥')) await reload();
  };

  const remove = async () => {
    if (!current) return;
    if (!(await ui.confirm('删除 Provider', `删除「${current.name}」和它保存的密钥。历史运行记录保留。`, '删除', true))) return;
    if ((await ui.attempt(api.providerDelete(current.id), '已删除')) !== undefined) {
      await reload();
      go('#/providers');
    }
  };

  const used = current ? runs.filter((r) => r.provider_id === current.id) : [];
  const cost = used.reduce((a, r) => a + (r.cost_usd ?? 0), 0);

  return (
    <div className="page flush split">
      <aside className="plist wide">
        <div className="row" style={{ padding: '14px 14px 8px' }}>
          <b className="grow">{providers.length} 个 Provider</b>
          <a className="btn sm" href="#/providers/new">
            <Icon name="plus" size={13} />
            添加
          </a>
        </div>
        {providers.map((x) => (
          <a key={x.id} className={`pi ${x.id === current?.id ? 'on' : ''}`} href={`#/providers/${x.id}`}>
            <div className="row">
              <b>{x.name}</b>
              <span className="grow" />
              {x.protocol === 'native' ? (
                <span className="tag line">登录态</span>
              ) : x.secret_ref ? (
                <span className="tag moss">已存密钥</span>
              ) : (
                <span className="tag amber">缺密钥</span>
              )}
            </div>
            <small className="mono ell">{x.base_url ?? PROTOCOLS.find(([k]) => k === x.protocol)?.[1]}</small>
            <small className="mono ell">{x.default_model ?? '默认模型'}</small>
          </a>
        ))}
        {providers.length === 0 && <Empty icon="plug" title="还没有 Provider" body="不添加也可以运行：Agent 会用它自己在本机登录的账号。" />}
      </aside>
      <section className="pdetail narrow">
        <div className="row" style={{ marginBottom: 12 }}>
          <h2>{current ? current.name : '添加 Provider'}</h2>
          <span className="grow" />
          {current && (
            <button className="btn danger sm" onClick={() => void remove()}>
              删除
            </button>
          )}
          <button className="btn primary sm" disabled={!dirty && !secret} onClick={() => void save()}>
            保存
          </button>
        </div>
        {current && used.length > 0 && (
          <div className="kpis" style={{ marginBottom: 14 }}>
            <div>
              <span>最近运行</span>
              <b>{used.length}</b>
            </div>
            <div>
              <span>费用</span>
              <b>${cost.toFixed(2)}</b>
            </div>
          </div>
        )}
        <div className="form">
          {!current && (
            <Field label="从模板开始">
              <div className="pick">
                {TEMPLATES.map(([n, patch]) => (
                  <button key={n} onClick={() => set({ ...blank, ...patch, name: n === '自定义' ? f.name : n })}>
                    {n}
                  </button>
                ))}
              </div>
            </Field>
          )}
          <Field label="名称">
            <input className="input" value={f.name} onChange={(e) => set({ name: e.target.value })} placeholder="例如：DeepSeek" />
          </Field>
          <Field label="协议" hint={PROTOCOLS.find(([k]) => k === f.protocol)?.[2]}>
            <div className="seg">
              {PROTOCOLS.map(([k, n]) => (
                <button key={k} className={k === f.protocol ? 'on' : ''} onClick={() => set({ protocol: k })}>
                  {n}
                </button>
              ))}
            </div>
          </Field>
          {compat.length > 0 && (
            <Field label="适用的 Agent" hint={compat.some((c) => !c.compatible) ? '协议不匹配的 Agent 要经 LiteLLM 之类的代理桥接' : undefined}>
              <div className="row" style={{ flexWrap: 'wrap', gap: 8 }}>
                {compat.map((c) =>
                  c.compatible ? (
                    <span key={c.agent} className="tag moss">
                      {c.agent_name} 可直接使用
                    </span>
                  ) : (
                    <button key={c.agent} className="btn sm" onClick={() => setGuide(c)}>
                      {c.agent_name} 不能直接用 · 看桥接配置
                    </button>
                  ),
                )}
              </div>
            </Field>
          )}
          {f.protocol !== 'native' && (
            <>
              <Field label="Base URL">
                <input className="input mono" value={f.base_url ?? ''} onChange={(e) => set({ base_url: e.target.value || null })} placeholder="https://…" />
              </Field>
              <Field label="API Key" hint={current?.secret_ref ? '已保存；留空表示不改' : '只写入系统凭据管理器，不进数据库'}>
                <div className="row">
                  <input
                    className="input mono grow"
                    type="password"
                    autoComplete="off"
                    value={secret}
                    onChange={(e) => setSecret(e.target.value)}
                    placeholder={current?.secret_ref ? '••••••••（已保存）' : 'sk-…'}
                  />
                  {current?.secret_ref && (
                    <button className="btn sm" onClick={() => void clearSecret()}>
                      清除
                    </button>
                  )}
                </div>
              </Field>
            </>
          )}
          <div className="form-grid">
            <Field label="默认模型">
              <input className="input mono" value={f.default_model ?? ''} onChange={(e) => set({ default_model: e.target.value || null })} />
            </Field>
            <Field label="小模型" hint="可选，Claude Code 的后台任务用">
              <input className="input mono" value={f.small_model ?? ''} onChange={(e) => set({ small_model: e.target.value || null })} />
            </Field>
          </div>
          <Field label="可选模型" hint="逗号或空格分隔">
            <input
              className="input mono"
              value={f.models.join(', ')}
              onChange={(e) => set({ models: e.target.value.split(/[,，\s]+/).filter(Boolean) })}
            />
          </Field>
          <Field label="价格（美元 / 百万 Token）" hint="Agent 不自报费用时用它估算；全部留 0 表示不估算">
            <div className="form-grid" style={{ gridTemplateColumns: '1fr 1fr 1fr' }}>
              {(
                [
                  ['input_per_mtok', '输入'],
                  ['output_per_mtok', '输出'],
                  ['cache_per_mtok', '缓存读取'],
                ] as const
              ).map(([k, n]) => (
                <label key={k} className="row">
                  <span className="dim">{n}</span>
                  <input
                    className="input mono grow"
                    inputMode="decimal"
                    value={f.pricing?.[k] ?? 0}
                    onChange={(e) => {
                      const pricing = { input_per_mtok: 0, output_per_mtok: 0, cache_per_mtok: 0, ...f.pricing, [k]: num(e.target.value) };
                      set({ pricing: pricing.input_per_mtok || pricing.output_per_mtok || pricing.cache_per_mtok ? pricing : null });
                    }}
                  />
                </label>
              ))}
            </div>
          </Field>
          <div className="form-grid">
            <Field label="代理" hint="可选">
              <input className="input mono" value={f.proxy ?? ''} onChange={(e) => set({ proxy: e.target.value || null })} placeholder="http://127.0.0.1:7890" />
            </Field>
          </div>
          <Field label="图片" hint="模型看不了图时打开：PRD 里的图片改发文字描述">
            <Switch on={f.text_only} onChange={(v) => set({ text_only: v })} label="只能处理文字" />
          </Field>
          <Field label="额外环境变量" hint="每行一条 NAME=value；不要在这里放密钥">
            <textarea
              className="textarea mono"
              rows={3}
              value={env}
              onChange={(e) => {
                setEnv(e.target.value);
                setDirty(true);
              }}
            />
          </Field>
          <div className="callout tide">
            <Icon name="shield" size={15} />
            <div>密钥只在启动对应的 Agent 进程时注入，不会写进数据库、日志或 Prompt。</div>
          </div>
        </div>
      </section>
      {guide?.bridge_guide && (
        <Modal
          title={`经 LiteLLM 让 ${guide.agent_name} 使用这个 Provider`}
          hint="示例里不含密钥：LiteLLM 从环境变量读上游密钥。"
          onClose={() => setGuide(null)}
          footer={
            <button className="btn" onClick={() => setGuide(null)}>
              关闭
            </button>
          }
        >
          <Md src={guide.bridge_guide} />
        </Modal>
      )}
    </div>
  );
}
