use crate::model::{snapshot_for, UsageWindow, ProviderId, ProviderSnapshot, ProviderState};
use crate::providers::FetchCtx;
use serde_json::Value;

fn epoch_to_iso(secs: i64) -> Option<String> {
    // 0 / negative = no real reset time — render nothing rather than 1970 (review minor)
    chrono::DateTime::from_timestamp(secs, 0).filter(|_| secs > 0).map(|d| d.to_rfc3339())
}

fn window_from(
    id: &str,
    title: &str,
    w: &Value,
) -> Option<UsageWindow> {
    let used = w.get("used_percent").and_then(|u| u.as_f64())?;
    let minutes = w.get("limit_window_seconds").and_then(|s| s.as_i64()).map(|s| (s / 60).max(1));
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
        id: id.into(),
        title: title.into(),
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

fn parse_usage(v: &Value) -> Vec<UsageWindow> {
    let mut windows = Vec::new();
    if let Some(rl) = v.get("rate_limit") {
        if let Some(w) = rl.get("primary_window").and_then(|w| window_from("5h", "5h", w)) {
            windows.push(w);
        }
        if let Some(w) = rl.get("secondary_window").and_then(|w| window_from("week", "wk", w)) {
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
            let mapped = rl
                .and_then(|r| r.get("primary_window").and_then(|w| window_from(&slugify(name), name, w)))
                .or_else(|| rl.and_then(|r| window_from(&slugify(name), name, r)));
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

/// Expiry of a fresh access token: from its JWT exp, else a conservative 8h.
fn exp_ms_for(access: &str, now_ms: u64) -> u64 {
    let exp = jwt_exp_epoch(access).unwrap_or(0);
    if exp > 0 {
        (exp as i128 * 1000) as u64
    } else {
        now_ms + 28800 * 1000
    }
}

pub struct CodexAuth {
    access_token: String,
    refresh_token: Option<String>,
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
        refresh_token: field("refresh_token", "refreshToken"),
        account_id,
    })
}

/// In-memory refresh (design: never writes ~/.codex/auth.json).
async fn refresh_access_token(
    http: &reqwest::Client,
    refresh_token: &str,
) -> Result<String, String> {
    let resp = http
        .post("https://auth.openai.com/oauth/token")
        .header("Content-Type", "application/json")
        .body(
            serde_json::json!({
                "client_id": "app_EMoamEEZ73f0CkXaXp7hrann",
                "grant_type": "refresh_token",
                "refresh_token": refresh_token,
                "scope": "openid profile email"
            })
            .to_string(),
        )
        .send()
        .await
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("refresh failed: {}", resp.status()));
    }
    let v: Value = resp.json().await.map_err(|e| e.to_string())?;
    v.get("access_token")
        .and_then(|t| t.as_str())
        .map(String::from)
        .ok_or_else(|| "no access_token".to_string())
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
        snap.with_message("Run `codex` once to log in — its stored token is read, never written")
    } else {
        snap
    }
}

async fn fetch_with_auth(ctx: &FetchCtx, auth: Option<CodexAuth>) -> ProviderSnapshot {
    let Some(mut auth) = auth else {
        return snapshot_for(ProviderId::Codex, ProviderState::AuthRequired, vec![]);
    };
    // Review I4: cache-first — refresh once per expiry, never re-play a rotated token.
    use crate::providers::{first_token_step, CachedStep};
    let now_ms = crate::providers::now_epoch_ms();
    let mut force_refreshable = auth.refresh_token.clone();
    let mut token = match first_token_step(crate::providers::cached_token("codex"), now_ms) {
        CachedStep::CachedValid(t) => t,
        CachedStep::CachedRotate(rt) => {
            force_refreshable = None;
            match refresh_access_token(&ctx.http, &rt).await {
                Ok(new) => {
                    crate::providers::store_token("codex", new.clone(), rt, exp_ms_for(&new, now_ms));
                    new
                }
                Err(_) => return snapshot_for(ProviderId::Codex, ProviderState::AuthRequired, vec![]),
            }
        }
        CachedStep::UseFile => {
            let file_token = auth.access_token.clone();
            if token_needs_refresh(&file_token) {
                let Some(rt) = auth.refresh_token.clone() else {
                    return snapshot_for(ProviderId::Codex, ProviderState::AuthRequired, vec![]);
                };
                match refresh_access_token(&ctx.http, &rt).await {
                    Ok(new) => {
                        crate::providers::store_token("codex", new.clone(), rt.clone(), exp_ms_for(&new, now_ms));
                        force_refreshable = Some(rt);
                        new
                    }
                    Err(_) => return snapshot_for(ProviderId::Codex, ProviderState::AuthRequired, vec![]),
                }
            } else {
                file_token
            }
        }
    };
    let usage = usage_call(&ctx.http, &token, auth.account_id.as_deref()).await;
    match usage {
        Ok(v) => {
            let windows = parse_usage(&v);
            if windows.is_empty() {
                // Review minor: an empty 200 is "No quota reported", not an empty card
                snapshot_for(ProviderId::Codex, ProviderState::NoData, vec![])
            } else {
                snapshot_for(ProviderId::Codex, ProviderState::Ok, windows)
            }
        }
        Err(ProviderState::AuthRequired) if force_refreshable.is_some() => {
            let rt = force_refreshable.unwrap();
            match refresh_access_token(&ctx.http, &rt).await {
                Ok(new) => {
                    crate::providers::store_token("codex", new.clone(), rt, exp_ms_for(&new, now_ms));
                    token = new;
                    match usage_call(&ctx.http, &token, auth.account_id.as_deref()).await {
                        Ok(v) => {
                            let windows = parse_usage(&v);
                            if windows.is_empty() {
                                snapshot_for(ProviderId::Codex, ProviderState::NoData, vec![])
                            } else {
                                snapshot_for(ProviderId::Codex, ProviderState::Ok, windows)
                            }
                        }
                        Err(state) => snapshot_for(ProviderId::Codex, state, vec![]),
                    }
                }
                Err(_) => snapshot_for(ProviderId::Codex, ProviderState::AuthRequired, vec![]),
            }
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
}
