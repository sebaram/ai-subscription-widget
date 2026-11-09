import type { UsageWindow } from '../types';
import { colorFor } from '../color';

function trackColor(w: UsageWindow, th: { amber: number; red: number }): string {
  // Unknown data never pretends to be 0% — it shows "—" with an empty track.
  return w.isKnown && w.usedPercent !== null ? colorFor(w.usedPercent, th) : 'transparent';
}

export function UsageBar({ w, th }: { w: UsageWindow; th: { amber: number; red: number } }) {
  const pct = w.isKnown && w.usedPercent !== null ? `${w.usedPercent.toFixed(0)}%` : '—';
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 5, minWidth: 0, flex: 1 }}>
      <span style={{ width: 15, color: '#8a93a6', fontSize: 9 }}>{w.title}</span>
      <div style={{ flex: 1, height: 5, borderRadius: 3, background: '#2a313e', minWidth: 40 }}>
        <div style={{ width: `${(w.usedPercent ?? 0).toFixed(1)}%`, height: '100%', borderRadius: 3, background: trackColor(w, th) }} />
      </div>
      <span style={{ width: 28, textAlign: 'right', fontSize: 10, fontWeight: 500 }}>{pct}</span>
    </div>
  );
}
