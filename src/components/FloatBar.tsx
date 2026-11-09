import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect, useRef } from 'react';
import type { Config, ProviderSnapshot, UsageSnapshot } from '../types';
import { isTauri, setClickThrough, showPopup } from '../tauri';
import { floatRows } from '../floatWindows';
import { UsageBar, type BarStyle } from './UsageBar';
import { ProviderLogo } from '../logos';

const DEFAULT_TH = { amber: 50, red: 80 };
const DEFAULT_BS: BarStyle = { thickness: 5, font: 10 };

const STATE_TEXT: Record<string, string> = {
  'auth-required': 'Auth required',
  'not-running': 'Open app to track',
  'no-data': 'No data',
  'parse-failed': 'Parsing failed',
  disabled: '',
};

function Row({ p, cfg }: { p: ProviderSnapshot; cfg: Config | null }) {
  const stateText = p.state === 'ok' ? '' : (STATE_TEXT[p.state] ?? p.state);
  const bs: BarStyle = {
    thickness: cfg?.barThickness ?? DEFAULT_BS.thickness,
    font: cfg?.fontSize ?? DEFAULT_BS.font,
  };
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: bs.font * 0.4 + 1 + 'px 0' }}>
      <div style={{ width: 76, display: 'flex', flexDirection: 'column', flexShrink: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <ProviderLogo id={p.id} color={p.brandColor} />
          <span style={{ fontSize: bs.font + 1, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</span>
        </div>
        {stateText && <span style={{ paddingLeft: 14, fontSize: bs.font * 0.8, color: '#8a93a6', whiteSpace: 'nowrap' }}>{stateText}</span>}
      </div>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
        {floatRows(p, cfg).map((w) => (
          <UsageBar key={w.id} w={w} th={cfg?.thresholds ?? DEFAULT_TH} bs={bs} />
        ))}
      </div>
    </div>
  );
}

export function FloatBar({ snap, cfg }: { snap: UsageSnapshot | null; cfg: Config | null }) {
  const clickThrough = cfg?.floatBar.clickThrough ?? false;
  const downAt = useRef<{ x: number; y: number; dragging: boolean } | null>(null);
  useEffect(() => {
    if (!isTauri()) return;
    void setClickThrough(clickThrough);
  }, [clickThrough]);

  return (
    <div
      onMouseDown={(e) => {
        downAt.current = { x: e.clientX, y: e.clientY, dragging: false };
      }}
      onMouseMove={(e) => {
        const d = downAt.current;
        if (!d) return;
        if (!d.dragging && Math.hypot(e.clientX - d.x, e.clientY - d.y) > 6 && isTauri()) {
          d.dragging = true;
          getCurrentWindow().startDragging().catch(() => {});
        }
      }}
      onMouseUp={() => {
        const d = downAt.current;
        downAt.current = null;
        if (d && !d.dragging) {
          void showPopup();
        }
      }}
      style={{
        background: 'rgba(23,28,38,0.96)', border: '1px solid #2a313e', borderRadius: 12,
        padding: '8px 12px', color: '#e8eaf0', fontFamily: "'Segoe UI',sans-serif",
        fontSize: (cfg?.fontSize ?? DEFAULT_BS.font) + 1,
        opacity: (cfg?.floatBar.opacity ?? 0.95),
        width: '100%', height: '100%', boxSizing: 'border-box',
        display: 'flex', flexDirection: 'column', justifyContent: 'center',
        cursor: 'default', userSelect: 'none', overflow: 'hidden',
      }}
    >
      {(snap?.providers ?? []).map((p) => (
        <Row key={p.id} p={p} cfg={cfg} />
      ))}
    </div>
  );
}
