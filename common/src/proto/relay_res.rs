//! Relay to Resolver Proto

use std::fmt::Debug;

use serde::Deserialize;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::proto::RelayId;
use crate::proto::pack::Packer;
use crate::sysutils::SystemLoad;
use crate::trace;
use crate::types::bytes::Bytes;

/// Domain separation tag mixed into the [`LifetimeP::RelayHello`] signed
/// transcript. Bumping this value forces a clean wire-format break.
pub const RELAY_HELLO_SIG_DOMAIN: &[u8] = b"promtuz-relay-hello-v1";

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum LifetimeP {
    /// Initial registration message sent by a relay node to a resolver.
    ///
    /// Carries the relay's full Ed25519 identity public key alongside the
    /// truncated [`RelayId`] so the resolver can both verify the id-to-key
    /// binding (BLAKE3 truncation) and check the attached signature.
    ///
    /// `sig` is an Ed25519 signature over:
    /// `RELAY_HELLO_SIG_DOMAIN || PROTOCOL_VERSION (BE u16)
    ///   || relay_id (10 bytes) || pubkey (32 bytes) || timestamp (BE u128)`
    RelayHello {
        /// Stable cryptographic ID derived from the node's public key.
        relay_id:  RelayId,
        /// Full Ed25519 identity public key of the relay. Required so the
        /// resolver can recover the verification key from the wire — the
        /// `relay_id` alone is a BLAKE3 truncation and is not invertible.
        pubkey:    Bytes<32>,
        timestamp: u128,
        /// Ed25519 signature over the transcript described on this enum.
        sig:       Bytes<64>,
        // TODO: I'd rather use bitset
        // pub capabilities: Vec<String>,
    },

    /// Resolver's acknowledgement of a node registration (`NodeHello`).
    ///
    /// Confirms acceptance, conveys heartbeat timing, or explains rejection.
    HelloAck {
        /// Resolver's current unix time (used for clock-drift checking).
        resolver_time: u128,
    },

    /// Periodic heartbeat sent by a node to indicate that it is still alive
    /// and to provide useful runtime metrics to the resolver.
    RelayHeartbeat {
        /// The node's stable cryptographic ID.
        relay_id: RelayId,

        /// Packed load value:
        ///
        /// upper 7 bits = CPU usage (0–100), lower 7 bits = memory usage (0–100).
        load: SystemLoad,

        /// Node uptime in seconds since its last restart.
        uptime_seconds: u64,
    },
}

/// Builds the canonical signing transcript for [`LifetimeP::RelayHello`].
///
/// Both the relay (signing side) and the resolver (verifying side) call
/// this to derive the exact byte string fed to Ed25519 — using a single
/// helper keeps the two sides byte-for-byte identical.
pub fn relay_hello_signing_input(
    relay_id: &RelayId, pubkey: &[u8; 32], timestamp: u128,
) -> Vec<u8> {
    let mut buf = Vec::with_capacity(
        RELAY_HELLO_SIG_DOMAIN.len() + 2 + RelayId::LEN + 32 + 16,
    );
    buf.extend_from_slice(RELAY_HELLO_SIG_DOMAIN);
    buf.extend_from_slice(&crate::PROTOCOL_VERSION.to_be_bytes());
    buf.extend_from_slice(relay_id.as_bytes());
    buf.extend_from_slice(pubkey);
    buf.extend_from_slice(&timestamp.to_be_bytes());
    buf
}

#[derive(Serialize, Deserialize, Debug, PartialEq, Eq)]
pub enum ResolverPacket {
    Lifetime(LifetimeP),
}

impl ResolverPacket {
    pub async fn send(self, tx: &mut (impl AsyncWriteExt + Unpin)) -> anyhow::Result<()> {
        let packet = self.pack()?;

        trace!("sent packet {}", hex::encode(&packet));

        tx.write_all(&packet).await?;
        Ok(tx.flush().await?)
    }
}
