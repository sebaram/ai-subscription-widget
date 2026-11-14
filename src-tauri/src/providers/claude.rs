use crate::model::{snapshot_for, UsageWindow, ProviderId, ProviderSnapshot, ProviderState, QuotaInfo};
use crate::providers::FetchCtx;
use serde_json::Value;

/// (json key, window id, title, window minutes)
const LANES: [(&str, &str, &str, i64); 5] = [
    ("five_hour", "5h", "5h", 300),
    ("seven_day", "week", "wk", 10080),
    ("seven_day_oauth_apps", "apps", "wk apps", 10080),
    ("seven_day_opus", "opus", "wk opus", 10080),
    ("seven_day_sonnet", "sonnet", "wk sonnet", 10080),
];

fn parse_usage(v: &Value, show_missing: bool) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    for (key, id, title, minutes) in LANES {
        // A null lane = the API says this window doesn't exist for the account.
        // Hidden by default; shown as an honest "—" row when the user opts in.
        match v.get(key) {
            None | Some(Value::Null) => {
                if show_missing {
                    windows.push(UsageWindow {
                        id: id.into(),
                        title: title.into(),
                        window_minutes: Some(minutes),
                        used_percent: None,
                        quota: None,
                        resets_at: None,
                        pace_expected_percent: None,
                        is_known: false,
                    });
                }
                continue;
            }
            Some(lane) => {
                let utilization = lane.get("utilization").and_then(|u| u.as_f64());
                let resets_at = lane.get("resets_at").and_then(|r| r.as_str()).map(String::from);
                windows.push(UsageWindow {
                    id: id.into(),
                    title: title.into(),
                    window_minutes: Some(minutes),
                    used_percent: utilization,
                    quota: None,
                    resets_at,
                    pace_expected_percent: None,
                    is_known: utilization.is_some(),
                });
            }
        }
    }
    if let Some(extra) = v.get("extra_usage") {
        let enabled = extra.get("is_enabled").and_then(|b| b.as_bool()).unwrap_or(false);
        let limit = extra.get("monthly_limit").and_then(|m| m.as_f64());
        if enabled && limit.unwrap_or(0.0) > 0.0 {
            let used = extra.get("used_credits").and_then(|c| c.as_f64()).unwrap_or(0.0);
            windows.push(UsageWindow {
                id: "extra".into(),
                title: "extra usage".into(),
                window_minutes: None,
                used_percent: Some((used / limit.unwrap() * 100.0).min(100.0)),
                quota: Some(QuotaInfo { used: format!("${used:.2}"), total: format!("${:.2}", limit.unwrap()) }),
                resets_at: None,
                pace_expected_percent: None,
                is_known: true,
            });
        }
    }
    // pace for every known window with a reset time
    for w in &mut windows {
        if w.is_known {
            if let (Some(minutes), Some(resets)) = (w.window_minutes, w.resets_at.clone()) {
                w.pace_expected_percent = crate::model::pace_expected(minutes, &resets);
            }
        }
    }
    windows
}

pub struct Creds {
    access_token: String,
    expires_at_ms: Option<u64>,
}

fn creds_path() -> std::path::PathBuf {
    // CLAUDE_CONFIG_DIR replaces the config root (reference §4); default ~/.claude
    if let Ok(dir) = std::env::var("CLAUDE_CONFIG_DIR") {
        return std::path::PathBuf::from(dir).join(".credentials.json");
    }
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".claude")
        .join(".credentials.json")
}

fn parse_creds(v: &Value) -> Option<Creds> {
    let oauth = v.get("claudeAiOauth")?;
    // MCP-only payloads have no claudeAiOauth gate and are rejected (reference §4)
    let access_token = oauth.get("accessToken")?.as_str()?.trim().to_string();
    if access_token.is_empty() {
        return None;
    }
    Some(Creds {
        access_token,
        expires_at_ms: oauth.get("expiresAt").and_then(|e| e.as_f64()).map(|e| e as u64),
    })
}

fn load_creds_from(dir: &std::path::Path) -> Option<Creds> {
    let text = std::fs::read_to_string(dir).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    parse_creds(&v)
}

