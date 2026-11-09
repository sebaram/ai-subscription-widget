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

fn parse_usage(v: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    for (key, id, title, minutes) in LANES {
        let Some(lane) = v.get(key) else { continue };
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
    refresh_token: Option<String>,
    #[allow(dead_code)]
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
        refresh_token: oauth.get("refreshToken").and_then(|r| r.as_str()).map(String::from),
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

/// In-memory only refresh — never written back (design: read-only creds).
async fn refresh_access_token(
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<(String, Option<String>), String> {
    let enc = |s: &str| {
        let mut out = String::new();
        for b in s.bytes() {
            if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
                out.push(b as char);
            } else {
                out.push_str(&format!("%{b:02X}"));
            }
        }
        out
    };
    let body = format!(
        "grant_type=refresh_token&refresh_token={}&client_id={}",
        enc(refresh_token),
        enc("9d1c250a-e61b-44d9-88ed-5944d1962f5e")
    );
    let resp = http
        .post("https://platform.claude.com/v1/oauth/token")
        .header("Accept", "application/json")
        .header("Content-Type", "application/x-www-form-urlencoded")
        .body(body)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("refresh failed: {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    let access = v.get("access_token").and_then(|t| t.as_str()).ok_or("no access_token")?;
    Ok((
        access.to_string(),
        v.get("refresh_token").and_then(|t| t.as_str()).map(String::from),
    ))
}

async fn usage_call(
    http: &reqwest::Client,
    token: &str,
) -> Result<Value, ProviderState> {
    let get = | cedar: bool | {
        let mut req = http
            .get(if cedar { "https://api.anthropic.com/api/oauth/usage?cedar_ember=1" } else { "https://api.anthropic.com/api/oauth/usage" })
            .header("Authorization", format!("Bearer {token}"))
            .header("Accept", "application/json")
            .header("Content-Type", "application/json")
            .header("anthropic-beta", "oauth-2025-04-20")
            .header("User-Agent", "claude-cli/2.1.0 (external, cli)");
        req
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
    fetch_with_creds(ctx, creds).await
}

async fn fetch_with_creds(ctx: &FetchCtx, creds: Option<Creds>) -> ProviderSnapshot {
    let Some(creds) = creds else {
        return snapshot_for(ProviderId::Claude, ProviderState::AuthRequired, vec![]);
    };
    let mut token = creds.access_token;
    // expired access token → refresh in memory when a refresh token exists
    if let Some(expiry) = creds.expires_at_ms {
        if token_needs_refresh(expiry) {
            match &creds.refresh_token {
                Some(rt) => match refresh_access_token(&ctx.http, rt).await {
                    Ok((new_token, _)) => token = new_token,
                    Err(_) => return snapshot_for(ProviderId::Claude, ProviderState::AuthRequired, vec![]),
                },
                None => return snapshot_for(ProviderId::Claude, ProviderState::AuthRequired, vec![]),
            }
        }
    }
    let windows = match usage_call(&ctx.http, &token).await {
        Ok(v) => {
            let windows = parse_usage(&v);
            if windows.is_empty() {
                return snapshot_for(ProviderId::Claude, ProviderState::NoData, vec![]);
            }
            windows
        }
        Err(state) => return snapshot_for(ProviderId::Claude, state, vec![]),
    };
    snapshot_for(ProviderId::Claude, ProviderState::Ok, windows)
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
        let windows = parse_usage(&serde_json::from_str::<Value>(FIXTURE).unwrap());
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
        let windows = parse_usage(&minimal);
        assert_eq!(windows.len(), 1);
        assert!(!windows.iter().any(|w| w.id != "5h"));
    }

    #[test]
    fn claude_null_utilization_is_unknown_not_zero() {
        let windows = parse_usage(&serde_json::from_str::<Value>(MINIMAL).unwrap());
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].used_percent, None);
        assert!(!windows[0].is_known);
    }

    #[test]
    fn claude_extra_usage_maps_quota() {
        let windows = parse_usage(&serde_json::from_str::<Value>(FIXTURE).unwrap());
        let extra = windows.iter().find(|w| w.id == "extra").expect("extra usage window");
        assert_eq!(extra.quota.as_ref().unwrap().used, "$5.00");
        assert_eq!(extra.quota.as_ref().unwrap().total, "$10.00");
        assert!((extra.used_percent.unwrap() - 50.0).abs() < 1e-6);
    }

    #[test]
    fn claude_disabled_extra_usage_omitted() {
        let v = json!({"extra_usage": {"is_enabled": false, "monthly_limit": 10.0, "used_credits": 5.0}});
        assert!(parse_usage(&v).is_empty());
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
        assert_eq!(c.refresh_token.as_deref(), Some("ref"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn claude_mcp_only_payload_rejected() {
        assert!(parse_creds(&json!({"mcpOAuth": {"some": 1}})).is_none());
    }
}
