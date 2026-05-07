//! `(target_ipk → relay_descriptor)` lookup cache, lives on the *serving*
//! relay (not on libcore).
//!
//! Phase 1a defines the type so [`Dht::cache`] has something to point at;
//! phase 1f swaps this placeholder `HashMap`-with-cap for a proper LRU
//! once the lookup path lands and we know the access pattern.
//!
//! design-doc: §4.4 ("Cached answer for repeat recipients"), §9.1
//! (`cache.rs`).

use std::collections::HashMap;
use std::time::Instant;

use super::config::LOOKUP_CACHE_CAP;

/// Cached lookup answer. We memoize a *successful* §4.4-quorum result so
/// subsequent dispatches in the same conversation skip the DHT walk.
///
/// design-doc: §4.4 — TTL is `min(60s, time_until_record_not_after - 30s)`
/// at insert time; expiry is enforced by checking `expires_at` on every
/// `get`.
#[derive(Debug, Clone)]
pub(crate) struct CachedAnswer {
    /// Owning relay's NodeId (full 32 bytes).
    pub relay_id: [u8; 32],

    /// Owning relay's last-known socket address. Stored as a string-ish
    /// `(ip, port)` tuple so `SocketAddr` doesn't leak its `Display`
    /// dependency into this header. Phase 1e will likely replace this
    /// with a fuller `RelayDescriptor` once the lookup return type
    /// stabilises.
    pub relay_addr: std::net::SocketAddr,

    /// Owning relay's full Ed25519 pubkey (cert-pin material).
    pub relay_pubkey: [u8; 32],

    /// `Instant` after which this entry is considered stale and dropped on
    /// next access.
    pub expires_at: Instant,
}

/// Bounded `(user_ipk → CachedAnswer)` map.
///
/// **TODO: phase 1f — replace with proper LRU.** The naive `HashMap` here
/// has no recency tracking, so a busy relay can blow past `cap` and
/// arbitrarily evict on `insert`. The dispatch's design-doc rationale
/// for not adding the `lru` crate dependency in phase 1a is that the
/// access pattern (LRU vs LFU vs TTL-only) isn't pinned down yet.
#[derive(Debug)]
pub(crate) struct LookupCache {
    pub map: HashMap<[u8; 32], CachedAnswer>,
    pub cap: usize,
}

impl LookupCache {
    pub(crate) fn empty() -> Self {
        Self { map: HashMap::with_capacity(LOOKUP_CACHE_CAP.min(1024)), cap: LOOKUP_CACHE_CAP }
    }

    /// Look up an entry. Returns `Some` only if the entry exists *and*
    /// has not yet expired.
    pub(crate) fn get(&self, _user_ipk: &[u8; 32]) -> Option<&CachedAnswer> {
        // TODO: phase 1f — also drop expired entries opportunistically
        // (or via a background sweeper).
        unimplemented!("phase 1f: LookupCache::get");
    }

    /// Insert or refresh a cached answer.
    pub(crate) fn put(&mut self, _user_ipk: [u8; 32], _answer: CachedAnswer) {
        // TODO: phase 1f — LRU eviction once we add the `lru` dep.
        unimplemented!("phase 1f: LookupCache::put");
    }

    /// Drop a specific entry (e.g. on `FindValue` returning `NotPresent`
    /// for what the cache claimed was online).
    pub(crate) fn invalidate(&mut self, _user_ipk: &[u8; 32]) {
        // TODO: phase 1f.
        unimplemented!("phase 1f: LookupCache::invalidate");
    }
}
