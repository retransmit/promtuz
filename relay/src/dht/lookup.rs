//! Iterative `FindNode` / `FindValue` walks with α=3 parallelism and
//! per-hop hedging.
//!
//! Phase 1a defines the public surface (`lookup_node`, `lookup_value`) so
//! the `forward.rs` integration in phase 3-prep and the publish path
//! (phase 1f) can see a stable signature; the iterative loop body is
//! phase 1e.
//!
//! design-doc: §4 (lookup protocol), §4.1 (FindNode iterative algorithm),
//! §4.2 (FindValue), §4.3 (termination), §4.4 (Sybil cross-check).

use std::sync::Arc;

use common::quic::id::NodeId;

use super::Dht;
use super::store::PresenceRecord;

/// Outcome of an iterative `FindValue` walk.
///
/// design-doc: §4.2 (Found / NotPresent / Closer collapsed into a
/// caller-friendly trichotomy).
//
// Variant size: `Found(PresenceRecord)` is ~250 B while `NotPresent`/`Failed`
// are zero-sized. Boxing `PresenceRecord` would shrink the enum but every
// caller in the lookup path then needs an extra deref; phase 1e will revisit
// once the access pattern is concrete.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum FindValueOutcome {
    /// We collected a quorum of agreeing replies (`MIN_QUORUM`); the
    /// returned record is the picked-by-RTT winner.
    Found(PresenceRecord),

    /// The k closest peers all responded with `NotPresent`. Authoritative
    /// "user is offline."
    NotPresent,

    /// Lookup failed (network partition, attack, max hops exceeded). The
    /// caller falls back to a delayed retry per §4.3.
    Failed,
}

/// Iterative `FindNode` walk to discover the k closest peers to `target`.
///
/// Used by:
/// - bootstrap's "self-FindNode" forced-convergence step (§3.5),
/// - the publish path to find STORE recipients (§5.2),
/// - bucket-refresh to re-discover stale ranges (§3.2).
///
/// Returns the top-k peers by XOR distance the walk converged on.
pub(crate) async fn lookup_node(
    _dht: &Arc<Dht>, _target: NodeId,
) -> Vec<crate::dht::routing::RoutingEntry> {
    // TODO: phase 1e — α=3 parallel iterative walk with LOOKUP_HEDGE_MS
    // hedging, terminating per §4.3 case (1) or (2). For now phase 1a
    // returns an empty vec is wrong (callers would silently behave as if
    // the network were empty), so explicitly panic until the body lands.
    unimplemented!("phase 1e: iterative FindNode walk");
}

/// Iterative `FindValue` walk for `user_ipk`.
///
/// design-doc: §4.2, §4.4 (cross-check requires `MIN_QUORUM` agreeing
/// replies on `(generation, relay_id)`).
pub(crate) async fn lookup_value(_dht: &Arc<Dht>, _user_ipk: [u8; 32]) -> FindValueOutcome {
    // TODO: phase 1e — same iterative loop as lookup_node but each hop
    // sends `FindValue { user_ipk }` and respects the §4.4 quorum check.
    unimplemented!("phase 1e: iterative FindValue walk");
}
