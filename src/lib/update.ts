import { useSyncExternalStore } from 'react';
import { api, onUpdateProgress, type UpdateInfo, type UpdateProgress } from './api';

/** 检查更新的结果和安装进度。关闭更新窗口后仍然保留。 */
interface UpdateState {
  info: UpdateInfo | null;
  checking: boolean;
  progress: UpdateProgress | null;
  error: string | null;
}

let state: UpdateState = { info: null, checking: false, progress: null, error: null };
let pendingCheck: Promise<UpdateInfo | null> | null = null;
const subscribers = new Set<() => void>();
let listening = false;

function set(patch: Partial<UpdateState>) {
  state = { ...state, ...patch };
  subscribers.forEach((f) => f());
}

function subscribe(f: () => void) {
  if (!listening) {
    listening = true;
    // 只在安装进行中才收进度，免得收尾时迟到的事件把按钮又锁住
    onUpdateProgress((p) => state.progress && set({ progress: p }));
  }
  subscribers.add(f);
  return () => {
    subscribers.delete(f);
  };
}

export function useUpdate(): UpdateState {
  return useSyncExternalStore(subscribe, () => state);
}

export function checkUpdate() {
  if (state.progress) return;
  if (pendingCheck) return pendingCheck;
  set({ checking: true, error: null });
  pendingCheck = api.updateCheck().then(
    (info) => {
      set({ info });
      return info;
    },
    (e) => {
      set({ error: String(e) });
      return null;
    },
  ).finally(() => {
    pendingCheck = null;
    set({ checking: false });
  });
  return pendingCheck;
}

export async function installUpdate(expectedVersion: string) {
  if (state.checking || state.progress || !state.info?.newer || state.info.install_blocked) return;
  set({ error: null, progress: { phase: 'checking' } });
  try {
    await api.updateInstall(expectedVersion);
  } catch (e) {
    set({ error: String(e) });
  }
  set({ progress: null });
}
