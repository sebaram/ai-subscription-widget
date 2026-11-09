# AI Usage Widget Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build "AI Usage" — a Windows 11 Tauri 2 widget showing Claude/Codex/Antigravity/Ollama-Cloud limit windows (5h/weekly/monthly) as always-visible twin bars plus a detail popup, with honest data states and hot-reloadable config.

**Architecture:** Rust backend owns all fetching (per-provider module + match-arm registry, ported from Win-CodexBar's shape), runs a poll scheduler, and pushes typed `usage-snapshot` events. React frontend is purely presentational with three thin windows (`floatbar`, `popup`, `settings`) rendered from one SPA by window label.

**Tech Stack:** Tauri 2, Rust stable (msvc), reqwest (rustls), serde, notify, sysinfo, netstat2, chrono; Vite + React + TypeScript; vitest on the frontend; npm (NOT pnpm — not installed, npm 11 suffices).

**Spec:** `docs/superpowers/specs/2026-11-09-ai-usage-widget-design.md` (read it first)
**API reference:** `docs/research/provider-api-details.md` — exact headers, URLs, JSON field paths per provider. The reference WINS on any field-name conflict. Do not re-derive.

## Global Constraints

- Tauri **2.x** (not 1.x). Rust stable, target `x86_64-pc-windows-msvc`.
- Honest states only: `ok`, `auth-required`, `not-running`, `no-data`, `parse-failed`, `disabled`. A window with unknown data has `isKnown:false` and renders "Unavailable" — **never 0%**.
- No invented "daily" window anywhere. Local Ollama is out of scope entirely.
- Never write to `~/.claude/.credentials.json` / `~/.codex/auth.json`. Token refreshes happen in memory per call only.
- Ollama poll interval ≥ 60 s (API limit 10 req/min). Default global poll: 300 s.
- Thresholds: green < 50 %, amber ≥ 50 < 80, red ≥ 80. Colors: `#34c07c` / `#e2b93d` / `#e8503a`. Brand colors: claude `#d9775f`, codex `#10a37f`, antigravity `#4285f4`, ollama `#8b9ba8`.
- Config file: `%APPDATA%\ai-usage\config.json`, camelCase keys, hot-reloaded (file watcher), always loads with defaults on missing/corrupt file.
- Float bar: always-on-top, frameless, no taskbar entry, transparent-capable; antigravity float shows **most-constrained group per window** (worst-of); popup shows all groups.
- Auth-required if `OLLAMA_API_KEY` empty (env var or config field).
- All user-visible strings English; code comments only for non-obvious why.

## Review Focus

Failure modes the spec implies but unit tests below don't fully cover — each is pinned by a named test in its owning task:

1. **Corrupt/missing config file must never panic or blank the widget** — `config::load_returns_defaults_on_malformed_file` (Task 2).
2. **Ollama legacy plan shape** (session/weekly instead of allowance) must produce session+weekly bars, not an empty card — `ollama_legacy_plan_maps_session_and_weekly` (Task 4).
3. **Antigravity with two model groups** must show worst-of per window on the float bar — `antigravity_worst_of_groups_per_window` (Task 7).
4. **Expired Claude/Codex access tokens** refresh in memory; impossible refresh → `auth-required` state, never a stuck 0% — code path Task 5/6, live check Task 12.
5. **Antigravity IDE closed, no gemini OAuth** → `not-running` state rendering "Open Antigravity to track" — `antigravity_no_process_no_oauth_is_not_running` (Task 7), render Task 8.

---

### Task 1: Toolchain + Tauri scaffold

**Files:**
- Create (via scaffold, then moved): package.json, vite.config.ts, index.html, src/*, src-tauri/{Cargo.toml, tauri.conf.json, icons/*}
- Rename in place: package `name: "ai-usage"`, productName `AI Usage`, identifier `com.juyoung.aiusage`

**Interfaces:**
- Produces: runnable Tauri 2 app scaffold; frontend at repo root, Rust crate in `src-tauri/`; npm scripts `dev` (vite), `tauri` (@tauri-apps/cli 2.x).

- [ ] **Step 1: Install VS Build Tools VC workload (background, long)**

```powershell
winget install --id Microsoft.VisualStudio.2022.BuildTools --accept-source-agreements --accept-package-agreements --override "--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```
Start this first — 5+ GB, run in background while other steps proceed.

- [ ] **Step 2: Install Rust via rustup (winget)**

```powershell
winget install --id Rustlang.Rustup --accept-source-agreements --accept-package-agreements
rustup default stable-msvc
```
Verify: `cargo --version` succeeds in a NEW shell (PATH refreshed).

- [ ] **Step 3: Scaffold into temp, move into repo root**

```powershell
npm create tauri-app@latest ai-usage-scaffold -- --template react-ts --manager npm --identifier com.juyoung.aiusage --yes
Move-Item ai-usage-scaffold/* ai-usage-scaffold/.* repo-root-root, excluding .git/; delete ai-usage-scaffold dir
```
Then in package.json set `"name": "ai-usage"`; in src-tauri/tauri.conf.json set `productName: "AI Usage"`, `identifier: "com.juyoung.aiusage"`.

- [ ] **Step 4: Verify dev window opens** (after Build Tools finished)

Run: `npm run tauri dev` — expected: a Tauri window opens with the template UI; close it. (If `link.exe` missing → Build Tools still installing; continue other tasks, retry later.)

- [ ] **Step 5: Commit**

```bash
git add -A && git commit -m "chore: scaffold Tauri 2 app"
```

### Task 2: Config module — defaults, load/save, hot-reload watcher

**Files:**
- Create: `src-tauri/src/config.rs`, `src-tauri/src/config.rs` tests (inline `#[cfg(test)]`)
- Modify: `src-tauri/src/lib.rs` (call `spawn_config_watcher` in setup)

**Interfaces:**
- Produces (later tasks consume):
```rust
#[derive(Serialize, Deserialize, Clone, Debug)] #[serde(rename_all = "camelCase", default)]
pub struct Config {
    pub providers: Vec<ProviderCfg>,          // order = display order
    pub thresholds: Thresholds,               // { amber: f64 = 50.0, red: f64 = 80.0 }
    pub poll_seconds: u64,                    // default 300
    pub per_provider_poll: HashMap<String, u64>,
    pub float_bar: FloatBarCfg,               // { opacity: f64 = 0.95, click_through: bool, position: Option<(f64,f64)> }
    pub theme: String,                        // "dark" (only supported value in v1)
    pub ollama_api_key: String,               // default ""
    pub autostart: bool,                      // default true
}
#[derive(...)] pub struct ProviderCfg { pub id: String, pub enabled: bool, pub float_windows: Option<Vec<String>> } // None = show all
pub fn config_path() -> std::path::PathBuf   // %APPDATA%\ai-usage\config.json
pub fn load() -> Config                      // missing/malformed → defaults, file written
pub fn save(cfg: &Config) -> ()              // atomic write (tmp + rename)
pub fn spawn_config_watcher(app: tauri::AppHandle) // notify debounce 300ms; on change: load() + emit "config-changed" with new Config JSON
pub fn defaults() -> Config                  // providers: claude,codex,antigravity,ollama all enabled, float_windows None
```

- [ ] **Step 1: Write failing tests** — `load_missing_file_returns_defaults`, `load_malformed_file_returns_defaults` (write `{not json` to temp path via overriding `config_path`, assert file rewritten with defaults), `serde_camel_case_roundtrip` (Config→JSON string contains `"pollSeconds"` and key `floatWindows`).
- [ ] **Step 2: Run:** `cargo test config` — expect FAIL (module absent).
- [ ] **Step 3: Implement** config.rs per interfaces (use `dirs` crate for `%APPDATA%`). Watcher: `notify` crate, 300ms debounce, on event → load + emit.
- [ ] **Step 4: Run:** `cargo test config` — PASS.
- [ ] **Step 5: Commit** `feat: config module with hot reload`

### Task 3: Model types + scheduler + registry skeleton + frontend listening

**Files:**
- Create: `src-tauri/src/model.rs`, `src-tauri/src/scheduler.rs`, `src-tauri/src/providers/mod.rs`, `src/types.ts`, `src/tauri.ts`
- Modify: `src-tauri/src/lib.rs` (state, commands), `src/App.tsx` (render snapshot), `src-tauri/tests/` none

**Interfaces:**
- Consumes: `config::Config`.
- Produces (all later tasks depend on these exact shapes):
```rust
// model.rs — all Serialize with rename_all="camelCase" (these JSON keys are the FE contract)
pub enum ProviderId { Claude, Codex, Antigravity, Ollama }           // serde lowercase: "claude"...
pub enum ProviderState { Ok, AuthRequired, NotRunning, NoData, ParseFailed, Disabled }
pub struct QuotaInfo { used: String, total: String }
pub struct UsageWindow { id, title, windowMinutes: Option<i64>, usedPercent: Option<f64>, quota: Option<QuotaInfo>, resetsAt: Option<String /*ISO8601*/>, paceExpectedPercent: Option<f64>, isKnown: bool }
pub struct ProviderSnapshot { id, name, brandColor, state, refreshedAt: String, windows: Vec<UsageWindow> }
pub struct UsageSnapshot { providers: Vec<ProviderSnapshot>, refreshedAt: String }
pub fn spec(id) -> (name: &'static str, brand_color: &'static str) // see Global Constraints colors
// providers/mod.rs
pub struct FetchCtx<'a> { pub cfg: &'a Config, pub http: reqwest::Client, pub http_loopback: reqwest::Client } // loopback client: .no_proxy() + danger_accept_invalid_certs(true)
pub async fn fetch_snapshot(id: ProviderId, ctx: &FetchCtx) -> ProviderSnapshot  // match arm per provider; disabled → Disabled snapshot
// scheduler.rs
pub fn start(app: AppHandle) // tokio task per enabled provider: interval per config (global 300s, ollama >=60s), tick → fetch_snapshot → emit "usage-snapshot"; ALSO listens for "config-changed" internally via re-created intervals
```
Event name pinned: **`usage-snapshot`** payload = `UsageSnapshot` JSON. Commands pinned: `refresh_now()` (re-fetch enabled providers immediately), `open_popup()`.

- TS mirror in `src/types.ts` — same fields camelCase, `type ProviderState = "ok"|"auth-required"|"not-running"|"no-data"|"parse-failed"|"disabled"`.
- `src/tauri.ts`: `onSnapshot(cb)`, `onConfig(cb)`, `refreshNow()`, `openPopup()` — thin wrappers over `@tauri-apps/api/event`/`core`; when NOT running inside Tauri (no `__TAURI_INTERNALS__`), a **mock shim** emits a canned fake snapshot every 2 s (so `npm run dev` in plain browser + vitest/screenshots work without backend).
- `App.tsx`: reads `getCurrentWebviewWindow().label`, renders `<FloatBar/>` | `<Popup/>` | `<Settings/>` placeholder; placeholder renders provider name + windows as text lines.

- [ ] **Step 1: Failing test** `snapshot_json_uses_camel_case_keys` in model.rs: build one snapshot with two windows, serde_json::to_string, assert contains `"usedPercent"`, `"windowMinutes"`, `"isKnown"`, `"refreshedAt"`.
- [ ] **Step 2: Run `cargo test model` → FAIL.**
- [ ] **Step 3: Implement** model.rs, providers/mod.rs skeleton (each arm → `todo!()` replaced with Disabled snapshot this task), scheduler.rs, config-driven intervals.
- [ ] **Step 4: Run `cargo test model` → PASS; `npm run dev` in browser → mocked rows appear. Also `npm run tauri dev` → placeholder rows with real (empty) snapshots, no console errors.**
- [ ] **Step 5: Commit** `feat: snapshot model, scheduler, provider registry skeleton`

### Task 4: Ollama provider (cloud balance)

**Files:**
- Create: `src-tauri/src/providers/ollama.rs`; Modify: `providers/mod.rs` (match arm)
- Reference: `docs/research/provider-api-details.md` §6 — exact URL/headers/fields.

**Interfaces:**
- Consumes: `FetchCtx` (`cfg.ollama_api_key` empty ⇒ auth-required), `http` client.
- Produces: `pub async fn fetch(ctx: &FetchCtx) -> ProviderSnapshot`.
- Windows: standard plan → one `month` window (`id:"month"`, `windowMinutes: 43200`, usedPercent=(allowance−balance)/allowance×100, quota `$X of $Y`, resetsAt=period.until); legacy plan → `session` (300 min) + `week` (10080) from remaining_percent (used = 100−remaining); `purchased.balance_usd` → informational extra window `purchased` (isKnown true, usedPercent:null, quota). Key empty → state `auth-required`.

- [ ] **Step 1: Failing tests** (fixture JSON consts inline from reference §6): `ollama_standard_plan_maps_monthly_bar` (allowance 35.0, balance 26.6 → 24.0 %, quota "$26.60 of $35.00", resetsAt parsed), `ollama_legacy_plan_maps_session_and_weekly` (remaining_percent 75 → used 25 %), `ollama_empty_key_is_auth_required`, `ollama_unknown_shape_is_no_data_not_zero`.
- [ ] **Step 2: `cargo test ollama` → FAIL.**
- [ ] **Step 3: Implement** — GET `https://ollama.com/api/balance` Bearer; branch on shape; reqwest errors → `parse-failed`? no: network error → `no-data` with isKnown:false.
- [ ] **Step 4: `cargo test ollama` → PASS. Commit** `feat: ollama cloud provider`

