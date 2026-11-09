pub mod keystore;

use crate::providers::FetchCtx;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::FetchCtx;
    use serde_json::json;

    const FIXTURE: &str = r#"{
        "billingCycleStart": "2026-10-17T09:06:27.000Z",
        "billingCycleEnd": "2026-11-17T09:06:27.000Z",
        "membershipType": "pro",
        "limitType": "user",
        "isUnlimited": false,
        "individualUsage": {
            "plan": {
                "enabled": true, "used": 1974, "limit": 2000, "remaining": 26,
                "autoPercentUsed": 3.906, "apiPercentUsed": 4.8, "totalPercentUsed": 3.987,
                "breakdown": {"included": 1974, "bonus": 0, "total": 1974}
            },
            "onDemand": {"enabled": false, "used": 0, "limit": 0, "remaining": 0}
        },
        "teamUsage": {}
    }"#;

    #[test]
    fn cursor_dashboard_lanes_auto_api() {
        // what Cursor's own dashboard shows (user-verified 2026-11-10):
        // auto ~4%, api ~5% — not the cents ratio (98.7%) nor totalPercentUsed
        let windows = parse_summary(&serde_json::from_str::<Value>(FIXTURE).unwrap());
        assert_eq!(windows.len(), 2, "auto + api lanes");
        let auto = &windows[0];
        assert_eq!(auto.id, "auto");
        assert!((auto.used_percent.unwrap() - 3.906).abs() < 0.01);
        let api = &windows[1];
        assert_eq!(api.id, "api");
        assert!((api.used_percent.unwrap() - 4.8).abs() < 0.01);
        assert!(api.resets_at.as_deref().unwrap().contains("2026-11-17"));
    }

    #[test]
    fn cursor_unlimited_is_informational_not_zero() {
        let v = serde_json::from_str::<Value>(r#"{"isUnlimited": true, "membershipType": "pro", "individualUsage": {"plan": {"enabled": true}}}"#).unwrap();
        let windows = parse_summary(&v);
        assert!(windows.is_empty(), "unlimited → no fake meter");
    }

    #[test]
    fn cursor_falls_back_to_total_percent_without_lane_percents() {
        let v = serde_json::from_str::<Value>(r#"{"individualUsage": {"plan": {"enabled": true, "totalPercentUsed": 61.5, "used": 10, "limit": 2000}}}"#).unwrap();
        let windows = parse_summary(&v);
        assert_eq!(windows.len(), 1);
        assert!((windows[0].used_percent.unwrap() - 61.5).abs() < 0.01);
    }

    #[test]
    fn cursor_on_demand_lane_when_enabled_with_limit() {
        let v = serde_json::from_str::<Value>(r#"{"individualUsage": {"plan": {"enabled": true, "autoPercentUsed": 1.0, "apiPercentUsed": 2.0}, "onDemand": {"enabled": true, "used": 250, "limit": 1500}}}"#).unwrap();
        let windows = parse_summary(&v);
        let od = windows.iter().find(|w| w.id == "on-demand").expect("on-demand lane");
        assert!((od.used_percent.unwrap() - 16.6667).abs() < 0.01);
        assert_eq!(od.quota.as_ref().unwrap().used, "$2.50");
    }

    #[test]
    fn cursor_empty_shape_is_no_data() {
        assert!(parse_summary(&json!({})).is_empty());
    }
}

use crate::model::{snapshot_for, QuotaInfo, UsageWindow, ProviderId, ProviderSnapshot, ProviderState};
use serde_json::Value;

fn parse_summary(v: &Value) -> Vec<UsageWindow> {
    // (validated against Cursor's own dashboard 2026-11-10): the dashboard shows
    // AUTO % and API % lanes — the API's autoPercentUsed/apiPercentUsed match
    // them exactly; the 'used/limit' cent fields and totalPercentUsed do NOT.
    if v.get("isUnlimited").and_then(|b| b.as_bool()) == Some(true) {
        return vec![];
    }
    let plan = v.get("individualUsage").and_then(|i| i.get("plan"));
    let Some(plan) = plan else { return vec![] };
    if plan.get("enabled").and_then(|b| b.as_bool()) == Some(false) {
        return vec![];
    }
    let resets_at = v.get("billingCycleEnd").and_then(|r| r.as_str()).map(String::from);
    let window_minutes = v
        .get("billingCycleStart")
        .and_then(|s| s.as_str())
        .zip(resets_at.as_deref())
        .and_then(|(start, end)| {
            let s = chrono::DateTime::parse_from_rfc3339(start).ok()?;
            let e = chrono::DateTime::parse_from_rfc3339(end).ok()?;
            Some(((e - s).num_minutes().max(1), s))
        });
    let lane = |id: &str, title: &str, percent: Option<f64>| -> Option<UsageWindow> {
        let used_percent = percent?;
        Some(UsageWindow {
            id: id.into(),
            title: title.into(),
            window_minutes: window_minutes.as_ref().map(|(m, _)| *m),
            used_percent: Some(used_percent.clamp(0.0, 100.0)),
            quota: None,
            resets_at: resets_at.clone(),
            pace_expected_percent: window_minutes
                .as_ref()
                .and_then(|(m, _)| resets_at.as_deref().and_then(|r| crate::model::pace_expected(*m, r))),
            is_known: true,
        })
    };
    // dashboard-visible lanes first (auto % / api %)
    let mut windows: Vec<UsageWindow> = [
        lane("auto", "auto", plan.get("autoPercentUsed").and_then(|p| p.as_f64())),
        lane("api", "api", plan.get("apiPercentUsed").and_then(|p| p.as_f64())),
    ]
    .into_iter()
    .flatten()
    .collect();
    if windows.is_empty() {
        // fall back: whatever self-reported percent exists, else cents math
        let total = plan.get("totalPercentUsed").and_then(|p| p.as_f64()).or_else(|| {
            plan.get("used")
                .zip(plan.get("limit"))
                .and_then(|(u, l)| {
                    let (u, l) = (u.as_f64()?, l.as_f64()?);
                    (l > 0.0).then_some(u / l * 100.0)
                })
        });
        if let Some(p) = total {
            if let Some(w) = lane("month", "mo", Some(p)) {
                windows.push(w);
            }
        }
    }
    // on-demand as an informational lane when it has a real limit
    if let Some(od) = v.get("individualUsage").and_then(|i| i.get("onDemand")) {
        let enabled = od.get("enabled").and_then(|b| b.as_bool()) == Some(true);
        let limit = od.get("limit").and_then(|l| l.as_f64()).unwrap_or(0.0);
        let used = od.get("used").and_then(|u| u.as_f64()).unwrap_or(0.0);
        if enabled && limit > 0.0 {
            windows.push(UsageWindow {
                id: "on-demand".into(),
                title: "on-demand".into(),
                window_minutes: None,
                used_percent: Some(((used / limit) * 100.0).clamp(0.0, 100.0)),
                quota: Some(QuotaInfo { used: format!("${:.2}", used / 100.0), total: format!("${:.2}", limit / 100.0) }),
                resets_at: None,
                pace_expected_percent: None,
                is_known: true,
            });
        }
    }
    windows
}

pub async fn fetch(ctx: &FetchCtx) -> ProviderSnapshot {
    let home = dirs::home_dir().unwrap_or_default();
    let Some(session) = keystore::read_session(&home) else {
        return snapshot_for(ProviderId::Cursor, ProviderState::AuthRequired, vec![])
            .with_message("Open Cursor once and log in — we read its stored session read-only");
    };
    let url = "https://cursor.com/api/usage-summary";
    let resp = ctx
        .http
        .get(url)
        .header("Cookie", format!("WorkosCursorSessionToken={}", session.cookie_value()))
        .header("Accept", "application/json")
        .send()
        .await;
    let (state, windows) = match resp {
        Ok(r) if r.status().as_u16() == 401 || r.status().as_u16() == 403 => (ProviderState::AuthRequired, vec![]),
        Ok(r) => match r.json::<Value>().await {
            Ok(v) => {
                let windows = parse_summary(&v);
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
    snapshot_for(ProviderId::Cursor, state, windows)
}
