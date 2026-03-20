mod dictionary;

use anyhow::{Context, Result};
use api::PeerId;
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
    /// Get the `PeerId` corresponding to this keypair's public key.
    pub fn peer_id(&self) -> PeerId {
        PeerId::new(self.signing_key.verifying_key().to_bytes())
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
pub fn verify(peer_id: PeerId, message: &[u8], signature: &Signature) -> Result<()> {
    let vk = VerifyingKey::from_bytes(peer_id.as_bytes()).context("invalid public key")?;
    let sig = DalekSig::from_bytes(signature.as_bytes());
    vk.verify(message, &sig)
        .context("signature verification failed")
}

/// Derive a deterministic ed25519 keypair from an arbitrary secret.
///
/// The secret is hashed with SHA-512 and the first 32 bytes are used as the
/// SigningKey seed. This is intentionally deterministic so the same secret
/// always produces the same `PeerId`.
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
