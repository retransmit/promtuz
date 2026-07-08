//! Unix-socket control channel: lets `pzrelay <subcommand>` drive the running
//! daemon, which holds the fjall single-writer lock a second process can't take.
//! Today it serves `clear-db`; the dispatch is a plain line protocol so more
//! commands (info, reload, …) drop in as new match arms.

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use common::info;
use common::warn;
use tokio::io::AsyncBufReadExt;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::io::BufReader;
use tokio::net::UnixListener;
use tokio::net::UnixStream;
use tokio_util::sync::CancellationToken;

use crate::storage::db::Store;

/// Daemon side: bind the control socket at `sock` and dispatch commands until
/// cancelled. Best-effort — a bind failure is logged and the daemon runs on.
pub async fn serve(store: Arc<Store>, sock: PathBuf, cancel: CancellationToken) {
    if let Some(parent) = sock.parent() {
        let _ = std::fs::create_dir_all(parent); // no-op for /run/pzrelay (RuntimeDirectory)
    }
    let _ = std::fs::remove_file(&sock); // clear a stale socket from a crash
    let listener = match UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(e) => {
            warn!("control socket bind {} failed: {e}", sock.display());
            return;
        },
    };

    // `clear-db` is destructive and the protocol is unauthenticated, so the
    // socket's file mode IS the authz: 0600 restricts it to the daemon's own
    // uid. Root (an admin's `sudo pzrelay clear-db`) bypasses this; any other
    // local user is denied. Fail closed — never serve a world-reachable wipe.
    if let Err(e) = std::fs::set_permissions(&sock, std::fs::Permissions::from_mode(0o600)) {
        warn!("control socket chmod failed: {e}; refusing to serve");
        let _ = std::fs::remove_file(&sock);
        return;
    }
    info!("control socket at {}", sock.display());

    loop {
        tokio::select! {
            _ = cancel.cancelled() => break,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let store = store.clone();
                    tokio::spawn(async move {
                        if let Err(e) = handle_conn(stream, store).await {
                            warn!("control conn: {e}");
                        }
                    });
                },
                Err(e) => warn!("control accept: {e}"),
            },
        }
    }
    let _ = std::fs::remove_file(&sock);
}

async fn handle_conn(mut stream: UnixStream, store: Arc<Store>) -> Result<()> {
    let (rd, mut wr) = stream.split();
    let mut cmd = String::new();
    BufReader::new(rd).read_line(&mut cmd).await.context("read command")?;

    let reply = match cmd.trim() {
        "clear-db" => match store.clear_all() {
            Ok(n) => format!("ok: cleared {n} entries\n"),
            Err(e) => format!("error: clear-db: {e}\n"),
        },
        other => format!("error: unknown command '{other}'\n"),
    };
    wr.write_all(reply.as_bytes()).await.context("write reply")?;
    Ok(())
}

/// Client side of `pzrelay clear-db`: send the command, print the daemon's reply.
pub async fn clear_db_client(sock: &Path) -> Result<()> {
    let mut stream = UnixStream::connect(sock)
        .await
        .with_context(|| format!("connect {} — is the relay running?", sock.display()))?;
    stream.write_all(b"clear-db\n").await.context("send command")?;

    let mut reply = String::new();
    stream.read_to_string(&mut reply).await.context("read reply")?;
    print!("{reply}");
    if reply.starts_with("error") {
        anyhow::bail!("clear-db failed");
    }
    Ok(())
}
