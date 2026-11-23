import { tr } from '../lib/i18n';
// 新建工单：左边放原文，右边是预填的属性
import { open } from '@tauri-apps/plugin-dialog';
import { useEffect, useRef, useState } from 'react';
import { api, type Priority, type TicketDraft, type TicketStatus, type TicketType } from '../lib/api';
import { useData } from '../lib/data';
import { Icon } from '../lib/Icon';
import { imageFiles, imageMd, insertAt, parseImages, selectionOf, uploadImage } from '../lib/images';
import { useConfirmTicketRun } from '../lib/start';
import { useChooseRun } from '../lib/runOptions';
import { AGENTS, Field, Md, PRIO, Prio, STAGE_NAME, Seg, Stamp, TYPES, go, useUi } from '../lib/ui';

type Mode = 'paste' | 'file' | 'form';

const KIND_NAME: Record<string, string> = { backend: tr('后端'), frontend: tr('前端'), fullstack: tr('全栈'), other: tr('其它') };

export function NewTicketPage() {
  const { projects, providers, settings, reload } = useData();
  const ui = useUi();
  const confirmRun = useConfirmTicketRun();
  const chooseRun = useChooseRun();
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
  const [bugInfo, setBugInfo] = useState<TicketDraft['bug_info']>(null);
  const [reasons, setReasons] = useState<string[]>([]);
  const [saving, setSaving] = useState(false);
  const [importing, setImporting] = useState(false);
  const [warnings, setWarnings] = useState<string[]>([]);
  /** AI 分析过：是谁分析的。null = 当前结果来自本机规则。 */
  const [analyzedBy, setAnalyzedBy] = useState<string | null>(null);
  const [analyzing, setAnalyzing] = useState(false);
  const [aiError, setAiError] = useState<string | null>(null);
  const area = useRef<HTMLTextAreaElement | null>(null);
  const fileInput = useRef<HTMLInputElement | null>(null);

  const prefill = (d: TicketDraft) => {
    setTitle(d.title);
    setType(d.type);
    setPriority(d.priority);
    setLabels(d.labels.join(' '));
    setPicked(d.project_ids);
    setBugInfo(d.bug_info);
    setReasons(d.reasons);
    setRecognized(true);
  };

  const recognize = async (text: string) => {
    if (!text.trim()) return;
    const d = await ui.attempt(api.ticketDraft(text));
    if (!d) return;
    prefill(d);
    setAnalyzedBy(null);
    setAiError(null);
  };

  // AI 分析：可选的一步。失败或取消时右边保留已有的结果
  const analyze = async () => {
    if (!raw.trim() || analyzing) return;
    if (!recognized) await recognize(raw);
    setAnalyzing(true);
    setAiError(null);
    try {
      const d = await api.ticketAnalyze(raw);
      prefill(d);
      setAnalyzedBy(
        [AGENTS[d.agent]?.name ?? d.agent, d.provider, d.model].filter(Boolean).join(' · '),
      );
    } catch (e) {
      setAiError(String(e));
    } finally {
      setAnalyzing(false);
    }
  };
  // 离开页面时不让分析在后台继续花钱
  useEffect(() => () => void api.ticketAnalyzeCancel(), []);

  const pickFile = async () => {
    const path = await open({
      multiple: false,
      filters: [{ name: tr('需求文档'), extensions: ['md', 'markdown', 'txt', 'docx', 'pdf'] }],
    });
    if (typeof path !== 'string') return;
    setImporting(true);
    const doc = await ui.attempt(api.importDocument(path));
    setImporting(false);
    if (!doc) return;
    setRaw(doc.markdown);
    setFileName(doc.file_name);
    setWarnings(doc.warnings);
    await recognize(doc.markdown);
  };

  // 粘贴 / 拖进来的截图：存成用户图（默认权威），在光标处插入引用
  const addImages = async (files: File[], el: HTMLTextAreaElement | null) => {
    const sel = selectionOf(el);
    const added: string[] = [];
    for (const f of files) {
      const id = await ui.attempt(uploadImage(null, f));
      if (!id) break;
      added.push(imageMd(tr('需求方截图'), id, 'authoritative'));
    }
    setRaw((cur) => insertAt(cur, sel, added));
  };
  const onPaste = (e: React.ClipboardEvent<HTMLTextAreaElement>) => {
    const files = imageFiles(e.clipboardData);
    if (!files.length) return;
    e.preventDefault();
    void addImages(files, e.currentTarget);
  };
  const pickImage = async (files: FileList | null) => {
    if (files?.length) await addImages(Array.from(files), area.current);
    if (fileInput.current) fileInput.current.value = '';
  };
  const imageCount = parseImages(raw).length;

  const showProps = mode === 'form' || recognized;

  const save = async (to: TicketStatus) => {
    if (!title.trim()) {
      ui.toast(mode === 'form' ? tr('请先填写标题') : tr('请先粘贴或导入需求内容，再点「识别」'));
      return;
    }
    if (to === 'developing' && picked.length === 0) {
      ui.toast(tr('开始修复前请先绑定一个项目'));
      return;
    }
    if (to === 'ai_review' && picked.length === 0) {
      ui.toast(tr('提交预审前请先绑定一个项目：AI 要在项目目录里只读地查看代码'));
      return;
    }
    setSaving(true);
    try {
      const bindings = picked.map((project_id) => ({
        project_id,
        role: projects.find((p) => p.id === project_id)?.kind === 'frontend' ? 'frontend' : 'backend',
        subdir: null,
      }));
      let options;
      if (to === 'ai_review' || to === 'developing') {
        const first = to === 'developing' ? bindings[0] : [...bindings].sort((a, b) =>
          a.role.localeCompare(b.role) || a.project_id.localeCompare(b.project_id))[0];
        const project = projects.find((p) => p.id === first?.project_id);
        const provider_id = project?.default_provider_id ?? settings.default_provider_id;
        options = await chooseRun({
          agent: project?.default_agent ?? settings.default_agent,
          provider_id,
          model: providers.find((p) => p.id === provider_id)?.default_model ?? null,
        }, to === 'ai_review' ? tr('提交预审') : tr('配置子任务运行'));
        if (!options || !(await confirmRun())) return;
      }
      const t = await api.ticketCreate({
        title: title.trim(),
        type,
        priority,
        labels: labels.split(/[,，\s]+/).filter(Boolean),
        due_date: due || null,
        content_md: raw,
        projects: bindings,
        acceptance_criteria: [],
        bug_info: type === 'bug' ? bugInfo : null,
        source_kind: mode === 'file' ? 'file' : 'manual',
        default_agent: null,
        default_provider_id: null,
      });
      let note = STAGE_NAME.draft;
      if (to === 'ai_review') {
        // 工单已经建好；预审提交不了（比如 Agent 没配好）就留在草稿，不让用户重复创建
        try {
          await api.reviewStart(t.id, options ?? undefined);
          note = tr('AI 预审已入队');
        } catch (e) {
          ui.toast(tr('已创建 {0}（草稿），但没能提交预审：{1}', t.key, String(e)));
          await reload();
          go(`#/ticket/${t.key}/review`);
          return;
        }
      } else if (to !== 'draft') {
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
        await api.taskStart(task.id, { runtime: options?.runtime });
        note = tr('已开始修复');
      }
      ui.toast(tr('已创建 {0}「{1}」· {2}', t.key, t.title, note));
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
            ['paste', <><Icon name="paste" size={14} />{tr('粘贴')}</>],
            ['file', <><Icon name="upload" size={14} />{tr('导入文件')}</>],
            ['form', <><Icon name="form" size={14} />{tr('表单')}</>],
          ]}
        />
        <div className="new-body">
          {mode === 'paste' && (
            <>
              <textarea
                ref={area}
                className="textarea paste"
                value={raw}
                onChange={(e) => setRaw(e.target.value)}
                onPaste={onPaste}
                placeholder={tr('把聊天记录、PRD 或 Bug 描述整段粘贴到这里…截图也可以直接粘贴。')}
                aria-label={tr('需求原文')}
              />
              <div className="row" style={{ marginTop: 10 }}>
                <button className="btn sm" disabled={!raw.trim()} onClick={() => void recognize(raw)}>
                  <Icon name="spark" size={13} />
                  {tr('识别标题和类型')}
                </button>
                <button className="btn sm" disabled={!raw.trim() || analyzing} onClick={() => void analyze()}>
                  <Icon name="spark" size={13} />
                  {analyzing ? tr('AI 正在分析…') : tr('AI 分析')}
                </button>
                <button className="btn sm" onClick={() => fileInput.current?.click()}>
                  <Icon name="upload" size={13} />
                  {tr('添加图片')}
                </button>
                <span className="dim">
                  {tr('支持 Markdown；你贴的截图默认是权威图')}{imageCount ? tr(' · 已有 {0} 张图片', imageCount) : ''}
                </span>
              </div>
            </>
          )}
          {mode === 'file' && (
            <>
              <div className="drop">
                <Icon name="upload" size={26} />
                <b>{tr('选择一个需求文档')}</b>
                <span>{tr('.md · .txt · .docx · .pdf，转为 Markdown 并保留图片附件')}</span>
                <button className="btn sm" disabled={importing} onClick={() => void pickFile()}>
                  {importing ? tr('正在转换…') : tr('选择文件')}
                </button>
              </div>
              {fileName && (
                <div className="filerow">
                  <Icon name="file" size={15} />
                  <span className="grow">
                    <b>{fileName}</b>
                    <span className="dim">
                      {' '}
                      · {raw.length} {tr('字')}{imageCount ? tr(' · {0} 张图片（仅参考）', imageCount) : ''}
                    </span>
                  </span>
                  <span className="tag moss">
                    <Icon name="check" size={11} />
                    {tr('已转换')}
                  </span>
                </div>
              )}
              {warnings.length > 0 && (
                <ul className="warnlist">
                  {warnings.map((w, i) => (
                    <li key={i}>{w}</li>
                  ))}
                </ul>
              )}
              {fileName && raw.trim() && (
                <div className="prose import-preview">
                  <Md src={raw} />
                </div>
              )}
            </>
          )}
          {mode === 'form' && (
            <div className="form-grid" style={{ gridTemplateColumns: '1fr' }}>
              <Field label={tr('标题')}>
                <input className="input" value={title} onChange={(e) => setTitle(e.target.value)} placeholder={tr('一句话说明要做什么')} />
              </Field>
              <Field label={tr('需求正文')} hint="Markdown">
                <textarea
                  ref={area}
                  className="textarea"
                  rows={13}
                  value={raw}
                  onChange={(e) => setRaw(e.target.value)}
                  onPaste={onPaste}
                  placeholder={tr('## 背景与目标\n\n## 功能需求\n\n## 验收标准')}
                />
              </Field>
            </div>
          )}
        </div>
      </div>
      <input ref={fileInput} type="file" accept="image/png,image/jpeg,image/gif,image/webp" multiple hidden onChange={(e) => void pickImage(e.target.files)} />
      <aside className="new-r">
        <div className="sec-title">
          <Icon name="spark" size={15} />
          {mode === 'form' ? tr('工单属性') : tr('识别结果')}
          {mode !== 'form' && recognized && <small>{tr('已根据原文预填，确认或修改后再创建')}</small>}
        </div>
        {mode !== 'form' && recognized && reasons.length > 0 && (
          <details className="rec-why">
            <summary>
              {tr('识别依据（')}{reasons.length} {tr('项，')}{analyzedBy ? tr('由 {0} 分析', analyzedBy) : tr('本机规则，没有调用 AI')}{tr('）')}
            </summary>
            <ul>
              {reasons.map((r) => (
                <li key={r}>{r}</li>
              ))}
            </ul>
            {type === 'bug' && bugInfo && (
              <ul>
                {bugInfo.steps && <li>{tr('复现步骤：')}{bugInfo.steps}</li>}
                {bugInfo.expected && <li>{tr('期望：')}{bugInfo.expected}</li>}
                {bugInfo.actual && <li>{tr('实际：')}{bugInfo.actual}</li>}
              </ul>
            )}
          </details>
        )}
        {mode !== 'form' && recognized && (
          <div className="rec-ai">
            {analyzing ? (
              <>
                <span className="dim">{tr('AI 正在分析正文，通常不到一分钟；结果只用来预填，不会自动建单。')}</span>
                <button className="btn sm" onClick={() => void api.ticketAnalyzeCancel()}>
                  {tr('取消')}
                </button>
              </>
            ) : (
              <>
                <span className="dim">
                  {aiError
                    ? tr('{0} 下面仍是{1}的结果。', aiError, analyzedBy ? tr('上一次分析') : tr('本机规则'))
                    : analyzedBy
                      ? tr('AI 的建议已填到下面，请确认或修改后再创建。')
                      : tr('想让 AI 帮忙判断类型、项目、优先级和 Bug 三要素？会把正文发给默认 Agent。')}
                </span>
                <button className="btn sm" disabled={!raw.trim()} onClick={() => void analyze()}>
                  <Icon name="spark" size={13} />
                  {analyzedBy || aiError ? tr('重新分析') : tr('AI 分析')}
                </button>
              </>
            )}
          </div>
        )}
        <div className="rec">
          {showProps ? (
            <>
              {mode !== 'form' && (
                <Field label={tr('标题')}>
                  <input className="input" value={title} onChange={(e) => setTitle(e.target.value)} />
                </Field>
              )}
              <Field label={tr('类型')}>
                <div className="pick">
                  {(Object.keys(TYPES) as TicketType[]).map((k) => (
                    <button key={k} className={k === type ? 'on' : ''} onClick={() => setType(k)}>
                      <Stamp type={k} />
                      {TYPES[k].name}
                    </button>
                  ))}
                </div>
              </Field>
              <Field label={tr('优先级')}>
                <div className="pick">
                  {(Object.keys(PRIO) as Priority[]).map((k) => (
                    <button key={k} className={k === priority ? 'on' : ''} onClick={() => setPriority(k)}>
                      <Prio p={k} />
                    </button>
                  ))}
                </div>
              </Field>
              <Field label={tr('绑定项目')} hint={tr('可多选')}>
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
                    {tr('还没有注册项目。')}<a className="lnk" href="#/projects">{tr('去注册 →')}</a>
                  </p>
                )}
              </Field>
              <div className="form-grid">
                <Field label={tr('标签')} hint={tr('逗号或空格分隔')}>
                  <input className="input" value={labels} onChange={(e) => setLabels(e.target.value)} placeholder={tr('例如：支付 导出')} />
                </Field>
                <Field label={tr('截止日期')} hint={tr('可选')}>
                  <input className="input" type="date" value={due} onChange={(e) => setDue(e.target.value)} />
                </Field>
              </div>
            </>
          ) : (
            <div className="rec-empty">
              <Icon name="spark" size={22} />
              <b>{tr('粘贴或导入后，点「识别」预填')}</b>
              <span>{tr('标题、类型和提到的项目；所有预填都可以改。')}</span>
            </div>
          )}
        </div>
      </aside>
      <footer className="new-f">
        <span className="dim">
          {showProps && type === 'bug' ? tr('Bug 走快速通道：跳过预审与人工审核，直接进入开发') : tr('需求工单要经过预审与人工审核后才能开发')}
        </span>
        <span className="grow" />
        <a className="btn ghost" href="#/board">
          {tr('取消')}
        </a>
        <button className="btn" disabled={saving} onClick={() => void save('draft')}>
          {tr('保存为草稿')}
        </button>
        {showProps && type === 'bug' ? (
          <button className="btn primary" disabled={saving} onClick={() => void save('developing')}>
            <Icon name="zap" size={14} />
            {tr('保存并开始修复')}
          </button>
        ) : (
          <button className="btn" disabled={saving} onClick={() => void save('ai_review')}>
            {tr('保存并提交预审')}
          </button>
        )}
      </footer>
    </div>
  );
}
