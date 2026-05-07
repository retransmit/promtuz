//! Server-side handlers for `MerkleSummary` / `MerkleDiff` /
//! `FetchRecord`, plus the client-side bisect driver.
//!
//! Phase 1a sketches the entry points; phase 1g lands them.
//!
//! design-doc: §2.4.6/§2.4.7/§2.4.8 (RPC types), §6.3 (sync sequence),
//! §7.3 (cold-join `FetchRecord` rate limit).

use std::sync::Arc;

use crate::dht::Dht;
use crate::dht::routing::RoutingEntry;

/// Pull-based anti-entropy round against `peer`. The whole sync
/// sequence (summary → diff → fetch → apply) runs to completion or
/// errors; the scheduler in `super::run_scheduler` drives one of these
/// per `ANTI_ENTROPY_INTERVAL_MS`.
///
/// design-doc: §6.3.
pub(crate) async fn sync_with(_dht: &Arc<Dht>, _peer: RoutingEntry) {
    // TODO: phase 1g —
    //   1. send MerkleSummary{ our_slices_bitset } -> peer,
    //   2. for each mismatching slice: bisect via MerkleDiff,
    //   3. for each diverging leaf: queue (ipk, peer_hash) pair,
    //   4. issue FetchRecord (capped at FETCH_RECORD_CONCURRENCY),
    //   5. validate (sig/ttl/owner) and apply via store::apply_record.
    unimplemented!("phase 1g: sync_with");
}
