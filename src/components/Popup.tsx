import { useEffect } from 'react';
import type { Config, ProviderSnapshot, UsageSnapshot, UsageWindow } from '../types';
import { colorFor } from '../color';
import { paceStatus, etaText, humanizeReset } from '../pace';
import { hideMe } from '../tauri';
import { ProviderLogo } from '../logos';

const STATE_TEXT: Record<string, string> = {
  'auth-required': 'Auth required — set credentials in Settings',
  'not-running': 'Open the app to track usage',
  'no-data': 'No data available',
  'parse-failed': 'Could not parse provider response',
  disabled: '',
};

const PACE_TEXT: Record<string, string> = { hot: 'running hot', under: 'under pace', on: 'on pace' };

function DetailRow({ w, th }: { w: UsageWindow; th: { amber: number; red: number } }) {
  const fillPct = (w.usedPercent ?? 0).toFixed(1);
  const pct = w.isKnown && w.usedPercent !== null ? `${w.usedPercent.toFixed(0)}%` : '— (Unavailable)';
  const status = paceStatus(w);
  const eta = etaText(w);
  const statusLine = status
    ? `${PACE_TEXT[status]}${eta ? ` · at current rate 100% in ${eta}` : ''}`
    : null;
  return (
    <div style={{ marginBottom: 6 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 7, marginBottom: 3 }}>
        <span style={{ width: w.id.startsWith('group:') ? 76 : 30, color: '#8a93a6', fontSize: 10 }}>{w.title}</span>
        <div style={{ flex: 1, height: 8, borderRadius: 4, background: '#2a313e', position: 'relative' }}>
          <div style={{ width: `${fillPct}%`, height: '100%', borderRadius: 4, background: w.isKnown && w.usedPercent !== null ? colorFor(w.usedPercent, th) : 'transparent' }} />
          {w.paceExpectedPercent !== null && (
            <div title="pace marker: where you'd normally be" style={{ position: 'absolute', left: `${w.paceExpectedPercent}%`, top: -2, width: 2, height: 12, background: status === 'hot' ? '#e8503a' : '#8a93a6', borderRadius: 1 }} />
          )}
        </div>
        <span style={{ width: 86, textAlign: 'right', fontWeight: 600, fontSize: 11 }}>{pct}</span>
      </div>
      {(statusLine || w.quota) && (
        <div style={{ fontSize: 9, color: '#8a93a6', paddingLeft: 37 }}>
          {statusLine}{statusLine && w.quota ? ' · ' : ''}
          {w.quota ? `${w.quota.used} of ${w.quota.total}` : null}
        </div>
      )}
    </div>
  );
}

function ProviderBlock({ p, cfg }: { p: ProviderSnapshot; cfg: Config | null }) {
  const th = cfg?.thresholds ?? { amber: 50, red: 80 };
  const resets = p.windows.filter((w) => w.resetsAt).map((w) => humanizeReset(w.resetsAt!))[0];
  const stateText = p.state === 'ok' ? '' : (STATE_TEXT[p.state] ?? p.state);
  return (
    <div style={{ marginBottom: 12 }}>
      <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 5 }}>
        <ProviderLogo id={p.id} color={p.brandColor} />
        <strong style={{ fontSize: 12 }}>{p.name}</strong>
        <span style={{ marginLeft: 'auto', color: '#8a93a6', fontSize: 10 }}>{resets ? `resets in ${resets}` : ''}</span>
      </div>
      {stateText && <div style={{ fontSize: 10, color: '#e2b93d', marginBottom: 5 }}>{stateText}</div>}
      {p.message && <div style={{ fontSize: 10, color: '#8a93a6', marginBottom: 5, whiteSpace: 'pre-wrap' }}>{p.message}</div>}
      {p.windows.map((w) => <DetailRow key={w.id} w={w} th={th} />)}
    </div>
  );
}

export function Popup({ snap, cfg }: { snap: UsageSnapshot | null; cfg: Config | null }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') void hideMe(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  }, []);
  return (
    <div
      style={{
        background: 'rgba(23,28,38,0.98)', border: '1px solid #2a313e', borderRadius: 14,
        padding: '14px 16px', color: '#e8eaf0', fontFamily: "'Segoe UI',sans-serif", width: 380, userSelect: 'none',
      }}
    >
      <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
        <strong style={{ fontSize: 13 }}>AI Usage</strong>
        {snap && <span style={{ color: '#8a93a6', fontSize: 10 }}>refreshed {new Date(snap.refreshedAt).toLocaleTimeString()}</span>}
      </div>
      {(snap?.providers ?? []).map((p) => <ProviderBlock key={p.id} p={p} cfg={cfg} />)}
      <div style={{ fontSize: 9, color: '#8a93a6' }}>Esc to close</div>
    </div>
  );
}