### Task 5: Claude provider

**Files:**
- Create: `src-tauri/src/providers/claude.rs`; Modify: `providers/mod.rs`
- Reference: §1 (OAuth usage call), §4 (credentials file).

**Interfaces:**
- Produces: `pub async fn fetch(ctx) -> ProviderSnapshot`.
- Behavior: read `~/.claude/.credentials.json` → `claudeAiOauth.accessToken` (+`refreshToken`,`expiresAt` epoch ms). Missing file/keys ⇒ `auth-required`. Token expired (>now−30s skew): refresh IN MEMORY via `POST https://platform.claude.com/v1/oauth/token` (client_id + params from reference §4) — never write to file. Then GET `https://api.anthropic.com/api/oauth/usage?cedar_ember=1` with `Authorization: Bearer`, `anthropic-beta: oauth-2025-04-20`, claude-cli UA, JSON accept/content-type (exact per reference). Map: `five_hour` → window `5h` (300 min), `seven_day` → `week` (10080), `seven_day_oauth_apps`/`seven_day_sonnet`/`seven_day_opus` → named extras ONLY if present; `extra_usage` → extra `extra` window w/ quota if present. utilization already percent (0-100); null utilization ⇒ isKnown:false.

- [ ] **Step 1: Failing tests** — `claude_fixture_maps_5h_and_week` (fixture from reference §1: five_hour.utilization 42.0 resets_at "2026-11-09T15:46:00Z", seven_day 61.0 → windows 5h/week + titles "5h"/"wk", paceExpectedPercent present when window start computable), `claude_extras_only_when_present`, `claude_missing_credentials_file_is_auth_required`, `claude_token_expiry_detection` (expiresAt in past → needs_refresh true; in future → false).
- [ ] **Step 2: `cargo test claude` → FAIL.**
- [ ] **Step 3: Implement** (credentials path: `dirs::home_dir()/.claude/.credentials.json`; parse via serde_json::Value to tolerate shape drift; refresh fn = separate `refresh_access_token(refresh_token, &http) -> Result<String>`).
- [ ] **Step 4: `cargo test claude` → PASS. Commit** `feat: claude oauth usage provider`

