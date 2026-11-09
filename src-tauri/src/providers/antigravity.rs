pub mod discovery;

use crate::model::{pace_expected, snapshot_for, ProviderId, ProviderSnapshot, ProviderState, UsageWindow};
use crate::providers::FetchCtx;
use serde_json::Value;

const UA_REMOTE: &str = "antigravity/hub/2.9.1 windows/amd64";

fn used_percent_from_fraction(fraction: f64) -> f64 {
    100.0 - (fraction * 100.0).clamp(0.0, 100.0)
}

/// Maps a bucket window (string cadence alias or numeric minutes) to (id, title, minutes).
fn window_mapping(window: &Value) -> (String, String, Option<i64>) {
    let norm: Option<String> = match window {
        Value::String(s) => Some(
            s.to_lowercase()
                .replace('_', "-")
                .trim()
                .trim_end_matches(" limit")
                .trim()
                .to_string(),
        ),
        Value::Number(n) => n.as_i64().map(|m| m.to_string()),
        _ => None,
    };
    let Some(n) = norm else { return ("custom".into(), "custom".into(), None) };
    let is = |exact: &[&str]| exact.contains(&n.as_str()) || exact.iter().any(|e| n.ends_with(&format!("-{e}")));
    if is(&["300", "session", "5h", "5-hour", "five-hour", "five hour"]) {
        ("5h".into(), "5h".into(), Some(300))
    } else if is(&["10080", "weekly"]) {
        ("week".into(), "wk".into(), Some(10080))
    } else if is(&["43200", "monthly"]) {
        ("month".into(), "mo".into(), Some(43200))
    } else {
        ("custom".into(), "custom".into(), None)
    }
}

fn fraction_of(bucket: &Value) -> Option<f64> {
    bucket
        .get("remainingFraction")
        .or_else(|| bucket.get("remaining_fraction"))
        .and_then(|f| f.as_f64())
        .or_else(|| {
            bucket.get("remaining").and_then(|r| {
                r.get("remaining_fraction")
                    .or_else(|| r.get("value"))
                    .and_then(|f| f.as_f64())
            })
        })
}

fn reset_time_of(bucket: &Value) -> Option<String> {
    match bucket.get("reset_time").or_else(|| bucket.get("resetTime"))? {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => n
            .as_f64()
            .map(|secs| secs as i64)
            .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
            .map(|d| d.to_rfc3339()),
        _ => None,
    }
}

fn make_window(id: &str, title: &str, minutes: Option<i64>, used: f64, resets: Option<String>) -> UsageWindow {
    UsageWindow {
        id: id.into(),
        title: title.into(),
        window_minutes: minutes,
        used_percent: Some(used),
        quota: None,
        resets_at: resets.clone(),
        pace_expected_percent: minutes.and_then(|m| resets.as_deref().and_then(|r| pace_expected(m, r))),
        is_known: true,
    }
}

