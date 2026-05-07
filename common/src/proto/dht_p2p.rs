//! DHT Relay-to-Relay Wire Protocol
//!
//! This module is the source of truth for the DHT relay-to-relay wire
//! protocol described in `misc/specs/DHT.md` (§2). It carries:
//!
//! 1. The [`PresenceRecord`] data type (§1.1) and its dual-signature
//!    transcripts (user_sig per §1.1.1 paragraph 1; relay_sig per §1.1.1
//!    paragraph 2).
//! 2. The full RPC catalogue from §2.4: `Ping`/`Pong`, `FindNode`/`Resp`,
//!    `FindValue`/`Resp`, `Store`/`Resp`, `Tombstone`/`Resp`,
//!    `MerkleSummary`/`Resp`, `MerkleDiff`/`Resp`, and
//!    `FetchRecord`/`Resp`.
//! 3. Length-bound constants per §2.6 that downstream handlers check at
//!    deserialization / construction time.
//!
//! ## Why a `DhtRequest` + `DhtResponse` split (not a single `DhtPacket`)
//!
//! §2.1 describes a generic `DhtPacket { Rpc(DhtRpc), Sync(DhtSync) }`
//! shape, but does not pin a strict request/response layout — that is left
//! to the implementer. We choose **separate request and response enums**
//! plus a thin outer [`DhtPacket`] wrapper because:
//!
//! - Per-RPC bi-streams (§2.2): one stream carries exactly one request and
//!   one (possibly multi-frame) response, so the *direction* is implicit
//!   in the stream side. Splitting the enums means the dispatcher on each
//!   side can match exhaustively against only the variants it ever
//!   receives, instead of dynamic-checking "did the peer send a response
//!   to a question I never asked".
//! - Mirrors the exemplar in `common/src/proto/relay_res.rs` (`LifetimeP`
//!   — all packet kinds are sibling variants of one enum) but specialises
//!   it to a request/response idiom because every DHT call has exactly
//!   one of each, whereas the relay/resolver lifecycle is asymmetric.
//!
//! [`DhtPacket`] still exists as a convenience for the framing layer so a
//! future non-RPC sync mode (push, gossip, etc.) can join the same wire
//! grammar without breaking existing RPCs.
//!
//! ## Signing transcript discipline
//!
//! Every helper that builds a Ed25519 signing input mirrors the layout
//! pioneered in `common/src/proto/relay_res.rs::signing_input`:
//!
//! ```text
//!   <domain> || PROTOCOL_VERSION (BE u16) || <fields in declaration order>
//! ```
//!
//! Each transcript has its own unique domain string so a captured
//! signature for one packet kind cannot be replayed as another (§1.1.1
//! paragraph 5). Both signing and verifying sides call the same helper —
//! it is the contract between them.

use std::cmp::Ordering;
use std::net::SocketAddr;

use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use thiserror::Error;

use crate::PROTOCOL_VERSION;
use crate::proto::RelayId;
use crate::types::bytes::Bytes;

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:===:==:  CONSTANTS  :==:===:===:===:===||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

/// Base domain-separation tag for every DHT signed transcript. Per
/// `misc/specs/DHT.md` §0 (`DHT_DOMAIN_PREFIX`).
pub const DHT_DOMAIN_PREFIX: &[u8] = b"promtuz-dht-v1";

/// Domain-separation tag mixed into the *user-roam* signing input. Per
/// §1.1.1 paragraph 1: U signs `DHT_DOMAIN_PREFIX || "-roam-v1" ||
/// PROTOCOL_VERSION (BE u16) || user_ipk || relay_id || generation
/// (BE u64)`.
pub const DHT_USER_ROAM_SIG_DOMAIN: &[u8] = b"promtuz-dht-v1-roam-v1";

/// Domain-separation tag mixed into the *relay-presence* counter-signing
/// input. Per §1.1.1 paragraph 2: R signs every record field except
/// `relay_sig` itself, prefixed by `DHT_DOMAIN_PREFIX || "-presence-v1"
/// || PROTOCOL_VERSION (BE u16)`.
pub const DHT_PRESENCE_SIG_DOMAIN: &[u8] = b"promtuz-dht-v1-presence-v1";

/// Domain-separation tag for the connection-level [`DhtHello`] handshake
/// sent as the very first frame on a fresh `peer/1` connection.
///
/// Distinct from every other DHT signing-input tag (`-roam-v1`,
/// `-presence-v1`, `-tombstone-v1`) so a captured signature for a
/// presence record cannot be replayed as a connection hello and vice
/// versa. Mirrors the resolver-side
/// [`crate::proto::relay_res::RELAY_HELLO_SIG_DOMAIN`] discipline (one
/// domain string per packet kind).
pub const DHT_HELLO_SIG_DOMAIN: &[u8] = b"promtuz-dht-hello-v1";

/// Maximum permitted clock skew between the dialing relay's signed
/// `timestamp` and the receiver's local clock, in milliseconds. Anything
/// outside this window is treated as a replay or a misconfigured clock
/// and rejected with [`crate::quic::CloseReason::DhtClockSkew`].
///
/// Mirrors `HELLO_MAX_SKEW_MS` at `resolver/src/resolver/mod.rs:47` —
/// the resolver applies the same window to `RelayHello`/`RelayHeartbeat`,
/// and consistency across packet kinds keeps a relay's local clock-drift
/// behaviour identical against either receiver.
pub const MAX_DHT_HELLO_SKEW_MS: u64 = 60_000;

/// Replication factor `k` (§0). Bounds [`FindNodeResp::closer`] and the
/// `Closer` arm of [`FindValueOutcome`].
pub const DHT_K: usize = 3;

/// Future-skew tolerance applied to [`PresenceRecord::not_before`] in
/// milliseconds (§0, `PRESENCE_MAX_FUTURE_SKEW_MS`). A record is rejected
/// with [`PresenceVerifyError::NotYetValid`] when `not_before > now +
/// PRESENCE_MAX_FUTURE_SKEW_MS`.
pub const PRESENCE_MAX_FUTURE_SKEW_MS: u64 = 60_000;

/// Maximum length of [`MerkleDiff::path`] (§2.6). Equals tree depth: 16-bit
/// leaf address space at 4 bits per level = 4 levels.
pub const MAX_MERKLE_DIFF_PATH: usize = 4;

/// Branching factor of the per-slice radix-16 trie (§0, `MERKLE_FANOUT`)
/// — also the bound on
/// [`MerkleDiffResp::Children::hashes`] length per §2.6.
pub const MERKLE_FANOUT: usize = 16;

/// Bound on [`MerkleDiffResp::Leaves::entries`] per RPC (§2.6). Larger
/// result sets split across multiple sequential `MerkleDiff` calls.
pub const MAX_MERKLE_DIFF_LEAVES: usize = 64;

/// Bound on [`FetchRecord::user_ipks`] and matching
/// [`FetchRecordResp::records`] per RPC (§2.6).
pub const MAX_FETCH_RECORD_LEAVES: usize = 64;

/// Alias for `MAX_FETCH_RECORD_LEAVES`, exported under the name the
/// implementation prompt asked for. Same semantics, single source of
/// truth.
pub const MAX_FETCH_RECORD_BATCH: usize = MAX_FETCH_RECORD_LEAVES;

/// Bound on [`FindNodeResp::closer`] / [`FindValueOutcome::Closer`] entry
/// counts per §2.6. Equal to the replication factor `k`.
pub const MAX_FIND_NODE_RESULTS: usize = DHT_K;

