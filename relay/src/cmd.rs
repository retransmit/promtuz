//! One-shot subcommands that run instead of the daemon (no endpoint, no wait).

use std::io::Read;

use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use common::node::enroll::cert_is_valid;
use common::node::enroll::csr_pem;
use common::node::enroll::validate_cert_pem;
use common::quic::config::setup_crypto_provider;
use common::quic::id::NodeId;
use common::quic::p256::secret_from_key;

use crate::util::config::AppConfig;

/// `pzrelay enroll`: if already enrolled, say so; otherwise print the CSR and
/// install a signed cert pasted on stdin (no file fiddling). A running relay's
/// cert-watcher picks the new cert up automatically.
pub fn enroll(cfg: &AppConfig) -> Result<()> {
    let _ = setup_crypto_provider();
    let net = &cfg.network;

    // Load-only: never mint a key here (that would be root-owned and unreadable
    // by the pzrelay service). The daemon generates it on first start.
    let signing = secret_from_key(&net.key_path).map_err(|_| {
        anyhow!(
            "no node key at {} — start the relay once to generate it",
            net.key_path.display()
        )
    })?;
    let key_pub = signing.verifying_key().to_bytes();
    let node_id = NodeId::new(key_pub);

    if cert_is_valid(&net.cert_path, &net.root_ca_path, &node_id, &key_pub).unwrap_or(false) {
        println!("already enrolled — {} certifies node {node_id}", net.cert_path.display());
        return Ok(());
    }

    println!("{}", csr_pem(&signing, &node_id));
    eprintln!("↑ CSR for node {node_id}");
    eprintln!("Sign it (certgen sign), paste the signed cert below, then Ctrl-D:");

    let mut pem = String::new();
    std::io::stdin().read_to_string(&mut pem).context("reading cert from stdin")?;
    if pem.trim().is_empty() {
        bail!("no cert pasted");
    }

    // Reject a bad paste before it hits disk (a running daemon would otherwise
    // wake on the write and re-reject it).
    validate_cert_pem(pem.as_bytes(), &net.root_ca_path, &node_id, &key_pub)
        .context("pasted cert rejected")?;
    std::fs::write(&net.cert_path, &pem)
        .with_context(|| format!("writing {}", net.cert_path.display()))?;

    println!("enrolled — wrote {}. A running relay starts serving automatically.", net.cert_path.display());
    Ok(())
}
