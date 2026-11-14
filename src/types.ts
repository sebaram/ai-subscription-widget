// Mirror of src-tauri/src/model.rs — serde camelCase is the on-the-wire contract.
export type ProviderState = 'ok' | 'auth-required' | 'not-running' | 'no-data' | 'parse-failed' | 'disabled';

export interface QuotaInfo {
  used: string;
  total: string;
}

export interface UsageWindow {
  id: string;
  title: string;
  windowMinutes: number | null;
  usedPercent: number | null;
  quota: QuotaInfo | null;
  resetsAt: string | null;
  paceExpectedPercent: number | null;
  isKnown: boolean;
}

export interface ProviderSnapshot {
  id: string;
  name: string;
  brandColor: string;
  state: ProviderState;
  /** Honest explanation for non-ok states (auth steps, offline hints). */
  message?: string | null;
  refreshedAt: string;
  windows: UsageWindow[];
}

export interface UsageSnapshot {
  providers: ProviderSnapshot[];
  refreshedAt: string;
}

export interface ProviderCfg {
  id: string;
  enabled: boolean;
  floatWindows: string[] | null;
}

export interface Config {
  providers: ProviderCfg[];
  thresholds: { amber: number; red: number };
  pollSeconds: number;
  perProviderPoll: Record<string, number>;
  floatBar: { opacity: number; clickThrough: boolean; position: [number, number] | null };
  theme: string;
  ollamaApiKey: string;
  autostart: boolean;
  autoUpdate: boolean;
  /** Google daily-cloudcode fallback when the IDE is closed — off by default (spec). */
  antigravityOauth: boolean;
  /** Visual: bar fill thickness in px. */
  barThickness: number;
  /** Visual: base font size in px. */
  fontSize: number;
  /** Keep absent windows (e.g. Claude five_hour:null) visible as "—" rows. */
  showMissingWindows: boolean;
  /** Show time-until-reset next to each lane's percentage. */
  showResetTime: boolean;
}

export interface UpdateStatus {
  currentVersion: string;
  availableVersion: string | null;
  error: string | null;
}
