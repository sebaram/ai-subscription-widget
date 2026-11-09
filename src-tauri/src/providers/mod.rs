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
            .build()
            .expect("reqwest client builds");
        let http_loopback = reqwest::Client::builder()
            .danger_accept_invalid_certs(true) // Antigravity's LSP uses a self-signed cert on loopback only
            .no_proxy()
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
