use crate::config;
use crate::model::{now_iso, ProviderId, ProviderSnapshot, UsageSnapshot};
use crate::providers::FetchCtx;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::AppHandle;
use tauri::Emitter;
use tokio::sync::Mutex;

pub const OLLAMA_MIN_POLL: u64 = 60; // ollama.com API allows 10 req/min — stay far below

pub struct SnapshotCache(pub Mutex<HashMap<String, ProviderSnapshot>>);

fn provider_interval(cfg: &config::Config, slug: &str) -> u64 {
    let mut secs = cfg.per_provider_poll.get(slug).copied().unwrap_or(cfg.poll_seconds);
    if slug == "ollama" {
        secs = secs.max(OLLAMA_MIN_POLL);
    }
    secs.max(1)
}

async fn fetch_due(
    app: &AppHandle,
    cache: &Mutex<HashMap<String, ProviderSnapshot>>,
    due: &[String],
) {
    let cfg = config::load();
    let ctx = FetchCtx::new(cfg.clone());
    {
        let mut guard = cache.lock().await;
        for slug in due {
            let Some(id) = ProviderId::from_slug(slug) else { continue };
            let snap = crate::providers::fetch_snapshot(id, &ctx).await;
            guard.insert(slug.clone(), snap);
        }
    }
    let guard = cache.lock().await;
    let providers: Vec<ProviderSnapshot> = cfg
        .providers
        .iter()
        .filter(|pc| pc.enabled)
        .filter_map(|pc| guard.get(&pc.id).cloned())
        .collect();
    drop(guard);
    let snap = UsageSnapshot { providers, refreshed_at: now_iso() };
    let _ = app.emit("usage-snapshot", &snap);
}

/// Refresh every enabled provider immediately and emit (uses the shared cache).
pub async fn refresh_and_emit(app: &AppHandle, cache: &Mutex<HashMap<String, ProviderSnapshot>>) {
    let cfg = config::load();
    let due: Vec<String> = cfg
        .providers
        .iter()
        .filter(|pc| pc.enabled)
        .map(|pc| pc.id.clone())
        .collect();
    fetch_due(app, cache, &due).await;
}

pub fn start(app: AppHandle, cache: Arc<SnapshotCache>) {
    tauri::async_runtime::spawn(async move {
        let mut last: HashMap<String, Instant> = HashMap::new();
        loop {
            // config is re-read each tick, so config edits reshape the schedule live
            let cfg = config::load();
            let mut due: Vec<String> = Vec::new();
            for pc in &cfg.providers {
                if !pc.enabled {
                    continue;
                }
                let interval = Duration::from_secs(provider_interval(&cfg, &pc.id));
                if last.get(&pc.id).is_none_or(|t| t.elapsed() >= interval) {
                    last.insert(pc.id.clone(), Instant::now());
                    due.push(pc.id.clone());
                }
            }
            if !due.is_empty() {
                fetch_due(&app, &cache.0, &due).await;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}
