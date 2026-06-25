//! `(user_ipk → relay_descriptor)` lookup cache, lives on the *serving*
//! relay (not on libcore). Memoizes a successful quorum `FindValue` result
//! so repeat dispatches to the same recipient skip the DHT walk.
//!
//! Bounded LRU with lazy TTL expiry, no external dependency: recency is a
//! monotonic stamp per entry; a full insert evicts the least-recently-used
//! entry; expiry is checked on `get` (a stale entry is dropped, never
//! returned).

use std::collections::HashMap;
use std::time::Instant;

use super::config::LOOKUP_CACHE_CAP;

/// Cached lookup answer: where a given user's presence record said its
/// home relay is. Expiry is enforced by checking `expires_at` on every
/// `get`.
#[derive(Debug, Clone)]
pub(crate) struct CachedAnswer {
    /// Owning relay's NodeId (full 32 bytes).
    pub relay_id: [u8; 32],

    /// Owning relay's last-known socket address.
    pub relay_addr: std::net::SocketAddr,

    /// Owning relay's full Ed25519 pubkey (cert-pin material).
    pub relay_pubkey: [u8; 32],

    /// Instant after which this entry is stale and dropped on next access.
    pub expires_at: Instant,
}

/// Bounded LRU of `user_ipk → CachedAnswer` with lazy TTL expiry.
#[derive(Debug)]
pub(crate) struct LookupCache {
    map: HashMap<[u8; 32], Entry>,
    cap: usize,
    /// Monotonic recency counter; the entry with the smallest stamp is the
    /// least-recently-used.
    seq: u64,
}

#[derive(Debug)]
struct Entry {
    answer: CachedAnswer,
    used:   u64,
}

impl LookupCache {
    pub(crate) fn empty() -> Self {
        Self {
            map: HashMap::with_capacity(LOOKUP_CACHE_CAP.min(1024)),
            cap: LOOKUP_CACHE_CAP,
            seq: 0,
        }
    }

    fn tick(&mut self) -> u64 {
        self.seq += 1;
        self.seq
    }

    /// Fetch a still-fresh entry, bumping its recency. Returns `None` if the
    /// key is absent or has expired (an expired entry is dropped in passing,
    /// so it never lingers).
    pub(crate) fn get(&mut self, user_ipk: &[u8; 32], now: Instant) -> Option<CachedAnswer> {
        // Resolve existence + expiry first so no immutable borrow is held
        // across the `tick()` mutation below.
        match self.map.get(user_ipk) {
            None => return None,
            Some(e) if e.answer.expires_at <= now => {
                self.map.remove(user_ipk);
                return None;
            },
            Some(_) => {},
        }
        let stamp = self.tick();
        let e = self.map.get_mut(user_ipk).expect("present: just checked");
        e.used = stamp;
        Some(e.answer.clone())
    }

    /// Insert or refresh an answer. When a *new* key would exceed `cap`, the
    /// least-recently-used entry is evicted first.
    pub(crate) fn put(&mut self, user_ipk: [u8; 32], answer: CachedAnswer) {
        let stamp = self.tick();
        if !self.map.contains_key(&user_ipk) && self.map.len() >= self.cap
            && let Some(lru) = self.map.iter().min_by_key(|(_, e)| e.used).map(|(k, _)| *k)
        {
            self.map.remove(&lru);
        }
        self.map.insert(user_ipk, Entry { answer, used: stamp });
    }

    /// Drop a specific entry — e.g. a live `FindValue` returned `NotPresent`
    /// for what the cache claimed was online.
    pub(crate) fn invalidate(&mut self, user_ipk: &[u8; 32]) {
        self.map.remove(user_ipk);
    }
}
