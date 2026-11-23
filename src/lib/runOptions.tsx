import { useId, useState } from 'react';
import { api, type DevelopmentTaskPlan, type Provider, type RunOverrides, type TicketRunConfig, type TicketRunOptions } from './api';
import { useData } from './data';
import { tr } from './i18n';
import { AgentTag, Field, Modal, useUi } from './ui';

export function RunOptionsFields({ config, providers, value, onChange }: {
  config: TicketRunConfig;
  providers: Provider[];
  value: TicketRunOptions;
  onChange: (options: TicketRunOptions) => void;
}) {
  const id = useId();
  const models = providers.find((p) => p.id === config.provider_id)?.models ?? [];
  const effort = config.agent === 'codex'
    ? ['minimal', 'low', 'medium', 'high', 'xhigh']
    : config.agent === 'claude_code' ? ['low', 'medium', 'high'] : [];
  const runtime = (patch: RunOverrides) => onChange({ ...value, runtime: { ...value.runtime, ...patch } });
  return (
    <div>
      <div className="row" style={{ margin: '10px 0' }}><AgentTag agent={config.agent} /></div>
      <Field label={tr('模型')}>
        <input className="input mono" aria-label={tr('模型')} list={models.length ? id : undefined}
          value={value.runtime?.model ?? ''} placeholder={config.model ?? tr('默认模型')}
          onChange={(e) => runtime({ model: e.target.value })} />
        {models.length > 0 && <datalist id={id}>{models.map((model) => <option key={model} value={model} />)}</datalist>}
      </Field>
      <Field label={tr('思考程度')}>
        <select className="input" aria-label={tr('思考程度')} disabled={!effort.length}
          value={value.runtime?.reasoning_effort ?? ''} onChange={(e) => runtime({ reasoning_effort: e.target.value })}>
          <option value="">{effort.length ? tr('跟随默认') : tr('此 Agent 不支持单独设置思考程度')}</option>
          {effort.map((level) => <option key={level} value={level}>{level}</option>)}
        </select>
      </Field>
      {config.reusable_run && (
        <Field label={tr('会话')}>
          <select className="input" aria-label={tr('会话')} value={value.resume_run_id ?? ''}
            onChange={(e) => onChange({ ...value, resume_run_id: e.target.value || undefined })}>
            <option value="">{tr('新建会话')}</option>
            <option value={config.reusable_run.id}>{tr('复用第 {0} 次工单运行的会话', config.reusable_run.number)}</option>
          </select>
          {value.resume_run_id && <p className="dim">{tr('将整理并沿用之前会话中仍有效的背景和决定，以当前 PRD 和验收标准为准。')}</p>}
        </Field>
      )}
    </div>
  );
}

function RunOptionsDialog({ config, providers, title, finish }: {
  config: TicketRunConfig;
  providers: Provider[];
  title: string;
  finish: (options: TicketRunOptions | null) => void;
}) {
  const [options, setOptions] = useState<TicketRunOptions>({});
  return (
    <Modal title={title} width={480} onClose={() => finish(null)} footer={
      <>
        <button className="btn" onClick={() => finish(null)}>{tr('取消')}</button>
        <button className="btn primary" onClick={() => finish(options)}>{tr('确定')}</button>
      </>
    }>
      <RunOptionsFields config={config} providers={providers} value={options} onChange={setOptions} />
    </Modal>
  );
}

export function useChooseRun() {
  const ui = useUi();
  const { providers } = useData();
  return (config: TicketRunConfig, title: string) =>
    ui.choose<TicketRunOptions>((finish) => <RunOptionsDialog config={config} providers={providers} title={title} finish={finish} />);
}

export function useChooseTicketRun() {
  const ui = useUi();
  const choose = useChooseRun();
  return async (id: string, purpose: 'review' | 'split' | 'impact', title: string) => {
    const config = await ui.attempt(api.ticketRunConfig(id, purpose));
    return config ? choose(config, title) : null;
  };
}

function DevelopmentDialog({ tasks, providers, finish }: {
  tasks: DevelopmentTaskPlan[];
  providers: Provider[];
  finish: (value: Record<string, RunOverrides> | null) => void;
}) {
  const [runtime, setRuntime] = useState<Record<string, RunOverrides>>({});
  return (
    <Modal title={tr('开始开发')} hint={tr('以下设置仅用于本次入队的子任务，等待依赖的子任务仍使用各自的默认设置。')}
      onClose={() => finish(null)} footer={
        <>
          <button className="btn" onClick={() => finish(null)}>{tr('取消')}</button>
          <button className="btn primary" onClick={() => finish(runtime)}>{tr('开始开发')}</button>
        </>
      }>
      {tasks.map(({ id, title, config }) => (
        <fieldset key={id}>
          <legend>{title}</legend>
          <RunOptionsFields config={config} providers={providers} value={{ runtime: runtime[id] }}
            onChange={(options) => setRuntime((prev) => ({ ...prev, [id]: options.runtime ?? {} }))} />
        </fieldset>
      ))}
    </Modal>
  );
}

export function useChooseDevelopment() {
  const ui = useUi();
  const { providers } = useData();
  return async (id: string) => {
    const tasks = await ui.attempt(api.developmentPlan(id));
    if (!tasks) return null;
    if (!tasks.length) return {};
    return ui.choose<Record<string, RunOverrides>>((finish) => <DevelopmentDialog tasks={tasks} providers={providers} finish={finish} />);
  };
}