### Task 6: Codex provider

**Files:**
- Create: `src-tauri/src/providers/codex.rs`; Modify: `providers/mod.rs`
- Reference: §3.

**Interfaces:**
- Produces: `pub async fn fetch(ctx) -> ProviderSnapshot`.
- Behavior: read `~/.codex/auth.json` → access token (+ `tokens.account_id` else JWT claim from token), missing ⇒ `auth-required`. GET `https://chatgpt.com/backend-api/wham/usage` with `Authorization: Bearer`, `ChatGPT-Account-Id: <id>`, UA per reference. Map `rate_limit.primary_window{used_percent,limit_window_seconds,reset_at}` → `5h`; `secondary_window` → `week`; `additional_rate_limits[]` → named windows (id from name-slug). In-memory refresh on 401 via `POST https://auth.openai.com/oauth/token` (client_id per reference §3), never writing auth.json.

- [ ] **Step 1: Failing tests** — `codex_fixture_maps_primary_secondary` (primary used_percent 12.0, limit_window_seconds 18000, secondary 33.0/604800 → ids 5h/week), `codex_additional_rate_limits_become_named_windows`, `codex_account_id_from_jwt_fallback` (fixture JWT → claim `chatgpt_account_id`), `codex_missing_auth_is_auth_required`.
- [ ] **Step 2: `cargo test codex` → FAIL.**
- [ ] **Step 3: Implement. Step 4: PASS. Commit** `feat: codex usage provider`