/// Bytes of the slice bitset in [`MerkleSummary::slices`] (§2.4.6 / §2.6
/// "256 bits = fixed bitset"). 256 slices over the keyspace = 32 bytes.
pub const MERKLE_SUMMARY_SLICE_BITS: usize = 256;

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:===:==:  DHT HELLO  :==:===:===:===:===||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

/// Connection-level signed handshake sent as the **very first frame** on
/// a freshly-opened `peer/1` (relay-to-relay) connection. The dialing
/// relay opens a uni-stream, frames a [`DhtHello`] and shuts the stream;
/// the receiving relay verifies the signature and binds the resulting
/// [`crate::quic::id::NodeId`] to the connection for the rest of its
/// lifetime.
///
/// **Why an application-layer hello rather than mTLS?** The relay's
/// `peer/1` ALPN is currently configured `with_no_client_auth()` because
/// the same QUIC `Endpoint` also accepts `client/1` connections, and
/// clients have no certs. mTLS on `peer/1` would require either two
/// endpoints or a per-ALPN client-auth toggle (neither exists yet in
/// `quinn`'s public API). An application-layer signed hello mirrors the
/// existing relay-to-resolver pattern (see
/// [`crate::proto::relay_res::LifetimeP::RelayHello`]) and gives us
/// equivalent identity binding — the dialing relay's `NodeId` is proven
/// by Ed25519 signature against the wire transcript, and the receiver
/// can drop the connection on any failure.
///
/// **Wire layout** (field order is load-bearing — both signing and
/// verifying sides walk the [`dht_hello_signing_input`] helper which
/// visits these in declaration order):
///
/// ```text
/// DhtHello {
///   node_id:   [u8; 32],   // claimed identity = BLAKE3(pubkey)
///   pubkey:    [u8; 32],   // dialer's full Ed25519 identity pubkey
///   timestamp: u64,        // ms since epoch; ±MAX_DHT_HELLO_SKEW_MS window
///   sig:       [u8; 64],   // Ed25519 signature over the canonical transcript
/// }
/// ```
///
/// **Signed transcript** (`dht_hello_signing_input`):
/// ```text
/// DHT_HELLO_SIG_DOMAIN || PROTOCOL_VERSION (BE u16)
///   || node_id (32) || pubkey (32) || timestamp (BE u64)
/// ```
///
/// design-doc: `misc/specs/DHT.md` §2.3 (relay-to-relay handshake), §2.5
/// (close-reason codes), §8.1-8.2 (Sybil/eclipse rationale).
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DhtHello {
    /// Dialer's stable cryptographic ID, claimed identity. The verifier
    /// also checks `BLAKE3(pubkey) == node_id` (id-binding) so an attacker
    /// cannot present a benign pubkey under a different node_id.
    pub node_id:   crate::quic::id::NodeId,
    /// Dialer's full Ed25519 identity public key. Carried alongside
    /// `node_id` because `node_id` is a BLAKE3 hash and is therefore not
    /// invertible — the verifier needs the full key to check `sig`.
    /// Same reasoning as [`crate::proto::relay_res::LifetimeP::RelayHello::pubkey`].
    pub pubkey:    Bytes<32>,
    /// Sender-local Unix time in milliseconds. Bound into the signed
    /// transcript so the receiver can reject replays outside an accepted
    /// clock-skew window ([`MAX_DHT_HELLO_SKEW_MS`]).
    pub timestamp: u64,
    /// Ed25519 signature over [`dht_hello_signing_input`]. Verified
    /// under `pubkey` using `verify_strict`, mirroring the resolver's
    /// `RelayHello` verification at
    /// `resolver/src/resolver/mod.rs::verify_signed_packet`.
    pub sig:       Bytes<64>,
}