/// groups[].buckets[] → float windows (worst-of per window type) + labeled group extras.
fn parse_summary(v: &Value) -> Vec<UsageWindow> {
    let groups = v
        .get("groups")
        .or_else(|| v.get("response").and_then(|r| r.get("groups")))
        .or_else(|| v.get("summary").and_then(|s| s.get("groups")))
        .and_then(|g| g.as_array());
    let Some(groups) = groups else { return vec![] };

    // worst-of per window type across ALL groups
    let mut worst: Vec<(String, String, Option<i64>, f64, Option<String>)> = Vec::new();
    let mut extras: Vec<UsageWindow> = Vec::new();

    for g in groups {
        let display = g
            .get("displayName")
            .or_else(|| g.get("name"))
            .and_then(|d| d.as_str())
            .unwrap_or("models")
            .to_string();
        let Some(buckets) = g.get("buckets").and_then(|b| b.as_array()) else { continue };
        for b in buckets {
            if b.get("disabled").and_then(|d| d.as_bool()) == Some(true) {
                continue;
            }
            let Some(fraction) = fraction_of(b) else { continue };
            let (wid, wtitle, minutes) = window_mapping(
                b.get("window").unwrap_or_else(|| b.get("bucketId").or_else(|| b.get("id")).unwrap_or(&Value::Null)),
            );
            let resets = reset_time_of(b);
            let used = used_percent_from_fraction(fraction);
            let slot = worst.iter_mut().find(|w| w.0 == wid);
            match slot {
                Some(s) if used > s.3 => *s = (wid.clone(), wtitle.clone(), minutes, used, resets.clone()),
                Some(_) => {}
                None => worst.push((wid.clone(), wtitle.clone(), minutes, used, resets.clone())),
            }
            extras.push(make_window(
                &format!("group:{display}:{wid}"),
                &format!("{display} {wtitle}"),
                minutes,
                used,
                resets,
            ));
        }
    }

    let mut windows: Vec<UsageWindow> = worst
        .into_iter()
        .map(|(id, title, minutes, used, resets)| make_window(&id, &title, minutes, used, resets))
        .collect();
    windows.extend(extras);
    windows
}

/// Legacy GetUserStatus fallback: per-model quotas.
fn parse_user_status(v: &Value) -> Vec<UsageWindow> {
    let Some(configs) = v
        .get("userStatus")
        .and_then(|u| u.get("cascadeModelConfigData"))
        .and_then(|c| c.get("clientModelConfigs"))
        .and_then(|c| c.as_array())
    else {
        return vec![];
    };
    let mut windows = Vec::new();
    for cfg in configs {
        let Some(q) = cfg.get("quota_info").or_else(|| cfg.get("quotaInfo")) else { continue };
        let Some(fraction) = q.get("remaining_fraction").or_else(|| q.get("remainingFraction")).and_then(|f| f.as_f64())
        else {
            continue; // unknown quota → skip, never fabricate
        };
        let label = cfg
            .get("label")
            .or_else(|| cfg.get("model_id").or_else(|| cfg.get("id")))
            .and_then(|l| l.as_str())
            .unwrap_or("model");
        windows.push(make_window(
            &format!("model:{}", label.to_lowercase().replace(' ', "-")),
            label,
            None,
            (1.0 - fraction).clamp(0.0, 1.0) * 100.0,
            q.get("reset_time").or_else(|| q.get("resetTime")).and_then(|r| r.as_str()).map(String::from),
        ));
    }
    windows
}

async fn probe_live(
    ctx: &FetchCtx,
    info: &discovery::LspInfo,
) -> Option<u16> {
    for port in discovery::probe_order(info) {
        let url = format!("https://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/GetUnleashData");
        let mut req = ctx
            .http_loopback
            .post(&url)
            .header("Connect-Protocol-Version", "1")
            .header("Content-Type", "application/json");
        if let Some(token) = &info.csrf_token {
            req = req.header("X-Codeium-Csrf-Token", token);
        }
        if let Ok(resp) = req.body("{}").send().await {
            let s = resp.status().as_u16();
            if s == 200 || s == 401 {
                return Some(port);
            }
        }
    }
    None
}