/// Claude CLI owns this file; we only read it (design constraint).
fn token_needs_refresh(expires_at_ms: u64) -> bool {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(u64::MAX);
    now_ms.saturating_add(30_000) >= expires_at_ms // 30 s clock skew
}

async fn usage_call(
    http: &reqwest::Client,
    token: &str,
) -> Result<Value, ProviderState> {
    let get = |_cedar: bool | {
        http
            .get(if _cedar { "https://api.anthropic.com/api/oauth/usage?cedar_ember=1" } else { "https://api.anthropic.com/api/oauth/usage" })
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", "claude-cli/2.1.0 (external, cli)")
    };
    let first = get(true).send().await;
    let Ok(resp) = first else { return Err(ProviderState::NoData) };
    let status = resp.status().as_u16();
    // ?cedar_ember=1 opt-in retry: 400 or 403-without-profile retried at the plain path (reference §1)
    let resp = match status {
        400 | 403 => get(false).send().await.map_err(|_| ProviderState::NoData)?,
        _ => resp,
    };
    match resp.status().as_u16() {
        401 => Err(ProviderState::AuthRequired),
        200 => resp.json::<Value>().await.map_err(|_| ProviderState::ParseFailed),
        _ => Err(ProviderState::NoData),
    }
}

pub async fn fetch(ctx: &FetchCtx) -> ProviderSnapshot {
    let creds = load_creds_from(&creds_path());
    let snap = fetch_with_creds(ctx, creds).await;
    if snap.state == ProviderState::AuthRequired {
        snap.with_message("Run `claude` (Claude Code) to refresh your login, then refresh this widget — credentials are read only")
    } else if snap.state == ProviderState::ParseFailed {
        snap.with_message("Could not parse the usage response (API may have changed)")
    } else {
        snap
    }
}