/// Build the canonical signing transcript for [`DhtHello`].
///
/// Layout:
/// ```text
///   DHT_HELLO_SIG_DOMAIN || PROTOCOL_VERSION (BE u16)
///     || node_id (32) || pubkey (32) || timestamp (BE u64)
/// ```
///
/// The transcript layout deliberately mirrors
/// [`crate::proto::relay_res::relay_hello_signing_input`] field-for-field
/// — the only differences are the domain tag (so signatures are
/// non-replayable across packet kinds) and the `timestamp` width (`u64`
/// here vs `u128` in `relay_res`; chosen for parity with the rest of
/// `dht_p2p.rs` which uses `u64` for all wall-clock fields like
/// `not_before` / `not_after` / `deleted_at`).
///
/// Both signing (dialer) and verifying (receiver) sides call this helper,
/// which makes it the byte-for-byte contract — there is no second
/// implementation to keep in sync.
pub fn dht_hello_signing_input(
    node_id: &crate::quic::id::NodeId, pubkey: &[u8; 32], timestamp: u64,
) -> Vec<u8> {
    // domain (varies) + version (2) + node_id (32) + pubkey (32) + ts (8) = 76
    // + domain bytes.
    let mut buf = Vec::with_capacity(
        DHT_HELLO_SIG_DOMAIN.len() + 2 + crate::quic::id::NodeId::LEN + 32 + 8,
    );
    buf.extend_from_slice(DHT_HELLO_SIG_DOMAIN);
    buf.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    buf.extend_from_slice(node_id.as_bytes());
    buf.extend_from_slice(pubkey);
    buf.extend_from_slice(&timestamp.to_be_bytes());
    buf
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:===:==: PRESENCE :==:===:===:===:===:===||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

/// User-presence record asserting "I, relay R, host an authenticated
/// session for user U, valid until T."
///
/// Wire/storage layout per `misc/specs/DHT.md` §1.1, postcard-encoded.
/// **Field declaration order is load-bearing** — both the relay-side
/// signing transcript (§1.1.1 paragraph 2) and the postcard wire
/// representation visit fields in declaration order, so re-ordering them
/// silently breaks every replica's signature check.
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PresenceRecord {
    /// User U's Ed25519 identity public key (also the DHT key for this
    /// record). 32 bytes.
    pub user_ipk: Bytes<32>,
    /// `BLAKE3(R's NodeKey)` — the relay's full-width NodeId (§9.6 widened
    /// from 10 to 32 bytes).
    pub relay_id: RelayId,
    /// R's full Ed25519 identity public key. Carried so verifiers do not
    /// need a side channel to recover the verification key from
    /// `relay_id` alone (a hash is non-invertible). Same justification as
    /// the `pubkey` field on `LifetimeP::RelayHello`.
    pub relay_pubkey: Bytes<32>,
    /// `not_before` in milliseconds since the Unix epoch, set by R's
    /// local clock at publish time. Bound into the relay signing
    /// transcript so a misbehaving R cannot disclaim its timestamp.
    pub not_before: u64,
    /// `not_after = not_before + PRESENCE_TTL_MS`. Replicas reject the
    /// record once the current wall-clock passes `not_after` (§1.1.2).
    pub not_after: u64,
    /// User-controlled monotonic counter. Primary tiebreaker for
    /// multi-writer conflict resolution (§5.3); also the only field that
    /// stops a replayed `user_sig` from outliving a roam (§1.1.2).
    pub generation: u64,
    /// Bitset for future capability negotiation. v1 = `0`. Carried in the
    /// relay signing transcript so a future capability flip cannot be
    /// retroactively grafted onto an existing record.
    pub capabilities: u16,
    /// User's Ed25519 signature over
    /// [`presence_record_user_signing_input`]. Authorises R to publish on
    /// U's behalf for this `(user_ipk, relay_id, generation)` tuple.
    pub user_sig: Bytes<64>,
    /// Relay's Ed25519 signature over
    /// [`presence_record_relay_signing_input`]. Binds the *whole* record
    /// — including the timestamps — to R's identity, so a replica can
    /// attribute timestamp drift to the specific R that signed it (§8).
    pub relay_sig: Bytes<64>,
}

/// Tombstone payload used by [`DhtRequest::Tombstone`].
///
/// Per §1.2: a tombstone entry holds `(generation, deleted_at)` plus a
/// relay signature so a replica can attribute the deletion to the relay
/// that issued it. Tombstones are honoured by replicas for `2 ×
/// PRESENCE_TTL_MS` after `deleted_at`, then garbage-collected.
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombstoneRecord {
    /// User the tombstone targets — same DHT key space as
    /// [`PresenceRecord::user_ipk`].
    pub user_ipk: Bytes<32>,
    /// Relay that issued the tombstone (= the previous record's
    /// `relay_id`).
    pub relay_id: RelayId,
    /// Issuer's full Ed25519 pubkey, for verification (mirrors
    /// [`PresenceRecord::relay_pubkey`]).
    pub relay_pubkey: Bytes<32>,
    /// Generation of the record being tombstoned. A tombstone with
    /// `generation` strictly less than the locally-held record's
    /// generation is ignored (§5.3 ordering applies in reverse).
    pub generation: u64,
    /// Wall-clock at which R observed the disconnect, in ms since epoch.
    pub deleted_at: u64,
    /// R's Ed25519 signature over
    /// [`tombstone_signing_input`].
    pub relay_sig: Bytes<64>,
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:==:  SIGNING TRANSCRIPT HELPERS  :==:===:===||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

/// Build the canonical user-roam signing transcript.
///
/// Layout per §1.1.1 paragraph 1:
/// ```text
///   DHT_DOMAIN_PREFIX || b"-roam-v1" || PROTOCOL_VERSION (BE u16)
///     || user_ipk (32) || relay_id (32) || generation (BE u64)
/// ```
///
/// Both U (signing) and the relay-side verifier (during
/// [`PresenceRecord::verify`] in `crypto`-enabled crates) call this
/// helper, which makes it the byte-for-byte contract between the two
/// sides — there is no second implementation to keep in sync.
pub fn presence_record_user_signing_input(
    user_ipk: &[u8; 32], relay_id: &RelayId, generation: u64,
) -> Vec<u8> {
    let mut buf =
        Vec::with_capacity(DHT_USER_ROAM_SIG_DOMAIN.len() + 2 + 32 + RelayId::LEN + 8);
    buf.extend_from_slice(DHT_USER_ROAM_SIG_DOMAIN);
    buf.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    buf.extend_from_slice(user_ipk);
    buf.extend_from_slice(relay_id.as_bytes());
    buf.extend_from_slice(&generation.to_be_bytes());
    buf
}

/// Build the canonical relay-presence countersigning transcript.
///
/// Per §1.1.1 paragraph 2 the relay signs over the wire-canonical bytes
/// of every other field, in declaration order, prefixed with
/// `DHT_DOMAIN_PREFIX || b"-presence-v1" || PROTOCOL_VERSION (BE u16)`.
/// The order below mirrors [`PresenceRecord`] field order *exactly* —
/// reorder one and the signature stops verifying.
pub fn presence_record_relay_signing_input(
    user_ipk: &[u8; 32], relay_id: &RelayId, relay_pubkey: &[u8; 32], not_before: u64,
    not_after: u64, generation: u64, capabilities: u16, user_sig: &[u8; 64],
) -> Vec<u8> {
    // domain (varies) + version (2) + ipk (32) + relay_id (RelayId::LEN) +
    // relay_pubkey (32) + 3 × u64 (8 each) + capabilities (2) + user_sig (64)
    let mut buf = Vec::with_capacity(
        DHT_PRESENCE_SIG_DOMAIN.len() + 2 + 32 + RelayId::LEN + 32 + 8 + 8 + 8 + 2 + 64,
    );
    buf.extend_from_slice(DHT_PRESENCE_SIG_DOMAIN);
    buf.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    buf.extend_from_slice(user_ipk);
    buf.extend_from_slice(relay_id.as_bytes());
    buf.extend_from_slice(relay_pubkey);
    buf.extend_from_slice(&not_before.to_be_bytes());
    buf.extend_from_slice(&not_after.to_be_bytes());
    buf.extend_from_slice(&generation.to_be_bytes());
    buf.extend_from_slice(&capabilities.to_be_bytes());
    buf.extend_from_slice(user_sig);
    buf
}

/// Domain-separation tag for [`tombstone_signing_input`]. A distinct
/// suffix ensures a captured `relay_sig` over a [`PresenceRecord`] cannot
/// be replayed as a tombstone (or vice versa).
pub const DHT_TOMBSTONE_SIG_DOMAIN: &[u8] = b"promtuz-dht-v1-tombstone-v1";

/// Build the canonical tombstone signing transcript.
///
/// Layout (mirrors [`presence_record_relay_signing_input`] but without a
/// `user_sig` field):
/// ```text
///   DHT_TOMBSTONE_SIG_DOMAIN || PROTOCOL_VERSION (BE u16)
///     || user_ipk (32) || relay_id (32) || relay_pubkey (32)
///     || generation (BE u64) || deleted_at (BE u64)
/// ```
pub fn tombstone_signing_input(
    user_ipk: &[u8; 32], relay_id: &RelayId, relay_pubkey: &[u8; 32], generation: u64,
    deleted_at: u64,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(
        DHT_TOMBSTONE_SIG_DOMAIN.len() + 2 + 32 + RelayId::LEN + 32 + 8 + 8,
    );
    buf.extend_from_slice(DHT_TOMBSTONE_SIG_DOMAIN);
    buf.extend_from_slice(&PROTOCOL_VERSION.to_be_bytes());
    buf.extend_from_slice(user_ipk);
    buf.extend_from_slice(relay_id.as_bytes());
    buf.extend_from_slice(relay_pubkey);
    buf.extend_from_slice(&generation.to_be_bytes());
    buf.extend_from_slice(&deleted_at.to_be_bytes());
    buf
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:==: VERIFICATION ERROR :==:===:===:===:||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

/// Reasons a [`PresenceRecord`] can fail validation.
///
/// Maps onto the close-reason codes defined in
/// [`crate::quic::CloseReason`] (§2.5): a `BadUserSig`/`BadRelaySig`
/// becomes `DhtBadSignature`, and `Expired`/`NotYetValid` become
/// `DhtClockSkew`.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum PresenceVerifyError {
    /// `user_sig` did not validate under `user_ipk`.
    #[error("presence record: bad user signature")]
    BadUserSig,
    /// `relay_sig` did not validate under the supplied `relay_pubkey`.
    #[error("presence record: bad relay signature")]
    BadRelaySig,
    /// `now > not_after` — record's TTL has elapsed.
    #[error("presence record: expired (now > not_after)")]
    Expired,
    /// `not_before > now + PRESENCE_MAX_FUTURE_SKEW_MS` — record claims a
    /// future activation outside the accepted skew window.
    #[error("presence record: not yet valid (clock skew too large)")]
    NotYetValid,
    /// `not_after <= not_before` (degenerate window) or another
    /// structural violation noticed before signature checks.
    #[error("presence record: malformed field")]
    MalformedField,
    /// `relay_pubkey` did not parse as an Ed25519 verifying key. Treated
    /// distinctly from `BadRelaySig` so callers can attribute key-shape
    /// problems separately from signature-mismatch problems.
    #[error("presence record: malformed relay pubkey")]
    MalformedRelayPubkey,
    /// `relay_id` does not match `BLAKE3(relay_pubkey)`. Caught here so a
    /// caller cannot smuggle in a relay_pubkey that signs valid
    /// transcripts but is not the identity behind the claimed
    /// `relay_id`.
    #[error("presence record: relay_id != BLAKE3(relay_pubkey)")]
    RelayIdMismatch,
}

/// Reasons a [`DhtHello`] can fail the inbound verification at
/// `relay/src/dht/handler.rs::handle_peer_connection`.
///
/// Maps onto the [`crate::quic::CloseReason`]`::Dht*` variants 1:1:
/// - [`Self::IdMismatch`], [`Self::MalformedPubkey`], [`Self::BadSignature`]
///   → `DhtBadSignature` (or `DhtMalformedKey` for malformed pubkey
///   shape — caller's choice).
/// - [`Self::ClockSkew`] → `DhtClockSkew`.
///
/// design-doc: §2.5.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum DhtHelloVerifyError {
    /// `node_id != BLAKE3(pubkey)` — the dialer is presenting a pubkey
    /// that does not hash to the claimed identity.
    #[error("dht hello: node_id != BLAKE3(pubkey)")]
    IdMismatch,
    /// `pubkey` did not parse as an Ed25519 verifying key. Distinct from
    /// [`Self::BadSignature`] so callers can attribute key-shape problems
    /// separately from signature-mismatch problems (mirrors
    /// [`PresenceVerifyError::MalformedRelayPubkey`]).
    #[error("dht hello: malformed Ed25519 pubkey")]
    MalformedPubkey,
    /// `sig` did not validate under `pubkey` over the canonical
    /// transcript.
    #[error("dht hello: bad signature")]
    BadSignature,
    /// `|now_ms − timestamp| > MAX_DHT_HELLO_SKEW_MS`. Indicates either a
    /// replay outside the skew window or a misconfigured clock on the
    /// dialer.
    #[error("dht hello: stale or future timestamp (clock skew)")]
    ClockSkew,
}

impl PresenceRecord {
    /// Multi-writer conflict resolution per `misc/specs/DHT.md` §5.3.
    ///
    /// Returns [`Ordering::Greater`] when `self` is the canonical winner
    /// (replicas should keep `self` and reject `other` as `Stale`),
    /// [`Ordering::Less`] when `other` wins, [`Ordering::Equal`] only on
    /// a literal byte-identical record (statistically near-impossible in
    /// the wild, exercised in tests).
    ///
    /// Order: `generation` desc, then `not_before` desc, then `relay_id`
    /// lex desc — i.e. the higher-generation / fresher-timestamp /
    /// later-lex record wins.
    ///
    /// design-doc: §5.3
    pub fn compare(&self, other: &Self) -> Ordering {
        // Higher generation wins.
        match self.generation.cmp(&other.generation) {
            Ordering::Equal => {}
            ord => return ord,
        }
        // Same generation — fresher republish wins.
        match self.not_before.cmp(&other.not_before) {
            Ordering::Equal => {}
            ord => return ord,
        }
        // Exact tie — deterministic lex tiebreak on the *full* relay_id
        // bytes. Lex-larger wins so the operator-visible "preferred" id
        // is also the one with the larger byte string, mirroring
        // mainline Kademlia's deterministic-but-arbitrary tie rule.
        self.relay_id.as_bytes().cmp(other.relay_id.as_bytes())
    }
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:==: CRYPTO-GATED VERIFY :==:===:===:===||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

#[cfg(feature = "crypto")]
mod verify_impl {
    use ed25519_dalek::Signature;
    use ed25519_dalek::Verifier;
    use ed25519_dalek::VerifyingKey;

    use super::DhtHello;
    use super::DhtHelloVerifyError;
    use super::MAX_DHT_HELLO_SKEW_MS;
    use super::PRESENCE_MAX_FUTURE_SKEW_MS;
    use super::PresenceRecord;
    use super::PresenceVerifyError;
    use super::dht_hello_signing_input;
    use super::presence_record_relay_signing_input;
    use super::presence_record_user_signing_input;
    use crate::quic::id::NodeId;

    impl DhtHello {
        /// Validate a [`DhtHello`] received as the first frame on an
        /// inbound `peer/1` connection. Returns `Ok(())` and binds the
        /// connection's authenticated [`NodeId`] (callers stash
        /// `self.node_id` post-success) on a clean check.
        ///
        /// Mirrors the order, semantics and failure modes of the
        /// resolver-side `verify_signed_packet` at
        /// `resolver/src/resolver/mod.rs:315-346`:
        ///
        /// 1. **id ↔ pubkey binding**: `BLAKE3(pubkey) == node_id`.
        ///    Catches an attacker presenting a benign pubkey under a
        ///    different claimed `node_id`.
        /// 2. **Pubkey shape**: `pubkey` parses as an Ed25519 verifying
        ///    key. Surfaced as `MalformedPubkey` (distinct from
        ///    `BadSignature`) so operators can distinguish key-shape
        ///    problems from sig-mismatch problems.
        /// 3. **Signature**: `sig` verifies under `pubkey` over the
        ///    canonical transcript built by
        ///    [`dht_hello_signing_input`]. Uses `verify_strict` (same
        ///    choice as `resolver/src/resolver/mod.rs:332`) for the
        ///    standard small-subgroup defence.
        /// 4. **Timestamp window**: `|now_ms − timestamp| ≤
        ///    MAX_DHT_HELLO_SKEW_MS`.
        ///
        /// `now_ms` is wall-clock in milliseconds since the Unix epoch,
        /// passed in explicitly so unit tests can pin a deterministic
        /// clock.
        pub fn verify(&self, now_ms: u64) -> Result<(), DhtHelloVerifyError> {
            // 1. id-binding to pubkey. NodeId::new = BLAKE3(pubkey) —
            //    same construction every other call site uses (cf.
            //    `verify_signed_packet` and `PresenceRecord::verify`).
            let derived_id = NodeId::new(self.pubkey.as_ref());
            if derived_id != self.node_id {
                return Err(DhtHelloVerifyError::IdMismatch);
            }

            // 2. Pubkey shape.
            let vk = VerifyingKey::from_bytes(&self.pubkey.0)
                .map_err(|_| DhtHelloVerifyError::MalformedPubkey)?;

            // 3. Signature.
            let sig = Signature::from_bytes(&self.sig.0);
            let msg = dht_hello_signing_input(&self.node_id, &self.pubkey.0, self.timestamp);
            vk.verify_strict(&msg, &sig)
                .map_err(|_| DhtHelloVerifyError::BadSignature)?;

            // 4. Timestamp freshness (replay protection).
            let skew = now_ms.abs_diff(self.timestamp);
            if skew > MAX_DHT_HELLO_SKEW_MS {
                return Err(DhtHelloVerifyError::ClockSkew);
            }

            Ok(())
        }
    }

    impl PresenceRecord {
        /// Validate the record end-to-end:
        ///
        /// 1. Structural: `not_after > not_before` (a record with a
        ///    zero-length window is always rejected, before crypto).
        /// 2. `relay_id == BLAKE3(relay_pubkey)` — prevents an attacker
        ///    from supplying a benign `relay_pubkey` that signs valid
        ///    transcripts but is not the identity behind the claimed
        ///    `relay_id`.
        /// 3. Time window: `not_before <= now +
        ///    PRESENCE_MAX_FUTURE_SKEW_MS` and `not_after > now`.
        ///    Per §1.1.2 / §1.1.3 — bounds usefulness of a captured
        ///    record to ~10 minutes.
        /// 4. `user_sig` verifies under the embedded `user_ipk` over
        ///    [`presence_record_user_signing_input`].
        /// 5. `relay_sig` verifies under `relay_pubkey` over
        ///    [`presence_record_relay_signing_input`].
        ///
        /// `now_ms` is wall-clock in milliseconds since the Unix epoch.
        /// Callers pass it explicitly so unit tests can pin a
        /// deterministic clock and so a network-wide clock-skew shim
        /// (future) can be inserted without touching this function.
        pub fn verify(&self, now_ms: u64) -> Result<(), PresenceVerifyError> {
            // 1. Structural sanity. Cheap, runs before any crypto.
            if self.not_after <= self.not_before {
                return Err(PresenceVerifyError::MalformedField);
            }

            // 2. id-binding to pubkey. NodeId::new = BLAKE3(pubkey) —
            // same construction every other call site uses.
            let derived_id = NodeId::new(self.relay_pubkey.as_ref());
            if derived_id != self.relay_id {
                return Err(PresenceVerifyError::RelayIdMismatch);
            }

            // 3. Time window (per §1.1.2).
            if self.not_before > now_ms.saturating_add(PRESENCE_MAX_FUTURE_SKEW_MS) {
                return Err(PresenceVerifyError::NotYetValid);
            }
            if now_ms >= self.not_after {
                return Err(PresenceVerifyError::Expired);
            }

            // 4. User signature.
            let user_vk = VerifyingKey::from_bytes(&self.user_ipk.0)
                .map_err(|_| PresenceVerifyError::BadUserSig)?;
            let user_sig = Signature::from_bytes(&self.user_sig.0);
            let user_msg = presence_record_user_signing_input(
                &self.user_ipk.0,
                &self.relay_id,
                self.generation,
            );
            user_vk
                .verify(&user_msg, &user_sig)
                .map_err(|_| PresenceVerifyError::BadUserSig)?;

            // 5. Relay countersignature.
            let relay_vk = VerifyingKey::from_bytes(&self.relay_pubkey.0)
                .map_err(|_| PresenceVerifyError::MalformedRelayPubkey)?;
            let relay_sig = Signature::from_bytes(&self.relay_sig.0);
            let relay_msg = presence_record_relay_signing_input(
                &self.user_ipk.0,
                &self.relay_id,
                &self.relay_pubkey.0,
                self.not_before,
                self.not_after,
                self.generation,
                self.capabilities,
                &self.user_sig.0,
            );
            relay_vk
                .verify(&relay_msg, &relay_sig)
                .map_err(|_| PresenceVerifyError::BadRelaySig)?;

            Ok(())
        }
    }
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:===:==: NODE DESCRIPTOR :==:===:===:===||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

/// Descriptor returned in [`FindNodeResp`] / [`FindValueOutcome::Closer`]
/// per §2.4.2. Carries everything a requester needs to make a first
/// contact with a previously-unknown peer (id, address, full pubkey for
/// cert-chain verification on first connect).
#[serde_as]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeDescriptor {
    /// Peer's NodeId — full 32 bytes (§9.6 widening).
    pub id:     RelayId,
    /// Peer's QUIC endpoint. `serde_with::DisplayFromStr` matches the
    /// existing convention in `client_res.rs::RelayDescriptor`.
    #[serde_as(as = "serde_with::DisplayFromStr")]
    pub addr:   SocketAddr,
    /// Peer's full Ed25519 identity public key, so the requester can
    /// verify the cert chain on its first connect rather than chasing a
    /// side-channel.
    pub pubkey: Bytes<32>,
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:===:==:  RPC PAYLOADS  :==:===:===:===:||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

// --- Ping / Pong (§2.4.1) ----------------------------------------------

/// Liveness probe / RTT sample. **Unsigned** at the application layer:
/// per §2.4.1, mTLS already binds the connection to a specific cert.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Ping {
    pub nonce:     Bytes<16>,
    pub timestamp: u64,
}

/// Reply to [`Ping`]. Echoes the nonce so the requester can correlate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pong {
    pub nonce:     Bytes<16>,
    pub timestamp: u64,
}

// --- FindNode (§2.4.2) -------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindNode {
    /// Any 256-bit Key — a `NodeId`, an `IPK`, etc. We type as
    /// `Bytes<32>` deliberately (rather than `RelayId`) to encode "this
    /// is a key, not necessarily a relay id".
    pub target:    Bytes<32>,
    /// Requester's NodeId — redundant with the cert SPKI but cheap and
    /// lets the responder index its routing-table updates without
    /// re-deriving from the cert chain.
    pub requester: RelayId,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindNodeResp {
    /// Up to `k = MAX_FIND_NODE_RESULTS` closest peers responder knows
    /// of. Length-bound enforced at deserialization.
    pub closer: Vec<NodeDescriptor>,
}

// --- FindValue (§2.4.3) ------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindValue {
    pub user_ipk:  Bytes<32>,
    pub requester: RelayId,
}

