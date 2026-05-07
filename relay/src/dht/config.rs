//! DHT-wide constants and the operator-tunable [`DhtConfig`].
//!
//! Constants are baked in here as `pub const` (§0 of `misc/specs/DHT.md`).
//! Anything that should be operator-tunable is on [`DhtConfig`] in the
//! relay's TOML; the rest is intentionally hard-coded so all relays in the
//! network agree on protocol parameters without per-deployment drift.
//!
//! design-doc: §0 (constants table), §11.8 (default `enabled = false`)

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Replication & lookup parameters
// ---------------------------------------------------------------------------

/// Replication factor — number of replicas per `(user_ipk → PresenceRecord)`.
///
/// design-doc: §0 (`k = 3`)
pub const K: usize = 3;

/// Lookup parallelism — concurrent `FindNode`/`FindValue` RPCs in flight
/// per iterative walk.
///
/// design-doc: §0 (`α = 3`)
pub const ALPHA: usize = 3;

/// Per-bucket capacity (k-bucket size).
///
/// design-doc: §0 (`B = 16`)
pub const BUCKET_SIZE: usize = 16;

/// Number of k-buckets in the routing table — one per leading-zero-bit class
/// of a 256-bit `NodeId`.
///
/// design-doc: §0 (`BUCKETS = 256`)
pub const BUCKETS: usize = 256;

// ---------------------------------------------------------------------------
// Lookup timing
// ---------------------------------------------------------------------------

/// Per-hop hedged-request delay. After this elapses with no reply we issue
/// the next candidate query in parallel.
///
/// design-doc: §0 (`LOOKUP_HEDGE_MS = 150`)
pub const LOOKUP_HEDGE_MS: u64 = 150;

/// Per-RPC timeout ceiling.
///
/// design-doc: §0 (`LOOKUP_RPC_TIMEOUT_MS = 1500`)
pub const LOOKUP_RPC_TIMEOUT_MS: u64 = 1500;

/// Maximum hops per iterative lookup.
///
/// design-doc: §0 (`LOOKUP_MAX_HOPS = 8`)
pub const LOOKUP_MAX_HOPS: u32 = 8;

// ---------------------------------------------------------------------------
// Presence record lifetimes
// ---------------------------------------------------------------------------

/// Presence record TTL — replicas reject records older than this past
/// `not_after`.
///
/// design-doc: §0 (`PRESENCE_TTL_MS = 600_000`, 10 minutes)
pub const PRESENCE_TTL_MS: u64 = 600_000;

/// Republish cadence — owning relay refreshes every record this often.
/// Must be `<<` `PRESENCE_TTL_MS`.
///
/// design-doc: §0 (`PRESENCE_REPUBLISH_MS = 240_000`, 4 minutes)
pub const PRESENCE_REPUBLISH_MS: u64 = 240_000;

/// Future-clock tolerance on signed `not_before`.
///
/// design-doc: §0 (`PRESENCE_MAX_FUTURE_SKEW_MS = 60_000`, 1 minute)
pub const PRESENCE_MAX_FUTURE_SKEW_MS: u64 = 60_000;

// ---------------------------------------------------------------------------
// Merkle / anti-entropy
// ---------------------------------------------------------------------------

/// Top-level slice prefix size in bits — slices the keyspace into
/// `2^MERKLE_SLICE_BITS = 256` equal regions.
///
/// design-doc: §0 (`MERKLE_SLICE_BITS = 8`)
pub const MERKLE_SLICE_BITS: u32 = 8;

/// Leaf granularity in bits — each Merkle leaf covers `2^MERKLE_LEAF_BITS`
/// keys within its slice.
///
/// design-doc: §0 (`MERKLE_LEAF_BITS = 16`)
pub const MERKLE_LEAF_BITS: u32 = 16;

/// Branching factor of the per-slice trie.
///
/// design-doc: §0 (`MERKLE_FANOUT = 16`, 4 bits per level)
pub const MERKLE_FANOUT: usize = 16;

/// Optional per-slice bloom filter bit width (operator visibility only).
///
/// design-doc: §0 (`BLOOM_BITS = 65_536`)
pub const BLOOM_BITS: u32 = 65_536;

/// Optional per-slice bloom filter hash count.
///
/// design-doc: §0 (`BLOOM_HASHES = 6`)
pub const BLOOM_HASHES: u32 = 6;

