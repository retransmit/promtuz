use chacha20poly1305::aead::{OsRng, rand_core::RngCore};

pub use ed25519_dalek::{SecretKey, SigningKey, VerifyingKey as PublicKey};

pub mod sign;

pub fn get_signing_key() -> SigningKey {
    SigningKey::generate(&mut OsRng)
}

pub fn get_nonce<const N: usize>() -> [u8; N] {
    let mut nonce = [0u8; N];
    OsRng.fill_bytes(&mut nonce);
    nonce
}
