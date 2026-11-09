use crate::model::{snapshot_for, ProviderId, ProviderSnapshot, ProviderState};
use crate::providers::FetchCtx;

/// Skeleton until the Antigravity provider task lands the real fetcher.
pub async fn fetch(_ctx: &FetchCtx) -> ProviderSnapshot {
    snapshot_for(ProviderId::Antigravity, ProviderState::NoData, vec![])
}