### Task 7: Antigravity provider (local LSP probe)

**Files:**
- Create: `src-tauri/src/providers/antigravity.rs`, `src-tauri/src/providers/antigravity/discovery.rs`; Modify: `providers/mod.rs`
- Reference: §5 (paths, headers, exact arithmetic `used_percent = 100 − (fraction*100).clamp(0,100)`, window minutes 300/10080).

**Interfaces:**
- Produces: `pub async fn fetch(ctx) -> ProviderSnapshot`.
- Pipeline: discovery.rs → `find_lsp_command_line() -> Option<LspInfo { csrf_token: Option<String>, port_hint: Option<u16>, pid: u32 }>` via sysinfo (names containing `language_server` or == `agy.exe`); `find_port(pid) -> Option<u16>` via netstat2 LISTEN sockets owned by pid (filtered to loopback), try port_hint & ext_port+0..19 fallbacks as ordering hint. antigravity.rs: probe `POST https://127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/GetUnleashData` via `http_loopback` + header `X-Codeium-Csrf-Token` (when token exists) + `Connect-Protocol-Version: 1`; 200/401 ⇒ alive. Then quota chain: RetrieveUserQuotaSummary `{"forceRefresh":true}` → parse `groups[].buckets[]{remainingFraction|remaining.remainingFraction, window, resetTime}` (window 300 min ⇒ `5h`, 10080 ⇒ `week`); 404 → GetUserStatus → `userStatus.cascadeModelConfigData.clientModelConfigs[].quotaInfo.remainingFraction` + resetTime (title per model).
- Selection: **per window type keep highest usedPercent across ALL groups/buckets (worst-of)**; every group's buckets also become extra windows `group:{groupId}:{5h|week}` with group title.
- No process found ⇒ `not-running` (state text handled in UI).