/// Anti-entropy pull cadence — how often we pull a `MerkleSummary` from a
/// random peer in our routing table.
///
/// design-doc: §0 (`ANTI_ENTROPY_INTERVAL_MS = 30_000`)
pub const ANTI_ENTROPY_INTERVAL_MS: u64 = 30_000;

/// Bucket-refresh staleness threshold.
///
/// design-doc: §0 (`BUCKET_REFRESH_MS = 3_600_000`, 1 hour)
pub const BUCKET_REFRESH_MS: u64 = 3_600_000;

// ---------------------------------------------------------------------------
// Wire-signature domain
// ---------------------------------------------------------------------------

/// Base domain prefix used to construct every DHT wire-signature transcript
/// (§1.1.1). Sub-domains (`-roam-v1`, `-presence-v1`, …) are appended at
/// signing time so a captured signature for one packet kind cannot be
/// replayed as another.
///
/// design-doc: §0 (`DHT_DOMAIN_PREFIX = "promtuz-dht-v1"`)
pub const DHT_DOMAIN_PREFIX: &[u8] = b"promtuz-dht-v1";

// ---------------------------------------------------------------------------
// Quorum and lookup-cache parameters
// ---------------------------------------------------------------------------

/// Minimum number of agreeing `FindValue` replies required to accept a
/// first-time lookup answer.
///
/// design-doc: §4.4 (cross-checking, `min_quorum = 2`)
pub const MIN_QUORUM: usize = 2;

/// Cap on the number of entries the lookup-result cache holds.
///
/// design-doc: §4.4 (cache for repeat recipients)
pub const LOOKUP_CACHE_CAP: usize = 4_096;

// ---------------------------------------------------------------------------
// RPC bounds (§2.6)
// ---------------------------------------------------------------------------

/// Maximum entries returned in a single `FetchRecord` request/response.
///
/// design-doc: §2.6 length bounds
pub const FETCH_RECORD_MAX: usize = 64;

/// Maximum entries packed into a single `MerkleDiff::Leaves` response.
///
/// design-doc: §2.6 length bounds
pub const MERKLE_DIFF_LEAVES_MAX: usize = 64;

/// Maximum depth of a `MerkleDiff::path` (radix-16 over 16-bit leaf space).
///
/// design-doc: §2.6 length bounds
pub const MERKLE_DIFF_PATH_MAX: usize = 4;

/// Maximum concurrent `FetchRecord` RPCs a fresh-joiner issues during
/// cold-join, to avoid DoSing neighbours.
///
/// design-doc: §7.3 (`Rate-limit: J caps concurrent FetchRecord to 8`)
pub const FETCH_RECORD_CONCURRENCY: usize = 8;

// ---------------------------------------------------------------------------
// Operator-tunable config (TOML-deserialisable)
// ---------------------------------------------------------------------------

/// Operator-tunable subset of the DHT parameters.
///
/// Only knobs that genuinely vary per-deployment live here — everything
/// else is a hard-coded `pub const` above (§0). The full design-doc rationale
/// for keeping protocol parameters out of operator config is "all relays in
/// the network must agree" — TOML drift would silently break routing.
///
/// design-doc: §10 (Phase 1 feature-flag), §11.8 (default `enabled = false`)
#[derive(Deserialize, Debug, Clone, Default)]
#[serde(deny_unknown_fields)]
pub struct DhtConfig {
    /// Master kill-switch. When `false`, the relay constructs no [`Dht`] and
    /// every code path that would touch one falls through to the
    /// pre-DHT behaviour.
    ///
    /// design-doc: §11.8 — Phase 1 default is `false`; flip to `true` only
    /// inside test deployments until Phase 3 cutover.
    ///
    /// [`Dht`]: crate::dht::Dht
    #[serde(default)]
    pub enabled: bool,

    /// Override of [`BUCKET_SIZE`] for testing. `None` means "use the
    /// constant" (the canonical production value).
    ///
    /// Allowing this to vary lets a test cluster run with a smaller bucket
    /// size to force eviction-path coverage with a tractable peer count.
    /// Production deployments should leave it unset.
    #[serde(default)]
    pub bucket_size: Option<usize>,
}

impl DhtConfig {
    /// Effective bucket size: the operator override if set, otherwise the
    /// canonical [`BUCKET_SIZE`].
    pub fn bucket_size(&self) -> usize {
        self.bucket_size.unwrap_or(BUCKET_SIZE)
    }
}