/// Three response shapes per §2.4.3 / §4.2.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FindValueOutcome {
    /// Responder is in the k owners and has the record.
    Found(PresenceRecord),
    /// Responder *is* in the k closest but has no record. Authoritative
    /// "user is offline" — terminates the iterator early per §4.2.
    NotPresent,
    /// Responder is not in the k closest; here are the closest peers it
    /// knows. Length-bound `MAX_FIND_NODE_RESULTS` per §2.6.
    Closer(Vec<NodeDescriptor>),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FindValueResp {
    pub result: FindValueOutcome,
}

// --- Store (§2.4.4) ----------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Store {
    pub record: PresenceRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum StoreOutcome {
    Stored,
    /// Responder already has a strictly-newer record per §5.3 ordering.
    Stale,
    /// Responder is not in the k closest owners by its current routing
    /// view; record dropped (see §5.4).
    NotOwner,
    /// Either user_sig or relay_sig failed to verify.
    BadSig,
    /// Record's TTL has already elapsed at the time of the STORE.
    TtlExpired,
    /// Per-source rate limit tripped — see §8.4.
    RateLimited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoreResp {
    pub outcome: StoreOutcome,
}

// --- Tombstone (§2.4.5) ------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    pub record: TombstoneRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TombstoneOutcome {
    Stored,
    /// Local record has higher generation; tombstone ignored.
    Stale,
    /// Responder not in the k closest owners.
    NotOwner,
    /// Tombstone's relay signature failed verification.
    BadSig,
    /// Per-source rate limit tripped.
    RateLimited,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TombstoneResp {
    pub outcome: TombstoneOutcome,
}

// --- MerkleSummary (§2.4.6) -------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleSummary {
    /// Bitset of which slices the requester wants roots for. 256 slices
    /// total (§0); encoded as a fixed 32-byte array. Wrapped in
    /// [`Bytes`] so postcard ships it as a length-elided byte string
    /// rather than a length-prefixed `Vec`.
    pub slices: Bytes<32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleSummaryResp {
    /// `(slice_id, root_hash)` pairs for each requested slice the
    /// responder also has populated. Bounded by 256 (one per possible
    /// slice).
    pub roots: Vec<(u8, Bytes<32>)>,
}

// --- MerkleDiff (§2.4.7) ----------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MerkleDiff {
    pub slice_id: u8,
    /// Path of nibble indices from the slice root (e.g. `[3, 11, 7]` =
    /// `root.children[3].children[11].children[7]`). Bounded by
    /// [`MAX_MERKLE_DIFF_PATH`] per §2.6 (= tree depth).
    pub path:     Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MerkleDiffResp {
    /// Path resolved to an internal node — return its child hashes.
    /// Length is exactly [`MERKLE_FANOUT`] (= 16) per §2.6.
    Children { hashes: Vec<Bytes<32>> },
    /// Path resolved to a leaf — return covered (user_ipk, record-hash)
    /// pairs. Bounded by [`MAX_MERKLE_DIFF_LEAVES`] per §2.6.
    Leaves { entries: Vec<(Bytes<32>, Bytes<32>)> },
}

// --- FetchRecord (§2.4.8) ---------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRecord {
    /// Bounded by [`MAX_FETCH_RECORD_BATCH`] per §2.6.
    pub user_ipks: Vec<Bytes<32>>,
}

