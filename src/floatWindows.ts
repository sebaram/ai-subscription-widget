import type { Config, ProviderSnapshot, UsageWindow } from './types';

// Main limit windows; everything else (group extras, credits, custom cadences,
// per-model legacy rows) is popup-only unless the user names it explicitly.
const DEFAULT_FLOAT_IDS = new Set(['5h', 'week', 'month', 'session', 'g:gemini', 'g:others', 'g:argon', 'auto', 'api']);

// Canonical display order: weekly on top, then the 5-hour lane, then
// per-model Antigravity lanes (gemini/others/argon), then monthly, then
// Cursor's dashboard lanes (auto/api).
const RANK: Record<string, number> = {
  week: 0, '5h': 1, session: 1,
  'g:gemini': 2, 'g:others': 3, 'g:argon': 4,
  month: 5, auto: 6, api: 7,
  'on-demand': 8,
};

export function floatRows(p: ProviderSnapshot, cfg: Config | null): UsageWindow[] {
  const want = cfg?.providers.find((c) => c.id === p.id)?.floatWindows;
  const rows = want
    ? p.windows.filter((w) => want.includes(w.id))
    : p.windows.filter((w) => DEFAULT_FLOAT_IDS.has(w.id));
  return [...rows].sort((a, b) => (RANK[a.id] ?? 99) - (RANK[b.id] ?? 99));
}
