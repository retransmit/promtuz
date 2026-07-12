use anyhow::Result;
use common::debug;
use common::proto::Sender;
use common::proto::client_rel::QueryP;
use common::proto::client_rel::QueryResultP;
use common::proto::client_rel::SRelayPacket;
use quinn::SendStream;

use crate::quic::handler::client::ClientCtxHandle;

pub(super) async fn handle_misc(
    packet: QueryP, ctx: ClientCtxHandle, tx: &mut SendStream,
) -> Result<()> {
    use QueryP::*;
    use SRelayPacket::*;

    match packet {
        PubAddress => {
            let addr = ctx.conn.remote_address();

            use QueryResultP::*;

            QueryResult(PubAddress { addr }).send(tx).await.map_err(|e| e.into())
        },
    }
}

/// Store `IPK → P` so the DHT enqueue path can wake this device. Bound to the
/// connection-authenticated `ctx.ipk`; the client cannot register for another
/// IPK. Not cleared on disconnect (an offline device is exactly the one to
/// wake). Fire-and-forget — no reply.
pub(super) async fn handle_register_push(pseudonym: [u8; 32], ctx: ClientCtxHandle) -> Result<()> {
    ctx.relay.push_pseudonyms.write().insert(ctx.ipk.to_bytes(), pseudonym);
    debug!("client({}) registered push-pseudonym", ctx.conn.remote_address());
    Ok(())
}
