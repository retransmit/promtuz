//! 256-bit XOR primitive over the unified `NodeId` / `IPK` keyspace.
//!
//! [`xor32`] is the single canonical helper used everywhere a relay
//! (or resolver) needs to compute "distance between two 32-byte ids
//! under bitwise XOR" — Kademlia-style routing-table sorts, K-closest
//! ownership checks, and replica fan-out share this operation.
//!
//! The result is byte-array-comparable: a lex compare on the output
//! is equivalent to an unsigned big-endian compare on the 256-bit
//! distance (Kademlia XOR distance).
//!
//! Per-byte iteration (`std::array::from_fn`) compiles to the same
//! inlined SIMD shape as a hand-rolled loop and keeps the indices
//! within the array type, so `clippy::indexing_slicing` is satisfied
//! without an `#[allow]`.

/// 32-byte XOR distance between two ids, big-endian-comparable.
///
/// `xor32(a, b)[..]` lex-compares as the unsigned 256-bit distance
/// between `a` and `b`.
#[inline]
pub fn xor32(a: &[u8; 32], b: &[u8; 32]) -> [u8; 32] {
    std::array::from_fn(|i| a[i] ^ b[i])
}
