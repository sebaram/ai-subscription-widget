use crate::model::{snapshot_for, UsageWindow, ProviderId, ProviderSnapshot, ProviderState};
use crate::providers::FetchCtx;
use serde_json::Value;

fn epoch_to_iso(secs: i64) -> Option<String> {
    // 0 / negative = no real reset time — render nothing rather than 1970 (review minor)
    chrono::DateTime::from_timestamp(secs, 0).filter(|_| secs > 0).map(|d| d.to_rfc3339())
}

fn window_identity(minutes: i64, positional: &str) -> (String, String) {
    // Derive identity from the REAL window length; positional names lie now that
    // OpenAI made the weekly window primary on some plans (live-verified 2026-11-09).
    match minutes {
        300 => ("5h".to_string(), "5h".to_string()),
        10080 => ("week".to_string(), "wk".to_string()),
        43200 => ("month".to_string(), "mo".to_string()),
        _ => (positional.to_string(), positional.to_string()),
    }
}

fn window_from(positional: &str, w: &Value) -> Option<UsageWindow> {
    let used = w.get("used_percent").and_then(|u| u.as_f64())?;
    let minutes = w.get("limit_window_seconds").and_then(|s| s.as_i64()).map(|s| (s / 60).max(1));
    let (id, title) = window_identity(minutes.unwrap_or(0), positional);
    let resets_at = w
        .get("reset_at")
        .or_else(|| w.get("resets_at"))
        .and_then(|r| r.as_i64())
        .and_then(epoch_to_iso)
        .or_else(|| w.get("resets_at").and_then(|r| r.as_str()).map(String::from));
    let pace = resets_at
        .as_deref()
        .and_then(|r| minutes.and_then(|m| crate::model::pace_expected(m, r)));
    Some(UsageWindow {
        id,
        title,
        window_minutes: minutes,
        used_percent: Some(used),
        quota: None,
        resets_at,
        pace_expected_percent: pace,
        is_known: true,
    })
}

fn slugify(name: &str) -> String {
    name.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '.' { c.to_ascii_lowercase() } else { '-' })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn window_from_named(named: Option<&str>, positional: &str, w: &Value) -> Option<UsageWindow> {
    let mut out = window_from(positional, w)?;
    if let Some(n) = named {
        // model-specific limits keep their own identity (they would otherwise collide with the main 5h/week lanes)
        out.id = n.to_string();
        out.title = positional.to_string();
    }
    Some(out)
}

fn parse_usage(v: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    if let Some(rl) = v.get("rate_limit") {
        // null windows (no such limit on this plan) are skipped, never mapped
        if let Some(w) = rl.get("primary_window").and_then(|w| window_from("5h", w)) {
            windows.push(w);
        }
        if let Some(w) = rl.get("secondary_window").and_then(|w| window_from("week", w)) {
            windows.push(w);
        }
    }
    if let Some(adds) = v.get("additional_rate_limits").and_then(|a| a.as_array()) {
        for a in adds {
            let name = a
                .get("limit_name")
                .and_then(|n| n.as_str())
                .or_else(|| a.get("metered_feature").and_then(|f| f.as_str()))
                .unwrap_or("extra limit");
            let rl = a.get("rate_limit");
            let slug = slugify(name);
            let mapped = rl
                .and_then(|r| r.get("primary_window").and_then(|w| window_from_named(Some(&slug), name, w)))
                .or_else(|| rl.and_then(|r| window_from_named(Some(&slug), name, r)));
            if let Some(w) = mapped {
                windows.push(w);
            }
        }
    }
    if let Some(credits) = v.get("credits").filter(|c| c.get("unlimited").and_then(|u| u.as_bool()) != Some(true)) {
        if let Some(balance) = credits.get("balance") {
            let text = match balance {
                Value::Number(n) => format!("${n}"),
                Value::String(s) => s.clone(),
                _ => String::new(),
            };
            if !text.is_empty() {
                windows.push(UsageWindow {
                    id: "credits".into(),
                    title: "credits".into(),
                    window_minutes: None,
                    used_percent: None,
                    quota: Some(crate::model::QuotaInfo { used: text, total: "—".into() }),
                    resets_at: None,
                    pace_expected_percent: None,
                    is_known: true,
                });
            }
        }
    }
    windows
}

