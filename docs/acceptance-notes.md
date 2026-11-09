# Acceptance notes — 2026-11-09

## Verified while session locked (evidence from process/window enumeration)

- `ai-usage.exe` running (`npm run tauri dev`), hot-reload watcher cycle proven
  (multiple "File changed → Rebuild → Running" cycles in dev log).
- Windows present, matching `tauri.conf.json`:
  - floatbar `AI Usage` **visible** at (154,154)–(506,354) = 352×200 ✓
  - popup 430×600, hidden until click ✓
  - settings 470×620, hidden until menu/command ✓
- Capabilities bug found and fixed live: scaffold capability allowed only the
  old "main" window, so `event.listen` was denied in all three new windows
  (vite log). Fixed in `capabilities/default.json`; post-fix log is clean.
- Config hot-reload test executed: `codex.enabled=false` written to
  `%APPDATA%\ai-usage\config.json` while running → watcher reloads + re-emits;
  visual confirmation pending desktop unlock (row should disappear).

## Suite gates at completion

- Rust: `cargo test` — 43 passed / 0 failed
- Frontend: vitest — 15 passed (2 files), `tsc --noEmit` clean

## Pending desktop unlock (visual leg)

1. Float bar visible with real provider rows/states.
2. Codex row absent (config edit proof), then re-enabled via Settings + Save
   → row returns without restart (save path proof).
3. Tray icon: left click opens popup near cursor; Escape hides.
4. Ollama card shows the cloud plan from the user's `.env` key once the
   balance endpoint answers.
