//! Anti-entropy: periodic `MerkleSummary`-driven sync against routing-table
//! peers, bisect-on-mismatch via `MerkleDiff`, fetch missing records.
//!
//! Phase 1a stubs the scheduler; phase 1g lands the body.
//!
//! design-doc: §6 (replication & anti-entropy), §6.3 (sync RPC sequence).

pub(crate) mod merkle;
pub(crate) mod rpc;

use std::sync::Arc;

use super::Dht;

/// Per-relay anti-entropy state. Lives inside `Dht::merkle` (a
/// `parking_lot::RwLock<MerkleState>` per §9.3) — write-heavy because
/// every record write/delete updates the slice's leaf-to-root path.
///
/// design-doc: §6.1 (per-slice Merkle tree), §6.2 (slice boundaries).
#[derive(Debug)]
pub(crate) struct MerkleState {
    /// Per-slice trees, lazily allocated. The map is small in practice
    /// (§6.2 — `≈ 1` slice per relay at 10k-relay scale).
    pub trees: std::collections::HashMap<u8, merkle::SliceTree>,
}

impl MerkleState {
    pub(crate) fn empty() -> Self {
        Self { trees: std::collections::HashMap::new() }
    }
}

/// Spawn the anti-entropy scheduler — runs every
/// `ANTI_ENTROPY_INTERVAL_MS` against a randomly-chosen peer.
pub(crate) async fn run_scheduler(_dht: Arc<Dht>) {
    // TODO: phase 1g — `tokio::time::interval` loop, pick a peer
    // (preferring our ownership window, then random), call
    // `rpc::sync_with(peer, dht).await`.
    unimplemented!("phase 1g: anti-entropy scheduler");
}