/// Reply to a [`FetchRecord`] request.
///
/// Carries **both** live records and tombstones so anti-entropy
/// converges on deletions, not only insertions (§6.3 — "Tombstones
/// converge the same way"). The combined length
/// `records.len() + tombstones.len()` is bounded by
/// [`MAX_FETCH_RECORD_BATCH`] per §2.6; a single IPK never appears in
/// both lists (the responder returns the tombstone if present, else the
/// record, never both — tombstones supersede live records at store time
/// per §5.3, but anti-entropy still has to ship the chosen one).
///
/// **Wire-format compatibility note:** prior to this widening,
/// `FetchRecordResp` was a single `records` vec. The shape change is
/// implicit-versioned by `PROTOCOL_VERSION = 2` (declared in
/// `common::PROTOCOL_VERSION`); peers running the older shape are
/// gated by ALPN and refuse to interop. There is no separate version
/// flag because the reply payload is *not* signed.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchRecordResp {
    /// Live records the responder holds for the requested IPKs.
    /// Bounded — combined with [`Self::tombstones`] — by
    /// [`MAX_FETCH_RECORD_BATCH`] per §2.6.
    pub records:    Vec<PresenceRecord>,
    /// Tombstones the responder holds for the requested IPKs. A
    /// tombstone supersedes any local live record (per §5.3 conflict
    /// rules applied at store time), so anti-entropy carrying
    /// tombstones makes deletions converge across replicas. Bounded —
    /// combined with [`Self::records`] — by [`MAX_FETCH_RECORD_BATCH`]
    /// per §2.6.
    pub tombstones: Vec<TombstoneRecord>,
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:==:  REQUEST / RESPONSE  :==:===:===:===||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

