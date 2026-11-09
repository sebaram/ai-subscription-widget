# Provider API details (extracted from CodexBar / Win-CodexBar)

Citations: `research-codexbar:` = steipete/CodexBar (Swift), `research-win-codexbar:` = nesszer/Win-CodexBar (Rust).
Both clones were at `%TEMP%\research-codexbar` / `%TEMP%\research-win-codexbar` when extracted. Everything below is what the source shows; anything not shown is marked UNKNOWN.

## 1. Claude — OAuth usage API

- `GET https://api.anthropic.com/api/oauth/usage?cedar_ember=1` — research-codexbar: Sources/CodexBarCore/Providers/Claude/ClaudeOAuth/ClaudeOAuthUsageFetcher.swift:61-98
- Headers (exact):
  ```
  Authorization: Bearer <accessToken>
  Accept: application/json
  Content-Type: application/json
  anthropic-beta: oauth-2025-04-20
  User-Agent: claude-cli/<version> (external, cli)
  ```
- User-Agent is built as `claude-code/<v>` then `claude-code/` replaced with `claude-cli/` and ` (external, cli)` appended; `<v>` = detected Claude CLI version, fallback `2.1.0` (same file, lines 66-67, 93-98, 212-215).
- `?cedar_ember=1` opt-in: HTTP 400, or 403 whose body lacks `user:profile`, is retried once at the plain path without the query (lines 102-109). 401 → unauthorized; 429 → back off (honors `Retry-After`) (lines 117-127, 188-202).
- Response shape (`five_hour`, `seven_day`, `seven_day_oauth_apps`, `seven_day_opus`, `seven_day_sonnet`; unknown-key tolerant decode, lines 303-322, 373-397):
  ```
  five_hour              : { utilization: Double?, resets_at: String (ISO8601) }
  seven_day              : same window shape
  seven_day_oauth_apps   : same window shape
  seven_day_opus         : same window shape
  seven_day_sonnet       : same window shape
  ```
- Extra keys:
  - `extra_usage` → `{ is_enabled: Bool?, monthly_limit: Double?, used_credits: Double?, utilization: Double?, currency: String? }` (lines 383-397)
  - `iguana_necktie` → cloud credits `{ limit_dollars: Double, used_dollars?, remaining_dollars?, resets_at: String (ISO8601), locked_reason? }` — research-codexbar: Sources/CodexBarCore/Providers/Claude/ClaudeCloudCreditsSnapshot.swift:24-30
  - `cedar_ember` → reset status `{ eligible: Bool, grants: [{ resets_left: Int, resets_total: Int?, starts_at, ends_at, paused }] }` — research-codexbar: Sources/CodexBarCore/Providers/Claude/ClaudeRateLimitResetCredits.swift:67-133
  - `seven_day_routines` aliases tried in order: `seven_day_routines`, `seven_day_claude_routines`, `claude_routines`, `routines`, `routine`, `seven_day_cowork`, `cowork` (fetcher lines 308-318)
  - Scoped weekly limits: the raw `limits` list is re-parsed with the web path's entry parser; each entry may name the model it scopes via `scope.model.display_name` (lines 180-186, 296-300)

## 2. Claude — web fallback (claude.ai sessionKey)

- Endpoints — research-codexbar: Sources/CodexBarCore/Providers/Claude/ClaudeWeb/ClaudeWebAPIFetcher.swift:110-115
  - `GET https://claude.ai/api/organizations`
  - `GET https://claude.ai/api/organizations/{org_id}/usage` (`?cedar_ember=1` requested first; retry without opt-in on other statuses)
  - `GET https://claude.ai/api/organizations/{org_id}/prepaid/credits`
  - `GET https://claude.ai/api/account`
- Headers on every request (lines 602-604, 665-667):
  ```
  Cookie: sessionKey=<sessionKey>
  Accept: application/json
  ```
- `GET /organizations` parses an array of `{ uuid: String, name: String? }` (also matches chat-capability fields); picks target org, else first non-API-only org (lines 806-828).
- Usage response field paths (lines 702-754):
  ```
  five_hour.utilization          -> session % used
  five_hour.resets_at            -> ISO8601 date
  seven_day.utilization          -> weekly % used
  seven_day.resets_at
  seven_day_sonnet.utilization   -> preferred over seven_day_opus.utilization
  extra_usage                    -> extra usage cost
  cedar_ember                    -> reset-credit grants
  iguana_necktie                 -> cloud credits
  ```
- Account info (`/account`): `email_address`, `memberships[].organization.{uuid,name,rate_limit_tier,billing_type}`, `memberships[].seat_tier` (lines 846-918).
- 401/403 → unauthorized; 403 with `cf-mitigated: challenge` header or body containing "Just a moment" → Cloudflare challenge (not fixable by re-auth) (lines 677-700).

