//! Push wire types: device → gateway registration and relay → gateway wake
//! (PUSH.md §3–4). Defined here now; the gateway's dispatch to APNs/FCM is a
//! later cut. The gateway holds `P → token`; relays hold `IPK → P` — neither
//! alone links a user to a wakeable device.

use serde::Deserialize;
use serde::Serialize;

use crate::types::bytes::Bytes;

/// Which platform wake service a token targets. The tag travels with every
/// registration so the gateway can add APNs / UnifiedPush as new dispatch arms
/// without a registry migration — the iOS-readiness pin (PUSH.md §6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PushProvider {
    Fcm,
    Apns,
    UnifiedPush,
}

/// Device registers `P → token` with the gateway (PUSH.md §3). Signed by the
/// device IPK so the gateway accepts only genuine registrations, yet the
/// gateway learns the token only under the pseudonym `P`, never the IPK.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterToken {
    /// Per-install push pseudonym `P` — random, not derivable from the IPK.
    pub pseudonym: Bytes<32>,
    pub provider:  PushProvider,
    /// Opaque platform token (FCM registration token / APNs device token /
    /// UnifiedPush endpoint URL). Variable length.
    pub token:     Vec<u8>,
    /// Device IPK signature over the registration.
    pub sig:       Bytes<64>,
}

/// A home relay asks the gateway to wake a device (PUSH.md §4). Only a relay
/// holding the recipient's queue initiates this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WakeRequest {
    /// Recipient's push pseudonym `P` (the relay holds `IPK → P`).
    pub pseudonym: Bytes<32>,
    /// Wake payload: the queued MLS ciphertext envelope (≤ push size limit) or
    /// a contentless pointer. The gateway forwards it blind.
    pub payload:   Vec<u8>,
}

/// One-RPC-per-bi-stream request the gateway unpacks (mirrors the resolver's
/// `ClientRequest`). `Register` arrives over `client/1`, `Wake` over
/// `relay/1`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PushRequest {
    Register(RegisterToken),
    Wake(WakeRequest),
}