/// All inbound DHT request payloads. One variant per RPC in §2.4.
///
/// The acceptor side dispatches on the variant and replies with the
/// matching [`DhtResponse`] variant via the same bi-stream (§2.2).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DhtRequest {
    Ping(Ping),
    FindNode(FindNode),
    FindValue(FindValue),
    Store(Store),
    Tombstone(Tombstone),
    MerkleSummary(MerkleSummary),
    MerkleDiff(MerkleDiff),
    FetchRecord(FetchRecord),
}

/// All outbound DHT response payloads. Mirrored 1:1 with [`DhtRequest`]
/// — the dispatcher relies on this pairing to deserialise without an
/// out-of-band request-id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DhtResponse {
    Pong(Pong),
    FindNode(FindNodeResp),
    FindValue(FindValueResp),
    Store(StoreResp),
    Tombstone(TombstoneResp),
    MerkleSummary(MerkleSummaryResp),
    MerkleDiff(MerkleDiffResp),
    FetchRecord(FetchRecordResp),
}

/// Outer DHT framing wrapper. Per §2.1, the wire grammar is open to
/// non-RPC traffic in the future (gossip, capability bits) — keeping the
/// `Request` / `Response` discriminator at the *outer* level lets new
/// non-RPC variants slot in without breaking the existing per-variant
/// payload codecs.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DhtPacket {
    Request(DhtRequest),
    Response(DhtResponse),
}

//===:===:===:===:===:===:===:===:===:===:===:===:===||
//===:===:===:===:===:===:  TESTS  :===:===:===:===:==||
//===:===:===:===:===:===:===:===:===:===:===:===:===||

#[cfg(all(test, feature = "crypto"))]
mod tests {
    use std::cmp::Ordering;

    use chacha20poly1305::aead::OsRng;
    use ed25519_dalek::Signer;
    use ed25519_dalek::SigningKey;

    use super::*;
    use crate::PROTOCOL_VERSION;
    use crate::proto::pack::Packer;
    use crate::proto::pack::Unpacker;
    use crate::quic::id::NodeId;

    /// Mint a fresh Ed25519 keypair via OS-RNG. Mirrors the existing
    /// `crypto::get_signing_key` pattern at `common/src/crypto/mod.rs:36`
    /// — `chacha20poly1305::aead::OsRng` is the rand_core-0.6 CSPRNG
    /// that `ed25519-dalek 2.x::SigningKey::generate` expects.
    fn fresh_signing_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    /// Build a fresh, internally-consistent presence record signed by
    /// `user` (as the IPK identity) and `relay` (as the relay
    /// countersignature). All other fields are sane defaults.
    fn build_record(
        user: &SigningKey, relay: &SigningKey, generation: u64, not_before: u64,
        ttl_ms: u64,
    ) -> PresenceRecord {
        let user_ipk: [u8; 32] = user.verifying_key().to_bytes();
        let relay_pubkey: [u8; 32] = relay.verifying_key().to_bytes();
        let relay_id = NodeId::new(relay_pubkey);
        let not_after = not_before + ttl_ms;
        let capabilities: u16 = 0;

        let user_msg =
            presence_record_user_signing_input(&user_ipk, &relay_id, generation);
        let user_sig = user.sign(&user_msg);

        let relay_msg = presence_record_relay_signing_input(
            &user_ipk,
            &relay_id,
            &relay_pubkey,
            not_before,
            not_after,
            generation,
            capabilities,
            &user_sig.to_bytes(),
        );
        let relay_sig = relay.sign(&relay_msg);

        PresenceRecord {
            user_ipk: user_ipk.into(),
            relay_id,
            relay_pubkey: relay_pubkey.into(),
            not_before,
            not_after,
            generation,
            capabilities,
            user_sig: user_sig.to_bytes().into(),
            relay_sig: relay_sig.to_bytes().into(),
        }
    }

    #[test]
    fn presence_record_round_trip_and_verify() {
        let user = fresh_signing_key();
        let relay = fresh_signing_key();

        let now: u64 = 1_700_000_000_000;
        let rec = build_record(&user, &relay, /* gen */ 1, /* nb */ now, /* ttl */ 600_000);

        // Serialize → deserialize. Postcard is the wire format for
        // every other proto module; staying consistent.
        let bytes = rec.ser().expect("postcard serialize");
        let decoded = PresenceRecord::deser(&bytes).expect("postcard deserialize");
        assert_eq!(decoded, rec);

        // verify() with a `now` inside the validity window must pass.
        decoded.verify(now + 1).expect("freshly-signed record must verify");
    }

    #[test]
    fn presence_record_tampered_user_ipk_fails_user_sig() {
        let user = fresh_signing_key();
        let relay = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let rec = build_record(&user, &relay, 1, now, 600_000);

        // Flip a byte in user_ipk. The user_sig was bound to the
        // *original* ipk via the user-roam transcript, so the verify
        // step recomputes the transcript with the tampered ipk and
        // fails. (Note: verify() also re-derives relay_id from
        // relay_pubkey, so tampering with user_ipk hits user_sig
        // checking before reaching the relay_sig stage.)
        let mut tampered = rec.clone();
        tampered.user_ipk.0[0] ^= 0x01;

        match tampered.verify(now + 1) {
            Err(PresenceVerifyError::BadUserSig) => {}
            other => panic!("expected BadUserSig, got {other:?}"),
        }
    }