## 3. Codex — ChatGPT backend usage

- `GET https://chatgpt.com/backend-api/wham/usage` — research-codexbar: Sources/CodexBarCore/Providers/Codex/CodexOAuth/CodexOAuthUsageFetcher.swift:380-424
  - Base URL overridable via `chatgpt_base_url` in `$HOME/.codex/config.toml` (or `CODEX_HOME/config.toml`); if the base does not contain `/backend-api`, path becomes `/api/codex/usage`; `https://chatgpt.com` / `https://chat.openai.com` get `/backend-api` appended (lines 568-675).
- Headers (lines 410-416):
  ```
  Authorization: Bearer <accessToken>
  User-Agent: CodexBar
  Accept: application/json
  ChatGPT-Account-Id: <accountId>          # only when known; note capital "Id"
  ```
  The companion `/wham/rate-limit-reset-credits` endpoint additionally sends `OpenAI-Beta: codex-1`, `originator: Codex Desktop`, and spells the header `ChatGPT-Account-ID` (lines 532-545). 401 → token expired.
- Response shape (lines 6-333; windows decode tolerantly so one bad window never kills the rest):
  ```
  account_id | accountId          : String?
  plan_type                       : String? guest|free|go|plus|pro|free_workspace|team|business|education|quorum|k12|enterprise|edu
  rate_limit.primary_window       : { used_percent: Int, reset_at: Int (epoch seconds), limit_window_seconds: Int }
  rate_limit.secondary_window     : same window shape (weekly)
  rate_limit.individual_limit     : { limit: Double?, used: Double?, remaining_percent|remainingPercent: Double?, resets_at|reset_at: Int? }
  additional_rate_limits[]        : { limit_name: String?, metered_feature: String?, rate_limit: <RateLimitDetails shape> }  (e.g. GPT-5.3-Codex-Spark)
  credits                         : { has_credits: Bool, unlimited: Bool, balance: Double|String }
  individual_limit (root)         : same shape as rate_limit.individual_limit
  spend_control.individual_limit  : same shape (team/enterprise monthly pool)
  ```
  Source precedence for the credit limit: root → `rate_limit` → `spend_control` (lines 72-83).
- Credentials from `~/.codex/auth.json` (`CODEX_HOME` env overrides the home) — research-codexbar: Sources/CodexBarCore/Providers/Codex/CodexOAuth/CodexOAuthCredentials.swift:97-112, 269-303:
  ```
  OPENAI_API_KEY                  : String  (API-key mode; no OAuth)
  tokens.access_token | accessToken
  tokens.refresh_token | refreshToken
  tokens.id_token     | idToken
  tokens.account_id   | accountId
  last_refresh                    : ISO8601 String
  ```
  Account-id fallback if `tokens.account_id` empty: decode the JWT payload of `id_token` (then `access_token`) and read `chatgpt_account_id`, or claim `https://api.openai.com/auth`.chatgpt_account_id, or first `organizations[].id` (lines 483-513). Access-token expiry read from the JWT `exp` claim (lines 516-556).
- Token refresh — research-codexbar: Sources/CodexBarCore/Providers/Codex/CodexOAuth/CodexTokenRefresher.swift:6-58:
  ```
  POST https://auth.openai.com/oauth/token
  Content-Type: application/json
  body: { "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
          "grant_type": "refresh_token",
          "refresh_token": <refreshToken>,
          "scope": "openid profile email" }
  ```
  Response: `access_token`, `refresh_token` (rotates if present), `id_token`; error codes `refresh_token_expired` / `refresh_token_reused` / `invalid_grant` / `refresh_token_invalidated`. Write-back, when used, rewrites `~/.codex/auth.json` preserving other keys and stamping `last_refresh` as ISO8601, only when source is `codexHome` (`save`, CodexOAuthCredentials.swift:324-364).
  CAVEAT for a reimplementer: in CodexBar's shipped usage path `CodexTokenRefresher` is never called — needing a refresh throws `nativeRefreshRequired` and falls back to letting the Codex CLI refresh its own auth.json ("No source has a safe writer handoff from the usage path", CodexProviderDescriptor.swift:468-478). Whether to auto-refresh in Tauri is our call.

## 4. Claude — local credentials (`.claude/.credentials.json`)

