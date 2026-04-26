use common::debug;
use common::proto::client_res::ClientRequest;
use common::proto::pack::Packer;
use common::proto::pack::Unpacker;
use common::warn;

use crate::quic::handler::Handler;
use crate::resolver::ResolverRef;
use crate::resolver::rpc::HandleRPC;

pub trait HandleClient {
    async fn handle_client(self, resolver: ResolverRef);
}

impl HandleClient for Handler {
    /// Per-stream contract: **one RPC per bi-stream**. The client opens a
    /// fresh bi-stream per request, sends the request, reads the reply,
    /// and the stream is closed. This keeps state simple, makes
    /// concurrency a per-stream property of QUIC itself, and avoids the
    /// half-closed-stream foot-gun the previous loop suffered from.
    async fn handle_client(self, resolver: ResolverRef) {
        let conn = self.conn.clone();
        let addr = conn.remote_address();

        debug!("incoming client({}) conn", addr);

        loop {
            let (mut send, mut recv) = match conn.accept_bi().await {
                Ok(s) => s,
                Err(_) => break,
            };

            let resolver = resolver.clone();

            tokio::spawn(async move {
                // 1. read one request
                let req = match ClientRequest::unpack(&mut recv).await {
                    Ok(req) => req,
                    Err(e) => {
                        warn!("client({addr}) request decode failed: {e}");
                        return;
                    },
                };

                // 2. dispatch (no lock — Resolver is now Arc<Resolver>)
                let res = match resolver.handle_rpc(req).await {
                    Ok(res) => res,
                    Err(e) => {
                        warn!("client({addr}) rpc handler failed: {e}");
                        return;
                    },
                };

                // 3. encode + write + finish, exactly once
                let packet = match res.pack() {
                    Ok(p) => p,
                    Err(e) => {
                        warn!("client({addr}) response encode failed: {e}");
                        return;
                    },
                };

                if let Err(e) = send.write_all(&packet).await {
                    warn!("client({addr}) response write failed: {e}");
                    return;
                }
                if let Err(e) = send.finish() {
                    warn!("client({addr}) stream finish failed: {e}");
                }
            });
        }
    }
}
