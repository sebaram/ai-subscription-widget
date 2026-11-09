use crate::model::{snapshot_for, ProviderId, ProviderSnapshot, ProviderState};
use crate::providers::FetchCtx;

/// Skeleton until the Ollama provider task lands the real fetcher.
pub async fn fetch(_ctx: &FetchCtx) -> ProviderSnapshot {
    snapshot_for(ProviderId::Ollama, ProviderState::NoData, vec![])
}
