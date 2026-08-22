//! What ties an MLS leaf to a promtuz identity.
//!
//! A leaf signs with a key of its own (see `signer.rs`: a leaf compromise
//! must not be an identity compromise), so the credential has to say which
//! identity chose that key — and prove it, or any member could put any IPK
//! on their leaf and speak as that person. The credential is therefore
//! `ipk ‖ Sig_ipk(DOMAIN ‖ leaf signature key)`, checked wherever a leaf is
//! read as a person: the sender of a message, the roster of a Welcome, the
//! leaves a commit adds, a fetched KeyPackage.
//!
//! Leaves minted before this binding carry the bare 32-byte IPK. A pair
//! group still takes those — there is nobody in it to impersonate but the
//! one peer, whose identity the pairing already established — so existing
//! direct chats keep working. A group chat does not.

use ed25519_dalek::Signature;
use ed25519_dalek::Signer as _;
use ed25519_dalek::SigningKey;
use ed25519_dalek::VerifyingKey;
use openmls::prelude::BasicCredential;
use openmls::prelude::Credential;
use openmls::prelude::LeafNode;
use openmls::prelude::Member;

const DOMAIN: &[u8] = b"promtuz-mls-leaf-v1";
const BOUND_LEN: usize = 32 + 64;

fn binding_input(leaf_signature_key: &[u8]) -> Vec<u8> {
    [DOMAIN, leaf_signature_key].concat()
}

/// A credential naming the signer's identity and proving it chose
/// `leaf_signature_key`.
pub fn bound_credential(ipk_signer: &SigningKey, leaf_signature_key: &[u8]) -> BasicCredential {
    let sig = ipk_signer.sign(&binding_input(leaf_signature_key));
    let mut bytes = Vec::with_capacity(BOUND_LEN);
    bytes.extend_from_slice(&ipk_signer.verifying_key().to_bytes());
    bytes.extend_from_slice(&sig.to_bytes());
    BasicCredential::new(bytes)
}

/// The identity a leaf belongs to, and how well it proved it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LeafIdentity {
    /// Signed over the leaf's own signature key.
    Bound([u8; 32]),
    /// The bare IPK of a leaf minted before the binding existed.
    Legacy([u8; 32]),
}

impl LeafIdentity {
    pub fn ipk(self) -> [u8; 32] {
        match self {
            Self::Bound(k) | Self::Legacy(k) => k,
        }
    }

    /// Under `strict`, only a bound leaf is anyone.
    pub fn ipk_if(self, strict: bool) -> Option<[u8; 32]> {
        match self {
            Self::Bound(k) => Some(k),
            Self::Legacy(k) if !strict => Some(k),
            Self::Legacy(_) => None,
        }
    }
}

/// Read a credential against the signature key of the leaf it sits on.
/// `None` for anything that is neither a valid binding nor the legacy form.
pub fn leaf_identity(credential: &Credential, leaf_signature_key: &[u8]) -> Option<LeafIdentity> {
    let bytes = credential.serialized_content();
    match bytes.len() {
        32 => Some(LeafIdentity::Legacy(bytes.try_into().ok()?)),
        BOUND_LEN => {
            let ipk: [u8; 32] = bytes[..32].try_into().ok()?;
            let vk = VerifyingKey::from_bytes(&ipk).ok()?;
            let sig = Signature::from_slice(&bytes[32..]).ok()?;
            vk.verify_strict(&binding_input(leaf_signature_key), &sig).ok()?;
            Some(LeafIdentity::Bound(ipk))
        },
        _ => None,
    }
}

pub fn member_ipk(m: &Member, strict: bool) -> Option<[u8; 32]> {
    leaf_identity(&m.credential, &m.signature_key)?.ipk_if(strict)
}

pub fn leaf_node_ipk(leaf: &LeafNode, strict: bool) -> Option<[u8; 32]> {
    leaf_identity(leaf.credential(), leaf.signature_key().as_slice())?.ipk_if(strict)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn signer(seed: u8) -> SigningKey {
        SigningKey::from_bytes(&[seed; 32])
    }

    #[test]
    fn a_bound_credential_names_its_signer_for_that_leaf_only() {
        let alice = signer(1);
        let leaf = [7u8; 32];
        let cred: Credential = bound_credential(&alice, &leaf).into();
        assert_eq!(
            leaf_identity(&cred, &leaf),
            Some(LeafIdentity::Bound(alice.verifying_key().to_bytes()))
        );
        assert_eq!(leaf_identity(&cred, &[8u8; 32]), None, "another leaf key: not hers");
    }

    /// What the binding is for: a credential that merely *says* Alice, made
    /// by someone who is not Alice, is nobody in a group chat.
    #[test]
    fn a_claimed_ipk_without_her_signature_is_nobody() {
        let alice = signer(1);
        let mallory = signer(2);
        let leaf = [7u8; 32];
        let mut forged = alice.verifying_key().to_bytes().to_vec();
        forged.extend_from_slice(&mallory.sign(&binding_input(&leaf)).to_bytes());
        let cred: Credential = BasicCredential::new(forged).into();
        assert_eq!(leaf_identity(&cred, &leaf), None);

        let bare: Credential = BasicCredential::new(alice.verifying_key().to_bytes().to_vec()).into();
        let legacy = leaf_identity(&bare, &leaf).expect("legacy form parses");
        assert_eq!(legacy.ipk_if(true), None, "and the bare form is nobody in a group");
        assert_eq!(legacy.ipk_if(false), Some(alice.verifying_key().to_bytes()), "but still the peer in a pair");
    }
}
