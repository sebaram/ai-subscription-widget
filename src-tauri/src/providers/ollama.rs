use crate::model::{snapshot_for, UsageWindow, ProviderId, ProviderSnapshot, ProviderState};
use crate::providers::FetchCtx;
use serde_json::Value;

fn amount(v: &Value) -> Option<f64> {
    match v {
        Value::Number(n) => n.as_f64().filter(|x| x.is_finite()),
        Value::String(s) => s.trim().trim_start_matches('$').parse::<f64>().ok().filter(|x| x.is_finite()),
        _ => None,
    }
}

fn parse_balance(v: &Value) -> Vec<UsageWindow> {
    let Some(included) = v.get("included") else { return vec![] };
    let mut windows = Vec::new();

    // Standard plan: monthly allowance + balance
    if let (Some(allowance), Some(balance)) = (
        included.get("allowance_usd").and_then(amount),
        included.get("balance_usd").and_then(amount),
    ) {
        let used = (allowance - balance).max(0.0);
        let (used_percent, is_known) = if allowance > 0.0 {
            (Some(((used / allowance) * 100.0).min(100.0)), true)
        } else {
            (None, false) // zero allowance: informational, no fake meter
        };
        let resets_at = included.get("period").and_then(|p| p.get("until")).and_then(|u| u.as_str()).map(String::from);
        windows.push(UsageWindow {
            id: "month".into(),
            title: "mo".into(),
            window_minutes: Some(43200),
            used_percent,
            quota: (allowance > 0.0).then(|| crate::model::QuotaInfo {
                used: format_usd(used),
                total: format_usd(allowance),
            }),
            pace_expected_percent: resets_at.as_deref().and_then(|r| crate::model::pace_expected(43200, r)),
            resets_at,
            is_known,
        });
    }

    // Legacy plan: session/weekly remaining_percent
    for (key, id, title, minutes) in [
        ("session", "session", "5h", 300i64),
        ("weekly", "week", "wk", 10080),
    ] {
        if let Some(sub) = included.get(key) {
            let Some(remaining) = sub.get("remaining_percent").and_then(|r| r.as_f64()) else { continue };
            windows.push(UsageWindow {
                id: id.into(),
                title: title.into(),
                window_minutes: Some(minutes),
                used_percent: Some(((100.0 - remaining).max(0.0)).min(100.0)),
                quota: None,
                resets_at: sub.get("resets_at").and_then(|r| r.as_str()).map(String::from),
                pace_expected_percent: None,
                is_known: true,
            });
        }
    }

    // Purchased credit: informational, no percentage
    if let Some(credit) = v.get("purchased").and_then(|p| p.get("balance_usd")).and_then(amount) {
        windows.push(UsageWindow {
            id: "purchased".into(),
            title: "credits".into(),
            window_minutes: None,
            used_percent: None,
            quota: Some(crate::model::QuotaInfo { used: format_usd(credit), total: "—".into() }),
            resets_at: None,
            pace_expected_percent: None,
            is_known: true,
        });
    }

    windows
}

pub fn format_usd(v: f64) -> String {
    format!("${v:.2}")
}

/// Resolution order: config field → OLLAMA_API_KEY env → `.env` files (cwd / cwd parent / exe dir).
pub fn resolve_key(cfg_key: &str) -> String {
    let mut dirs = vec![std::env::current_dir().ok()];
    if let Some(cwd) = std::env::current_dir().ok() {
        dirs.push(cwd.parent().map(std::path::Path::to_path_buf));
    }
    if let Ok(exe) = std::env::current_exe() {
        dirs.push(exe.parent().map(std::path::Path::to_path_buf));
    }
    let search_dirs: Vec<std::path::PathBuf> = dirs.into_iter().flatten().collect();
    resolve_key_from(cfg_key, std::env::var("OLLAMA_API_KEY").ok().as_deref(), &search_dirs)
}

fn resolve_key_from(cfg_key: &str, env_key: Option<&str>, search_dirs: &[std::path::PathBuf]) -> String {
    let cfg = cfg_key.trim();
    if !cfg.is_empty() {
        return cfg.to_string();
    }
    if let Some(k) = env_key {
        if !k.trim().is_empty() {
            return k.trim().to_string();
        }
    }
    for env_path in search_dirs.iter().map(|d| d.join(".env")) {
        if let Ok(text) = std::fs::read_to_string(&env_path) {
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("OLLAMA_API_KEY=") {
                    let v = v.trim();
                    if !v.is_empty() {
                        return v.to_string();
                    }
                }
            }
        }
    }
    String::new()
}

pub async fn fetch(ctx: &FetchCtx) -> ProviderSnapshot {
    let snap = fetch_with_key(ctx, &resolve_key(&ctx.cfg.ollama_api_key)).await;
    map_message_auth_required(snap, "Paste your Ollama Cloud API key in Settings → Connection")
}

fn map_message_auth_required(snap: ProviderSnapshot, message: &str) -> ProviderSnapshot {
    if snap.state == ProviderState::AuthRequired {
        snap.with_message(message)
    } else if snap.state == ProviderState::ParseFailed {
        snap.with_message("Could not read the balance response (API may have changed)")
    } else {
        snap
    }
}

