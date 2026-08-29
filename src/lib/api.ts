// Typed wrappers around Tauri IPC + engine event listeners.
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

// ──────────────────────────── Types ────────────────────────────

export interface Category {
  name: string;
  folder: string;
  extensions: string[];
}

export interface AppConfig {
  download_dir: string;
  categories: Category[];
  global_speed_limit: number;
  close_to_tray: boolean;
  scheduled_start: number | null;
  post_download_action: string | null;
  max_concurrent: number;
  proxy_url: string;
}

export interface HistoryEntry {
  id: number;
  url: string;
  filename: string;
  outcome: 'done' | 'failed' | 'canceled';
  total_bytes: number | null;
  finished_at: number;
  last_error: string | null;
}

export interface Task {
  id: number;
  url: string;
  destination: string;
  total_bytes: number | null;
  downloaded_bytes: number;
  state: 'queued' | 'probing' | 'downloading' | 'paused' | 'done' | 'failed';
  segments: number;
  last_error: string | null;
  priority: number;
  speed_limit: number;
  last_speed: number;
  category: string | null;
}

export interface Progress {
  task_id: number;
  downloaded_bytes: number;
  total_bytes: number | null;
  speed_bps: number;
}

/** Per-download HTTP options: headers, cookies, basic auth, proxy override. */
export interface DownloadOptions {
  headers?: Record<string, string>;
  cookies?: string;
  username?: string;
  password?: string;
  proxy?: string;
}

// ──────────────────────────── API ────────────────────────────

export const api = {
  // ── Downloads ──
  addDownload: (
    url: string,
    destination = '',
    segments = 8,
    options?: DownloadOptions,
    duplicateAction?: string,
  ) =>
    invoke<number>('add_download', {
      url,
      destination,
      segments,
      headers: options?.headers ?? null,
      cookies: options?.cookies ?? null,
      username: options?.username ?? null,
      password: options?.password ?? null,
      proxy: options?.proxy ?? null,
      duplicateAction: duplicateAction ?? null,
    }),
  checkDuplicate: (url: string, destination = '') =>
    invoke<{
      duplicate: boolean;
      kind: 'none' | 'url' | 'file';
      existing_task_id: number | null;
      existing_state: string | null;
      path: string | null;
    }>('check_duplicate', { url, destination }),
  addDownloads: (urls: string[], destination = '', segments = 8) =>
    invoke<number[]>('add_downloads', { urls, destination, segments }),
  pause: (id: number) => invoke<void>('pause_download', { id }),
  resume: (id: number) => invoke<void>('resume_download', { id }),
  remove: (id: number) => invoke<void>('remove_download', { id }),
  list: () => invoke<Task[]>('list_downloads'),
  setSpeedLimit: (bytesPerSec: number) => invoke<void>('set_speed_limit', { bytesPerSec }),
  setTaskSpeedLimit: (id: number, bytesPerSec: number) =>
    invoke<void>('set_task_speed_limit', { id, bytesPerSec }),
  moveTaskUp: (id: number) => invoke<void>('move_task_up', { id }),
  moveTaskDown: (id: number) => invoke<void>('move_task_down', { id }),

  // ── Config ──
  getConfig: () => invoke<AppConfig>('get_config'),
  setConfig: (config: AppConfig) => invoke<void>('set_config', { newCfg: config }),
  updateCategories: (categories: Category[]) => invoke<void>('update_categories', { categories }),
  setDownloadDir: (path: string) => invoke<void>('set_download_dir', { path }),

  // ── Schedule & Post-action ──
  setSchedule: (timestamp: number | null) => invoke<void>('set_schedule', { timestamp }),
  setPostAction: (action: string | null) => invoke<void>('set_post_action', { action }),
  setMaxConcurrent: (max: number) => invoke<void>('set_max_concurrent', { max }),
  setProxy: (proxyUrl: string) => invoke<void>('set_proxy', { proxyUrl }),

  // ── History ──
  getHistory: () => invoke<HistoryEntry[]>('get_history'),
  clearHistory: () => invoke<void>('clear_history'),

  // ── Jalali calendar helpers ──
  gregorianToJalali: (gy: number, gm: number, gd: number) =>
    invoke<{ year: number; month: number; day: number }>('gregorian_to_jalali_cmd', { gy, gm, gd }),
  jalaliToGregorian: (jy: number, jm: number, jd: number) =>
    invoke<{ year: number; month: number; day: number }>('jalali_to_gregorian_cmd', { jy, jm, jd }),

  // ── Browser ──
  setupBrowser: (extensionId: string) => invoke('setup_browser_integration', { extensionId }),
  stageExtension: () => invoke<string>('stage_extension_folder'),
  detectBrowsers: () => invoke<string[]>('detect_browsers'),
  openUrl: (url: string) => invoke<void>('open_url', { url }),

  // ── Events ──
  onProgress: (cb: (p: Progress) => void) =>
    listen<Progress>('download-progress', (e) => cb(e.payload)),
  onState: (cb: (taskId: number, state: Task['state']) => void) =>
    listen<{ taskId: number; state: Task['state'] }>('download-state', (e) =>
      cb(e.payload.taskId, e.payload.state),
    ),
  onClipboardUrl: (cb: (url: string) => void) =>
    listen<string>('clipboard-url', (e) => cb(e.payload)),
};
