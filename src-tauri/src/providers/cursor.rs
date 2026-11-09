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
    fn cursor_plan_percent_from_used_and_limit_cents() {
        // the API's own totalPercentUsed (3.99) contradicts its cents here:
        // used $19.74 of limit $20.00 = 98.7% — that is what the dashboard shows
        let windows = parse_summary(&serde_json::from_str::<Value>(FIXTURE).unwrap());
        assert_eq!(windows.len(), 1);
        let month = &windows[0];
        assert_eq!(month.id, "month");
        assert!((month.used_percent.unwrap() - 98.7).abs() < 0.01);
        let q = month.quota.clone().unwrap();
        assert_eq!(q.used, "$19.74");
        assert_eq!(q.total, "$20.00");
        assert_eq!(month.resets_at.as_deref(), Some("2026-11-17T09:06:27.000Z"));
    }

    #[test]
    fn cursor_unlimited_is_informational_not_zero() {
        let v = serde_json::from_str::<Value>(r#"{"isUnlimited": true, "membershipType": "pro", "individualUsage": {"plan": {"enabled": true, "used": 10, "limit": 0}}}"#).unwrap();
        let windows = parse_summary(&v);
        assert!(windows.is_empty(), "unlimited → no fake meter");
    }

    #[test]
    fn cursor_empty_shape_is_no_data() {
        assert!(parse_summary(&json!({})).is_empty());
    }
}

use crate::model::{snapshot_for, QuotaInfo, UsageWindow, ProviderId, ProviderSnapshot, ProviderState};
use serde_json::Value;

fn parse_summary(v: &Value) -> Vec<UsageWindow> {
    // (live-verified 2026-11-09) percent = used/limit from cents fields; the
    // response's own totalPercentUsed disagreed with these numbers for this account.
    if v.get("isUnlimited").and_then(|b| b.as_bool()) == Some(true) {
        return vec![];
    }
    let plan = v.get("individualUsage").and_then(|i| i.get("plan"));
    let Some(plan) = plan else { return vec![] };
    let used = plan.get("used").and_then(|u| u.as_f64());
    let limit = plan.get("limit").and_then(|l| l.as_f64());
    let Some((used, limit)) = used.zip(limit) else { return vec![] };
    if limit <= 0.0 {
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
    let pace = window_minutes
        .as_ref()
        .and_then(|(m, _)| resets_at.as_deref().and_then(|r| crate::model::pace_expected(*m, r)));
    vec![UsageWindow {
        id: "month".into(),
        title: "mo".into(),
        window_minutes: window_minutes.as_ref().map(|(m, _)| *m),
        used_percent: Some((((used / limit) * 100.0).clamp(0.0, 100.0))),
        quota: Some(QuotaInfo {
            used: format!("${:.2}", used / 100.0),
            total: format!("${:.2}", limit / 100.0),
        }),
        resets_at: resets_at.clone(),
        pace_expected_percent: pace,
        is_known: true,
    }]
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
