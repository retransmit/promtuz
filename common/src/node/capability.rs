//! CA-attested node capabilities (PUSH.md §1).
//!
//! A capability is a bit the RootCA stamps into a leaf cert's custom X.509
//! extension. Because it rides *inside* the CA-signed cert, anyone who
//! verifies the chain also verifies the capability — a node cannot
//! self-assert one, and there is no separate registry to forge against.
//!
//! This module owns the *semantic* — the bitset, the OID, and the
//! bytes↔bitset codec. The DER plumbing to embed or extract the extension
//! lives with whoever already parses certs: `certgen` writes it,
//! relay/gateway read it off an already-parsed `X509Certificate`.

use serde::Deserialize;
use serde::Serialize;

/// OID arcs for the capability extension. Self-issued private tag: it only has
/// to be unique inside our own closed CA (we are the sole issuer *and* the sole
/// verifier), so it is registered nowhere.
///
// ponytail: arbitrary private arc — swap freely, it's one const and never
// leaves our PKI.
pub const CAPABILITY_OID: &[u64] = &[1, 3, 6, 1, 4, 1, 58888, 1];

/// CA-attested capability bitset carried in a node's leaf cert (PUSH.md §1).
///
/// Extensible: add bits, never renumber. Trust (CA tier) and capability are
/// orthogonal — the tier says *how trusted*, these bits say *what it offers*.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeCapabilities(pub u32);

impl NodeCapabilities {
    pub const RELAY: u32 = 1 << 0; // basic store-and-forward (every relay)
    pub const PUSH_GATEWAY: u32 = 1 << 1; // holds APNs/FCM creds, runs the wake path
    pub const BLOB_STORE: u32 = 1 << 2; // content-addressed encrypted media
    pub const CALL_RELAY: u32 = 1 << 3; // SFrame / TURN for A/V
    pub const HIGH_AVAILABILITY: u32 = 1 << 4; // tier-1 stable-node SLA

    pub const fn empty() -> Self {
        Self(0)
    }

    /// Return a copy with `bit` set (chainable).
    pub const fn with(self, bit: u32) -> Self {
        Self(self.0 | bit)
    }

    /// True iff every bit in `bit` is set.
    pub const fn has(self, bit: u32) -> bool {
        self.0 & bit == bit
    }

    /// Bytes to embed as the extension content.
    ///
    // ponytail: plain 4-byte LE u32. If caps ever outgrow 32 bits or become a
    // struct, switch to postcard here + in `decode` and bump the format.
    pub fn encode(self) -> Vec<u8> {
        self.0.to_le_bytes().to_vec()
    }

    /// Parse the extension content back to a bitset. Strict: exactly 4 bytes,
    /// else `None` (a malformed extension yields no capabilities, never a
    /// wrong guess).
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        bytes.try_into().ok().map(|b| Self(u32::from_le_bytes(b)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_decode_round_trips() {
        let caps = NodeCapabilities::empty()
            .with(NodeCapabilities::PUSH_GATEWAY)
            .with(NodeCapabilities::HIGH_AVAILABILITY);
        assert_eq!(NodeCapabilities::decode(&caps.encode()), Some(caps));
    }

    #[test]
    fn has_checks_all_bits() {
        let caps = NodeCapabilities::empty().with(NodeCapabilities::PUSH_GATEWAY);
        assert!(caps.has(NodeCapabilities::PUSH_GATEWAY));
        assert!(!caps.has(NodeCapabilities::BLOB_STORE));
    }

    #[test]
    fn decode_rejects_wrong_length() {
        assert_eq!(NodeCapabilities::decode(&[]), None);
        assert_eq!(NodeCapabilities::decode(&[1, 2, 3]), None);
    }
}
