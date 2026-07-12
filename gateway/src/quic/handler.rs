use std::sync::Arc;

use common::debug;
use common::info;
use common::proto::pack::Unpacker;
use common::proto::push::PushRequest;
use common::quic::protorole::ProtoRole;
use common::warn;
use quinn::Connection;

use crate::gateway::Gateway;

/// Per-connection handler. Serves the one-RPC-per-bi-stream contract (mirrors
/// the resolver's client handler): each accepted bi-stream is one
/// [`PushRequest`].
///
/// `Register` (devices, `client/1`) verifies + stores `P → token`. `Wake`
/// (home relays, `relay/1`) resolves `P → token`; the actual APNs/FCM dispatch
/// is the next cut.
pub struct Handler;

impl Handler {
    pub async fn handle(conn: Connection, gateway: Arc<Gateway>) {
        let addr = conn.remote_address();
        debug!("incoming conn from {addr}");

        // Only devices (`client/1`, registration) and home relays (`relay/1`,
        // wake) talk to the gateway. Anything else is closed.
        match ProtoRole::from_conn(&conn) {
            Some(ProtoRole::Client | ProtoRole::Relay) => {},
            Some(_) => return conn.close(0u32.into(), b"UnsupportedALPN"),
            None => return conn.close(0u32.into(), b"NoALPN"),
        }

        while let Ok((_send, mut recv)) = conn.accept_bi().await {
            match PushRequest::unpack(&mut recv).await {
                Ok(PushRequest::Register(reg)) => match gateway.registry.register(&reg) {
                    Ok(()) => debug!("gateway: registered pseudonym from {addr}"),
                    Err(e) => warn!("gateway: rejected registration from {addr}: {e}"),
                },
                Ok(PushRequest::Wake(req)) => match gateway.registry.resolve(&req.pseudonym.0) {
                    // ponytail: dispatch (FCM HTTP v1) is the next cut — for now
                    // we prove the P→token resolve half.
                    Some(entry) => info!(
                        "gateway: wake for known pseudonym → {:?} ({} B token), \
                         {} B payload — dispatch pending",
                        entry.provider,
                        entry.token.len(),
                        req.payload.len(),
                    ),
                    None => warn!("gateway: wake for unknown pseudonym from {addr}"),
                },
                Err(e) => {
                    warn!("gateway: request decode failed from {addr}: {e}");
                    break;
                },
            }
        }
    }
}