fn b64url_decode(input: &str) -> Option<Vec<u8>> {
    let mut table = [255u8; 256];
    for (i, ch) in "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
        .as_bytes()
        .iter()
        .enumerate()
    {
        table[*ch as usize] = i as u8;
    }
    let mut out = Vec::new();
    let mut buf = 0u32;
    let mut bits = 0u32;
    for &b in input.as_bytes() {
        if b == b'=' {
            break;
        }
        let v = table[b as usize];
        if v == 255 {
            return None;
        }
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// account_id resolution: tokens.account_id → JWT chatgpt_account_id → nested OpenAI claim → organizations[].id
fn jwt_account_id(token: &str) -> Option<String> {
    let payload_b64 = token.split('.').nth(1)?;
    let payload = b64url_decode(payload_b64)?;
    let v: Value = serde_json::from_slice(&payload).ok()?;
    v.get("chatgpt_account_id")
        .and_then(|a| a.as_str())
        .map(String::from)
        .or_else(|| {
            v.get("https://api.openai.com/auth")
                .and_then(|auth| auth.get("chatgpt_account_id"))
                .and_then(|a| a.as_str())
                .map(String::from)
        })
        .or_else(|| {
            v.get("organizations")
                .and_then(|o| o.as_array())
                .and_then(|a| a.first())
                .and_then(|o| o.get("id"))
                .and_then(|i| i.as_str())
                .map(String::from)
        })
}

fn jwt_exp_epoch(token: &str) -> Option<i64> {
    let payload_b64 = token.split('.').nth(1)?;
    let payload = b64url_decode(payload_b64)?;
    let v: Value = serde_json::from_slice(&payload).ok()?;
    v.get("exp").and_then(|e| e.as_i64())
}

fn token_needs_refresh(token: &str) -> bool {
    match jwt_exp_epoch(token) {
        Some(exp) => {
            let now = chrono::Utc::now().timestamp();
            now + 30 >= exp
        }
        None => false, // no exp claim: try as-is, the endpoint will 401 if stale
    }
}

pub struct CodexAuth {
    access_token: String,
    account_id: Option<String>,
}

pub fn auth_path() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("CODEX_HOME") {
        return std::path::PathBuf::from(home).join("auth.json");
    }
    dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".codex")
        .join("auth.json")
}

fn load_auth_from(path: &std::path::Path) -> Option<CodexAuth> {
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    if v.get("OPENAI_API_KEY").is_some() && v.get("tokens").is_none() {
        return None; // API-key mode has no OAuth usage endpoint
    }
    let tokens = v.get("tokens")?;
    let field = |snake: &str, camel_name: &str| -> Option<String> {
        tokens
            .get(snake)
            .or_else(|| tokens.get(camel_name))
            .and_then(|t| t.as_str())
            .map(String::from)
    };
    let access_token = field("access_token", "accessToken")?;
    let account_id = field("account_id", "accountId")
        .or_else(|| field("id_token", "idToken").and_then(|t| jwt_account_id(&t)))
        .or_else(|| jwt_account_id(&access_token));
    Some(CodexAuth {
        access_token,
        account_id,
    })
}

async fn usage_call(http: &reqwest::Client, token: &str, account_id: Option<&str>) -> Result<Value, ProviderState> {
    let mut req = http
        .get("https://chatgpt.com/backend-api/wham/usage")
        .header("Authorization", format!("Bearer {token}"))
        .header("Accept", "application/json")
        .header("User-Agent", "AIUsage/0.1");
    if let Some(id) = account_id {
        req = req.header("ChatGPT-Account-Id", id);
    }
    let resp = req.send().await.map_err(|_| ProviderState::NoData)?;
    match resp.status().as_u16() {
        401 => Err(ProviderState::AuthRequired),
        200 => resp.json::<Value>().await.map_err(|_| ProviderState::ParseFailed),
        _ => Err(ProviderState::NoData),
    }
}

pub async fn fetch(ctx: &FetchCtx) -> ProviderSnapshot {
    let auth = load_auth_from(&auth_path());
    let snap = fetch_with_auth(ctx, auth).await;
    if snap.state == ProviderState::AuthRequired {
        snap.with_message("Run `codex` to refresh your login, then refresh this widget — credentials are read only")
    } else {
        snap
    }
}

