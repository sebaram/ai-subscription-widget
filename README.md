<img src="src-tauri/icons/128x128.png" width="56" align="left" alt="AI Usage" />

# AI Usage

Lightweight Windows 11 Tauri v2 desktop widget displaying live subscription usage windows for **Claude, Codex, Antigravity, Ollama Cloud, and Cursor**.

<br clear="left" />

## Features

- **Always-on-top float bar** — draggable, resizable, configurable opacity, optional click-through mode, and green/amber/red threshold bars.
- **Click-to-expand detail view** — click the float bar (or open the popup from the system tray) to view pace markers, reset countdowns, `$` quota amounts, and per-model lanes.
- **Honest states** — reports `auth-required`, `not-running`, `no-data`, or `parse-failed` rather than fabricating `0%`.
- **Hot-reloaded config** — settings live in `%APPDATA%\ai-usage\config.json` and update live without restarting; includes autostart support.

## Supported Providers

| Provider | Shown Windows | Credential / Data Source |
|---|---|---|
| **Claude** | `5h`, weekly, extra usage | Reads `~/.claude/.credentials.json` (Claude Code login) |
| **Codex** | `5h`, weekly, model-specific limits | Reads `~/.codex/auth.json` (Codex CLI login) |
| **Antigravity** | Weekly, `5h`, per-model groups (Gemini / Others / Argon) | Probes local IDE language server or bundled `agy` CLI (optional read-only IDE Google OAuth fallback when closed) |
| **Ollama Cloud** | Monthly allowance (or legacy session/weekly) | Uses `OLLAMA_API_KEY` (Settings, env var, or `.env`) |
| **Cursor** | Pro plan `auto`, `api`, `on-demand` lanes | Reads Cursor IDE local session (`state.vscdb`, DPAPI + AES-GCM) |

## Privacy & Credentials

- Zero telemetry or third-party backend servers; all requests go directly from your machine to each provider's usage endpoint.
- Existing CLI and IDE credential files/databases are opened **read-only** and are never modified on disk (any OAuth token refresh happens strictly in memory).
- Provider usage endpoints are unofficial/internal and may change without notice.

## Install (Windows 11 x64)

Download `AI.Usage_<version>_x64-setup.exe` (NSIS installer), `.msi`, or portable `ai-usage.exe` from [Releases](https://github.com/sebaram/ai-subscription-widget/releases).

> Builds are currently unsigned, so Windows SmartScreen may show *"Windows protected your PC"* — click **More info → Run anyway**.

## Development

### Prerequisites

- Node.js 20+
- Rust stable (`x86_64-pc-windows-msvc`) + Visual Studio Build Tools (C++ workload)
- Microsoft Edge WebView2 (preinstalled on Windows 11)

### Commands

```bash
npm install
npm run tauri dev
```

Run checks and production build:

```bash
npm test
npm run build
cargo test --manifest-path src-tauri/Cargo.toml
npm run tauri build
```

## Acknowledgments & License

Provider API research was informed by [steipete/CodexBar](https://github.com/steipete/CodexBar) and [nesszer/Win-CodexBar](https://github.com/nesszer/Win-CodexBar).

Licensed under the [MIT License](LICENSE).
