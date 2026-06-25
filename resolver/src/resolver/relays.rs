use std::sync::Arc;
use std::time::Instant;

use common::proto::RelayId;
use common::proto::client_res::RelayDescriptor;
use common::types::bytes::Bytes;
use parking_lot::Mutex;
use quinn::Connection;

/// Per-relay registry entry held under the resolver's `relays` map.
///
/// `last_heartbeat_at` is the resolver's local-clock observation of the
/// most recent authenticated `RelayHello`/`RelayHeartbeat` from this
/// relay. It is used as the recency proxy for the `rtt_near` ranking in
/// [`ClientRequest::GetBootstrapPeers`]: until the resolver tracks
/// per-relay RTT directly (Vivaldi-or-similar, future work),
/// most-recently-heard-from is the best signal of "this relay has
/// good network position towards us."
///
/// Stored as `Instant` rather than ms-since-epoch so the recency
/// comparison is monotonic regardless of wall-clock jumps. Wrapped in a
/// `Mutex` so the heartbeat path can update it under the registry's
/// outer `RwLock` *read* guard — a recency bump is per-entry-local state
/// that doesn't need to gate every other reader on the map.
///
/// [`ClientRequest::GetBootstrapPeers`]: common::proto::client_res::ClientRequest::GetBootstrapPeers
#[derive(Debug, Clone)]
pub struct RelayEntry {
    pub id: RelayId,
    pub conn: Arc<Connection>,
    /// Relay's full Ed25519 identity public key, captured from the
    /// authenticated `RelayHello` at registration time. Carried so the
    /// resolver can include it in [`RelayDescriptor`] responses without
    /// re-deriving from the cert chain on every `GetRelays` /
    /// `GetBootstrapPeers` call. See `RelayDescriptor::pubkey` doc for
    /// why bootstrap consumers need this.
    pub pubkey: Bytes<32>,
    /// Instant of the last authenticated lifetime packet
    /// (`RelayHello` or `RelayHeartbeat`). Wrapped in `Arc<Mutex<...>>`
    /// so heartbeat-driven updates don't require the outer registry
    /// `RwLock` to be taken in write mode.
    pub last_heartbeat_at: Arc<Mutex<Instant>>,
}

impl RelayEntry {
    pub fn new(id: RelayId, conn: Arc<Connection>, pubkey: Bytes<32>) -> Self {
        Self {
            id,
            conn,
            pubkey,
            last_heartbeat_at: Arc::new(Mutex::new(Instant::now())),
        }
    }

    pub fn to_descriptor(&self) -> RelayDescriptor {
        RelayDescriptor {
            id:     self.id,
            addr:   self.conn.remote_address(),
            pubkey: self.pubkey,
        }
    }

    /// Latest observation of this relay's liveness, as an [`Instant`].
    /// Cloned out of the per-entry `Mutex` so callers don't hold the
    /// lock across whatever they do next.
    pub fn last_heartbeat_at(&self) -> Instant {
        *self.last_heartbeat_at.lock()
    }

    /// Update [`Self::last_heartbeat_at`] to `now`. Called from the
    /// authenticated `RelayHeartbeat` path. The update is unconditional
    /// — the caller has already verified the heartbeat is fresh and
    /// well-signed (`Resolver::verify_heartbeat`), so an out-of-order
    /// arrival should still bump recency: it's a strictly newer
    /// observation than whatever was stored before.
    pub fn touch_heartbeat(&self, now: Instant) {
        *self.last_heartbeat_at.lock() = now;
    }
}
