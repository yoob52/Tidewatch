// 设置：工作区与并发、默认执行配置、自动化、通知与外观、本机环境
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useState } from 'react';
import { api, type Detection, type Settings, type SystemInfo } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { AGENTS, PERMISSIONS, Seg, Switch, useUi } from '../lib/ui';

const SECTIONS = ['工作区与并发', '默认执行配置', '自动化', '通知与外观', '本机环境'];
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
              <span>子任务的 worktree 建在这里；留空则放在数据目录下。Windows 上建议用短路径。</span>
            </div>
            <div className="sctl">
              <input
                className="input mono"
                style={{ width: 300 }}
                value={workspace}
                onChange={(e) => setWorkspace(e.target.value)}
                onBlur={() => workspace !== (settings.workspace_dir ?? '') && void save({ workspace_dir: workspace || null })}
                placeholder="默认：数据目录/worktrees"
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
              <span>同时运行的 Agent 数；超出的进入队列，按工单优先级和入队时间出队。</span>
            </div>
            <div className="sctl">{stepper(settings.max_concurrency, 1, 8, 1, (n) => void save({ max_concurrency: n }))}</div>
          </div>
          <div className="srow">
            <div>
              <b>单次运行超时</b>
              <span>超过这个时长，观澜会停止 Agent 并把运行记为失败。单位：分钟。</span>
            </div>
            <div className="sctl">{stepper(settings.run_timeout_minutes, 10, 480, 10, (n) => void save({ run_timeout_minutes: n }))}</div>
          </div>
        </div>

        <h3 id="set-1">{SECTIONS[1]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>默认 Agent</b>
              <span>优先级：子任务 → 工单 → 项目 → 这里。</span>
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
              <span>不选则用 Agent 在本机登录的账号。</span>
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
              <span>{PERMISSION_HINT[settings.default_permission]}。full_auto 不能设为默认，只能在子任务上单独选，每次开始都要确认。</span>
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
              <b>运行结束后自动检查</b>
              <span>依次执行项目配置的 Lint、类型检查、测试和构建命令，结果显示在验收页。</span>
            </div>
            <div className="sctl">
              <Switch on={settings.auto_test} onChange={(v) => void save({ auto_test: v })} label="运行结束后自动检查" />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>合并方式</b>
              <span>merge 保留子任务分支上的每个提交；squash 压成一个提交。观澜不会自动推送到远程。</span>
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
              <span>worktree 总会清理；分支默认保留。</span>
            </div>
            <div className="sctl">
              <Switch on={settings.delete_branch_after_merge} onChange={(v) => void save({ delete_branch_after_merge: v })} label="合并后删除子任务分支" />
            </div>
          </div>
        </div>

        <h3 id="set-3">{SECTIONS[3]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>系统通知</b>
              <span>运行完成、失败或被中断时弹出桌面通知。</span>
            </div>
            <div className="sctl">
              <Switch on={settings.notifications} onChange={(v) => void save({ notifications: v })} label="系统通知" />
            </div>
          </div>
          <div className="srow">
            <div>
              <b>外观</b>
              <span>只保存在这台机器上。</span>
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

        <h3 id="set-4">{SECTIONS[4]}</h3>
        <div className="card">
          <div className="srow">
            <div>
              <b>观澜</b>
              <span>{info ? `${info.version} · ${info.os}` : '读取中…'}</span>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>数据库</b>
              <span className="mono">{info?.db_path ?? '—'}</span>
              {info && (
                <span>
                  结构第 {info.schema_version} 版 · SQLite {info.sqlite_version}
                </span>
              )}
            </div>
          </div>
          <div className="srow">
            <div>
              <b>密钥存放</b>
              <span>{info?.secret_store ?? '—'}</span>
            </div>
          </div>
          <div className="srow">
            <div>
              <b>Git</b>
              <span>{info ? (info.git_version ?? '未安装：worktree 隔离需要 git') : '—'}</span>
              {info?.long_paths_hint && <span>{info.long_paths_hint}</span>}
            </div>
          </div>
          {(agents ?? []).map((a) => (
            <div className="srow" key={a.kind}>
              <div>
                <b>{AGENTS[a.kind]?.name ?? a.kind}</b>
                <span className="mono">{a.installed ? (a.path ?? '内置') : '未检测到'}</span>
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
                <span>检测中…</span>
              </div>
            </div>
          )}
        </div>
      </section>
    </div>
  );
}