- Path: `~/.claude/.credentials.json`; `CLAUDE_CONFIG_DIR` replaces the config root, `CLAUDE_SECURESTORAGE_CONFIG_DIR` (env `CLAUDE_SECURESTORAGE_CONFIG_DIR`) replaces the credentials root alone — research-codexbar: Sources/CodexBarCore/Providers/Claude/ClaudeConfigPaths.swift:5-6, 87-101; read at ClaudeOAuth/ClaudeOAuthCredentials.swift:1505-1511.
- File shape — research-codexbar: Sources/CodexBarCore/Providers/Claude/ClaudeOAuth/ClaudeOAuthCredentialModels.swift:97-136:
  ```
  { "claudeAiOauth": {
      "accessToken":      String   (required, non-empty)
      "refreshToken":     String?
      "expiresAt":        Double   (epoch milliseconds)
      "scopes":           [String]
      "rateLimitTier":    String?
      "subscriptionType": String?
  } }
  ```
  A payload with only `mcpOAuth` is rejected (MCP-only keychain state).
- Refresh behavior: `~/.claude/.credentials.json` is Claude CLI-owned; when its token is expired CodexBar throws `refreshDelegatedToClaudeCLI` and re-auth happens via the CLI — no direct refresh and no write-back for this file (ClaudeOAuthCredentials.swift:1553-1621, 1606-1621).
- When CodexBar itself owns the credential (its own keychain cache), it refreshes directly: `POST https://platform.claude.com/v1/oauth/token`, headers `Content-Type: application/x-www-form-urlencoded`, `Accept: application/json`, body `grant_type=refresh_token`, `refresh_token=...`, `client_id=9d1c250a-e61b-44d9-88ed-5944d1962f5e` (the public Claude Code CLI OAuth client id, overridable via `CODEXBAR_CLAUDE_OAUTH_CLIENT_ID`) — ClaudeOAuthCredentials.swift:26-28, 1384-1398. Response: `access_token`, `refresh_token?`, `expires_in` (Int), `token_type` (lines 1694-1706). Refreshed tokens are written back to CodexBar's keychain cache only, never to `.claude/.credentials.json` (lines 1342-1354, 1657-1691).

## 5. Antigravity

