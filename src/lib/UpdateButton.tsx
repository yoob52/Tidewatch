import { useEffect, useRef, useState } from 'react';
import { api, type UpdateProgress } from './api';
import { Icon } from './Icon';
import { tr } from './i18n';
import { Modal, useUi } from './ui';
import { checkUpdate, installUpdate, useUpdate } from './update';

function UpdateProgressLine({ p }: { p: UpdateProgress }) {
  if (p.phase !== 'downloading') {
    return <p>{p.phase === 'checking' ? tr('正在查找安装包…') : p.phase === 'installing' ? tr('签名校验通过，正在安装…') : tr('安装完成，正在重启…')}</p>;
  }
  const mb = (n: number) => (n / 1048576).toFixed(1);
  const pct = p.total ? Math.min(100, Math.round((p.downloaded / p.total) * 100)) : null;
  return (
    <>
      <p>
        {tr('正在下载')} {mb(p.downloaded)} MB{p.total ? tr(' / {0} MB（{1}%）', mb(p.total), pct) : ''}
      </p>
      <div className="bar" style={{ maxWidth: 320 }}>
        <i style={{ width: `${pct ?? 0}%` }} />
      </div>
    </>
  );
}

export function UpdateButton() {
  const ui = useUi();
  const { info, checking, progress, error } = useUpdate();
  const [open, setOpen] = useState(false);
  const [confirmVersion, setConfirmVersion] = useState<string | null>(null);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    let active = true;
    void checkUpdate()?.then((u) => {
      if (active && u?.newer) ui.toast(tr('观澜有新版本 {0}（当前 {1}），点击右上角的更新按钮查看。', u.latest, u.current));
    });
    return () => {
      active = false;
    };
  }, [ui]);

  const close = () => {
    setOpen(false);
    setConfirmVersion(null);
    button.current?.focus();
  };
  const label = progress ? tr('正在更新…') : info?.newer ? tr('有新版本：{0}', info.latest) : tr('检查更新');
  const releaseUrl = info?.url;
  const availableVersion = info?.newer && !info.install_blocked ? info.latest : null;

  return (
    <>
      <button
        className="iconbtn"
        ref={button}
        aria-label={label}
        aria-haspopup="dialog"
        title={label}
        onClick={() => {
          setOpen(true);
          if (!info?.newer && !progress) void checkUpdate();
        }}
      >
        <Icon name="download" />
        {info?.newer && <span className="pip" aria-hidden="true" />}
      </button>
      {open && (
        <Modal
          title={confirmVersion ? tr('安装 {0}', confirmVersion) : tr('检查更新')}
          width={500}
          onClose={close}
          footer={confirmVersion ? (
            <>
              <button key="cancel" className="btn" autoFocus onClick={() => setConfirmVersion(null)}>{tr('取消')}</button>
              <button className="btn primary" onClick={() => {
                setConfirmVersion(null);
                void installUpdate(confirmVersion);
              }}>{tr('下载并安装')}</button>
            </>
          ) : (
            <>
              <button key="close" className="btn" autoFocus onClick={close}>{tr('关闭')}</button>
              <button className="btn" disabled={checking || !!progress} onClick={() => void checkUpdate()}>{tr('重新检查')}</button>
              {availableVersion && (
                <button className="btn primary" disabled={checking || !!progress} onClick={() => setConfirmVersion(availableVersion)}>
                  {progress ? tr('正在更新…') : tr('下载并安装')}
                </button>
              )}
            </>
          )}
        >
          {confirmVersion ? (
            <>
              <p>{tr('观澜会下载安装包，校验签名通过后安装，然后自动重启。校验不过不会安装。')}</p>
              <p>{tr('有运行在进行时不能安装；排队中的运行会在重启后继续。')}</p>
            </>
          ) : (
            <>
              <div role="status">
                {checking ? <p>{tr('正在检查更新…')}</p> : info && (
                  <>
                    {info.newer ? (
                      <p>
                        <b>{tr('有新版本：{0}', info.latest)}</b>
                        {info.install_blocked && <> {info.install_blocked}</>}
                      </p>
                    ) : !error && (
                      <p>{info.latest ? tr('已经是最新版本（最新发布 {0}）。', info.latest) : tr('还没有发布过正式版本。')}</p>
                    )}
                    {progress && <UpdateProgressLine p={progress} />}
                  </>
                )}
              </div>
              {error && <p role="alert" style={{ color: 'var(--cinnabar)' }}>{error}</p>}
              {!checking && info?.newer && releaseUrl && (
                <p><a
                  className="btn sm"
                  href={releaseUrl}
                  onClick={(e) => {
                    e.preventDefault();
                    void ui.attempt(api.openUrl(releaseUrl));
                  }}
                >{tr('打开发布页')} <Icon name="ext" size={12} /></a></p>
              )}
              {!checking && info?.newer && info.notes && (
                <details className="rec-why">
                  <summary>{tr('这一版的更新说明')}</summary>
                  <p style={{ whiteSpace: 'pre-wrap' }}>{info.notes}</p>
                </details>
              )}
            </>
          )}
        </Modal>
      )}
    </>
  );
}
