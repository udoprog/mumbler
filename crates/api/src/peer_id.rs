use core::fmt;

use base64::Engine as _;
use base64::display::Base64Display;
use musli_core::{Decode, Encode};
use serde_core::{Deserialize, Deserializer, de};

/// The engine used for base64.
static ENGINE: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// The identifier and public key for a peer.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
#[repr(transparent)]
pub struct PeerId {
    repr: [u8; 32],
}

impl PeerId {
    /// Construct a `PeerId` from the raw 32-byte representation.
    #[inline]
    pub const fn new(repr: [u8; 32]) -> Self {
        Self { repr }
    }

    /// Return the raw 32-byte public key bytes.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.repr
    }
}

impl fmt::Display for PeerId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this = Base64Display::new(&self.repr, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl fmt::Debug for PeerId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this = Base64Display::new(&self.repr, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl<'de> Deserialize<'de> for PeerId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = PeerId;

            #[inline]
            fn expecting(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
                write!(f, "a base64url-encoded 32-byte ed25519 public key")
            }

            #[inline]
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let mut dest = [0u8; 32];

                let len = ENGINE
                    .decode_slice(v, &mut dest[..])
                    .map_err(de::Error::custom)?;

                if len != 32 {
                    return Err(de::Error::custom(format!(
                        "invalid length: expected 32 bytes, got {len}"
                    )));
                }

                Ok(PeerId::new(dest))
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}
