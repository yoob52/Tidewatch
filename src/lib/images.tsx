// PRD 里的图片：`![说明](attachment:<id> "authoritative | reference[; superseded-by=<id>]")`。
// 效力只写在 Markdown 里，这里的函数都是对正文做文本改写，再由调用方保存成新版本。
import { useEffect, useState } from 'react';
import { api } from './api';
import { Icon } from './Icon';

export type Authority = 'authoritative' | 'reference';

export interface ImageRef {
  id: string;
  alt: string;
  authority: Authority;
  supersededBy: string | null;
  /** 在正文里的起止位置。 */
  start: number;
  end: number;
}

const RE = /!\[([^\]\n]*)\]\(attachment:([^\s)]+)(?:\s+"([^"\n]*)")?\)/g;

export function parseImages(md: string): ImageRef[] {
  const out: ImageRef[] = [];
  for (const m of md.matchAll(RE)) {
    const parts = (m[3] ?? '').split(';').map((s) => s.trim());
    const sup = parts.find((p) => p.startsWith('superseded-by='))?.slice('superseded-by='.length) || null;
    out.push({
      id: m[2],
      alt: m[1].trim(),
      authority: parts.some((p) => p.toLowerCase() === 'authoritative') ? 'authoritative' : 'reference',
      supersededBy: sup,
      start: m.index,
      end: m.index + m[0].length,
    });
  }
  return out;
}

export function imageMd(alt: string, id: string, authority: Authority, supersededBy?: string | null): string {
  const a = alt.replace(/[[\]\n]/g, ' ').trim();
  return `![${a}](attachment:${id} "${authority}${supersededBy ? `; superseded-by=${supersededBy}` : ''}")`;
}

const rewrite = (md: string, id: string, f: (r: ImageRef) => string): string => {
  const r = parseImages(md).find((x) => x.id === id);
  return r ? md.slice(0, r.start) + f(r) + md.slice(r.end) : md;
};

export const setAuthority = (md: string, id: string, authority: Authority) =>
  rewrite(md, id, (r) => imageMd(r.alt, r.id, authority, authority === 'authoritative' ? null : r.supersededBy));

export const setAlt = (md: string, id: string, alt: string) =>
  rewrite(md, id, (r) => imageMd(alt, r.id, r.authority, r.supersededBy));

/** 移除引用；指向它的"已被取代"标记一并去掉。附件文件保留。 */
export function removeImage(md: string, id: string): string {
  let next = rewrite(md, id, () => '');
  for (const r of parseImages(next).filter((x) => x.supersededBy === id))
    next = rewrite(next, r.id, (x) => imageMd(x.alt, x.id, x.authority, null));
  return next.replace(/\n{3,}/g, '\n\n');
}

/** 新图标为权威并放在原图位置；原图留在其后，标为已被取代的参考图。 */
export const replaceImage = (md: string, oldId: string, newId: string, alt: string) =>
  rewrite(
    md,
    oldId,
    (r) => `${imageMd(alt, newId, 'authoritative')}\n\n${imageMd(r.alt || '原 PRD 图', r.id, 'reference', newId)}`,
  );

const toBase64 = (file: Blob) =>
  new Promise<string>((resolve, reject) => {
    const fr = new FileReader();
    fr.onload = () => resolve(String(fr.result));
    fr.onerror = () => reject(new Error('读取图片失败'));
    fr.readAsDataURL(file);
  });

/** 存成用户图，返回附件 id。 */
export async function uploadImage(ticketId: string | null, file: File): Promise<string> {
  const a = await api.attachmentAdd(ticketId, file.name || '粘贴的截图.png', await toBase64(file), 'user');
  return a.id;
}

/** 剪贴板 / 拖放里的图片文件。 */
export function imageFiles(data: DataTransfer | null): File[] {
  return Array.from(data?.files ?? []).filter((f) => f.type.startsWith('image/'));
}

/** 把一段文字插到 textarea 的光标处，前后各留一个空行。 */
export function insertAt(text: string, el: HTMLTextAreaElement | null, snippet: string): string {
  const at = el ? el.selectionStart : text.length;
  const before = text.slice(0, at).replace(/\n*$/, '');
  const after = text.slice(el ? el.selectionEnd : at).replace(/^\n*/, '');
  return `${before}${before ? '\n\n' : ''}${snippet}\n${after ? `\n${after}` : ''}`;
}

const cache = new Map<string, Promise<string>>();
const load = (id: string) => {
  let p = cache.get(id);
  if (!p) {
    p = api.attachmentData(id);
    cache.set(id, p);
    p.catch(() => cache.delete(id));
  }
  return p;
};

export interface ImageActions {
  setAuthority: (id: string, authority: Authority) => void;
  replace: (id: string) => void;
  remove: (id: string) => void;
  describe: (id: string) => void;
}

export function PrdImage({ img, actions, added }: { img: ImageRef; actions?: ImageActions; added?: boolean }) {
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [open, setOpen] = useState(!img.supersededBy);
  useEffect(() => {
    let live = true;
    setSrc(null);
    setFailed(false);
    load(img.id).then(
      (d) => live && setSrc(d),
      () => live && setFailed(true),
    );
    return () => {
      live = false;
    };
  }, [img.id]);
  const auth = img.authority === 'authoritative';
  return (
    <figure className={`prdimg ${auth ? 'auth' : 'ref'} ${img.supersededBy ? 'old' : ''} ${added ? 'ins' : ''}`}>
      <figcaption>
        <span className={`tag ${auth ? 'moss' : 'amber'}`}>
          <Icon name={auth ? 'check' : 'info'} size={11} />
          {auth ? '权威图' : '仅参考'}
        </span>
        {img.supersededBy && <span className="tag">已被取代</span>}
        <span className="grow alt">{img.alt || '未命名图片'}</span>
        {img.supersededBy && (
          <button className="btn ghost sm" onClick={() => setOpen(!open)}>
            {open ? '收起' : '展开'}
          </button>
        )}
        {actions && (
          <>
            <button className="btn ghost sm" onClick={() => actions.setAuthority(img.id, auth ? 'reference' : 'authoritative')}>
              {auth ? '改为仅参考' : '设为权威'}
            </button>
            {!img.supersededBy && (
              <button className="btn ghost sm" onClick={() => actions.replace(img.id)}>
                替换
              </button>
            )}
            <button className="btn ghost sm" onClick={() => actions.describe(img.id)}>
              文字描述
            </button>
            <button className="btn ghost sm" onClick={() => actions.remove(img.id)}>
              移除
            </button>
          </>
        )}
      </figcaption>
      {open &&
        (failed ? (
          <div className="prdimg-miss">图片文件不在了（{img.id}）</div>
        ) : src ? (
          <img src={src} alt={img.alt} />
        ) : (
          <div className="prdimg-miss">加载中…</div>
        ))}
    </figure>
  );
}
