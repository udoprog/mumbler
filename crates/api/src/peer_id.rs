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
pub struct PeerId {
    repr: [u8; 32],
}

impl PeerId {
    /// The zero peer ID, which is invalid and should not be used.
    pub const ZERO: Self = Self { repr: [0u8; 32] };

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
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a peer identifier")
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

/// An error raised by parsing an Id as a string.
pub struct ParsePeerIdError {
    kind: ParsePeerIdErrorKind,
}

#[derive(Debug)]
enum ParsePeerIdErrorKind {
    DecodeSliceError(DecodeSliceError),
    InvalidLength(usize),
}

impl fmt::Display for ParsePeerIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl fmt::Display for ParsePeerIdErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParsePeerIdErrorKind::DecodeSliceError(error) => {
                write!(f, "base64 decode error: {error}")
            }
            ParsePeerIdErrorKind::InvalidLength(len) => {
                write!(f, "invalid length: expected 32 bytes, got {len}")
            }
        }
    }
}

impl fmt::Debug for ParsePeerIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl Error for ParsePeerIdError {
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ParsePeerIdErrorKind::DecodeSliceError(error) => Some(error),
            _ => None,
        }
    }
}

impl From<DecodeSliceError> for ParsePeerIdError {
    #[inline]
    fn from(error: DecodeSliceError) -> Self {
        Self {
            kind: ParsePeerIdErrorKind::DecodeSliceError(error),
        }
    }
}

impl FromStr for PeerId {
    type Err = ParsePeerIdError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut dest = [0u8; 32];

        let len = ENGINE.decode_slice(s, &mut dest[..])?;

        if len != 32 {
            return Err(ParsePeerIdError {
                kind: ParsePeerIdErrorKind::InvalidLength(len),
            });
        }

        Ok(PeerId::new(dest))
    }
}