async fn fetch_with_key(ctx: &FetchCtx, key: &str) -> ProviderSnapshot {
    if key.is_empty() {
        return snapshot_for(ProviderId::Ollama, ProviderState::AuthRequired, vec![]);
    }
    let result = ctx
        .http
        .get("https://ollama.com/api/balance")
        .bearer_auth(&key)
        .header("Accept", "application/json")
        .send()
        .await;
    let (state, windows) = match result {
        Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => (ProviderState::AuthRequired, vec![]),
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let windows = parse_balance(&v);
                if windows.is_empty() {
                    (ProviderState::NoData, vec![])
                } else {
                    (ProviderState::Ok, windows)
                }
            }
            Err(_) => (ProviderState::ParseFailed, vec![]),
        },
        Err(_) => (ProviderState::NoData, vec![]),
    };
    snapshot_for(ProviderId::Ollama, state, windows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const STANDARD: &str = r#"{"included":{"allowance_usd":35.0,"balance_usd":26.6,"period":{"until":"2026-11-24T00:00:00Z"}},"purchased":{"balance_usd":10.0}}"#;
    const LEGACY: &str = r#"{"included":{"session":{"remaining_percent":75.0,"resets_at":"2026-11-09T15:00:00Z"},"weekly":{"remaining_percent":30.0,"resets_at":"2026-11-16T00:00:00Z"}}}"#;

    #[test]
    fn ollama_standard_plan_maps_monthly_bar() {
        let windows = parse_balance(&serde_json::from_str::<Value>(STANDARD).unwrap());
        assert_eq!(windows.len(), 2, "monthly + purchased credit");
        let month = &windows[0];
        assert_eq!(month.id, "month");
        assert_eq!(month.title, "mo");
        assert_eq!(month.window_minutes, Some(43200));
        assert!((month.used_percent.unwrap() - 24.0).abs() < 1e-6);
        assert!(month.is_known);
        assert_eq!(month.resets_at.as_deref(), Some("2026-11-24T00:00:00Z"));
        let q = month.quota.clone().unwrap();
        // design mock: used amount of allowance ("$8.40 of $35.00"), NOT the remaining balance
        assert_eq!(q.used, "$8.40");
        assert_eq!(q.total, "$35.00");
        let credit = &windows[1];
        assert_eq!(credit.id, "purchased");
        assert_eq!(credit.used_percent, None);
        assert_eq!(credit.quota.as_ref().unwrap().used, "$10.00");
    }

    #[test]
    fn ollama_legacy_plan_maps_session_and_weekly() {
        let windows = parse_balance(&serde_json::from_str::<Value>(LEGACY).unwrap());
        assert_eq!(windows.len(), 2);
        assert_eq!(windows[0].id, "session");
        assert_eq!(windows[0].window_minutes, Some(300));
        assert!((windows[0].used_percent.unwrap() - 25.0).abs() < 1e-6);
        assert_eq!(windows[1].id, "week");
        assert_eq!(windows[1].window_minutes, Some(10080));
        assert!((windows[1].used_percent.unwrap() - 70.0).abs() < 1e-6);
    }

    #[test]
    fn ollama_amounts_as_strings_parse_too() {
        let v = serde_json::json!({"included":{"allowance_usd":"35.0","balance_usd":"26.60","period":{"until":"2026-11-24T00:00:00Z"}}});
        let windows = parse_balance(&v);
        assert!((windows[0].used_percent.unwrap() - 24.0).abs() < 1e-6);
    }

    #[test]
    fn ollama_unknown_shape_is_no_data_not_zero() {
        let windows = parse_balance(&json!({"foo": 1}));
        assert!(windows.is_empty());
        // garbage amounts must also reject rather than fabricate numbers
        let bad = serde_json::json!({"included":{"allowance_usd":true,"balance_usd":26.6,"period":{"until":"2026-11-24T00:00:00Z"}}});
        assert!(parse_balance(&bad).is_empty());
    }

    #[tokio::test]
    async fn ollama_empty_key_is_auth_required() {
        let cfg = crate::config::defaults();
        let ctx = FetchCtx::new(cfg);
        let snap = fetch_with_key(&ctx, "").await;
        assert_eq!(snap.state, ProviderState::AuthRequired);
        assert!(snap.windows.is_empty());
    }

    #[test]
    fn ollama_resolve_key_prefers_config_field() {
        assert_eq!(resolve_key_from("cfg-key", Some("env-key"), &[]), "cfg-key");
        assert_eq!(resolve_key_from("", Some("env-key"), &[]), "env-key");
    }

    #[test]
    fn ollama_resolve_key_reads_dotenv_with_config_empty() {
        let dir = std::env::temp_dir().join(format!("aiusage-ollama-dotenv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(".env"), "OTHER=1\nOLLAMA_API_KEY=dotenv-secret\n").unwrap();
        assert_eq!(resolve_key_from("", None, std::slice::from_ref(&dir)), "dotenv-secret");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ollama_resolve_key_no_key_anywhere_is_empty() {
        assert_eq!(resolve_key_from("", None, &[]), "");
    }
}