async fn local_fetch(ctx: &FetchCtx, info: discovery::LspInfo) -> ProviderSnapshot {
    let Some(port) = probe_live(ctx, &info).await else {
        // process exists but server unreachable: not tracking anything honestly
        return snapshot_for(ProviderId::Antigravity, ProviderState::NoData, vec![]);
    };
    let quota_url = format!("https://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/RetrieveUserQuotaSummary");
    let with_csrf = |req: reqwest::RequestBuilder| match &info.csrf_token {
        Some(t) => req.header("X-Codeium-Csrf-Token", t),
        None => req,
    };
    let mut req = with_csrf(
        ctx.http_loopback
            .post(&quota_url)
            .header("Connect-Protocol-Version", "1")
            .header("Content-Type", "application/json"),
    );
    req = req.body(r#"{"forceRefresh":true}"#);
    let summary = req.send().await.ok().filter(|r| r.status().as_u16() == 200);
    let windows = match summary {
        Some(r) => match r.json::<Value>().await {
            Ok(v) => parse_summary(&v),
            Err(_) => vec![],
        },
        None => vec![],
    };
    if !windows.is_empty() {
        return snapshot_for(ProviderId::Antigravity, ProviderState::Ok, windows);
    }
    // fallback: legacy status endpoint
    let status_url = format!("https://127.0.0.1:{port}/exa.language_server_pb.LanguageServerService/GetUserStatus");
    let mut req = with_csrf(
        ctx.http_loopback
            .post(&status_url)
            .header("Connect-Protocol-Version", "1")
            .header("Content-Type", "application/json"),
    );
    req = req.body(r#"{"metadata":{"ideName":"antigravity","extensionName":"antigravity","ideVersion":"unknown","locale":"en"}}"#);
    let legacy = req.send().await.ok().filter(|r| r.status().as_u16() == 200);
    let windows = match legacy {
        Some(r) => match r.json::<Value>().await {
            Ok(v) => parse_user_status(&v),
            Err(_) => vec![],
        },
        None => vec![],
    };
    if windows.is_empty() {
        snapshot_for(ProviderId::Antigravity, ProviderState::NoData, vec![])
    } else {
        snapshot_for(ProviderId::Antigravity, ProviderState::Ok, windows)
    }
}

/// Remote Google quota — only when gemini CLI OAuth creds exist (design: optional).
async fn remote_fetch(ctx: &FetchCtx) -> Option<ProviderSnapshot> {
    let path = dirs::home_dir()?.join(".gemini").join("oauth_creds.json");
    let text = std::fs::read_to_string(path).ok()?;
    let v: Value = serde_json::from_str(&text).ok()?;
    let token = v.get("access_token").or_else(|| v.get("accessToken")).and_then(|t| t.as_str())?.to_string();
    let resp = ctx
        .http
        .post("https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary")
        .header("Authorization", format!("Bearer {token}"))
        .header("User-Agent", UA_REMOTE)
        .json(&serde_json::json!({}))
        .send()
        .await
        .ok()?;
    if resp.status().as_u16() != 200 {
        return Some(snapshot_for(ProviderId::Antigravity, ProviderState::AuthRequired, vec![]));
    }
    let windows = parse_summary(&resp.json::<Value>().await.unwrap_or(Value::Null));
    if windows.is_empty() {
        Some(snapshot_for(ProviderId::Antigravity, ProviderState::NoData, vec![]))
    } else {
        Some(snapshot_for(ProviderId::Antigravity, ProviderState::Ok, windows))
    }
}

pub async fn fetch(ctx: &FetchCtx) -> ProviderSnapshot {
    let Some(info) = discovery::find_lsp() else {
        return match remote_fetch(ctx).await {
            Some(snap) => snap,
            None => snapshot_for(ProviderId::Antigravity, ProviderState::NotRunning, vec![]),
        };
    };
    local_fetch(ctx, info).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn used_percent_from_fraction_clamps() {
        assert!((used_percent_from_fraction(1.0) - 0.0).abs() < 1e-9);
        assert!((used_percent_from_fraction(0.42) - 58.0).abs() < 1e-9);
        assert!((used_percent_from_fraction(1.5) - 0.0).abs() < 1e-9);
        assert!((used_percent_from_fraction(-0.1) - 100.0).abs() < 1e-9);
    }

    #[test]
    fn bucket_window_maps_300_10080() {
        assert_eq!(window_mapping(&json!(300)).0, "5h");
        assert_eq!(window_mapping(&json!("session")).0, "5h");
        assert_eq!(window_mapping(&json!("5-hour")).0, "5h");
        assert_eq!(window_mapping(&json!("pro-session")).0, "5h");
        assert_eq!(window_mapping(&json!(10080)).0, "week");
        assert_eq!(window_mapping(&json!("weekly")).0, "week");
        assert_eq!(window_mapping(&json!(43200)).0, "month");
        assert_eq!(window_mapping(&json!("daily")).0, "custom");
    }

    #[test]
    fn antigravity_worst_of_groups_per_window() {
        let v = json!({"groups": [
            {"displayName": "Gemini Models", "buckets": [
                {"window": "5h", "remainingFraction": 0.9, "reset_time": "2026-11-09T15:00:00Z"},
                {"window": "weekly", "remainingFraction": 0.49, "reset_time": "2026-11-16T00:00:00Z"}
            ]},
            {"displayName": "Claude and GPT models", "buckets": [
                {"window": "5h", "remainingFraction": 0.1, "reset_time": "2026-11-09T15:00:00Z"},
                {"window": "weekly", "remainingFraction": 0.8, "reset_time": "2026-11-16T00:00:00Z"}
            ]}
        ]});
        let windows = parse_summary(&v);
        let five = windows.iter().find(|w| w.id == "5h").expect("5h float window");
        assert!((five.used_percent.unwrap() - 90.0).abs() < 1e-6);
        let week = windows.iter().find(|w| w.id == "week").expect("week float window");
        assert!((week.used_percent.unwrap() - 51.0).abs() < 1e-6);
        assert!(windows.iter().any(|w| w.id == "group:Gemini Models:5h"));
        assert!(windows.iter().any(|w| w.id == "group:Claude and GPT models:5h"
            && (w.used_percent.unwrap() - 90.0).abs() < 1e-6));
    }

    #[test]
    fn antigravity_disabled_buckets_excluded() {
        let v = json!({"groups": [
            {"displayName": "G", "buckets": [
                {"window": "5h", "disabled": true, "remainingFraction": 0.0},
                {"window": "5h", "remainingFraction": 0.5}
            ]}
        ]});
        let windows = parse_summary(&v);
        assert!(windows.iter().any(|w| (w.used_percent.unwrap() - 50.0).abs() < 1e-6));
        assert_eq!(windows.iter().filter(|w| w.id == "5h").count(), 1);
    }

    #[test]
    fn antigravity_remaining_oneof_alias_parses() {
        let v = json!({"groups": [
            {"name": "Gemini", "buckets": [
                {"id": "gemini-weekly", "remaining": {"remaining_fraction": 0.25}}
            ]}
        ]});
        let windows = parse_summary(&v);
        let week = windows.iter().find(|w| w.id == "week").expect("bucket via id alias");
        assert!((week.used_percent.unwrap() - 75.0).abs() < 1e-6);
    }

    #[test]
    fn antigravity_legacy_status_maps_model_quotas() {
        let v = json!({"userStatus": {"cascadeModelConfigData": {"clientModelConfigs": [
            {"label": "Gemini 3 Pro", "quota_info": {"remaining_fraction": 0.3, "reset_time": "2026-11-09T15:00:00Z"}},
            {"label": "NoQuotaModel"}
        ]}}});
        let windows = parse_user_status(&v);
        assert_eq!(windows.len(), 1, "missing quota is skipped, never 0%");
        assert!((windows[0].used_percent.unwrap() - 70.0).abs() < 1e-6);
        assert_eq!(windows[0].title, "Gemini 3 Pro");
    }

    #[tokio::test]
    async fn antigravity_no_process_no_oauth_is_not_running() {
        if discovery::find_lsp().is_some() {
            return; // IDE started mid-run on this machine: environment test not applicable
        }
        let cfg = crate::config::defaults();
        let ctx = FetchCtx::new(cfg);
        let snap = fetch(&ctx).await;
        assert_eq!(snap.state, ProviderState::NotRunning);
    }
}
