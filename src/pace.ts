import type { UsageWindow } from './types';

export type PaceStatus = 'hot' | 'under' | 'on' | null;

export function paceStatus(w: UsageWindow): PaceStatus {
  if (!w.isKnown || w.usedPercent === null || w.paceExpectedPercent === null) return null;
  const delta = w.usedPercent - w.paceExpectedPercent;
  if (delta > 2) return 'hot';
  if (delta < -2) return 'under';
  return 'on';
}

function humanize(ms: number): string {
  const m = Math.round(ms / 60_000);
  if (m >= 1440) return `${Math.floor(m / 1440)}d ${Math.floor((m % 1440) / 60)}h`;
  if (m < 60) return `${m}m`;
  return `${Math.floor(m / 60)}h ${m % 60}m`;
}

export function humanizeReset(iso: string): string {
  return humanize(new Date(iso).getTime() - Date.now());
}

export function etaText(w: UsageWindow): string | null {
  if (!w.isKnown || w.usedPercent === null || w.usedPercent <= 0) return null;
  if (!w.resetsAt || !w.windowMinutes) return null;
  const startMs = new Date(w.resetsAt).getTime() - w.windowMinutes * 60_000;
  const elapsedMs = Date.now() - startMs;
  if (elapsedMs <= 0) return null;
  const rate = w.usedPercent / elapsedMs; // %/ms
  const fullInMs = (100 - w.usedPercent) / rate;
  return `~${humanize(fullInMs)}`;
}
