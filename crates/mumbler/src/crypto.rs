mod dictionary;

use anyhow::{Context, Result};
use api::PublicKey;
use ed25519_dalek::{Signature as DalekSig, Signer, Verifier, VerifyingKey};
use sha2::{Digest, Sha512};

use crate::remote::api::Signature;

/// An ed25519 keypair used for peer identity.
///
/// This type is opaque — ed25519-dalek types do not escape this module.
pub struct Keypair {
    signing_key: ed25519_dalek::SigningKey,
}

impl Keypair {
    /// Get the `PublicKey` corresponding to this keypair's public key.
    pub fn public_key(&self) -> PublicKey {
        PublicKey::new(self.signing_key.verifying_key().to_bytes())
    }

    /// Get the raw 32-byte public key bytes.
    pub fn public_key_bytes(&self) -> [u8; 32] {
        self.signing_key.verifying_key().to_bytes()
    }

    /// Sign a message with this keypair's private key.
    pub fn sign(&self, message: &[u8]) -> Signature {
        Signature::new(self.signing_key.sign(message).to_bytes())
    }
}

/// Verify that `signature` is a valid ed25519 signature over `message` made
/// with the private key corresponding to `public_key`.
pub fn verify(public_key: &PublicKey, message: &[u8], signature: &Signature) -> Result<()> {
    let verifying_key =
        VerifyingKey::from_bytes(public_key.as_bytes()).context("invalid public key")?;
    let signature = DalekSig::from_bytes(signature.as_bytes());
    verifying_key
        .verify(message, &signature)
        .context("signature verification failed")
}

/// Derive a deterministic ed25519 keypair from an arbitrary secret.
///
/// The secret is hashed with SHA-512 and the first 32 bytes are used as the
/// SigningKey seed. This is intentionally deterministic so the same secret
/// always produces the same `PublicKey`.
pub fn derive_keypair(secret: &[u8]) -> Keypair {
    let hash = Sha512::digest(secret);
    let seed: [u8; 32] = hash[..32]
        .try_into()
        .expect("SHA-512 always produces 64 bytes");
    let signing_key = ed25519_dalek::SigningKey::from_bytes(&seed);
    Keypair { signing_key }
}

/// Generate a random string of 16 words.
pub fn random_string() -> String {
    let mut s = String::new();

    for _ in 0..16 {
        if !s.is_empty() {
            s.push(' ');
        }

        let index = rand::random_range(0..dictionary::WORDS.len());
        s.push_str(dictionary::WORDS[index]);
    }

    s
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Peer identities in existing databases depend on these exact bytes.
    #[test]
    fn derive_keypair_is_stable() {
        let keypair = derive_keypair(b"mumbler known answer");
        let public_key: [u8; 32] = [
            0x3a, 0xa8, 0x82, 0x7a, 0x16, 0xea, 0x1a, 0x9c, 0x04, 0x1f, 0x1b, 0x37, 0x43, 0x9d,
            0x45, 0xeb, 0x0d, 0x08, 0xdd, 0xc3, 0xb3, 0xca, 0x4a, 0xda, 0xd2, 0xc4, 0xc2, 0x0e,
            0xd4, 0x8b, 0x40, 0x56,
        ];
        let signature: [u8; 64] = [
            0xc7, 0xae, 0x7c, 0x0f, 0x30, 0xe8, 0x7b, 0xe8, 0xef, 0xc3, 0xe3, 0x92, 0xd4, 0xdf,
            0x0b, 0x8b, 0x7b, 0xe0, 0x04, 0x52, 0xa7, 0x94, 0x62, 0x9f, 0xc8, 0x00, 0x9a, 0xc7,
            0x22, 0x3a, 0xb1, 0xe0, 0xbe, 0x32, 0x60, 0x62, 0x06, 0xbc, 0x68, 0x0c, 0x0f, 0xce,
            0x36, 0x78, 0x15, 0x7e, 0x46, 0x20, 0x6b, 0x15, 0x27, 0x17, 0x38, 0x35, 0x23, 0x4e,
            0x11, 0xf3, 0x31, 0xb2, 0x63, 0x6f, 0xc6, 0x0b,
        ];
        assert_eq!(keypair.public_key_bytes(), public_key);
        let sig = keypair.sign(b"message");
        assert_eq!(*sig.as_bytes(), signature);
        verify(&keypair.public_key(), b"message", &sig).unwrap();
        assert!(verify(&keypair.public_key(), b"other", &sig).is_err());
    }
}
