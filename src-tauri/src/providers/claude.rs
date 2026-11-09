use crate::model::{snapshot_for, ProviderId, ProviderSnapshot, ProviderState};
use crate::providers::FetchCtx;

/// Skeleton until the Claude provider task lands the real fetcher.
pub async fn fetch(_ctx: &FetchCtx) -> ProviderSnapshot {
    snapshot_for(ProviderId::Claude, ProviderState::NoData, vec![])
}
