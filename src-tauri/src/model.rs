use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum ProviderId {
    Claude,
    Codex,
    Antigravity,
    Ollama,
}

impl ProviderId {
    pub fn all() -> [ProviderId; 4] {
        [Self::Claude, Self::Codex, Self::Antigravity, Self::Ollama]
    }

    pub fn slug(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Antigravity => "antigravity",
            Self::Ollama => "ollama",
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        Some(match s {
            "claude" => Self::Claude,
            "codex" => Self::Codex,
            "antigravity" => Self::Antigravity,
            "ollama" => Self::Ollama,
            _ => return None,
        })
    }

    /// (display name, brand color) — values pinned in the design spec.
    pub fn spec(self) -> (&'static str, &'static str) {
        match self {
            Self::Claude => ("Claude", "#d9775f"),
            Self::Codex => ("Codex", "#10a37f"),
            Self::Antigravity => ("Antigravity", "#4285f4"),
            Self::Ollama => ("Ollama", "#8b9ba8"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderState {
    Ok,
    AuthRequired,
    NotRunning,
    NoData,
    ParseFailed,
    Disabled,
}

#[derive(Serialize, Clone, Debug)]
pub struct QuotaInfo {
    pub used: String,
    pub total: String,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UsageWindow {
    pub id: String,
    pub title: String,
    pub window_minutes: Option<i64>,
    pub used_percent: Option<f64>,
    pub quota: Option<QuotaInfo>,
    pub resets_at: Option<String>,
    pub pace_expected_percent: Option<f64>,
    pub is_known: bool,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProviderSnapshot {
    pub id: ProviderId,
    pub name: String,
    pub brand_color: String,
    pub state: ProviderState,
    pub message: Option<String>,
    pub refreshed_at: String,
    pub windows: Vec<UsageWindow>,
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct UsageSnapshot {
    pub providers: Vec<ProviderSnapshot>,
    pub refreshed_at: String,
}

pub fn now_iso() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn snapshot_for(id: ProviderId, state: ProviderState, windows: Vec<UsageWindow>) -> ProviderSnapshot {
    let (name, brand_color) = id.spec();
    ProviderSnapshot {
        id,
        name: name.into(),
        brand_color: brand_color.into(),
        state,
        message: None,
        refreshed_at: now_iso(),
        windows,
    }
}

impl ProviderSnapshot {
    /// Attach a human explanation for states like auth-required / not-running.
    pub fn with_message(mut self, message: impl Into<String>) -> Self {
        self.message = Some(message.into());
        self
    }
}

/// Share of the window that has elapsed, in percent of the whole window.
pub fn pace_expected(window_minutes: i64, resets_at_iso: &str) -> Option<f64> {
    let resets = chrono::DateTime::parse_from_rfc3339(resets_at_iso).ok()?;
    if window_minutes <= 0 {
        return None;
    }
    let start = resets.with_timezone(&chrono::Utc) - chrono::Duration::minutes(window_minutes);
    let elapsed = (chrono::Utc::now() - start).num_minutes().clamp(0, window_minutes);
    Some(elapsed as f64 * 100.0 / window_minutes as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshot_json_uses_camel_case_keys() {
        let snap = UsageSnapshot {
            refreshed_at: "2026-11-09T10:00:00Z".into(),
            providers: vec![ProviderSnapshot {
                id: ProviderId::Claude,
                name: "Claude".into(),
                brand_color: "#d9775f".into(),
                state: ProviderState::Ok,
                message: Some("on pace".into()),
                refreshed_at: "2026-11-09T10:00:00Z".into(),
                windows: vec![UsageWindow {
                    id: "5h".into(),
                    title: "5h".into(),
                    window_minutes: Some(300),
                    used_percent: Some(42.0),
                    quota: Some(QuotaInfo { used: "$1".into(), total: "$2".into() }),
                    resets_at: Some("2026-11-09T15:46:00Z".into()),
                    pace_expected_percent: Some(58.0),
                    is_known: true,
                }],
            }],
        };
        let s = serde_json::to_string(&snap).unwrap();
        for key in ["usedPercent", "windowMinutes", "isKnown", "refreshedAt", "paceExpectedPercent", "message"] {
            assert!(s.contains(&format!("\"{key}\"")), "missing key {key} in {s}");
        }
        assert!(s.contains("\"auth-required\"") == false);
        assert!(s.contains("claude"));
    }

    #[test]
    fn state_json_is_kebab_case() {
        let s = serde_json::to_string(&ProviderState::AuthRequired).unwrap();
        assert_eq!(s, "\"auth-required\"");
        assert_eq!(serde_json::to_string(&ProviderState::NotRunning).unwrap(), "\"not-running\"");
    }

    #[test]
    fn slug_roundtrip_for_registry() {
        for id in [ProviderId::Claude, ProviderId::Codex, ProviderId::Antigravity, ProviderId::Ollama] {
            assert_eq!(ProviderId::from_slug(id.slug()), Some(id));
        }
        assert_eq!(ProviderId::from_slug("nope"), None);
    }

    #[test]
    fn pace_expected_half_window_is_50() {
        // window started 150 of 300 minutes ago ⇒ 50%
        let resets = (chrono::Utc::now() + chrono::Duration::minutes(150)).to_rfc3339();
        let e = pace_expected(300, &resets);
        let v = e.unwrap();
        assert!((49.0..51.0).contains(&v), "got {v}");
    }

    #[test]
    fn pace_expected_clamps() {
        let future = (chrono::Utc::now() + chrono::Duration::minutes(600)).to_rfc3339();
        assert_eq!(pace_expected(300, &future), Some(0.0)); // window not started yet
        assert_eq!(pace_expected(300, &"not-a-date".to_string()), None);
    }
}