    #[test]
    fn presence_record_tampered_timestamp_fails_relay_sig() {
        let user = fresh_signing_key();
        let relay = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let rec = build_record(&user, &relay, 1, now, 600_000);

        // Tamper with not_after (a relay-signed-only field). user_sig
        // does not cover not_after, so verification reaches the
        // relay_sig stage and fails there.
        let mut tampered = rec.clone();
        tampered.not_after = tampered.not_after + 1;

        match tampered.verify(now + 1) {
            Err(PresenceVerifyError::BadRelaySig) => {}
            other => panic!("expected BadRelaySig, got {other:?}"),
        }
    }

    #[test]
    fn presence_record_expired_window_fails() {
        let user = fresh_signing_key();
        let relay = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        // ttl = 1 ms so the record is well past not_after at `now + 10`.
        let rec = build_record(&user, &relay, 1, now, 1);

        match rec.verify(now + 10) {
            Err(PresenceVerifyError::Expired) => {}
            other => panic!("expected Expired, got {other:?}"),
        }
    }

    #[test]
    fn presence_record_far_future_fails_not_yet_valid() {
        let user = fresh_signing_key();
        let relay = fresh_signing_key();
        // not_before is 10 minutes in the future; verify is called with
        // a `now` well below it, exceeding the 60-second skew.
        let now: u64 = 1_700_000_000_000;
        let nb = now + 600_000;
        let rec = build_record(&user, &relay, 1, nb, 600_000);

        match rec.verify(now) {
            Err(PresenceVerifyError::NotYetValid) => {}
            other => panic!("expected NotYetValid, got {other:?}"),
        }
    }

    #[test]
    fn presence_record_relay_id_mismatch_fails() {
        let user = fresh_signing_key();
        let relay_a = fresh_signing_key();
        let relay_b = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let mut rec = build_record(&user, &relay_a, 1, now, 600_000);

        // Replace relay_pubkey with a *different* relay's pubkey while
        // keeping the original (relay_a-derived) relay_id. This
        // simulates an attacker presenting a benign-looking pubkey for
        // signature verification while smuggling a different identity.
        rec.relay_pubkey = relay_b.verifying_key().to_bytes().into();
        match rec.verify(now + 1) {
            Err(PresenceVerifyError::RelayIdMismatch) => {}
            other => panic!("expected RelayIdMismatch, got {other:?}"),
        }
    }

    #[test]
    fn presence_record_compare_orders_by_generation_desc() {
        let user = fresh_signing_key();
        let relay = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let r_low = build_record(&user, &relay, /* gen */ 1, now, 600_000);
        let r_high = build_record(&user, &relay, /* gen */ 2, now, 600_000);

        // Higher generation wins per §5.3.
        assert_eq!(r_high.compare(&r_low), Ordering::Greater);
        assert_eq!(r_low.compare(&r_high), Ordering::Less);
    }

    #[test]
    fn presence_record_compare_breaks_tie_by_not_before_then_relay_id() {
        let user = fresh_signing_key();
        let relay_a = fresh_signing_key();
        let relay_b = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;

        // Same generation, different not_before — fresher republish
        // wins.
        let older = build_record(&user, &relay_a, 5, now, 600_000);
        let fresher = build_record(&user, &relay_a, 5, now + 1000, 600_000);
        assert_eq!(fresher.compare(&older), Ordering::Greater);

        // Same generation, same not_before, different relay_id — lex
        // tiebreak on the full 32-byte id.
        let r1 = build_record(&user, &relay_a, 5, now, 600_000);
        let r2 = build_record(&user, &relay_b, 5, now, 600_000);
        let expected = r1.relay_id.as_bytes().cmp(r2.relay_id.as_bytes());
        assert_eq!(r1.compare(&r2), expected);
        // Comparison is total: never returns Equal between two
        // distinct relay_ids generated from independent keypairs.
        assert_ne!(expected, Ordering::Equal);
    }

    #[test]
    fn user_signing_input_layout_is_stable() {
        // Pin the byte-layout of the user transcript so a future
        // refactor that subtly reorders fields blows up here, not
        // weeks later in production-style "all signatures are
        // suddenly invalid" mode.
        let ipk = [0u8; 32];
        let mut bytes = [0u8; 32];
        bytes[0] = 0x42;
        let relay_id = RelayId::from_bytes(bytes);
        let generation: u64 = 0xDEAD_BEEF_CAFE_F00D;

        let buf = presence_record_user_signing_input(&ipk, &relay_id, generation);

        // Domain (22) + version (2) + ipk (32) + relay_id (32) + gen
        // (8) = 96 bytes. Anchor on the total length so a stray field
        // change is caught immediately.
        assert_eq!(buf.len(), DHT_USER_ROAM_SIG_DOMAIN.len() + 2 + 32 + 32 + 8);

        // Spot-check the header.
        assert!(buf.starts_with(DHT_USER_ROAM_SIG_DOMAIN));
        let off = DHT_USER_ROAM_SIG_DOMAIN.len();
        assert_eq!(&buf[off..off + 2], &PROTOCOL_VERSION.to_be_bytes());
    }

    #[test]
    fn dht_packet_round_trip_for_every_request_variant() {
        // Smoke-test postcard serialization across every RPC variant
        // — catches missing serde derives or accidental
        // non-`Deserialize`-able fields. Uses dummy data; we only
        // care about the codec round-trip.
        let pubkey = [0u8; 32];
        let id = RelayId::from_bytes([0u8; 32]);
        let dummy_addr: SocketAddr = "127.0.0.1:1".parse().unwrap();
        let dummy_node = NodeDescriptor {
            id,
            addr: dummy_addr,
            pubkey: pubkey.into(),
        };
        let dummy_ipk = Bytes::<32>::from([0u8; 32]);
        let dummy_record = build_record(
            &fresh_signing_key(),
            &fresh_signing_key(),
            1,
            1_700_000_000_000,
            600_000,
        );

        let cases = vec![
            DhtRequest::Ping(Ping {
                nonce:     [0u8; 16].into(),
                timestamp: 1,
            }),
            DhtRequest::FindNode(FindNode {
                target:    [1u8; 32].into(),
                requester: id,
            }),
            DhtRequest::FindValue(FindValue {
                user_ipk:  dummy_ipk,
                requester: id,
            }),
            DhtRequest::Store(Store { record: dummy_record.clone() }),
            DhtRequest::Tombstone(Tombstone {
                record: TombstoneRecord {
                    user_ipk:     dummy_ipk,
                    relay_id:     id,
                    relay_pubkey: pubkey.into(),
                    generation:   1,
                    deleted_at:   1,
                    relay_sig:    [0u8; 64].into(),
                },
            }),
            DhtRequest::MerkleSummary(MerkleSummary { slices: [0u8; 32].into() }),
            DhtRequest::MerkleDiff(MerkleDiff {
                slice_id: 0,
                path:     vec![1, 2, 3],
            }),
            DhtRequest::FetchRecord(FetchRecord {
                user_ipks: vec![dummy_ipk],
            }),
        ];

        let responses = vec![
            DhtResponse::Pong(Pong {
                nonce:     [0u8; 16].into(),
                timestamp: 1,
            }),
            DhtResponse::FindNode(FindNodeResp { closer: vec![dummy_node.clone()] }),
            DhtResponse::FindValue(FindValueResp {
                result: FindValueOutcome::Found(dummy_record.clone()),
            }),
            DhtResponse::Store(StoreResp { outcome: StoreOutcome::Stored }),
            DhtResponse::Tombstone(TombstoneResp {
                outcome: TombstoneOutcome::Stored,
            }),
            DhtResponse::MerkleSummary(MerkleSummaryResp {
                roots: vec![(0, [0u8; 32].into())],
            }),
            DhtResponse::MerkleDiff(MerkleDiffResp::Children {
                hashes: vec![[0u8; 32].into(); MERKLE_FANOUT],
            }),
            DhtResponse::FetchRecord(FetchRecordResp {
                records:    vec![dummy_record],
                tombstones: Vec::new(),
            }),
        ];

        for req in cases {
            let pkt = DhtPacket::Request(req.clone());
            let bytes = pkt.ser().expect("serialize request");
            let decoded = DhtPacket::deser(&bytes).expect("deserialize request");
            assert_eq!(decoded, pkt);
        }
        for resp in responses {
            let pkt = DhtPacket::Response(resp.clone());
            let bytes = pkt.ser().expect("serialize response");
            let decoded = DhtPacket::deser(&bytes).expect("deserialize response");
            assert_eq!(decoded, pkt);
        }
    }

