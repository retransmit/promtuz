//! Adapter from a workspace `ed25519_dalek::SigningKey` to openmls's
//! `Signer` trait.
//!
//! # Why a separate signer
//!
//! The **leaf signing key** is distinct from the long-term IPK. Two
//! reasons:
//!
//! 1. MLS specifies that leaf signing keys can rotate (via Update
//!    commits) — IPK cannot.
//! 2. A compromised group state should not give the attacker
//!    `IPK_priv`. Using the IPK as the leaf sig key would couple the
//!    two trust boundaries.
//!
//! So the runtime structure is:
//!
//! - `IdentitySigner` (defined in `data::identity`) signs the
//!   **outer envelope** (`MlsApplicationEnvelopeP::sender_sig`,
//!   `WelcomeEnvelopeP::sender_sig`) under the IPK. This is the
//!   relay-protocol-layer signature. We do *not* implement
//!   openmls's `Signer` trait for it — the IPK never sees the
//!   MLS-internal transcript.
//! - `Ed25519Signer` (this module) holds a fresh
//!   `ed25519_dalek::SigningKey` and implements
//!   `openmls_traits::signatures::Signer`. It signs the
//!   **MLS-internal** content (commits, application messages,
//!   leaf-node binding, …). The credential carries the public half
//!   as the leaf's signature key.
//!
//! # Trait surface
//!
//! `openmls_traits::signatures::Signer` (0.5.0):
//!
//! ```ignore
//! pub trait Signer {
//!     fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, SignerError>;
//!     fn signature_scheme(&self) -> SignatureScheme;
//! }
//! ```
//!
//! Note that `openmls_basic_credential::SignatureKeyPair` already
//! implements this trait. We could reuse it directly — and we do, in
//! tests, for brevity. But we expose our own thin wrapper here so the
//! production code path can hold a `SigningKey` without depending on
//! the basic-credential crate's internal layout (which serialises
//! as bytes and stores both halves verbatim).
//!

// Public surface here is consumed by `messaging.rs`; the cdylib
// compiler can't see across the JNI boundary so flags it as dead.
// Mirrors the `provider.rs` pattern.
#![allow(dead_code)]

use ed25519_dalek::Signer as DalekSigner;
use ed25519_dalek::SigningKey;
use openmls_traits::signatures::{Signer, SignerError};
use openmls_traits::types::SignatureScheme;

/// A wrapper over an `ed25519_dalek::SigningKey` that satisfies
/// openmls's `Signer` trait.
///
/// The underlying key is held by value; on drop the workspace
/// `zeroize` feature on `ed25519-dalek` clears the bytes.
///
/// **Cloning is forbidden** so the secret half cannot accidentally
/// proliferate in memory; callers wrap in `Arc<...>` if shared.
pub struct Ed25519Signer {
    inner: SigningKey,
}

impl std::fmt::Debug for Ed25519Signer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ed25519Signer")
            .field("public", &self.inner.verifying_key().to_bytes())
            .finish_non_exhaustive()
    }
}

impl Ed25519Signer {
    /// Wrap a pre-existing `SigningKey`. Used by tests (deterministic
    /// fixture seeds) and by future production code paths that load a
    /// stored leaf signing key from the openmls storage provider.
    pub fn new(key: SigningKey) -> Self {
        Self { inner: key }
    }

    /// Generate a fresh signing key from `OsRng` and wrap it.
    ///
    /// We borrow `OsRng` from `ed25519_dalek`'s re-exported
    /// `rand_core` to avoid the workspace's multi-version `rand_core`
    /// (the `rand` crate brings 0.6, openmls brings 0.9, ed25519
    /// uses 0.6). Going through dalek's re-export pins us to the
    /// version dalek itself was compiled against.
    pub fn generate() -> Self {
        use ed25519_dalek::ed25519::signature::rand_core::OsRng;
        Self {
            inner: SigningKey::generate(&mut OsRng),
        }
    }

    /// Public key bytes (32 B). Useful for stamping the credential's
    /// `signature_key` field.
    pub fn public_key(&self) -> [u8; 32] {
        self.inner.verifying_key().to_bytes()
    }
}

impl Signer for Ed25519Signer {
    fn sign(&self, payload: &[u8]) -> Result<Vec<u8>, SignerError> {
        let sig = self.inner.sign(payload);
        Ok(sig.to_bytes().to_vec())
    }

    fn signature_scheme(&self) -> SignatureScheme {
        SignatureScheme::ED25519
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn public_key_roundtrips_through_dalek() {
        use ed25519_dalek::{Signature, Verifier, VerifyingKey};

        // Round-trip: build, extract pubkey, rebuild from bytes,
        // compare. The ed25519_dalek API is pubkey-deterministic over
        // the secret half, so this catches any drift in the wrapping.
        let secret = [0x55u8; 32];
        let s = Ed25519Signer::new(SigningKey::from_bytes(&secret));
        let pk1 = s.public_key();
        let pk2 = SigningKey::from_bytes(&secret).verifying_key().to_bytes();
        assert_eq!(pk1, pk2);

        // Wire contract: advertised scheme is ED25519.
        assert_eq!(Signer::signature_scheme(&s), SignatureScheme::ED25519);

        // sign() must emit a genuine 64-byte ed25519 signature that
        // verifies under the advertised pubkey — exercises the
        // `.to_bytes().to_vec()` glue in the Signer impl.
        let msg = b"some-mls-payload";
        let sig_bytes = Signer::sign(&s, msg).expect("sign");
        let sig_arr: [u8; 64] = sig_bytes.as_slice().try_into().expect("64-byte sig");
        let vk = VerifyingKey::from_bytes(&pk1).expect("valid pubkey");
        vk.verify(msg, &Signature::from_bytes(&sig_arr))
            .expect("signature verifies under public key");
    }
}
