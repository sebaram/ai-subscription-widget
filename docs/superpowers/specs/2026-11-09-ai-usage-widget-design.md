# AI Usage widget — design

**Date:** 2026-11-09 · **Author:** brainstorming session with Juyoung (user)
**Status:** draft for builder check

## Why

Juyoung runs Claude, Codex, Antigravity, and Ollama on Windows and wants their
subscription limits visible at all times. Existing tools (steipete/CodexBar,
nesszer/Win-CodexBar) hide most limit windows behind their compact modes and
render misleading 0% for Ollama and Antigravity. The two requirements of this
project:

1. **All real limit windows visible at once, as bars** (5-hour + weekly per
   provider; monthly for Ollama Cloud), intuitive color first, exact numbers on
   attention.
2. **Honest provider tracking** — no fake meters: figure out what each provider
   actually exposes and show nothing when there is nothing.

Source of ground truth: four research reports (2026-11-09) analyzing
CodexBar 7.0.0 and Win-CodexBar 0.70.0 source, official Ollama API docs, and
Antigravity community tooling. Verified mechanisms are stated below; the
builder should not re-derive them.

## Decisions made by Juyoung

- **Stack:** new Tauri 2 app in this repo (Rust backend, React/TypeScript UI).
  No fork. UI freedom matters; provider implementations may be ported from
  Win-CodexBar (MIT).
- **Form factor:** always-on-top **float bar** + **detail popup** (click) +
  tray icon.
- **Layout:** twin bars — one row per provider, one thin horizontal bar per
  limit window (5h / wk / mo), % at the right end (see mockups in
  `.superpowers/brainstorm/629-1794186137/content/layout-floatbar.html`).
- **Ollama:** Cloud plan via API key (official `/api/balance`). Juyoung will
  add the key; no OLLAMA_API_KEY set yet.
- **Cursor/Grok:** later (v2), so the provider registry must make adding a
  provider trivial.
- **Customization:** "easily customize" — config file + settings UI, hot
  reload.

This is a **single-user personal tool** on Windows 11. It is not a product;
no installer/packaging story is required beyond `cargo tauri dev/build`.

## Goals

1. Float bar shows every enabled provider × every real limit window as a bar,
   always visible, always on top, draggable, position remembered.
2. Popup shows on click/tray: bigger bars, pace marker (expected position),
   %, reset countdown, ETA ("runs out in ~5d"), `$ used / $ allowance` for
   Ollama, per-model-group rows for Antigravity.
3. Four providers work on this machine (see Environment facts).
4. Easy customization via one JSON config + small settings popup; file edits
   hot-reload without restart.
5. Honest states everywhere: `Auth required`, `Open Antigravity to track`,
   `No data`, `Parsing failed` — never a fabricated 0%.

## Anti-goals (what would make this a failure)

- Showing **invented limits**: no API exposes a "daily" window — do not render
  one. Local Ollama has no quota concept — do not render one.
- Rendering "0% used" when data is actually missing/unknown (CodexBar marks
  these `isSyntheticPlaceholder`/`usageKnown=false` — do the same, hide or
  label them).

## Non-goals (v1)

- Cursor/Grok providers (registry makes them a future module + config entry).
- Multi-account switching; historical usage graphs; spend accounting beyond
  Ollama's balance display; Windows Widgets panel integration; signed installer.
- HTML/cookie scraping for any provider (explicitly out: Ollama settings-page
  scraping is brittle and unnecessary; Chrome/Edge cookie decryption is blocked
  by App-Bound Encryption anyway).

## Constraints

- Windows 11, no admin rights, no global proxy interference with loopback
  probes (loopback traffic must bypass any system proxy).
- No new secrets stored beyond what the user enters (Ollama API key, stored
  under user profile; Claude/Codex credentials are already on disk and only
  read).
- Poll rates must respect provider limits (Ollama API: 10 req/min → poll
  ≥ 60 s; default global poll 5 min).

## Environment facts (2026-11-09, this machine)

- `~/.claude/.credentials.json` — **exists** (Claude usable via OAuth token).
- `~/.codex/auth.json` — **exists** (Codex usable; includes OAuth token).
- `~/.gemini/oauth_creds.json` — **absent** (Antigravity remote fallback needs
  `gemini` CLI login → treat as optional toggle, off by default).
- `%LOCALAPPDATA%\antigravity` — **exists** (Antigravity installed; its
  language-server process is NOT always running — discovery must be
  process-scan based, never path-hardcoded).
- `OLLAMA_API_KEY` — not set → Ollama card shows `Auth required` state until
  configured.

## Provider specs

### Claude (two mechanisms, OAuth preferred)

- **OAuth:** `GET https://api.anthropic.com/api/oauth/usage?cedar_ember=1`
  with `Authorization: Bearer <token>` + anthropic beta header. Token read
  from `~/.claude/.credentials.json`. JSON: `five_hour{utilization,
  resets_at}`, `seven_day{utilization, resets_at}`, plus
  `seven_day_oauth_apps`, `seven_day_opus`, `seven_day_sonnet`, `extra_usage`.
- **Fallback (manual sessionKey paste):** `GET /api/organizations` → org id →
  `GET /api/organizations/{org}/usage` (same window fields).