Google OAuth remote path (Win-CodexBar port of CodexBar's fetcher):
- Credentials — research-win-codexbar: rust/src/providers/antigravity/remote.rs:29-33, 88-120:
  ```
  file    : %USERPROFILE%\.codexbar\antigravity\oauth_creds.json
  env     : ANTIGRAVITY_OAUTH_CREDENTIALS_JSON   (file fields, snake_case with camelCase aliases:
                                                   access_token|accessToken, refresh_token|refreshToken,
                                                   expiry_date|expiresAt (epoch ms), id_token|idToken, email,
                                                   project_id|projectId, client_id|clientId, client_secret|clientSecret)
  client  : ANTIGRAVITY_OAUTH_CLIENT_ID / ANTIGRAVITY_OAUTH_CLIENT_SECRET  (else discovered from installed app artifacts)
  ```
- Remote endpoints (research-win-codexbar: remote.rs:40-44, 582-586, 663-679, 694-785; also research-codexbar: docs/antigravity.md:183-190):
  ```
  POST https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary   (preferred; 2s timeout cap)
  POST https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist
  POST https://cloudcode-pa.googleapis.com/v1internal:onboardUser
  POST https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels
  POST https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuota
  ```
  Headers (same for all; research-win-codexbar: remote.rs:168-174, 626-643):
  ```
  Authorization: Bearer <access_token>
  User-Agent: antigravity/hub/2.9.1 <windows|darwin|linux>/<amd64|arm64>
  Content-Type: application/json           (set by reqwest .json())
  ```
  Bodies: summary/quota/model calls `{"project": "<projectId>"}` or `{}` (line 659-662); `loadCodeAssist` `{"metadata": {"ideType": "ANTIGRAVITY", "platform": "PLATFORM_UNSPECIFIED", "pluginType": "GEMINI"}}` (lines 59-65, 672-675); `onboardUser` adds `"tierId": "<id>"` (line 698).
- Quota summary response — research-win-codexbar: rust/src/providers/antigravity/quota_summary.rs:14-81:
  ```
  { "response" | "summary" | "groups"(top level) = {
      groups[]: {
        displayName|name, description,
        buckets[]: {
          bucketId|id, displayName|name, description, disabled?: Bool,
          remainingFraction?: Double            (alias remaining_fraction)
          remaining?: { remaining_fraction?, case?: "remainingFraction", value? }   (gRPC oneof)
          reset_time?: String                   (alias reset_time; RFC3339 or epoch-seconds float)
          window?: String                       (explicit cadence, replaces id/name detection)
  }}}
  ```
- `remainingFraction` scale is **0..1**. Exact arithmetic (quota_summary.rs:263-267):
  ```rust
  let used_percent = remaining.map(|fraction| 100.0 - (fraction * 100.0).clamp(0.0, 100.0)).unwrap_or(0.0);
  ```
  Legacy model-quota path uses `(1.0 - remaining) * 100.0` with remaining defaulting to 1.0 (legacy_status.rs:243-247).
- Bucket window mapping (quota_summary.rs:8-12, 393-427):
  - Session aliases (normalized lowercase, `_`→`-`, optional ` limit` suffix, match exact or `-alias` suffix): `session`, `5h`, `5-hour`, `five hour`, `five-hour` → 300 minutes (5h).
  - Weekly: exact `weekly` or suffix `-weekly` → 10080 minutes (7×24×60).
  - Anything else → `Other` (window_minutes = None, informational).
  - An explicit non-empty `window` field replaces id/name candidates entirely.
- OAuth token refresh — research-win-codexbar: remote.rs:798-852: `POST https://oauth2.googleapis.com/token` with form body `client_id`, `client_secret`, `refresh_token`, `grant_type=refresh_token`. Response `access_token`, `expires_in` (seconds → new `expiry_date` = now + expires_in×1000 ms), `id_token`. Written back to `oauth_creds.json` on success (lines 846-851, 873-876). Project-id resolution may also persist the file.
- Local `GetUserStatus` (App/IDE/agy language server) — research-win-codexbar: rust/src/providers/antigravity/mod.rs:60-62, 135-158, 825-879:
  ```
  POST https://127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary   body {"forceRefresh": true}
  POST https://127.0.0.1:<port>/exa.language_server_pb.LanguageServerService/GetUserStatus              body {"metadata":{"ideName":"antigravity","extensionName":"antigravity","ideVersion":"unknown","locale":"en"}}
  Headers: Content-Type: application/json, Connect-Protocol-Version: 1,
           X-Codeium-Csrf-Token: <token>   (IDE desktop server only; agy CLI needs none)
  ```
  `GetUserStatus` response shape (legacy_status.rs:7-70): `user_status { email, plan_status.plan_info.{plan_name, plan_display_name}, user_tier.{id,name,description}, cascade_model_config_data.client_model_configs[] { label, model_id|id, quota_info { remaining_fraction (0..1), reset_time } } }`.

## 6. Ollama Cloud — `/api/balance`

- `GET https://ollama.com/api/balance` — research-win-codexbar: rust/src/providers/ollama/mod.rs:24, 111-123:
  ```
  Authorization: Bearer <API key>
  Accept: application/json
  User-Agent: CodexBar/1.0
  ```
  API key from context, env `OLLAMA_API_KEY` / `OLLAMA_KEY`, or stored settings (mod.rs:145-161). 401/403 → invalid key; 429 → rate limited.
- Current response shape parsed by `parse_api_balance` (mod.rs:190-255):
  ```
  included: {
    allowance_usd: number | decimal-digit string   (monthly allowance, required when included present)
    balance_usd:   number | decimal-digit string   (required)
    period: { until: String (RFC3339 with offset/Z; strict regex) }   -> reset time
  }
  purchased: { balance_usd: number | decimal-digit string }
  ```
  Derived: `used = max(0, allowance − balance)`; `percent = min(100, used/allowance*100)` (window = 43200 min = 30 days, calendar-month pace); zero allowance → informational no-meter row; `purchased.balance_usd` → balance-only "Credit balance" cost. Amounts reject booleans/nulls/non-finite (mod.rs:265-279).
- Legacy shape `included.session` / `included.weekly`: NOT parsed by Win-CodexBar's Rust (it only handles the `allowance_usd` and web-HTML paths). It exists in CodexBar's upstream plugin `ollama-api.ts` — research-codexbar: Sources/CodexBarCore/Resources/Plugins/ollama-api.ts:44-75:
  ```
  included.session: { remaining_percent: number 0..100, resets_at: String (RFC3339) }   -> primary, 300 min, used = 100 - remaining
  included.weekly:  { remaining_percent, resets_at }                                    -> secondary, 10080 min
  ```
- Web/cookie fallback: scrapes `GET https://ollama.com/settings` HTML (Session usage / Weekly usage / Monthly usage / Free usage labels); session cookie name `__Secure-session` (recognized set also includes `session`, `ollama_session`, `__Host-ollama_session`, `wos-session`, `__Secure-next-auth.session-token`, `next-auth.session-token`) — research-win-codexbar: rust/src/providers/ollama/cookies.rs:14-23, mod.rs:326-390. Response-field paths for scraping: UNKNOWN (it is HTML parsing, not JSON).
