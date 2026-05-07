//! Relay-side user-presence DHT.
//!
//! Top-level [`Dht`] struct wires the sub-systems together; the actual
//! routing-table / store / lookup / sync / publish logic lives in
//! sibling modules. Sub-lock layout follows §9.3 of the design doc.
//!
//! Phase 1a (this commit) ships the *shape* — module tree, struct
//! fields, public APIs, RocksDB column-family creation. Method bodies on
//! routing/lookup/store/etc. are stubbed so subsequent phases (1b–1h)
//! can drop in implementations without touching the wiring again.
//!
//! design-doc: `misc/specs/DHT.md`. Sections most relevant to this file:
//! §0 (constants), §1.2 (CFs), §1.3 (routing-table state), §9.1 (module
//! tree), §9.3 (Relay-struct sub-lock layout), §10 (Phase 1 = ship
//! feature-flagged off).

// Phase 1a is intentionally skeletal: the module tree, type signatures,
// and CF wiring exist but most bodies are `unimplemented!()`. Subsequent
// phases (1b–1h) populate them. Suppress dead-code warnings until those
// land — they're expected, and the noise drowns out real warnings.
#![allow(dead_code)]

pub(crate) mod bootstrap;
pub(crate) mod cache;
pub mod config;
pub(crate) mod handler;
pub(crate) mod lookup;
pub mod metrics;
pub(crate) mod publish;
pub(crate) mod routing;
pub(crate) mod store;
pub(crate) mod sync;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Context as _;
use anyhow::Result;
use common::quic::id::NodeId;
use ed25519_dalek::SigningKey;
use parking_lot::Mutex;
use parking_lot::RwLock;
use quinn::Connection;
use rust_rocksdb::ColumnFamilyDescriptor;
use rust_rocksdb::DB as RocksDB;
use rust_rocksdb::Options;

pub use config::DhtConfig;

use self::cache::LookupCache;
use self::metrics::Metrics;
use self::routing::RoutingTable;
use self::store::CF_DHT_MERKLE;
use self::store::CF_DHT_PRESENCE;
use self::sync::MerkleState;

/// Top-level DHT runtime state.
///
/// Lock granularity matches §9.3:
/// - [`routing`] — `RwLock<RoutingTable>`, read-mostly.
/// - [`merkle`] — `RwLock<MerkleState>`, write-heavy.
/// - [`cache`] — `Mutex<LookupCache>` (a `Mutex` because every cache
///   touch is a write — even reads bump LRU recency once phase 1f
///   swaps in a real LRU).
/// - [`peer_conns`] — `RwLock<HashMap<NodeId, Connection>>`, mirroring
///   the existing `Relay::clients` pattern.
///
/// All sub-locks are `parking_lot` and *never* held across `await` —
/// callers clone what they need out of the lock first (cf. the
/// project-wide rule documented at
/// `relay/src/quic/handler/client/events/forward.rs:59`).
///
/// design-doc: §9.3.
///
/// `routing`/`merkle`/`cache`/`peer_conns` are `pub(crate)` because only
/// in-relay code holds an `Arc<Dht>`; `node_id`/`signing_key`/`cfg`/
/// `metrics` are `pub` so admin tools (`bin/ldb.rs` and friends) can read
/// without going through accessor stubs.
#[derive(Debug)]
pub struct Dht {
    /// 256-bucket routing table (§3.1).
    pub(crate) routing: RwLock<RoutingTable>,

    /// Shared `RocksDB` handle (the *same* DB the relay's message queue
    /// uses — column families, not databases, separate the two domains).
    /// `rust-rocksdb` is internally concurrency-safe with the
    /// `multi-threaded-cf` feature already declared in `relay/Cargo.toml`.
    ///
    /// CF handles are looked up via [`RocksDB::cf_handle`] at use-time
    /// rather than cached on the struct because the returned
    /// `Arc<BoundColumnFamily<'_>>` is lifetime-tied to the DB and
    /// can't be stored back as a struct field without painful self-refs.
    pub(crate) rocks: Arc<RocksDB>,

