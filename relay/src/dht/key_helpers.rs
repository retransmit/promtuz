//! Shared DHT-key construction helpers.
//!
//! Centralises the small primitives that are otherwise re-implemented
//! across `mls_kp` / `mls_welcome` / future stash-flavoured handlers.
//! All of these compose into the unified 32-byte `NodeId` keyspace
//! (`misc/specs/DHT.md` §0) so the DHT routing layer doesn't need
//! to know which sub-namespace it's serving.

use common::quic::id::NodeId;

/// 32-byte stash key for `(domain, ipk)`.
///
/// Computes `BLAKE3(domain || ipk)` and returns the full digest as a
/// `[u8; 32]`. Callers pass a short literal `domain` (e.g. `b"kp:"` or
/// `b"welcome:"`) to namespace their stash entries away from presence
/// records (which use bare `ipk` as the key) and from each other.
///
/// Implemented via [`NodeId::new`] (which is `BLAKE3` of the input
/// bytes) over the concatenation `domain || ipk`. Same idiom used in
/// `dht::sync::merkle::hash_leaf` so we stay consistent with the
/// codebase rather than pulling `blake3` in as a direct relay dep.
pub fn stash_prefix(domain: &[u8], ipk: &[u8; 32]) -> [u8; 32] {
    let mut buf = Vec::with_capacity(domain.len() + ipk.len());
    buf.extend_from_slice(domain);
    buf.extend_from_slice(ipk);
    *NodeId::new(&buf).as_bytes()
}