- [ ] **Step 1: Failing tests** (all on fixture strings/serde values, no live process): `cmdline_parses_csrf_and_port` (`agy --csrf_token=tok123 --https_server_port 0 --app_data_dir antigravity` → csrf Some("tok123"), port_hint None), `bucket_window_maps_300_10080` (table: 300→"5h", 10080→"week", 43200→"month", other→title "custom"), `antigravity_worst_of_groups_per_window` (two-group fixture: Gemini 5h 10 %, Claude-GPT 5h 90 % → float window 5h = 90.0), `used_percent_from_fraction_clamps` (1.0→0.0, 0.42→58.0, 1.5→0.0), `antigravity_no_process_no_oauth_is_not_running` (fetch with no processes + no `%USERPROFILE%\.gemini\oauth_creds.json` → state NotRunning).
- [ ] **Step 2: `cargo test antigravity` → FAIL.**
- [ ] **Step 3: Implement** discovery + probe + parse chain. Loopback client only from `ctx.http_loopback` (no proxy, invalid certs accepted — TLS self-signed per spec).
- [ ] **Step 4: `cargo test antigravity` → PASS. Commit** `feat: antigravity lsp probe provider`

### Task 8: Float bar UI (twin bars)

**Files:**
- Create: `src/components/UsageBar.tsx`, `src/components/FloatBar.tsx`, `src/color.ts`, `src/color.test.ts`
- Modify: `src/App.tsx`, `src/styles.css`

**Interfaces:**
- Consumes: `types.ts` UsageSnapshot via `onSnapshot`, `Config` via `onConfig` (window filter: `ProviderCfg.floatWindows` list or all; provider order = config array order), `thresholds`.
- Produces: `export function colorFor(pct: number, th: {amber:number;red:number}): string` → `#34c07c` | `#e2b93d` | `#e8503a`.
- FloatBar visuals (from approved mockup): rows = provider dot (brandColor) + name + one thin horizontal bar per shown window (label 5h/wk/mo left, % right, rounded 5px); `isKnown:false` → row shows "—" instead of %; state ≠ ok → per-state text under name (`Auth required` etc. per spec). Window drag = move; on drop → invoke `save_float_position(x, y)`.

