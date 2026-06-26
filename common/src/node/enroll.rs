//! Node enrollment: load-or-create the single Ed25519 key, validate the
//! CA-issued cert, or emit a CSR and wait. Shared by relay and resolver.

use std::path::Path;
use std::sync::Arc;

use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::ServerCertVerifier;
use rustls::pki_types::CertificateDer;
use rustls::pki_types::ServerName;
use rustls::pki_types::UnixTime;

use base64::Engine as _;
use ed25519_dalek::Signer;
use ed25519_dalek::SigningKey;

use crate::quic::config::load_root_ca;
use crate::quic::id::NodeId;

/// Pull the 32-byte Ed25519 SPKI out of a DER cert. Ed25519 leaf certs carry
/// exactly one `03 21 00` (33-byte) BIT STRING — the pubkey (the signature is
/// `03 41 00`). Mirrors the hand-rolled DER in [`crate::quic::config`].
pub fn spki_ed25519(cert_der: &[u8]) -> Option<[u8; 32]> {
    let needle = [0x03, 0x21, 0x00];
    let pos = cert_der.windows(3).position(|w| w == needle)? + 3;
    cert_der.get(pos..pos + 32)?.try_into().ok()
}

fn first_cert_der(cert_path: &Path) -> Option<CertificateDer<'static>> {
    let pem = std::fs::read(cert_path).ok()?;
    let mut rd = std::io::BufReader::new(&pem[..]);
    rustls_pemfile::certs(&mut rd).flatten().next()
}

/// True iff `cert_path` exists, chains to `ca_path`, is unexpired, names
/// `node_id`, and its SPKI is our `key_pub` (i.e. it is *our* cert).
///
/// Requires the process crypto provider to be installed; the caller
/// ([`ensure_enrolled`]) does this via `setup_crypto_provider`.
pub fn cert_is_valid(
    cert_path: &Path, ca_path: &Path, node_id: &NodeId, key_pub: &[u8; 32],
) -> bool {
    let Some(leaf) = first_cert_der(cert_path) else {
        return false;
    };

    // It must certify *our* key, not just any CA-signed key.
    if spki_ed25519(leaf.as_ref()).as_ref() != Some(key_pub) {
        return false;
    }

    let Ok(roots) = load_root_ca(&ca_path.to_path_buf()) else {
        return false;
    };
    let Ok(verifier) = WebPkiServerVerifier::builder(Arc::new(roots)).build() else {
        return false;
    };
    let Ok(server_name) = ServerName::try_from(node_id.to_string()) else {
        return false;
    };

    verifier
        .verify_server_cert(&leaf, &[], &server_name, &[], UnixTime::now())
        .is_ok()
}

// ---------------------------------------------------------------------------
// PKCS#10 CSR generation (hand-rolled Ed25519 DER; mirrors the self-signed
// cert DER in `quic::config`). `certgen sign` re-derives CN/SAN from the
// pubkey, so the request carries only version + subject(CN) + SPKI + empty
// attributes, self-signed (proof of possession).
// ---------------------------------------------------------------------------

const ED25519_AID: &[u8] = &[0x06, 0x03, 0x2b, 0x65, 0x70];

/// Minimal DER tag-length-value with short/long-form length.
fn tlv(tag: u8, body: &[u8]) -> Vec<u8> {
    let n = body.len();
    if n < 128 {
        [&[tag, n as u8][..], body].concat()
    } else if n < 256 {
        [&[tag, 0x81, n as u8][..], body].concat()
    } else {
        [&[tag, 0x82][..], &(n as u16).to_be_bytes(), body].concat()
    }
}

fn spki_der(pubkey: &[u8; 32]) -> Vec<u8> {
    [
        &[0x30, 0x2a][..],
        &[0x30, 0x05][..],
        ED25519_AID,
        &[0x03, 0x21, 0x00][..],
        pubkey,
    ]
    .concat()
}

/// CertificationRequestInfo: version(0) + subject(CN) + SPKI + empty attrs.
fn csr_info(pubkey: &[u8; 32], cn: &str) -> Vec<u8> {
    let version: &[u8] = &[0x02, 0x01, 0x00];
    let cn_oid: &[u8] = &[0x06, 0x03, 0x55, 0x04, 0x03]; // id-at-commonName
    let cn_utf8 = [&[0x0c, cn.len() as u8][..], cn.as_bytes()].concat();
    // Name ::= SEQUENCE OF RDN; RDN ::= SET OF AttributeTypeAndValue
    let subject = tlv(0x30, &tlv(0x31, &tlv(0x30, &[cn_oid, &cn_utf8].concat())));
    let attributes: &[u8] = &[0xa0, 0x00]; // [0] IMPLICIT SET OF, empty
    tlv(0x30, &[version, &subject, &spki_der(pubkey), attributes].concat())
}

fn csr_der(info: &[u8], sig: &[u8; 64]) -> Vec<u8> {
    let sig_alg = tlv(0x30, ED25519_AID);
    let sig_bits = [&[0x03, 0x41, 0x00][..], sig].concat();
    tlv(0x30, &[info, &sig_alg, &sig_bits].concat())
}

fn pem_wrap(label: &str, der: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(der);
    let mut out = format!("-----BEGIN {label}-----\n");
    for chunk in b64.as_bytes().chunks(64) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push('\n');
    }
    out.push_str("-----END ");
    out.push_str(label);
    out.push_str("-----\n");
    out
}

/// Write a PKCS#10 CSR (PEM) for `signing`'s key, `CN = base32(node_id)`.
pub fn emit_csr(
    csr_path: &Path, signing: &SigningKey, node_id: &NodeId,
) -> std::io::Result<()> {
    let pubkey = signing.verifying_key().to_bytes();
    let info = csr_info(&pubkey, &node_id.to_string());
    let sig = signing.sign(&info).to_bytes();
    let der = csr_der(&info, &sig);
    std::fs::write(csr_path, pem_wrap("CERTIFICATE REQUEST", &der))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_missing_cert() {
        let id = NodeId::new(&[7u8; 32]);
        assert!(!cert_is_valid(
            Path::new("/nonexistent.crt"),
            Path::new("/nonexistent_ca.pem"),
            &id,
            &[7u8; 32],
        ));
    }

    #[cfg(feature = "certgen")]
    #[test]
    fn csr_parses_with_rcgen() {
        let key = SigningKey::from_bytes(&[3u8; 32]);
        let id = NodeId::new(&key.verifying_key().to_bytes());
        let path = std::env::temp_dir().join("pz_csr_roundtrip.csr");
        emit_csr(&path, &key, &id).unwrap();
        let pem = std::fs::read_to_string(&path).unwrap();
        // from_pem validates the PKCS#10 structure AND the self-signature.
        rcgen::CertificateSigningRequestParams::from_pem(&pem)
            .expect("rcgen parses our hand-rolled CSR");
        let _ = std::fs::remove_file(&path);
    }
}
