import type { Config, ProviderSnapshot, UsageSnapshot, UsageWindow } from './types';

function win(
  id: string,
  title: string,
  windowMinutes: number,
  usedPercent: number | null,
  opts: Partial<Pick<UsageWindow, 'paceExpectedPercent' | 'resetsAt' | 'quota'>> = {},
): UsageWindow {
  const full = {
    id, title, windowMinutes, usedPercent, isKnown: usedPercent !== null,
    paceExpectedPercent: opts.paceExpectedPercent ?? null,
    resetsAt: opts.resetsAt ?? null,
    quota: opts.quota ?? null,
  };
  return full;
}

function iso(minutesFromNow: number): string {
  return new Date(Date.now() + minutesFromNow * 60_000).toISOString();
}

export function mockSnapshot(drift: number): UsageSnapshot {
  const shift = (p: number) => Math.min(100, Math.max(0, p + drift));
  const providers: ProviderSnapshot[] = [
    {
      id: 'claude', name: 'Claude', brandColor: '#d9775f', state: 'ok', refreshedAt: new Date().toISOString(),
      windows: [
        win('5h', '5h', 300, shift(42), { paceExpectedPercent: 58, resetsAt: iso(114) }),
        win('week', 'wk', 10080, shift(61), { paceExpectedPercent: 73, resetsAt: iso(4320) }),
      ],
    },
    {
      id: 'codex', name: 'Codex', brandColor: '#10a37f', state: 'ok', refreshedAt: new Date().toISOString(),
      windows: [
        win('5h', '5h', 300, shift(12), { paceExpectedPercent: 24, resetsAt: iso(172) }),
        win('week', 'wk', 10080, shift(33), { paceExpectedPercent: 45, resetsAt: iso(5040) }),
      ],
    },
    {
      id: 'antigravity', name: 'Antigravity', brandColor: '#4285f4', state: 'ok', refreshedAt: new Date().toISOString(),
      windows: [
        win('5h', '5h', 300, shift(88), { paceExpectedPercent: 80, resetsAt: iso(48) }),
        win('week', 'wk', 10080, shift(51), { paceExpectedPercent: 55, resetsAt: iso(2880) }),
        win('group:gemini:5h', 'Gemini 5h', 300, shift(10), { resetsAt: iso(48) }),
      ],
    },
    {
      id: 'ollama', name: 'Ollama', brandColor: '#8b9ba8', state: 'ok', refreshedAt: new Date().toISOString(),
      windows: [
        win('month', 'mo', 43200, 24, { resetsAt: iso(12960), quota: { used: '$8.40', total: '$35.00' } }),
      ],
    },
  ];
  return { providers, refreshedAt: new Date().toISOString() };
}

export function mockConfig(): Config {
  return {
    providers: [
      { id: 'claude', enabled: true, floatWindows: null },
      { id: 'codex', enabled: true, floatWindows: null },
      { id: 'antigravity', enabled: true, floatWindows: null },
      { id: 'ollama', enabled: true, floatWindows: null },
    ],
    thresholds: { amber: 50, red: 80 },
    pollSeconds: 300,
    perProviderPoll: {},
    floatBar: { opacity: 0.95, clickThrough: false, position: null },
    theme: 'dark',
    ollamaApiKey: '',
    autostart: true,
  };
}
