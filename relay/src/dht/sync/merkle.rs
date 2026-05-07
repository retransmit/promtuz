//! Per-slice radix-16 Merkle trie covering 16-bit leaves of `user_ipk`.
//!
//! Phase 1a defines `SliceTree::{new,insert,remove,root}` so phase 1d
//! (`store.rs::apply_record`) and phase 1g (anti-entropy `MerkleDiff`
//! handler) can share the same type. Bodies are phase 1g.
//!
//! design-doc: §6.1 (per-slice Merkle tree).

use crate::dht::config::MERKLE_FANOUT;
use crate::dht::config::MERKLE_LEAF_BITS;

/// One slice's worth of Merkle state.
///
/// The slice covers all `user_ipk`s whose top `MERKLE_SLICE_BITS == 8`
/// bits equal `slice_id`. Within the slice, the next `MERKLE_LEAF_BITS
/// == 16` bits are walked as 4 nibbles (`MERKLE_FANOUT == 16` →
/// 4-bits-per-level → depth 4) to a leaf.
///
/// The `dht_merkle` RocksDB CF caches every internal node hash so we
/// never re-traverse the entire trie on a single record write — only
/// the affected leaf-to-root path. This in-memory `SliceTree` is the
/// hot view; on relay restart it's lazily rebuilt from the CF.
///
/// design-doc: §6.1.
#[derive(Debug, Default)]
pub(crate) struct SliceTree {
    /// Top byte of `user_ipk` this tree covers (0..256).
    pub slice_id: u8,

    /// `BLAKE3` hash at the slice root. `[0u8; 32]` if the slice is empty
    /// (and therefore distinguishable from any populated state by
    /// `MerkleSummary`).
    pub root_hash: [u8; 32],

    // Internal nodes / leaves are stored in the `dht_merkle` CF so the
    // in-memory representation stays small. Phase 1g will likely add a
    // `dirty: HashSet<(level, idx)>` here for batched re-hashing.
}

impl SliceTree {
    /// Empty tree for `slice_id`. No allocations beyond the struct
    /// itself; populated lazily as records arrive (§6.2).
    pub(crate) fn new(slice_id: u8) -> Self {
        Self { slice_id, root_hash: [0u8; 32] }
    }

    /// Mark a record's leaf as containing `record_hash` and re-hash up
    /// the path to root.
    ///
    /// `user_ipk` must satisfy `user_ipk[0] == self.slice_id`.
    pub(crate) fn insert(&mut self, _user_ipk: &[u8; 32], _record_hash: &[u8; 32]) {
        // TODO: phase 1g — walk 4 nibbles, update affected internal-node
        // hashes, persist to `dht_merkle` CF.
        let _ = (MERKLE_FANOUT, MERKLE_LEAF_BITS); // silence-only: phase 1g uses these
        unimplemented!("phase 1g: SliceTree::insert");
    }

    /// Remove a record's leaf and re-hash the path. Used for tombstones.
    pub(crate) fn remove(&mut self, _user_ipk: &[u8; 32]) {
        // TODO: phase 1g.
        unimplemented!("phase 1g: SliceTree::remove");
    }

    /// Convenience accessor; cheaper than re-reading the RocksDB CF for
    /// each `MerkleSummary` reply.
    pub(crate) fn root(&self) -> [u8; 32] {
        self.root_hash
    }
}
