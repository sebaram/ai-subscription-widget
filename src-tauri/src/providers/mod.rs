pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod ollama;

use crate::config::Config;
use crate::model::{ProviderId, ProviderSnapshot, ProviderState};

pub struct FetchCtx {
    pub cfg: Config,
    pub http: reqwest::Client,
    pub http_loopback: reqwest::Client,
}

impl FetchCtx {
    pub fn new(cfg: Config) -> Self {
        let http = reqwest::Client::builder()
            .user_agent("AIUsage/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .connect_timeout(std::time::Duration::from_secs(5))
            .build()
            .expect("reqwest client builds");
        let http_loopback = reqwest::Client::builder()
            .danger_accept_invalid_certs(true) // Antigravity's LSP uses a self-signed cert on loopback only
            .no_proxy()
            .timeout(std::time::Duration::from_secs(3))
            .connect_timeout(std::time::Duration::from_secs(3))
            .build()
            .expect("reqwest loopback client builds");
        Self { cfg, http, http_loopback }
    }

    pub fn disabled(&self, id: ProviderId) -> bool {
        !self.cfg.providers.iter().any(|p| p.id == id.slug() && p.enabled)
    }
}

/// One matching arm per provider, added by each provider task.
pub async fn fetch_snapshot(id: ProviderId, ctx: &FetchCtx) -> ProviderSnapshot {
    if ctx.disabled(id) {
        return crate::model::snapshot_for(id, ProviderState::Disabled, vec![]);
    }
    match id {
        ProviderId::Claude => claude::fetch(ctx).await,
        ProviderId::Codex => codex::fetch(ctx).await,
        ProviderId::Antigravity => antigravity::fetch(ctx).await,
        ProviderId::Ollama => ollama::fetch(ctx).await,
    }
}

// ---- token cache (review I4): refresh once per expiry, never re-play a rotated token

#[derive(Clone, Debug)]
pub struct CachedToken {
    pub access: String,
    pub refresh: String, // the rotated token once a refresh happened; empty = file token still in play
    pub expires_at_ms: u64,
}

pub enum CachedStep {
    CachedValid(String),
    CachedRotate(String),
    UseFile,
}

/// Decision for what to do with a cached token. The invariant: once we rotated,
/// the file's refresh token must never be used again (reuse can kill the token family).
pub fn first_token_step(cached: Option<CachedToken>, now_ms: u64) -> CachedStep {
    match cached {
        Some(t) if t.expires_at_ms > now_ms.saturating_add(30_000) => {
            CachedStep::CachedValid(t.access)
        }
        Some(t) if !t.refresh.is_empty() => CachedStep::CachedRotate(t.refresh),
        _ => CachedStep::UseFile,
    }
}

fn token_cache() -> &'static std::sync::Mutex<std::collections::HashMap<&'static str, CachedToken>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<&'static str, CachedToken>>> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

pub fn cached_token(provider: &'static str) -> Option<CachedToken> {
    token_cache().lock().unwrap_or_else(|e| e.into_inner()).get(provider).cloned()
}

pub fn store_token(provider: &'static str, access: String, refresh: String, expires_at_ms: u64) {
    token_cache().lock().unwrap_or_else(|e| e.into_inner()).insert(
        provider,
        CachedToken { access, refresh, expires_at_ms },
    );
}

pub fn now_epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(u64::MAX)
}

pub const AG_GEMINI_HINT: &str =
    "To track Antigravity with the IDE closed: install Gemini CLI once (`npm i -g @google/gemini-cli && gemini`), then enable Antigravity OAuth in Settings.";

/// Honest offline hint for the Antigravity card (review of design: never a bare 0%).
pub fn antigravity_offline_hint(oauth_enabled: bool, creds_exist: bool) -> Option<&'static str> {
    match (oauth_enabled, creds_exist) {
        (false, _) => Some(AG_GEMINI_HINT),
        (true, false) => Some("Run `gemini` once to create the Google login, then keep Antigravity OAuth on in Settings."),
        (true, true) => None, // remote path alive; its own errors carry messages
    }
}

#[cfg(test)]
mod auth_message_tests {
    use super::*;

    #[test]
    fn antigravity_offline_hint_matches_setup() {
        assert!(antigravity_offline_hint(false, false).unwrap().contains("gemini"));
        assert!(antigravity_offline_hint(false, true).unwrap().contains("gemini"));
        assert!(antigravity_offline_hint(true, false).unwrap().contains("Run `gemini`"));
        assert_eq!(antigravity_offline_hint(true, true), None);
    }
}

#[cfg(test)]
mod token_cache_tests {
    use super::*;

    fn tok(access: &str, refresh: &str, expires_in_ms: u64) -> CachedToken {
        CachedToken { access: access.into(), refresh: refresh.into(), expires_at_ms: now_epoch_ms() + expires_in_ms }
    }

    #[test]
    fn cached_fresh_token_is_reused_without_refresh() {
        let t = tok("fresh", "", 3_600_000);
        match first_token_step(Some(t.clone()), now_epoch_ms()) {
            CachedStep::CachedValid(a) => assert_eq!(a, "fresh"),
            _ => panic!("expected reuse"),
        }
    }

    #[test]
    fn expired_cached_token_rotates_with_cached_refresh_never_file() {
        // expired cache whose refresh was rotated: refresh again WITH the rotated token
        let t = tok("stale", "rotated-2", 0);
        match first_token_step(Some(t), now_epoch_ms() + 10_000) {
            CachedStep::CachedRotate(rt) => assert_eq!(rt, "rotated-2"),
            _ => panic!("expected rotation from cached token"),
        }
    }

    #[test]
    fn expired_cache_without_rotated_refresh_falls_back_to_file() {
        let t = tok("stale", "", 0);
        match first_token_step(Some(t), now_epoch_ms() + 10_000) {
            CachedStep::UseFile => {}
            _ => panic!("no rotated refresh: file path still valid"),
        }
    }

    #[test]
    fn no_cache_uses_file() {
        assert!(matches!(first_token_step(None, now_epoch_ms()), CachedStep::UseFile));
    }

    #[test]
    fn store_then_cached_roundtrip() {
        store_token("testprov", "a".into(), "r".into(), now_epoch_ms() + 1000);
        let c = cached_token("testprov").expect("stored");
        assert_eq!(c.access, "a");
        assert_eq!(c.refresh, "r");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A stalled loopback server must trip the per-request timeout, not hang forever
    /// (review C1 — a single frozen request used to freeze the whole widget).
    #[tokio::test]
    async fn loopback_stalled_socket_times_out() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        // accept and never respond
        std::thread::spawn(move || {
            let (_stream, _) = listener.accept().unwrap();
            std::thread::sleep(std::time::Duration::from_secs(10));
        });
        let ctx = FetchCtx::new(crate::config::defaults());
        let start = std::time::Instant::now();
        let result = ctx
            .http_loopback
            .get(format!("http://{addr}/"))
            .send()
            .await;
        assert!(result.is_err(), "stalled socket must error, not hang");
        assert!(start.elapsed() < std::time::Duration::from_secs(5), "timeout must be short, took {:?}", start.elapsed());
    }
}
