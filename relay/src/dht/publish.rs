//! Publish path: build a [`PresenceRecord`], find the k closest owners,
//! issue parallel `Store` RPCs, drive the §5.2 outcome state machine.
//!
//! Phase 1a stubs the entry point so the relay handshake (phase 1d/1f)
//! can already call `dht.publish(...).await` from a place that compiles.
//! The body lands in phase 1f.
//!
//! design-doc: §5.2 (publish path), §5.3 (conflict resolution rules
//! re-applied locally if we end up storing ourselves).

use std::sync::Arc;

use super::Dht;
use super::store::PresenceRecord;

/// Result of a single end-to-end publish attempt.
///
/// design-doc: §5.2 — collapses the per-replica `StoreResp` outcomes into
/// a caller-friendly aggregate. The publish path retries on `Failed`
/// (full re-walk + re-store, §5.2 last bullet); on `Stale` the caller
/// just logs (a higher-gen record is canonical).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PublishOutcome {
    /// At least `majority(K)` replicas reported `Stored`.
    Succeeded,

    /// The publish was outpaced by a higher-generation record from
    /// another publisher. Not retried.
    Stale,

    /// Could not reach majority — caller schedules a full re-walk + re-store.
    Failed,
}

/// Run the §5.2 publish workflow for a freshly-built `PresenceRecord`.
///
/// Caller is expected to have:
/// - obtained a fresh `user_sig` from the user inside the authenticated
///   handshake (§1.1.1),
/// - constructed the record with `not_before = now`,
///   `not_after = now + PRESENCE_TTL_MS`,
/// - signed `relay_sig` with [`Dht::signing_key`].
pub(crate) async fn publish(_dht: &Arc<Dht>, _record: PresenceRecord) -> PublishOutcome {
    // TODO: phase 1f — call `lookup::lookup_node(target = record.user_ipk)`
    // for a top-k shortlist, then issue parallel `Store` RPCs against
    // each, accumulate StoreResp outcomes per the §5.2 table.
    unimplemented!("phase 1f: publish-to-k-owners workflow");
}