    // -----------------------------------------------------------------
    // DhtHello — phase 1i (relay-to-relay connection-level handshake)
    // -----------------------------------------------------------------

    /// Sign a fresh, internally-consistent [`DhtHello`] with `key` at
    /// `timestamp`. Mirrors the dialer-side construction in
    /// `relay/src/dht/lookup.rs::send_dht_hello` so any drift between
    /// the production helper and the test fixture immediately blows up
    /// either side's verification.
    fn build_dht_hello(key: &SigningKey, timestamp: u64) -> DhtHello {
        let pubkey: [u8; 32] = key.verifying_key().to_bytes();
        let node_id = NodeId::new(pubkey);
        let msg = dht_hello_signing_input(&node_id, &pubkey, timestamp);
        let sig = key.sign(&msg);
        DhtHello {
            node_id,
            pubkey: pubkey.into(),
            timestamp,
            sig: sig.to_bytes().into(),
        }
    }

    #[test]
    fn dht_hello_round_trip() {
        // postcard encode → decode round-trip: catches any accidental
        // missing serde-derive or non-Deserialize-able field.
        let key = fresh_signing_key();
        let hello = build_dht_hello(&key, 1_700_000_000_000);
        let bytes = hello.ser().expect("postcard serialize");
        let decoded = DhtHello::deser(&bytes).expect("postcard deserialize");
        assert_eq!(decoded, hello);
    }

    #[test]
    fn dht_hello_signing_input_layout_is_stable() {
        // Pin the byte-layout of the transcript so a future refactor
        // that subtly reorders fields blows up here, not weeks later
        // in production "all hellos suddenly fail" mode. Mirrors
        // `user_signing_input_layout_is_stable` above.
        let pubkey = [0u8; 32];
        let mut bytes = [0u8; 32];
        bytes[0] = 0x42;
        let node_id = NodeId::from_bytes(bytes);
        let timestamp: u64 = 0xDEAD_BEEF_CAFE_F00D;

        let buf = dht_hello_signing_input(&node_id, &pubkey, timestamp);

        // Domain (20) + version (2) + node_id (32) + pubkey (32) +
        // ts (8) = 94 bytes. Anchor on the total length so a stray
        // field change is caught immediately.
        assert_eq!(buf.len(), DHT_HELLO_SIG_DOMAIN.len() + 2 + 32 + 32 + 8);

        // Spot-check the header.
        assert!(buf.starts_with(DHT_HELLO_SIG_DOMAIN));
        let off = DHT_HELLO_SIG_DOMAIN.len();
        assert_eq!(&buf[off..off + 2], &PROTOCOL_VERSION.to_be_bytes());
        let off = off + 2;
        assert_eq!(&buf[off..off + 32], node_id.as_bytes());
        let off = off + 32;
        assert_eq!(&buf[off..off + 32], &pubkey);
        let off = off + 32;
        assert_eq!(&buf[off..off + 8], &timestamp.to_be_bytes());
    }

    #[test]
    fn dht_hello_verify_accepts_freshly_signed() {
        let key = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let hello = build_dht_hello(&key, now);
        // ±0 skew → must accept.
        hello.verify(now).expect("freshly-signed hello must verify");
        // Inside the skew window → must accept.
        hello.verify(now + MAX_DHT_HELLO_SKEW_MS - 1).expect("inside skew");
        hello.verify(now - (MAX_DHT_HELLO_SKEW_MS - 1)).expect("inside skew");
    }

    #[test]
    fn dht_hello_verify_rejects_bad_pubkey_to_id_binding() {
        // Sign with `key_a` but claim `key_b`'s NodeId. Catches the
        // attacker-presenting-a-benign-pubkey-under-different-id case
        // (mirror of `presence_record_relay_id_mismatch_fails` above).
        let key_a = fresh_signing_key();
        let key_b = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let mut hello = build_dht_hello(&key_a, now);
        // Replace node_id with a *different* identity's id while keeping
        // the original (a-derived) pubkey + sig.
        let fake_id = NodeId::new(key_b.verifying_key().to_bytes());
        hello.node_id = fake_id;
        match hello.verify(now) {
            Err(DhtHelloVerifyError::IdMismatch) => {}
            other => panic!("expected IdMismatch, got {other:?}"),
        }
    }

    #[test]
    fn dht_hello_verify_rejects_stale_timestamp() {
        // Timestamp ~2 minutes in the past — far beyond the 60s skew.
        let key = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let stale_ts = now - 120_000;
        let hello = build_dht_hello(&key, stale_ts);
        match hello.verify(now) {
            Err(DhtHelloVerifyError::ClockSkew) => {}
            other => panic!("expected ClockSkew, got {other:?}"),
        }
    }

    #[test]
    fn dht_hello_verify_rejects_future_timestamp() {
        // Timestamp ~2 minutes in the future — far beyond the 60s skew.
        let key = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let future_ts = now + 120_000;
        let hello = build_dht_hello(&key, future_ts);
        match hello.verify(now) {
            Err(DhtHelloVerifyError::ClockSkew) => {}
            other => panic!("expected ClockSkew, got {other:?}"),
        }
    }

    #[test]
    fn dht_hello_verify_rejects_bad_signature() {
        // Flip one bit in the signature — verify must fail.
        let key = fresh_signing_key();
        let now: u64 = 1_700_000_000_000;
        let mut hello = build_dht_hello(&key, now);
        hello.sig.0[0] ^= 0x01;
        match hello.verify(now) {
            Err(DhtHelloVerifyError::BadSignature) => {}
            other => panic!("expected BadSignature, got {other:?}"),
        }
    }

    #[test]
    fn dht_hello_two_relays_authenticate_each_other_synchronously() {
        // Integration-style smoke test: two `Dht`-equivalent identity
        // keys; one constructs a hello, the other verifies it. The
        // resulting authenticated NodeId matches the claimed (and
        // signed) identity. Phase 2 will exercise the actual QUIC
        // round-trip (deferred per dispatch).
        let dialer_key = fresh_signing_key();
        let receiver_key = fresh_signing_key();
        // dialer != receiver — receiver verifies dialer's hello.
        assert_ne!(
            dialer_key.verifying_key().to_bytes(),
            receiver_key.verifying_key().to_bytes()
        );

        let now: u64 = 1_700_000_000_000;
        let hello = build_dht_hello(&dialer_key, now);

        // Wire round-trip (would be a quinn uni-stream in production).
        let bytes = hello.ser().expect("serialize");
        let received = DhtHello::deser(&bytes).expect("deserialize");

        // Receiver verifies and stashes `received.node_id` as the
        // connection's authenticated identity.
        received.verify(now + 5).expect("hello must verify");
        let authenticated_id = received.node_id;
        assert_eq!(
            authenticated_id,
            NodeId::new(dialer_key.verifying_key().to_bytes())
        );
    }
}
