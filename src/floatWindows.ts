import type { Config, ProviderSnapshot, UsageWindow } from './types';

// Main limit windows; everything else (group extras, credits, custom cadences,
// per-model legacy rows) is popup-only unless the user names it explicitly.
const DEFAULT_FLOAT_IDS = new Set(['5h', 'week', 'month', 'session']);

export function floatRows(p: ProviderSnapshot, cfg: Config | null): UsageWindow[] {
  const want = cfg?.providers.find((c) => c.id === p.id)?.floatWindows;
  if (want) return p.windows.filter((w) => want.includes(w.id));
  return p.windows.filter((w) => DEFAULT_FLOAT_IDS.has(w.id));
}
