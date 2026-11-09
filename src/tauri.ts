import { listen } from '@tauri-apps/api/event';
import { invoke } from '@tauri-apps/api/core';
import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import type { UsageSnapshot, Config } from './types';
import { mockSnapshot } from './mock';

export const isTauri = () => typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

export function windowLabel(): string {
  return isTauri() ? getCurrentWebviewWindow().label : 'floatbar';
}

export function onSnapshot(cb: (s: UsageSnapshot) => void): Promise<() => void> {
  if (!isTauri()) return mockEvents(cb);
  return listen<UsageSnapshot>('usage-snapshot', (e) => cb(e.payload));
}

export function onConfig(cb: (c: Config) => void): Promise<() => void> {
  if (!isTauri()) return Promise.resolve(() => {});
  return listen<Config>('config-changed', (e) => cb(e.payload));
}

export const refreshNow = () => (isTauri() ? invoke('refresh_now') : Promise.resolve());
export const openPopup = () => (isTauri() ? invoke('open_popup') : Promise.resolve());
export const hideMe = () => (isTauri() ? invoke('hide_me') : Promise.resolve());
export const saveConfig = (c: Config) => (isTauri() ? invoke('save_config', { cfg: c }) : Promise.resolve());
export const saveFloatPosition = (x: number, y: number) =>
  (isTauri() ? invoke('save_float_position', { x, y }) : Promise.resolve());

// Plain-browser dev mode: emit a canned, slowly-mutating snapshot every 2 s
// so the UI can be developed and screenshotted without the Rust backend.
function mockEvents(cb: (s: UsageSnapshot) => void): Promise<() => void> {
  let drift = 0;
  cb(mockSnapshot(drift));
  const t = setInterval(() => cb(mockSnapshot((drift += 3))), 2000);
  return Promise.resolve(() => clearInterval(t));
}
