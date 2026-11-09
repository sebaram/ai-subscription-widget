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
