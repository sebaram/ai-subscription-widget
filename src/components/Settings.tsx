import { useEffect, useState } from 'react';
import type { Config, ProviderCfg, UsageSnapshot } from '../types';
import { hideMe, refreshNow, saveConfig } from '../tauri';

const WINDOWS = ['5h', 'week', 'month'];

function ProviderRow({ pcfg, set }: { pcfg: ProviderCfg; set: (c: ProviderCfg) => void }) {
  const shown = pcfg.floatWindows ?? WINDOWS;
  const toggleWin = (win: string) => {
    const next = shown.includes(win) ? shown.filter((x) => x !== win) : [...shown, win];
    // null means "every window incl. future extras" — materialize a list once the user picks explicitly
    set({ ...pcfg, floatWindows: next.length === WINDOWS.length ? null : next });
  };
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 0', borderBottom: '1px solid #232a36' }}>
      <label style={{ display: 'flex', alignItems: 'center', gap: 6, width: 110 }}>
        <input type="checkbox" checked={pcfg.enabled} onChange={(e) => set({ ...pcfg, enabled: e.currentTarget.checked })} />
        <span>{pcfg.id}</span>
      </label>
      <span style={{ color: '#8a93a6', fontSize: 11 }}>bars:</span>
      {WINDOWS.map((win) => (
        <label key={win} style={{ fontSize: 11 }}>
          <input type="checkbox" checked={shown.includes(win)} onChange={() => toggleWin(win)} /> {win}
        </label>
      ))}
    </div>
  );
}

function Section({ title }: { title: string }) {
  return <div style={{ margin: '12px 0 6px', fontSize: 10, letterSpacing: 1, color: '#8a93a6' }}>{title.toUpperCase()}</div>;
}

const label: React.CSSProperties = { display: 'flex', alignItems: 'center', gap: 8, margin: '5px 0', fontSize: 12 };