- Windows: 5h → primary; 7d → secondary; the seven_day_* variants → named
  extra windows (only shown if present in payload).

### Codex

- `GET https://chatgpt.com/backend-api/wham/usage` (fallback
  `/api/codex/usage`), `Authorization: Bearer <token from
  ~/.codex/auth.json>`, `ChatGPT-Account-Id` header from same file.
- JSON: `rate_limit.primary_window{used_percent, limit_window_seconds,
  reset_at}` (5h) and `secondary_window{...}` (weekly); named extras in
  `additional_rate_limits[]`. Token expiry → refresh (CodexBar has a reference
  refresher; port or re-implement).

### Antigravity (local LSP probe; remote OAuth as optional fallback)

- **Discovery:** `Win32_Process` scan for `language_server_windows*`,
  `language_server.exe`, `agy.exe`. From the command line parse
  `--csrf_token` / `--extension_server_csrf_token` and the port flags.
  Antigravity 2.x may pass `--https_server_port 0` (random) — resolve the
  real port by enumerating the PID's listening TCP sockets.
- **Probe:** `POST https://127.0.0.1:<port>/
  exa.language_server_pb.LanguageServerService/GetUnleashData` (self-signed
  TLS: accept invalid certs **only** for 127.0.0.1; headers
  `X-Codeium-Csrf-Token`, `Connect-Protocol-Version: 1`). 200/401 ⇒ live.
- **Quota:** `POST .../RetrieveUserQuotaSummary` body `{"forceRefresh":true}`
  → `groups[].buckets[]{remainingFraction | remaining.remainingFraction,
  window, resetTime}`. Groups seen in the wild: "Gemini Models" and "Claude
  and GPT models", each carrying 5-hour and weekly buckets.
  `used% = 100 − remainingFraction×100`.
- **Fallbacks in order:** `GetUserStatus`
  (`userStatus.cascadeModelConfigData.clientModelConfigs[].quotaInfo.
  remainingFraction` + `resetTime`) — note the IDE server often 404s
  RetrieveUserQuotaSummary; then `GetCommandModelConfigs`; then (optional,
  toggled off) remote: `POST
  https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary`
  with Google OAuth bearer from `~/.gemini/oauth_creds.json`.
- **No process + no OAuth file** ⇒ card state "Open Antigravity to track".

### Ollama Cloud

- `GET https://ollama.com/api/balance`, `Authorization: Bearer
  <OLLAMA_API_KEY>` (env var or settings file).
- Standard plans: `included.balance_usd`, `included.allowance_usd`,
  `included.period{from,until}` → monthly bar, used% =
  (allowance − balance)/allowance, display `$8.40 / $35`.
- Legacy plans: `included.session{remaining_percent}` (5-hour window) and
  `included.weekly{remaining_percent}` → session/weekly bars at
  `100 − remaining_percent`. `purchased.balance_usd` → informational row.
- Unknown/absent plan shape ⇒ "No quota reported" state, not 0%.

## Architecture

- Rust backend owns everything: config watcher, scheduler, provider modules,
  HTTP, push. React owns nothing but drawing.
- **Provider registry** (port of Win-CodexBar's shape): `ProviderId` enum +
  `spec()` (slug, name, brand color) + `Provider` trait with
  `fetch_usage(ctx) -> ProviderSnapshot`; new provider = id + spec + module +
  registry arm (this is the Cursor/Grok door).
- **Snapshot model** (Tauri event, typed in TS):
  `{ providers: [{ id, name, color, state, refreshedAt,
     windows: [{ id, title, windowMinutes, usedPercent?, quota? ,
        quotaUnit?, resetsAt?, paceExpectedPercent?, isKnown }] }] }`.
  `isKnown=false` windows render as "Unavailable", never 0%.
- Windows: float bar (always-on-top, no taskbar entry, optional
  click-through, frameless, rounded), popup (frameless, opens on click /
  tray), settings. Positions persisted.
- Thresholds: green < 50 %, amber 50–80, red > 80 (configurable). Pace marker
  = expected used % (elapsed / window; simple linear, no workday logic in v1).

## Customization

Config file: `%APPDATA%\ai-usage\config.json` (exact location/format =
builder's call). Schema covers: `providers[]` (enable, order,
windows-shown-on-float vs popup-only), `thresholds`, `pollSeconds` (global +
per-provider), `floatBar` (anchor position, opacity, click-through, height),
`theme` (dark/system), `ollamaApiKey`. File watcher hot-reloads both sides.
Settings popup edits the common subset and writes the same file. Everything
sensible gets a default: an empty/missing config yields all four providers
enabled with defaults.

## Left to the builder

Exact config serialization, project scaffolding, component structure, retry/
backoff UX, token-refresher implementation details, fixture-based tests for
each fetcher (record real responses into fixtures during dev), build/run
scripts. Tests required for: JSON parsers per provider, percent math
(including `remaining_fraction` → %), config hot-reload. No tests for visual
layout.

## Open question for Juyoung (pending)

- Preferred location for the float bar on first run (top-right? bottom-right?)
  — cosmetic; default = user drags it anywhere, position persists.
