import { useEffect, useState } from 'react';
import type { Config, ProviderCfg } from '../types';
import { saveConfig } from '../tauri';

const WINDOWS = ['5h', 'week', 'month'];

function ProviderRow({ pcfg, set }: { pcfg: ProviderCfg; set: (c: ProviderCfg) => void }) {
  const shown = pcfg.floatWindows ?? WINDOWS;
  const toggleWin = (win: string) => {
    const next = shown.includes(win)
      ? shown.filter((x) => x !== win)
      : [...shown, win];
    // null means "every window incl. future extras" — materialize a list once the user picks explicitly
    set({ ...pcfg, floatWindows: next.length === WINDOWS.length ? null : next });
  };
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 0', borderBottom: '1px solid #232a36' }}>
      <label style={{ display: 'flex', alignItems: 'center', gap: 6, width: 120 }}>
        <input type="checkbox" checked={pcfg.enabled} onChange={(e) => set({ ...pcfg, enabled: e.currentTarget.checked })} />
        <span>{pcfg.id}</span>
      </label>
      <span style={{ color: '#8a93a6', fontSize: 11 }}>float bars:</span>
      {WINDOWS.map((win) => (
        <label key={win} style={{ fontSize: 11 }}>
          <input type="checkbox" checked={shown.includes(win)} onChange={() => toggleWin(win)} /> {win}
        </label>
      ))}
    </div>
  );
}

export function Settings({ cfg }: { cfg: Config | null }) {
  const [draft, setDraft] = useState<Config | null>(cfg);
  const [saved, setSaved] = useState(false);
  const [dirty, setDirty] = useState(false);
  useEffect(() => {
    // resync from incoming config until the user starts editing; after Save, accept again
    if (cfg && !dirty) setDraft(cfg);
  }, [cfg, dirty]);
  useEffect(() => { if (!draft && cfg) setDraft(cfg); }, [cfg, draft]);
  if (!draft) return null;

  const up = (patch: Partial<Config>) => { setDirty(true); setSaved(false); setDraft({ ...draft, ...patch }); };
  const setProvider = (c: ProviderCfg) =>
    up({ providers: draft.providers.map((p) => (p.id === c.id ? c : p)) });
  const num = (v: string, fallback: number) => (Number.isFinite(Number(v)) && v.trim() !== '' ? Number(v) : fallback);

  return (
    <div style={{
      background: 'rgba(23,28,38,0.98)', border: '1px solid #2a313e', borderRadius: 14,
      padding: 16, color: '#e8eaf0', fontFamily: "'Segoe UI',sans-serif", width: 420, userSelect: 'none',
    }}>
      <strong style={{ fontSize: 13 }}>Settings</strong>
      <div style={{ margin: '10px 0' }}>
        {draft.providers.map((p) => <ProviderRow key={p.id} pcfg={p} set={setProvider} />)}
      </div>
      <label>amber threshold % <input type="number" value={draft.thresholds.amber} onChange={(e) => up({ thresholds: { ...draft.thresholds, amber: num(e.currentTarget.value, draft.thresholds.amber) } })} /></label><br />
      <label>red threshold % <input type="number" value={draft.thresholds.red} onChange={(e) => up({ thresholds: { ...draft.thresholds, red: num(e.currentTarget.value, draft.thresholds.red) } })} /></label><br />
      <label>poll seconds <input type="number" value={draft.pollSeconds} onChange={(e) => up({ pollSeconds: num(e.currentTarget.value, draft.pollSeconds) })} /></label><br />
      <label>opacity <input type="number" step="0.05" min="0.2" max="1" value={draft.floatBar.opacity} onChange={(e) => up({ floatBar: { ...draft.floatBar, opacity: num(e.currentTarget.value, draft.floatBar.opacity) } })} /></label><br />
      <label>Ollama API key <input type="password" value={draft.ollamaApiKey} onChange={(e) => up({ ollamaApiKey: e.currentTarget.value })} /></label><br />
      <label><input type="checkbox" checked={draft.autostart} onChange={(e) => up({ autostart: e.currentTarget.checked })} /> start with Windows</label><br />
      <label><input type="checkbox" checked={draft.antigravityOauth} onChange={(e) => up({ antigravityOauth: e.currentTarget.checked })} /> Antigravity via Google OAuth (optional, needs `gemini` CLI login)</label>
      <div style={{ marginTop: 10 }}>
        <button style={{ cursor: 'pointer' }} onClick={async () => { await saveConfig(draft); setSaved(true); setDirty(false); }}>Save</button>
        <span style={{ marginLeft: 8, fontSize: 10, color: '#8a93a6' }}>{saved ? 'saved — applies live' : ''}</span>
      </div>
    </div>
  );
}
