import { useSyncExternalStore } from 'react';
import { api, onUpdateProgress, type UpdateInfo, type UpdateProgress } from './api';

/** 检查更新的结果和安装进度。放在页面之外：下载要几分钟，离开设置页再回来还得看得到。 */
interface UpdateState {
  info: UpdateInfo | null;
  progress: UpdateProgress | null;
  error: string | null;
}

let state: UpdateState = { info: null, progress: null, error: null };
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

export function setUpdateInfo(info: UpdateInfo) {
  set({ info, error: null });
}

export async function installUpdate() {
  if (state.progress) return;
  set({ error: null, progress: { phase: 'checking' } });
  try {
    await api.updateInstall();
  } catch (e) {
    set({ error: String(e) });
  }
  set({ progress: null });
}
