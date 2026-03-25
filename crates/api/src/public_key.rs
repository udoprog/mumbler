use core::error::Error;
use core::fmt;
use core::str::FromStr;

use base64::display::Base64Display;
use base64::{DecodeSliceError, Engine as _};
use musli_core::{Decode, Encode};
use serde_core::{Deserialize, Deserializer, de};

/// The engine used for base64.
static ENGINE: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// The identifier and public key for a peer.
#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
#[repr(transparent)]
pub struct PublicKey {
    repr: [u8; 32],
}

impl PublicKey {
    /// The zero peer ID, which is invalid and should not be used.
    pub const ZERO: Self = Self { repr: [0u8; 32] };

    /// Construct a `PublicKey` from the raw 32-byte representation.
    #[inline]
    pub const fn new(repr: [u8; 32]) -> Self {
        Self { repr }
    }

    /// Return the raw 32-byte public key bytes.
    #[inline]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.repr
    }

    #[inline]
    pub const fn is_zero(&self) -> bool {
        matches!(
            self.repr,
            [
                0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0
            ]
        )
    }
}

impl fmt::Display for PublicKey {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }

        let this = Base64Display::new(&self.repr, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl fmt::Debug for PublicKey {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl<'de> Deserialize<'de> for PublicKey {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = PublicKey;

            #[inline]
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a public key")
            }

            #[inline]
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                v.parse().map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}

/// An error raised by parsing a public key as a string.
pub struct ParsePublicKeyError {
    kind: ParsePublicKeyErrorKind,
}

#[derive(Debug)]
enum ParsePublicKeyErrorKind {
    DecodeSliceError(DecodeSliceError),
    InvalidLength(usize),
}

impl fmt::Display for ParsePublicKeyError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl fmt::Display for ParsePublicKeyErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParsePublicKeyErrorKind::DecodeSliceError(error) => {
                write!(f, "base64 decode error: {error}")
            }
            ParsePublicKeyErrorKind::InvalidLength(len) => {
                write!(f, "invalid length: expected 32 bytes, got {len}")
            }
        }
    }
}

impl fmt::Debug for ParsePublicKeyError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl Error for ParsePublicKeyError {
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ParsePublicKeyErrorKind::DecodeSliceError(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecodeSliceError> for ParsePublicKeyError {
    #[inline]
    fn from(error: DecodeSliceError) -> Self {
        Self {
            kind: ParsePublicKeyErrorKind::DecodeSliceError(error),
        }
    }
}

impl FromStr for PublicKey {
    type Err = ParsePublicKeyError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "0" {
            return Ok(Self::ZERO);
        }

        let mut dest = [0u8; 32];

        let len = ENGINE.decode_slice(s, &mut dest[..])?;

        if len != 32 {
            return Err(ParsePublicKeyError {
                kind: ParsePublicKeyErrorKind::InvalidLength(len),
            });
        }

        Ok(PublicKey::new(dest))
    }
}