    /// Per-slice Merkle anti-entropy state (§6).
    pub(crate) merkle: RwLock<MerkleState>,

    /// `(target_ipk → relay_descriptor)` cache for repeat lookups (§4.4).
    pub(crate) cache: Mutex<LookupCache>,

    /// Hot relay-to-relay connections, keyed by remote `NodeId`. Strong
    /// reference held here; routing-table entries hold a `Weak` (§1.3).
    pub(crate) peer_conns: RwLock<HashMap<NodeId, Connection>>,

    /// This relay's NodeId — `BLAKE3(NodeKey)` (§0).
    pub node_id: NodeId,

    /// This relay's identity signing key. Used to sign `relay_sig` on
    /// every outgoing `PresenceRecord` (§1.1.1) and tombstones.
    /// Distinct from the TLS server key (`Relay::keys::signing` is the
    /// one and only identity key — see the doc comment on
    /// `relay/src/relay/mod.rs::RelayKeys`).
    pub signing_key: SigningKey,

    /// Local copy of the runtime config so DHT code paths don't have to
    /// reach back into `Relay::cfg`.
    pub cfg: DhtConfig,

    /// Aggregate operation counters (§9.1).
    pub metrics: Metrics,
}

impl Dht {
    /// Construct the runtime DHT state.
    ///
    /// Idempotently opens the `dht_presence` and `dht_merkle` column
    /// families on the supplied `RocksDB` path. The same DB instance the
    /// caller already opened for the message queue is reused; CFs
    /// separate the two key-spaces (§1.2). "Already exists" is **not**
    /// an error — the relay-restart case is exactly that.
    ///
    /// **Important:** `rocks` must already have been opened with the
    /// `dht_presence` and `dht_merkle` CFs declared. We don't take an
    /// owned `DB` handle and reopen it because the message-queue side
    /// is already using the same handle and reopening would invalidate
    /// outstanding iterators. See `crate::util::rocksdb::rocksdb` —
    /// in this phase we extend that to declare the CFs up front, so
    /// `Dht::new` only verifies the CFs are present and stashes the
    /// shared `Arc<DB>`.
    pub fn new(
        node_id: NodeId, signing_key: SigningKey, cfg: DhtConfig, rocks: Arc<RocksDB>,
    ) -> Result<Self> {
        // Verify the required CFs were declared at DB-open time. If the
        // relay was started against an old DB without the CFs, surface
        // a clear error rather than panicking deep inside a put.
        rocks
            .cf_handle(CF_DHT_PRESENCE)
            .with_context(|| format!("missing column family `{CF_DHT_PRESENCE}` in DB"))?;
        rocks
            .cf_handle(CF_DHT_MERKLE)
            .with_context(|| format!("missing column family `{CF_DHT_MERKLE}` in DB"))?;

        Ok(Self {
            routing: RwLock::new(RoutingTable::empty(node_id)),
            rocks,
            merkle: RwLock::new(MerkleState::empty()),
            cache: Mutex::new(LookupCache::empty()),
            peer_conns: RwLock::new(HashMap::new()),
            node_id,
            signing_key,
            cfg,
            metrics: Metrics::new(),
        })
    }
}

/// Helper: the DHT-specific column family descriptors to pass into
/// `RocksDB::open_cf_descriptors` at relay-startup time.
///
/// Used by `crate::util::rocksdb` so DB-open and DHT-init aren't two
/// places that have to stay in sync about which CFs exist.
///
/// design-doc: §1.2.
pub fn dht_cf_descriptors() -> Vec<ColumnFamilyDescriptor> {
    // No prefix extractor on either CF — point lookups only on
    // `dht_presence` (32-byte keys), and `dht_merkle` keys are 3 bytes
    // (slice/level/index) which would be malformed under a 32-byte
    // prefix extractor anyway (§1.2 trade-off note).
    let presence_opts = Options::default();
    let merkle_opts = Options::default();
    vec![
        ColumnFamilyDescriptor::new(CF_DHT_PRESENCE, presence_opts),
        ColumnFamilyDescriptor::new(CF_DHT_MERKLE, merkle_opts),
    ]
}
