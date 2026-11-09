import { useEffect, useState } from 'react';
import { onConfig, onSnapshot, windowLabel } from './tauri';
import type { UsageSnapshot, Config } from './types';
import { FloatBar } from './components/FloatBar';

function App() {
  const [snap, setSnap] = useState<UsageSnapshot | null>(null);
  const [cfg, setCfg] = useState<Config | null>(null);

  useEffect(() => {
    const u1 = onSnapshot(setSnap);
    const u2 = onConfig(setCfg);
    return () => { u1.then((f) => f()); u2.then((f) => f()); };
  }, []);

  const label = windowLabel();
  if (label === 'floatbar') return <FloatBar snap={snap} cfg={cfg} />;
  return (
    <main className="container" style={{ fontSize: 13, color: '#e8eaf0' }}>
      <p>window: {label}{snap ? '' : ' (no snapshot yet)'}</p>
      <ul>
        {(snap?.providers ?? []).map((p) => (
          <li key={p.id}>
            <span style={{ color: p.brandColor }}>●</span> {p.name} [{p.state}]{' '}
            {p.windows.map((w) => (
              <span key={w.id}>
                {w.title} {w.isKnown ? `${w.usedPercent!.toFixed(1)}%` : '— (Unavailable)'}{' '}
              </span>
            ))}
          </li>
        ))}
      </ul>
      {cfg && <p>poll {cfg.pollSeconds}s · amber {cfg.thresholds.amber}% · red {cfg.thresholds.red}%</p>}
    </main>
  );
}

export default App;