- [ ] **Step 1: Failing vitest** `color.test.ts`: colorFor(49,…)=green, 50=amber, 79.9=amber, 80=red, 0=green (= #34c07c/#e2b93d/#e8503a).
- [ ] **Step 2: `npx vitest run` → FAIL.**
- [ ] **Step 3: Implement** color.ts, UsageBar, FloatBar (windowMinutes never rendered as daily; no daily row possible).
- [ ] **Step 4: vitest PASS; `npm run dev` browser shows mocked float bar.**
- [ ] **Step 5: Commit** `feat: float bar twin-bars UI`

### Task 9: Windows + tray wiring (popup opens where the user expects)

**Files:**
- Modify: `src-tauri/tauri.conf.json` (3 windows: `floatbar`, `popup`, `settings` — `visible:false`, frameless, no-taskbar, skipTaskbar, alwaysOnTop for floatbar; transparent true)
- Modify: `src-tauri/src/lib.rs` (tray: TrayIconBuilder with default app icon; left click → toggle popup near cursor; menu items "Open popup", "Settings…", "Refresh now", "Quit"; commands `open_popup`, `open_settings`, `hide_window(label)`)
- Create: `src/components/Popup.tsx` (detail cards from mockup: bigger bars, pace tick marker at expected %, reset countdown humanized from resetsAt, "$X of $Y" quota, per-model-group extras for antigravity, state messages; Escape → hide_window)

- [ ] **Step 1 (test):** Rust test `popup_position_command_clamps_to_screen` — open_popup positions inside virtual screen bounds (pure geometry helper test).
- [ ] **Step 2: FAIL → Step 3: implement** tray + commands + Popup.tsx → **Step 4: PASS; `npm run tauri dev`: tray icon appears; clicking float bar opens popup at cursor; Escape hides. No console errors.** → **Step 5: Commit** `feat: tray, window wiring, popup`

### Task 10: Settings window + write path

**Files:**
- Create: `src/components/Settings.tsx`; Modify: `lib.rs` (command `save_config(cfg: Config)`), `config.rs`

**Interfaces:**
- Consumes: `Config` (onConfig), produces `save_config` command → `config::save` + re-emit `config-changed`.
- Settings edits: provider enable toggles (in config order), float-windows per provider (checkbox 5h/wk/mo/extras), thresholds sliders, pollSeconds, opacity slider, ollamaApiKey input, autostart checkbox, theme select (dark/system → system renders same dark in v1). Save button → `save_config`.

- [ ] **Step 1 (test):** Rust `save_then_load_roundtrip_preserves_fields` (ProviderCfg order + floatWindows Some(["5h"]) preserved).
- [ ] **Step 2: FAIL. Step 3: implement. Step 4: PASS + `npm run tauri dev`: change threshold in settings → float bar color flips live without restart.** → **Step 5: Commit** `feat: settings window with hot reload`

### Task 11: Polish — autostart, position persist, opacity, click-through

**Files:**
- Modify: `Cargo.toml` (`tauri-plugin-autostart = "2"`), `lib.rs`, `FloatBar.tsx`, `config.rs`

**Interfaces:**
- Consumes: `Config.floatBar.{opacity, clickThrough, position}`, `Config.autostart`.
- Behavior: autostart init on login (respect config); floatbar applies opacity css (0.2–1.0); clickThrough toggles `set_ignore_cursor_events`; position restored from config on start; window moved event → save position.

- [ ] **Step 1 (test):** Rust `autostart_flag_toggles_plugin` (unit around plugin state fn). **Step 2: FAIL. Step 3 implement. Step 4: PASS + manual: float bar remembers position across restart. Step 5: Commit** `feat: autostart, position persistence, opacity, click-through`

### Task 12: Acceptance — see it, test live updates

**Files:**
- Create: `docs/acceptance-notes.md` (screenshots + result log); screenshots saved under `docs/screenshots/`

**Interfaces:** consumes everything.

- [ ] **Step 1: `npm run tauri dev` in background; wait for window.**
- [ ] **Step 2: Screenshot via PowerShell (CopyFromScreen → PNG) → Read the PNG: float bar visible, 4 provider rows, real states (claude/codex likely `ok` with real %; antigravity = `not-running` unless IDE open; ollama = `auth-required`).**
- [ ] **Step 3: Open popup (tray click) → screenshot → Read.**
- [ ] **Step 4: Live-update test:** edit `%APPDATA%\ai-usage\config.json` (set `pollSeconds: 60`, change `thresholds.amber` 20) → within ~5 s screenshot again → float bar reflects change without restart (UPDATE WORKS).
- [ ] **Step 5: Fix anything broken found in steps 2–4, repeat until pass, then `cargo test` + `npx vitest run` full suite green.**
- [ ] **Step 6: Commit** `test: acceptance run notes + screenshots`

## Self-review notes

- Spec coverage: float/popup/settings ✓ (T8–T10), four providers ✓ (T4–T7), honest states ✓ (T3 model, T7 review-focus, T8 render), hot reload ✓ (T2/T10), autostart ✓, worst-of ✓, no write-back ✓ (T5/T6), ollama poll floor ✓ (T3 scheduler), acceptance of "works as update" ✓ (T12).
- Types consistent: `usage-snapshot`/`config-changed` strings, camelCase JSON keys, ProviderState union — single definitions in T3.
- Proportion: signatures + tests, no full bodies — plan ≈ code complexity is low where tests pin behavior.
- Deliberately out: multi-account, window "daily", local Ollama, click-through default true, app self-update (user deferred beyond "works as update" = live data/config updates).
