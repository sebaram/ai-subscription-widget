import { getCurrentWindow } from '@tauri-apps/api/window';
import { useEffect } from 'react';
import type { Config, ProviderSnapshot, UsageSnapshot } from '../types';
import { isTauri, setClickThrough } from '../tauri';
import { UsageBar } from './UsageBar';

const DEFAULT_TH = { amber: 50, red: 80 };

const STATE_TEXT: Record<string, string> = {
  'auth-required': 'Auth required',
  'not-running': 'Open App to track',
  'no-data': 'No data',
  'parse-failed': 'Parsing failed',
  disabled: '',
};

function visibleWindows(p: ProviderSnapshot, cfg: Config | null) {
  const want = cfg?.providers.find((c) => c.id === p.id)?.floatWindows ?? null;
  return want ? p.windows.filter((w) => want.includes(w.id)) : p.windows;
}

function Row({ p, cfg }: { p: ProviderSnapshot; cfg: Config | null }) {
  const stateText = p.state === 'ok' ? '' : (STATE_TEXT[p.state] ?? p.state);
  return (
    <div style={{ display: 'flex', alignItems: 'center', gap: 8, padding: '4px 0' }}>
      <div style={{ width: 76, display: 'flex', flexDirection: 'column', flexShrink: 0 }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
          <span style={{ width: 8, height: 8, borderRadius: '50%', background: p.brandColor }} />
          <span style={{ fontSize: 11, whiteSpace: 'nowrap', overflow: 'hidden', textOverflow: 'ellipsis' }}>{p.name}</span>
        </div>
        {stateText && <span style={{ paddingLeft: 14, fontSize: 8, color: '#8a93a6', whiteSpace: 'nowrap' }}>{stateText}</span>}
      </div>
      <div style={{ flex: 1, display: 'flex', flexDirection: 'column', gap: 3, minWidth: 0 }}>
        {visibleWindows(p, cfg).map((w) => (
          <UsageBar key={w.id} w={w} th={cfg?.thresholds ?? DEFAULT_TH} />
        ))}
      </div>
    </div>
  );
}

export function FloatBar({ snap, cfg }: { snap: UsageSnapshot | null; cfg: Config | null }) {
  const clickThrough = cfg?.floatBar.clickThrough ?? false;
  useEffect(() => {
    if (!isTauri()) return;
    void setClickThrough(clickThrough);
  }, [clickThrough]);
  return (
    <div
      onMouseDown={() => { if (isTauri()) getCurrentWindow().startDragging().catch(() => {}); }}
      style={{
        background: 'rgba(23,28,38,0.96)', border: '1px solid #2a313e', borderRadius: 12,
        padding: '8px 12px', color: '#e8eaf0', fontFamily: "'Segoe UI',sans-serif",
        opacity: (cfg?.floatBar.opacity ?? 0.95), minWidth: 320, cursor: 'default', userSelect: 'none',
      }}
    >
      {(snap?.providers ?? []).map((p) => (
        <Row key={p.id} p={p} cfg={cfg} />
      ))}
    </div>
  );
}
