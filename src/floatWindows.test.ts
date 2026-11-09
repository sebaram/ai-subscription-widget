import { describe, expect, it } from 'vitest';
import { floatRows } from './floatWindows';
import type { ProviderSnapshot, UsageWindow, Config } from './types';

function win(id: string): UsageWindow {
  return {
    id, title: id, windowMinutes: 300, usedPercent: 10, quota: null,
    resetsAt: null, paceExpectedPercent: null, isKnown: true,
  };
}

function provider(windows: UsageWindow[]): ProviderSnapshot {
  return { id: 'antigravity', name: 'Antigravity', brandColor: '#4285f4', state: 'ok', refreshedAt: '', windows };
}

const cfgBase: Config = {
  providers: [{ id: 'antigravity', enabled: true, floatWindows: null }],
  thresholds: { amber: 50, red: 80 },
  pollSeconds: 300,
  perProviderPoll: {},
  floatBar: { opacity: 0.95, clickThrough: false, position: null },
  theme: 'dark',
  ollamaApiKey: '',
  autostart: true,
  antigravityOauth: false,
  barThickness: 5,
  fontSize: 10,
};

describe('floatRows', () => {
  it('default keeps main windows only: extras, credits, custom stay popup-only', () => {
    const p = provider([
      win('5h'), win('week'),
      win('group:Gemini Models:5h'),
      win('credits'),
      win('purchased'),
      win('model:gemini-3-pro'),
      win('custom'),
    ]);
    const rows = floatRows(p, cfgBase);
    expect(rows.map((w) => w.id)).toEqual(['week', '5h']); // canonical order, sorted
  });

  it('explicit floatWindows list wins (names apply literally)', () => {
    const cfg: Config = {
      ...cfgBase,
      providers: [{ id: 'antigravity', enabled: true, floatWindows: ['group:Gemini Models:5h'] }],
    };
    const p = provider([win('5h'), win('group:Gemini Models:5h')]);
    expect(floatRows(p, cfg).map((w) => w.id)).toEqual(['group:Gemini Models:5h']);
  });

  it('session window counts as a main window (ollama legacy plan)', () => {
    const p = provider([win('session'), win('custom')]);
    expect(floatRows(p, cfgBase).map((w) => w.id)).toEqual(['session']);
  });

  it('orders windows week → 5h → month, so 5h sits lower', () => {
    const p = provider([win('5h'), win('month'), win('week')]);
    expect(floatRows(p, cfgBase).map((w) => w.id)).toEqual(['week', '5h', 'month']);
  });
});
