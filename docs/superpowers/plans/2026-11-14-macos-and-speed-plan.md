# macOS Support & Tauri Speed Optimization Plan

- **Date:** 2026-11-14
- **Status:** Deferred / Planned
- **Architecture Decision:** Keep **Tauri v2 + React** (do not rewrite the UI to a pure-Rust GUI). Current sluggishness comes from backend polling bottlenecks and 3 eager WebViews rather than React itself, and macOS ships `WKWebView` built-in with low overhead.

---

## Part 1 — Backend & Startup Speed Fixes (Cross-Platform)

- [ ] **Concurrent provider polling (`src-tauri/src/scheduler.rs`)**: Replace sequential `for slug in due { fetch_snapshot(...).await }` with concurrent `JoinSet` / `join_all` (and emit incremental updates or cap slow providers) so `agy` CLI spawn (`45s` timeout) never stalls fast HTTP cards.
- [ ] **HTTP client reuse (`src-tauri/src/providers/mod.rs`, `src-tauri/src/scheduler.rs`)**: Build `http` and `http_loopback` `reqwest::Client` instances once at startup instead of per-poll in `FetchCtx::new()`, preserving TLS sessions and connection pools.
- [ ] **Targeted process scan (`src-tauri/src/providers/antigravity/discovery.rs`)**: Replace `sysinfo::System::new_all()` with `System::new_with_specifics(RefreshKind::nothing().with_processes(...))` to avoid scanning disks/memory/CPU/networks on every Antigravity check.
- [ ] **Eliminate idle polling loops (`src-tauri/src/scheduler.rs`, `src-tauri/src/lib.rs`)**: Share `Config` via `Arc<RwLock<Config>>` updated on `save_config` + `notify` watcher (drop the 1s `config::load()` disk read loop), and replace the 400ms `outer_position()` thread with a debounced `tauri::WindowEvent::Moved` handler.
- [ ] **Lazy auxiliary windows (`src-tauri/tauri.conf.json`, `src-tauri/src/lib.rs`)**: Boot only `floatbar` at startup; create `popup` and `settings` `WebviewWindow`s lazily on first open to cut startup WebView2 memory/processes by ~2/3.

---

## Part 2 — macOS Support

- [ ] **Compile gates**:
  - `src-tauri/src/providers/cursor/keystore.rs`: gate `dpapi_unprotect` (`#[link(name = "crypt32")]`) and Windows `oscrypt_key()` under `#[cfg(windows)]`.
  - `src-tauri/src/config.rs`: gate `.share_mode(0)` in `load_unreadable_file_never_overwrites_user_config` under `#[cfg(windows)]`.
- [ ] **macOS provider credentials & paths** (Codex & Ollama already work unchanged):
  - **Claude (`src-tauri/src/providers/claude.rs`)**: on `#[cfg(target_os = "macos")]`, read service `"Claude Code-credentials"` from macOS Keychain (`/usr/bin/security find-generic-password -s "Claude Code-credentials" -w`), falling back to `~/.claude/.credentials.json`.
  - **Cursor (`src-tauri/src/providers/cursor/keystore.rs`)**: resolve `state.vscdb` via `dirs::data_dir()` (`~/Library/Application Support/Cursor/User/globalStorage/state.vscdb` on macOS); add `#[cfg(target_os = "macos")]` Electron `safeStorage` `v10` decryption (read `"Cursor Safe Storage"` from macOS Keychain, PBKDF2-HMAC-SHA1 with salt `b"saltysalt"`, 1003 iterations, 16-byte key, AES-128-CBC with 16-byte `0x20` IV).
  - **Antigravity (`src-tauri/src/providers/antigravity.rs`, `token_source.rs`, `client_discovery.rs`, `discovery.rs`)**: use `dirs::data_dir()` for `state.vscdb`; add macOS app bundle paths (`/Applications/Antigravity.app/Contents/Resources/...`) and macOS `agy` CLI paths (`~/.antigravity/bin/agy`, `/opt/homebrew/bin/agy`, `/usr/local/bin/agy`, `PATH`); match process `name == "agy"` (not just `agy.exe`/`agy.*`); use `darwin/arm64` or `darwin/amd64` in `UA_REMOTE`.
- [ ] **macOS windowing, tray & UI**:
  - `src-tauri/tauri.conf.json`: set `"macOSPrivateApi": true` under `"app"` (required by Tauri v2 for `"transparent": true` windows on macOS).
  - `src-tauri/src/lib.rs`: set `#[cfg(target_os = "macos")] app.set_activation_policy(tauri::ActivationPolicy::Accessory);` in `.setup(...)` so no Dock icon shows (`skipTaskbar` is ignored on macOS); verify Retina logical/physical coordinates in `toggle_popup_at` / `toggle_popup`.
  - `src/components/Settings.tsx`, `src/components/FloatBar.tsx`, `src/components/Popup.tsx`: change `"start with Windows"` to `"start at login"`, make the config path footer platform-aware, and prepend `system-ui, -apple-system` to the font stack.
- [ ] **GitHub Actions CI (`.github/workflows/build.yml`)**: Run `cargo test` + `npm test` + `tauri-apps/tauri-action` on `macos-latest` (arm64/universal) and `windows-latest`, applying ad-hoc `codesign --force --deep -s -` on macOS until an Apple Developer ID is configured.