async fn fetch_with_auth(ctx: &FetchCtx, auth: Option<CodexAuth>) -> ProviderSnapshot {
    let Some(auth) = auth else {
        return snapshot_for(ProviderId::Codex, ProviderState::AuthRequired, vec![]);
    };
    // The CLI owns refresh tokens. Re-read its access token on each poll.
    if token_needs_refresh(&auth.access_token) {
        return snapshot_for(ProviderId::Codex, ProviderState::AuthRequired, vec![]);
    }
    match usage_call(&ctx.http, &auth.access_token, auth.account_id.as_deref()).await {
        Ok(v) => {
            let windows = parse_usage(&v);
            let state = if windows.is_empty() { ProviderState::NoData } else { ProviderState::Ok };
            snapshot_for(ProviderId::Codex, state, windows)
        }
        Err(state) => snapshot_for(ProviderId::Codex, state, vec![]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn b64url_json(v: serde_json::Value) -> String {
        let bytes = serde_json::to_vec(&v).unwrap();
        const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
        let mut out = String::new();
        for chunk in bytes.chunks(3) {
            let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
            let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
            let cs = [
                T[(n >> 18) as usize & 63],
                T[(n >> 12) as usize & 63],
                (chunk.len() > 1).then(|| T[(n >> 6) as usize & 63]).unwrap_or(b'='),
                (chunk.len() > 2).then(|| T[n as usize & 63]).unwrap_or(b'='),
            ];
            out.push_str(&String::from_utf8(cs.to_vec()).unwrap());
        }
        out
    }

    #[test]
    fn codex_fixture_maps_primary_secondary() {
        let v = json!({"rate_limit": {
            "primary_window": {"used_percent": 12, "reset_at": 1775000000i64, "limit_window_seconds": 18000},
            "secondary_window": {"used_percent": 33, "reset_at": 1776500000i64, "limit_window_seconds": 604800}
        }});
        let windows = parse_usage(&v);
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, "5h");
        assert_eq!(windows[0].window_minutes, Some(300));
        assert!((windows[0].used_percent.unwrap() - 12.0).abs() < 1e-6);
        assert!(!windows[0].resets_at.as_deref().unwrap().is_empty());
        assert_eq!(windows[1].id, "week");
        assert_eq!(windows[1].window_minutes, Some(10080));
        assert!((windows[1].used_percent.unwrap() - 33.0).abs() < 1e-6);
    }

    #[test]
    fn codex_pro_weekly_primary_keeps_week_identity_not_5h() {
        // live-verified 2026-11-09: OpenAI folded pro into a single weekly primary, no 5h window
        let v = json!({"rate_limit": {
            "primary_window": {"used_percent": 87, "reset_at": 1794445045i64, "limit_window_seconds": 604800},
            "secondary_window": null
        }});
        let windows = parse_usage(&v);
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].id, "week");
        assert_eq!(windows[0].title, "wk");
        assert_eq!(windows[0].window_minutes, Some(10080));
        assert!((windows[0].used_percent.unwrap() - 87.0).abs() < 1e-6);
    }

    #[test]
    fn codex_unknown_window_keeps_informational_minute_length() {
        let v = json!({"rate_limit": {
            "primary_window": {"used_percent": 5, "reset_at": 100i64, "limit_window_seconds": 900}
        }});
        let windows = parse_usage(&v);
        assert_eq!(windows[0].id, "5h"); // positional fallback for unknown cadence
        assert_eq!(windows[0].window_minutes, Some(15));
    }

    #[test]
    fn codex_additional_rate_limits_become_named_windows() {
        let v = json!({
            "additional_rate_limits": [
                {"limit_name": "GPT-5.3-Codex-Spark", "rate_limit": {
                    "primary_window": {"used_percent": 55, "reset_at": 100, "limit_window_seconds": 300}
                }}
            ]
        });
        let windows = parse_usage(&v);
        assert!(windows.iter().any(|w| w.title.contains("Spark")
            && w.id == "gpt-5.3-codex-spark"
            && (w.used_percent.unwrap() - 55.0).abs() < 1e-6));
    }

    #[test]
    fn codex_account_id_from_jwt_fallback() {
        let payload = json!({"chatgpt_account_id": "acc-7"});
        let jwt = format!("x.{}.y", b64url_json(payload));
        assert_eq!(jwt_account_id(&jwt).as_deref(), Some("acc-7"));
        let nested = json!({"https://api.openai.com/auth": {"chatgpt_account_id": "acc-9"}});
        let jwt2 = format!("h.{}.s", b64url_json(nested));
        assert_eq!(jwt_account_id(&jwt2).as_deref(), Some("acc-9"));
        assert_eq!(jwt_account_id("garbage"), None);
    }

    #[test]
    fn codex_auth_parse_fixture_file() {
        let dir = std::env::temp_dir().join(format!("aiusage-codex-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("auth.json"),
            r#"{"tokens":{"access_token":"at","refresh_token":"rt","account_id":"acc-1"},"last_refresh":"2026-11-01T00:00:00Z"}"#,
        )
        .unwrap();
        let auth = load_auth_from(&dir.join("auth.json")).expect("auth parsed");
        assert_eq!(auth.access_token, "at");
        assert_eq!(auth.account_id.as_deref(), Some("acc-1"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[tokio::test]
    async fn codex_missing_auth_is_auth_required() {
        let cfg = crate::config::defaults();
        let ctx = FetchCtx::new(cfg);
        let snap = fetch_with_auth(&ctx, None).await;
        assert_eq!(snap.state, ProviderState::AuthRequired);
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
        let snap = fetch_with_auth(&ctx, Some(CodexAuth { access_token: format!("x.{}.y", b64url_json(json!({"exp": 1}))), account_id: None })).await;
        assert_eq!(snap.state, ProviderState::AuthRequired);
        assert!(listener.accept().is_err(), "expired CLI credentials must not trigger OAuth refresh or usage requests");
    }
}
