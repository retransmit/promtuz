//! Debug FFI for the P2P transport: a manual "connect directly to this
//! contact" trigger for on-device hole-punch testing. Not a shipping
//! surface — the Android side gates it behind a debug build.

/// Punch a direct connection to `peer_hex` (a contact's 32-byte IPK in
/// hex), prove it with a ping/pong round-trip, and return a human-readable
/// report. Blocking — call it off the UI thread.
#[uniffi::export]
pub fn p2p_debug_connect(peer_hex: String) -> String {
    let Some(peer) = hex::decode(peer_hex.trim())
        .ok()
        .and_then(|v| <[u8; 32]>::try_from(v).ok())
    else {
        return "bad peer hex — want a 32-byte IPK".into();
    };

    crate::RUNTIME.block_on(async move {
        match crate::p2p::connect(peer).await {
            Ok(link) => {
                let addr = link.remote_address();
                match link.verify_roundtrip().await {
                    Ok(()) => format!("OK — direct link to {addr}"),
                    Err(e) => format!("connected to {addr} but round-trip failed: {e}"),
                }
            },
            Err(e) => format!("connect failed: {e}"),
        }
    })
}
