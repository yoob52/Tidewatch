import { invoke } from '@tauri-apps/api/core';

export interface SystemInfo {
  version: string;
  data_dir: string;
  db_path: string;
  schema_version: number;
  sqlite_version: string;
  git_version: string | null;
  long_paths_hint: string | null;
  os: string;
}

export interface Detection {
  kind: 'claude_code' | 'codex' | 'mock';
  installed: boolean;
  path: string | null;
  version: string | null;
}

export const api = {
  systemInfo: () => invoke<SystemInfo>('system_info'),
  detectAgents: () => invoke<Detection[]>('detect_agents'),
};
