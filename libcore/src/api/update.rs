use ed25519_dalek::Signature;
use ed25519_dalek::VerifyingKey;

const UPDATE_MANIFEST_PUBLIC_KEY: [u8; 32] = [
    0x4e, 0x32, 0x60, 0x78, 0x7e, 0xff, 0x22, 0x37, 0x79, 0xfb, 0xdb, 0xb9, 0xae, 0xf5, 0x90, 0x39,
    0x14, 0xf5, 0xed, 0x69, 0x8e, 0xbd, 0x5d, 0x0c, 0x99, 0x3d, 0x79, 0x42, 0xa9, 0x33, 0xfd, 0xcd,
];

/// Verify detached Ed25519 signature over unchanged update manifest bytes.
#[uniffi::export]
pub fn verify_update_manifest(manifest: Vec<u8>, signature: Vec<u8>) -> bool {
    let Ok(signature) = Signature::from_slice(&signature) else {
        return false;
    };
    let Ok(public_key) = VerifyingKey::from_bytes(&UPDATE_MANIFEST_PUBLIC_KEY) else {
        return false;
    };

    public_key.verify_strict(&manifest, &signature).is_ok()
}
