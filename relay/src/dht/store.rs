//! On-disk presence-record persistence, conflict resolution, and CF
//! lifecycle.
//!
//! Phase 1a wires the *names* of column families and exposes the
//! [`PresenceRecord`] shape so dependent phases can compile against a
//! stable surface; the body of `store`/`fetch`/`apply` is left to phase 1d.
//!
//! design-doc: §1.1 (PresenceRecord), §1.2 (RocksDB column families),
//! §5.3 (multi-writer conflict resolution).

use common::types::bytes::Bytes;
use serde::Deserialize;
use serde::Serialize;

/// Column-family name for the `(user_ipk → PresenceRecord)` map this relay
/// holds as a replica.
///
/// design-doc: §1.2 — keyed by `[u8; 32]` user IPK; values are
/// postcard-encoded `PresenceRecord`. No prefix extractor (point lookups
/// only).
pub const CF_DHT_PRESENCE: &str = "dht_presence";

/// Column-family name for cached internal Merkle-tree node hashes.
///
/// design-doc: §1.2 — keys are `merkle_key = slice_id(1) || level(1) ||
/// index_within_level(1)`, values are 32-byte BLAKE3 hashes.
pub const CF_DHT_MERKLE: &str = "dht_merkle";

/// User-presence record exchanged between relays.
///
/// Wire/storage layout per §1.1: postcard-encoded, two signatures (user +
/// relay) plus generation/TTL bookkeeping. Phase 1b will swap this in for
/// the wire-protocol version (likely with `Bytes<N>` newtypes); phase 1a
/// only needs the field shape so `store.rs`/`publish.rs`/`sync.rs` can
/// agree on a type name.
///
/// design-doc: §1.1 (`PresenceRecord`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PresenceRecord {
    /// Ed25519 identity public key of the user this record is about.
    pub user_ipk: Bytes<32>,

    /// Owning relay's NodeId (full 32 bytes — `BLAKE3(NodeKey)`).
    pub relay_id: Bytes<32>,

    /// Owning relay's full Ed25519 pubkey, so verifiers don't need a side
    /// channel (NodeId alone is non-invertible).
    pub relay_pubkey: Bytes<32>,

    /// Milliseconds since UNIX epoch — the bottom of this record's
    /// validity window.
    pub not_before: u64,

    /// Milliseconds since UNIX epoch — set by the publisher to
    /// `not_before + PRESENCE_TTL_MS`.
    pub not_after: u64,

    /// User-supplied monotonic counter. The conflict-resolution primary
    /// key (§5.3): a roam to a new relay strictly increments `generation`
    /// so a captured signature for the previous relay loses.
    pub generation: u64,

    /// Bitset for future use; v1 = 0.
    pub capabilities: u16,

    /// User's Ed25519 sig over the *user-roam* transcript:
    /// `DHT_DOMAIN_PREFIX || "-roam-v1" || PROTOCOL_VERSION (BE u16) ||
    /// user_ipk || relay_id || generation (BE u64)`.
    ///
    /// design-doc: §1.1.1.
    pub user_sig: Bytes<64>,

    /// Relay's Ed25519 sig over the wire-canonical bytes of every other
    /// field, prefixed with
    /// `DHT_DOMAIN_PREFIX || "-presence-v1" || PROTOCOL_VERSION`.
    ///
    /// design-doc: §1.1.1.
    pub relay_sig: Bytes<64>,
}

/// Tombstone record — published when an owning relay deliberately deletes
/// a presence (user disconnected). Honoured by replicas for
/// `2 × PRESENCE_TTL_MS` to prevent zombie-record resurrection.
///
/// Stored under the `tombstone_<user_ipk>` key prefix in [`CF_DHT_PRESENCE`]
/// (§1.2).
///
/// design-doc: §1.2 (Tombstones), §6.3 (anti-entropy applies same conflict
/// rules to tombstones).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TombstoneRecord {
    /// Same key shape as [`PresenceRecord::user_ipk`].
    pub user_ipk: Bytes<32>,

    /// Generation of the record being tombstoned. Replicas use the §5.3
    /// total order to decide whether the tombstone supersedes a record
    /// they may already hold.
    pub generation: u64,

    /// Milliseconds since UNIX epoch when the originating relay decided
    /// to delete.
    pub deleted_at: u64,

    /// Owning relay's NodeId.
    pub relay_id: Bytes<32>,

    /// Owning relay's full pubkey (same rationale as
    /// [`PresenceRecord::relay_pubkey`]).
    pub relay_pubkey: Bytes<32>,

    /// Relay signature over a tombstone-specific transcript (phase 1b
    /// will define the exact bytes).
    pub relay_sig: Bytes<64>,
}

/// Storage outcome reported back through `StoreResp` (§2.4.4).
///
/// Re-exported here for the in-process callers that need to distinguish
/// "stored locally" from "rejected because we're not in the k closest" —
/// the wire-level enum is duplicated in `dht_p2p.rs` (phase 1b).
///
/// design-doc: §2.4.4 (`StoreOutcome`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StoreOutcome {
    Stored,
    Stale,
    NotOwner,
    BadSig,
    TtlExpired,
    RateLimited,
}

/// Apply a freshly-arrived `PresenceRecord` against whatever this replica
/// already holds, using the §5.3 conflict-resolution rules.
///
/// Phase 1d will fill in the body. The signature is stable so phase 1e
/// (lookup) and phase 1f (publish) can call into it from their respective
/// code paths.
pub(crate) fn apply_record(_record: PresenceRecord) -> StoreOutcome {
    // TODO: phase 1d — generation > then not_before > then lex relay_id.
    unimplemented!("phase 1d: PresenceRecord conflict resolution");
}

/// Apply a freshly-arrived `TombstoneRecord` against whatever this replica
/// already holds.
pub(crate) fn apply_tombstone(_tombstone: TombstoneRecord) -> StoreOutcome {
    // TODO: phase 1d — same total order; tombstone supersedes any record
    // with the same `(user_ipk, generation)`.
    unimplemented!("phase 1d: TombstoneRecord application");
}
