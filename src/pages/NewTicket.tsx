// 新建工单：左边放原文，右边是预填的属性
import { open } from '@tauri-apps/plugin-dialog';
import { useState } from 'react';
import { api, type Priority, type TicketStatus, type TicketType } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { Field, PRIO, Prio, STAGE_NAME, Seg, Stamp, TYPES, go, useUi } from '../lib/ui';

type Mode = 'paste' | 'file' | 'form';

const KIND_NAME: Record<string, string> = { backend: '后端', frontend: '前端', fullstack: '全栈', other: '其它' };

export function NewTicketPage() {
  const { projects, reload } = useData();
  const ui = useUi();
  const [mode, setMode] = useState<Mode>('paste');
  const [raw, setRaw] = useState('');
  const [fileName, setFileName] = useState<string | null>(null);
  const [title, setTitle] = useState('');
  const [type, setType] = useState<TicketType>('feature_mixed');
  const [priority, setPriority] = useState<Priority>('medium');
  const [picked, setPicked] = useState<string[]>([]);
  const [labels, setLabels] = useState('');
  const [due, setDue] = useState('');
  const [recognized, setRecognized] = useState(false);
  const [saving, setSaving] = useState(false);

  const recognize = async (text: string) => {
    if (!text.trim()) return;
    const d = await ui.attempt(api.ticketDraft(text));
    if (!d) return;
    setTitle(d.title);
    setType(d.type);
    // 正文里提到了哪个项目的名字，就预选它
    const hit = projects.filter((p) => text.toLowerCase().includes(p.name.toLowerCase())).map((p) => p.id);
    setPicked(hit.length ? hit : projects.length === 1 ? [projects[0].id] : []);
    setRecognized(true);
  };

  const pickFile = async () => {
    const path = await open({ multiple: false, filters: [{ name: 'Markdown / 文本', extensions: ['md', 'markdown', 'txt'] }] });
    if (typeof path !== 'string') return;
    const text = await ui.attempt(api.readTextFile(path));
    if (text === undefined) return;
    setRaw(text);
    setFileName(path.split(/[\\/]/).pop() ?? path);
    await recognize(text);
  };

  const showProps = mode === 'form' || recognized;

  const save = async (to: TicketStatus) => {
    if (!title.trim()) {
      ui.toast(mode === 'form' ? '请先填写标题' : '请先粘贴或导入需求内容，再点「识别」');
      return;
    }
    if (to === 'developing' && picked.length === 0) {
      ui.toast('开始修复前请先绑定一个项目');
      return;
    }
    setSaving(true);
    try {
      const t = await api.ticketCreate({
        title: title.trim(),
        type,
        priority,
        labels: labels.split(/[,，\s]+/).filter(Boolean),
        due_date: due || null,
        content_md: raw,
        projects: picked.map((project_id) => ({
          project_id,
          role: projects.find((p) => p.id === project_id)?.kind === 'frontend' ? 'frontend' : 'backend',
          subdir: null,
        })),
        acceptance_criteria: [],
        source_kind: mode === 'file' ? 'file' : 'manual',
        default_agent: null,
        default_provider_id: null,
      });
      let note = STAGE_NAME.draft;
      if (to !== 'draft') {
        await api.ticketTransition(t.id, to);
        note = STAGE_NAME[to];
      }
      if (to === 'developing') {
        const task = await api.taskCreate({
          ticket_id: t.id,
          kind: 'bug',
          project_id: picked[0],
          title: t.title,
          spec_md: raw,
          acceptance_criteria: [],
          agent: null,
          provider_id: null,
          model: null,
          permission_profile: null,
          depends_on: [],
        });
        await api.taskStart(task.id);
        note = '已开始修复';
      }
      ui.toast(`已创建 ${t.key}「${t.title}」· ${note}`);
      await reload();
      go(to === 'developing' ? `#/ticket/${t.key}/tasks` : `#/ticket/${t.key}`);
    } catch (e) {
      ui.toast(String(e));
      void reload();
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="page flush newpage">
      <div className="new-l">
        <Seg
          className="lgseg"
          value={mode}
          onChange={setMode}
          options={[
            ['paste', <><Icon name="paste" size={14} />粘贴</>],
            ['file', <><Icon name="upload" size={14} />导入文件</>],
            ['form', <><Icon name="form" size={14} />表单</>],
          ]}
        />
        <div className="new-body">
          {mode === 'paste' && (
            <>
              <textarea
                className="textarea paste"
                value={raw}
                onChange={(e) => setRaw(e.target.value)}
                placeholder={'把聊天记录、PRD 或 Bug 描述整段粘贴到这里…'}
                aria-label="需求原文"
              />
              <div className="row" style={{ marginTop: 10 }}>
                <button className="btn sm" disabled={!raw.trim()} onClick={() => void recognize(raw)}>
                  <Icon name="spark" size={13} />
                  识别标题和类型
                </button>
                <span className="dim">支持 Markdown；识别按关键词在本机完成，不调用模型</span>
              </div>
            </>
          )}
          {mode === 'file' && (
            <>
              <div className="drop">
                <Icon name="upload" size={26} />
                <b>选择一个 Markdown 或文本文件</b>
                <span>.md · .markdown · .txt，2 MB 以内</span>
                <button className="btn sm" onClick={() => void pickFile()}>
                  选择文件
                </button>
              </div>
              {fileName && (
                <div className="filerow">
                  <Icon name="file" size={15} />
                  <span className="grow">
                    <b>{fileName}</b>
                    <span className="dim"> · {raw.length} 字</span>
                  </span>
                  <span className="tag moss">
                    <Icon name="check" size={11} />
                    已读入
                  </span>
                </div>
              )}
            </>
          )}
          {mode === 'form' && (
            <div className="form-grid" style={{ gridTemplateColumns: '1fr' }}>
              <Field label="标题">
                <input className="input" value={title} onChange={(e) => setTitle(e.target.value)} placeholder="一句话说明要做什么" />
              </Field>
              <Field label="需求正文" hint="Markdown">
                <textarea
                  className="textarea"
                  rows={13}
                  value={raw}
                  onChange={(e) => setRaw(e.target.value)}
                  placeholder={'## 背景与目标\n\n## 功能需求\n\n## 验收标准'}
                />
              </Field>
            </div>
          )}
        </div>
      </div>
      <aside className="new-r">
        <div className="sec-title">
          <Icon name="spark" size={15} />
          {mode === 'form' ? '工单属性' : '识别结果'}
          {mode !== 'form' && recognized && <small>已根据原文预填，可修改</small>}
        </div>
        <div className="rec">
          {showProps ? (
            <>
              {mode !== 'form' && (
                <Field label="标题">
                  <input className="input" value={title} onChange={(e) => setTitle(e.target.value)} />
                </Field>
              )}
              <Field label="类型">
                <div className="pick">
                  {(Object.keys(TYPES) as TicketType[]).map((k) => (
                    <button key={k} className={k === type ? 'on' : ''} onClick={() => setType(k)}>
                      <Stamp type={k} />
                      {TYPES[k].name}
                    </button>
                  ))}
                </div>
              </Field>
              <Field label="优先级">
                <div className="pick">
                  {(Object.keys(PRIO) as Priority[]).map((k) => (
                    <button key={k} className={k === priority ? 'on' : ''} onClick={() => setPriority(k)}>
                      <Prio p={k} />
                    </button>
                  ))}
                </div>
              </Field>
              <Field label="绑定项目" hint="可多选">
                {projects.length ? (
                  <div className="pick">
                    {projects.map((p) => (
                      <button
                        key={p.id}
                        className={picked.includes(p.id) ? 'on' : ''}
                        onClick={() => setPicked((s) => (s.includes(p.id) ? s.filter((x) => x !== p.id) : [...s, p.id]))}
                      >
                        <Icon name="folder" size={13} />
                        {p.name}
                        <span className="dim">{KIND_NAME[p.kind] ?? p.kind}</span>
                      </button>
                    ))}
                  </div>
                ) : (
                  <p className="dim">
                    还没有注册项目。<a className="lnk" href="#/projects">去注册 →</a>
                  </p>
                )}
              </Field>
              <div className="form-grid">
                <Field label="标签" hint="逗号或空格分隔">
                  <input className="input" value={labels} onChange={(e) => setLabels(e.target.value)} placeholder="例如：支付 导出" />
                </Field>
                <Field label="截止日期" hint="可选">
                  <input className="input" type="date" value={due} onChange={(e) => setDue(e.target.value)} />
                </Field>
              </div>
            </>
          ) : (
            <div className="rec-empty">
              <Icon name="spark" size={22} />
              <b>粘贴或导入后，点「识别」预填</b>
              <span>标题、类型和提到的项目；所有预填都可以改。</span>
            </div>
          )}
        </div>
      </aside>
      <footer className="new-f">
        <span className="dim">
          {showProps && type === 'bug' ? 'Bug 走快速通道：跳过预审与人工审核，直接进入开发' : '需求工单要经过预审与人工审核后才能开发'}
        </span>
        <span className="grow" />
        <a className="btn ghost" href="#/board">
          取消
        </a>
        <button className="btn" disabled={saving} onClick={() => void save('draft')}>
          保存为草稿
        </button>
        {showProps && type === 'bug' ? (
          <button className="btn primary" disabled={saving} onClick={() => void save('developing')}>
            <Icon name="zap" size={14} />
            保存并开始修复
          </button>
        ) : (
          <button className="btn" disabled={saving} onClick={() => void save('ai_review')}>
            保存并提交预审
          </button>
        )}
      </footer>
    </div>
  );
}