export function Settings({ cfg, snap }: { cfg: Config | null; snap: UsageSnapshot | null }) {
  const [draft, setDraft] = useState<Config | null>(cfg);
  const [saved, setSaved] = useState(false);
  const [dirty, setDirty] = useState(false);
  const [refreshed, setRefreshed] = useState(false);
  useEffect(() => {
    // resync from incoming config until the user starts editing; after Save, accept again
    if (cfg && !dirty) setDraft(cfg);
  }, [cfg, dirty]);
  useEffect(() => { if (!draft && cfg) setDraft(cfg); }, [cfg, draft]);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') void hideMe(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  if (!draft) return null;

  const up = (patch: Partial<Config>) => { setDirty(true); setSaved(false); setDraft({ ...draft, ...patch }); };
  const setProvider = (c: ProviderCfg) =>
    up({ providers: draft.providers.map((p) => (p.id === c.id ? c : p)) });
  const num = (v: string, fallback: number) => (Number.isFinite(Number(v)) && v.trim() !== '' ? Number(v) : fallback);
  const byId = (id: string) => snap?.providers.find((p) => p.id === id);

  return (
    <div style={{
      background: 'rgba(23,28,38,0.98)', border: '1px solid #2a313e', borderRadius: 0,
      padding: '12px 16px', color: '#e8eaf0', fontFamily: "'Segoe UI',sans-serif",
      height: '100%', boxSizing: 'border-box', userSelect: 'none',
      overflowY: 'auto' as const, overflowX: 'hidden' as const, display: 'flex', flexDirection: 'column',
    }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 10, position: 'sticky', top: 0, background: 'rgba(23,28,38,0.98)', paddingBottom: 8 }}>
        <strong style={{ fontSize: 14 }}>Settings</strong>
        <button style={{ marginLeft: 'auto', cursor: 'pointer' }} onClick={async () => { await refreshNow(); setRefreshed(true); setTimeout(() => setRefreshed(false), 2000); }}>Refresh now</button>
        <span style={{ fontSize: 10, color: '#8a93a6' }}>{refreshed ? 'refreshing…' : ''}</span>
        <button title="Close (Esc)" style={{ cursor: 'pointer', width: 28 }} onClick={() => void hideMe()}>✕</button>
      </div>

      <Section title="Providers" />
      {draft.providers.map((p) => <ProviderRow key={p.id} pcfg={p} set={setProvider} />)}

      <Section title="Connection" />
      <label style={label}>global poll seconds <input type="number" style={{ width: 90 }} value={draft.pollSeconds} onChange={(e) => up({ pollSeconds: num(e.currentTarget.value, draft.pollSeconds) })} /></label>
      <div style={{ fontSize: 10, color: '#8a93a6', marginBottom: 6 }}>Ollama is capped at ≥60s (API limit 10/min).</div>
      <label style={label}>Ollama Cloud API key <input type="password" style={{ width: 180 }} value={draft.ollamaApiKey} onChange={(e) => up({ ollamaApiKey: e.currentTarget.value })} placeholder="(from ollama.com/settings/keys)" /></label>
      <label style={label}><input type="checkbox" checked={draft.antigravityOauth} onChange={(e) => up({ antigravityOauth: e.currentTarget.checked })} /> Antigravity: track even when the IDE is closed (read-only use of Antigravity's own saved Google login)</label>

      <Section title="Visual" />
      <label style={label}>bar thickness <input type="range" min="3" max="14" value={draft.barThickness} onChange={(e) => up({ barThickness: num(e.currentTarget.value, draft.barThickness) })} /> <span style={{ width: 20 }}>{draft.barThickness}px</span></label>
      <label style={label}>font size <input type="number" min="7" max="18" value={draft.fontSize} onChange={(e) => up({ fontSize: num(e.currentTarget.value, draft.fontSize) })} />px</label>
      <label style={label}>opacity <input type="range" min="0.2" max="1" step="0.05" value={draft.floatBar.opacity} onChange={(e) => up({ floatBar: { ...draft.floatBar, opacity: num(e.currentTarget.value, draft.floatBar.opacity) } })} /> <span style={{ width: 30 }}>{Math.round(draft.floatBar.opacity * 100)}%</span></label>
      <label style={label}>amber at <input type="number" style={{ width: 64 }} value={draft.thresholds.amber} onChange={(e) => up({ thresholds: { ...draft.thresholds, amber: num(e.currentTarget.value, draft.thresholds.amber) } })} />% · red at <input type="number" style={{ width: 64 }} value={draft.thresholds.red} onChange={(e) => up({ thresholds: { ...draft.thresholds, red: num(e.currentTarget.value, draft.thresholds.red) } })} />%</label>
      <label style={label}>show missing windows as “—” (e.g. Claude 5h when absent) <input type="checkbox" checked={draft.showMissingWindows} onChange={(e) => up({ showMissingWindows: e.currentTarget.checked })} /></label>
      <label style={label}>click-through float bar <input type="checkbox" checked={draft.floatBar.clickThrough} onChange={(e) => up({ floatBar: { ...draft.floatBar, clickThrough: e.currentTarget.checked } })} /></label>
      <label style={label}><input type="checkbox" checked={draft.autostart} onChange={(e) => up({ autostart: e.currentTarget.checked })} /> start with Windows</label>

      <div style={{ marginTop: 12, display: 'flex', alignItems: 'center', gap: 10 }}>
        <button style={{ cursor: 'pointer' }} onClick={async () => { await saveConfig(draft); setSaved(true); setDirty(false); }}>Save</button>
        <span style={{ fontSize: 10, color: '#8a93a6' }}>{saved ? 'saved — applies live' : dirty ? 'unsaved changes' : ''}</span>
      </div>

      <Section title="Status" />
      <div style={{ fontSize: 11, color: '#8a93a6', width: '100%' }}>
        {(snap?.providers ?? []).map((p) => {
          const live = byId(p.id);
          return (
            <div key={p.id} style={{ padding: '3px 0', borderTop: '1px solid #232a36' }}>
              <span style={{ color: p.brandColor }}>● </span>
              <span style={{ color: '#e8eaf0' }}>{p.name}</span>
              <span style={{ marginLeft: 8 }}>{p.state}</span>
              {p.state !== 'ok' && p.message && <div style={{ fontSize: 10, color: '#e2b93d', whiteSpace: 'pre-wrap' }}>{p.message}</div>}
              {p.state === 'ok' && <span style={{ fontSize: 10 }}> · refreshed {new Date(p.refreshedAt).toLocaleTimeString()}</span>}
              {live && live.state !== 'ok' && <span style={{ fontSize: 10, color: '#e8503a' }}> · issue now</span>}
            </div>
          );
        })}
        {snap && (
          <button style={{ marginTop: 8, cursor: 'pointer' }} onClick={async () => { await navigator.clipboard.writeText(JSON.stringify(snap, null, 2)); }}>Copy diagnostics</button>
        )}
      </div>
      <div style={{ fontSize: 9, color: '#8a93a6', marginTop: 8 }}>Live config file: %APPDATA%\ai-usage\config.json (hot-reloads)</div>
    </div>
  );
}