async fn fetch_with_creds(ctx: &FetchCtx, creds: Option<Creds>) -> ProviderSnapshot {
    let Some(creds) = creds else {
        return snapshot_for(ProviderId::Claude, ProviderState::AuthRequired, vec![]);
    };
    // Claude Code owns refresh tokens; never rotate credentials on its behalf.
    if creds.expires_at_ms.is_some_and(token_needs_refresh) {
        return snapshot_for(ProviderId::Claude, ProviderState::AuthRequired, vec![]);
    }
    match usage_call(&ctx.http, &creds.access_token).await {
        Ok(v) => {
            let windows = parse_usage(&v, ctx.cfg.show_missing_windows);
            let state = if windows.is_empty() { ProviderState::NoData } else { ProviderState::Ok };
            snapshot_for(ProviderId::Claude, state, windows)
        }
        Err(state) => snapshot_for(ProviderId::Claude, state, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const FIXTURE: &str = r#"{
        "five_hour":   {"utilization": 42.0, "resets_at": "2026-11-09T15:46:00Z"},
        "seven_day":   {"utilization": 61.0, "resets_at": "2026-11-16T00:00:00Z"},
        "seven_day_sonnet": {"utilization": 10.0, "resets_at": "2026-11-16T00:00:00Z"},
        "extra_usage": {"is_enabled": true, "monthly_limit": 10.0, "used_credits": 5.0}
    }"#;
    const MINIMAL: &str = r#"{"five_hour": {"utilization": null, "resets_at": null}}"#;

    #[test]
    fn claude_fixture_maps_5h_and_week() {
        let windows = parse_usage(&serde_json::from_str::<Value>(FIXTURE).unwrap(), false);
        assert!(windows.len() >= 3);
        let five = &windows[0];
        assert_eq!(five.id, "5h");
        assert_eq!(five.window_minutes, Some(300));
        assert!((five.used_percent.unwrap() - 42.0).abs() < 1e-6);
        assert_eq!(five.resets_at.as_deref(), Some("2026-11-09T15:46:00Z"));
        let week = &windows[1];
        assert_eq!(week.id, "week");
        assert_eq!(week.window_minutes, Some(10080));
        assert!((week.used_percent.unwrap() - 61.0).abs() < 1e-6);
        assert!(windows.iter().any(|w| w.id == "sonnet" && (w.used_percent.unwrap() - 10.0).abs() < 1e-6));
    }

    #[test]
    fn claude_extras_only_when_present() {
        let minimal = json!({"five_hour": {"utilization": 70.0, "resets_at": "2026-11-09T15:00:00Z"}});
        let windows = parse_usage(&minimal, false);
        assert_eq!(windows.len(), 1);
        assert!(!windows.iter().any(|w| w.id != "5h"));
    }

    #[test]
    fn claude_null_utilization_is_unknown_not_zero() {
        let windows = parse_usage(&serde_json::from_str::<Value>(MINIMAL).unwrap(), false);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].used_percent, None);
        assert!(!windows[0].is_known);
    }

    #[test]
    fn claude_null_lane_is_omitted_not_dash() {
        // live response 2026-11-09: "five_hour": null → no such window at all
        let v = json!({"five_hour": null, "seven_day": {"utilization": 66.0, "resets_at": "2026-11-12T00:00:00Z"}});
        let windows = parse_usage(&v, false);
        assert_eq!(windows.len(), 1, "absent lane must vanish, not render —");
        assert_eq!(windows[0].id, "week");
    }

    #[test]
    fn claude_extra_usage_maps_quota() {
        let windows = parse_usage(&serde_json::from_str::<Value>(FIXTURE).unwrap(), false);
        let extra = windows.iter().find(|w| w.id == "extra").expect("extra usage window");
        assert_eq!(extra.quota.as_ref().unwrap().used, "$5.00");
        assert_eq!(extra.quota.as_ref().unwrap().total, "$10.00");
        assert!((extra.used_percent.unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn claude_disabled_extra_usage_omitted() {
        let v = json!({"extra_usage": {"is_enabled": false, "monthly_limit": 10.0, "used_credits": 5.0}});
        assert!(parse_usage(&v, false).is_empty());
    }

    #[tokio::test]
    async fn claude_missing_creds_yields_auth_required_no_network() {
        let cfg = crate::config::defaults();
        let ctx = FetchCtx::new(cfg);
        let snap = fetch_with_creds(&ctx, None).await;
        assert_eq!(snap.state, ProviderState::AuthRequired);
    }

    #[test]
    fn claude_token_expiry_detection() {
        assert!(token_needs_refresh(0));
        let future_ms = (std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64)
            + 60_000;
        assert!(!token_needs_refresh(future_ms));
    }

    #[test]
    fn claude_creds_parse_fixture_file() {
        let dir = std::env::temp_dir().join(format!("aiusage-claude-{}", std::process::id()));
        std::fs::create_dir_all(dir.join(".claude")).unwrap();
        std::fs::write(
            dir.join(".claude").join(".credentials.json"),
            r#"{"claudeAiOauth":{"accessToken":"tok","refreshToken":"ref","expiresAt":9999999999999}}"#,
        )
        .unwrap();
        let creds = load_creds_from(&dir.join(".claude").join(".credentials.json"));
        let c = creds.expect("creds parsed");
        assert_eq!(c.access_token, "tok");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn claude_mcp_only_payload_rejected() {
        assert!(parse_creds(&json!({"mcpOAuth": {"some": 1}})).is_none());
    }

    #[tokio::test]
    async fn expired_cli_credentials_never_send_network_requests() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        listener.set_nonblocking(true).unwrap();
        let mut ctx = FetchCtx::new(crate::config::defaults());
        ctx.http = reqwest::Client::builder()
            .proxy(reqwest::Proxy::all(format!("http://{}", listener.local_addr().unwrap())).unwrap())
            .timeout(std::time::Duration::from_millis(100))
            .build().unwrap();
        let snap = fetch_with_creds(&ctx, Some(Creds { access_token: "expired".into(), expires_at_ms: Some(0) })).await;
        assert_eq!(snap.state, ProviderState::AuthRequired);
        assert!(listener.accept().is_err(), "expired CLI credentials must not trigger OAuth refresh or usage requests");
    }
}
