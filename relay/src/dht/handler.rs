//! Inbound `peer/1` connection dispatcher.
//!
//! Replaces the old `relay/src/quic/handler/peer.rs` no-op stub with a
//! single funnel into the DHT's RPC handlers. Phase 1a sets up the
//! function signature so `quic/handler/peer.rs` can call it; phase 1b/1c
//! fills in the per-stream per-RPC dispatch.
//!
//! design-doc: §2.3 (ALPN reuse: `peer/1` = relay-to-relay), §9 (DHT
//! integration with the existing `handle_peer` stub).

use std::sync::Arc;

use quinn::Connection;

use super::Dht;

/// Drive a single inbound `peer/1` connection through its full lifetime.
///
/// The caller (`relay/src/quic/handler/peer.rs`) has already verified the
/// ALPN role and the cert chain via the QUIC handshake — anything that
/// reaches us here is a TLS-validated relay. From this point our job is:
///
/// 1. derive the peer's `NodeId = BLAKE3(spki_pubkey)` from the leaf cert,
/// 2. insert it into the routing table (§3.4 path 1),
/// 3. accept bi-streams in a loop and dispatch each one to the
///    appropriate `dht_p2p` RPC handler (phase 1b adds the wire types).
///
/// design-doc: §2.3 (no separate `RelayHello` for relay-to-relay — the
/// cert chain already proved the binding).
pub(crate) async fn handle_peer_connection(_dht: Arc<Dht>, _conn: Connection) {
    // TODO: phase 1b/1c —
    //   - extract cert SPKI, derive NodeId, verify against requester's
    //     self-claimed id (when carried in inbound RPCs),
    //   - loop on `accept_bi`, demultiplex `DhtPacket` (phase 1b types),
    //   - dispatch into `lookup`/`store`/`sync::rpc` handlers,
    //   - on `Connection::closed()`, evict the routing-table entry only
    //     if it still points at this exact `Connection` (`stable_id`
    //     race-guard, mirroring `remove_client_if_same` at
    //     `relay/src/quic/handler/client/mod.rs:43-52`).
    todo!("phase 1b/1c: peer/1 inbound RPC dispatch");
}
