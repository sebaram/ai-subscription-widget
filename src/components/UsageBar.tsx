import type { UsageWindow } from '../types';
import { colorFor } from '../color';
import { humanizeReset } from '../pace';

export interface BarStyle {
  thickness: number;
  font: number;
}

function trackColor(w: UsageWindow, th: { amber: number; red: number }): string {
  // Unknown data never pretends to be 0% — it shows "—" with an empty track.
  return w.isKnown && w.usedPercent !== null ? colorFor(w.usedPercent, th) : 'transparent';
}

export function UsageBar({ w, th, bs, showReset }: { w: UsageWindow; th: { amber: number; red: number }; bs: BarStyle; showReset?: boolean }) {
  const pct = w.isKnown && w.usedPercent !== null ? `${w.usedPercent.toFixed(0)}%` : '—';
  const reset = showReset && w.resetsAt ? humanizeReset(w.resetsAt) : '';
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: bs.font * 0.5, minWidth: 0, flex: 1 }}>
      <span style={{ width: bs.font * 5.5, color: '#8a93a6', fontSize: bs.font * 0.9, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', flexShrink: 0 }}>{w.title}</span>
      <div style={{ flex: 1, height: bs.thickness, borderRadius: bs.thickness / 2, background: '#2a313e', minWidth: 40 }}>
        <div style={{ width: `${(w.usedPercent ?? 0).toFixed(1)}%`, height: '100%', borderRadius: bs.thickness / 2, background: trackColor(w, th) }} />
      </div>
      <span style={{ width: bs.font * 2.8, textAlign: 'right', fontSize: bs.font, fontWeight: 500, flexShrink: 0 }}>{pct}</span>
      {showReset && (
        <span
          title={w.resetsAt ? `resets ${new Date(w.resetsAt).toLocaleString()}` : ''}
          style={{ width: bs.font * 4.2, textAlign: 'right', fontSize: bs.font * 0.85, color: '#8a93a6', whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis', flexShrink: 0 }}
        >{reset}</span>
      )}
    </div>
  );
}
