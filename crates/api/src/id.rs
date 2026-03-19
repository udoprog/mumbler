use core::error::Error;
#[cfg(feature = "sqll")]
use core::ffi::c_int;
use core::fmt;
use core::str::FromStr;

use base64::display::Base64Display;
use base64::engine::general_purpose::{GeneralPurpose, URL_SAFE_NO_PAD};
use base64::{DecodeSliceError, Engine as _};
use musli_core::{Decode, Encode};
use serde_core::{Deserialize, Deserializer, de};
#[cfg(feature = "sqll")]
use sqll::{BIND_INDEX, Bind, BindValue, FromColumn, Statement, ty};

/// The engine used for base64.
static ENGINE: GeneralPurpose = URL_SAFE_NO_PAD;

/// A base64-encoded u64, used for identifiers in the API.
#[derive(Default, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Id {
    raw: [u8; 8],
}

impl Id {
    /// The zero id.
    pub const ZERO: Self = Self { raw: [0u8; 8] };

    /// Create a new identifier from a u64.
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self {
            raw: id.to_be_bytes(),
        }
    }

    /// Test if this is the zero id.
    #[inline]
    pub const fn is_zero(&self) -> bool {
        matches!(self.raw, [0, 0, 0, 0, 0, 0, 0, 0])
    }

    #[inline]
    pub const fn as_non_zero(&self) -> Option<Self> {
        if self.is_zero() { None } else { Some(*self) }
    }

    /// Get the inner u64 value of the identifier.
    #[inline]
    pub const fn as_u64(&self) -> u64 {
        u64::from_be_bytes(self.raw)
    }

    /// Get the inner u64 value of the identifier.
    #[inline]
    pub const fn get(self) -> u64 {
        u64::from_be_bytes(self.raw)
    }

    /// Get bytes corresponding to the identifier.
    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        &self.raw[..]
    }
}

impl fmt::Display for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this = Base64Display::new(&self.raw, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl fmt::Debug for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this = Base64Display::new(&self.raw, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

#[derive(Debug)]
enum IdParseErrorKind {
    DecodeSliceError(DecodeSliceError),
    InvalidLength(usize),
}

/// An error raised by parsing an Id as a string.
pub struct IdParseError {
    kind: IdParseErrorKind,
}

impl fmt::Display for IdParseError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            IdParseErrorKind::DecodeSliceError(err) => write!(f, "base64 decode error: {err}"),
            IdParseErrorKind::InvalidLength(len) => {
                write!(f, "invalid length: expected 8 bytes, got {len}")
            }
        }
    }
}

impl fmt::Debug for IdParseError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl Error for IdParseError {}

impl From<DecodeSliceError> for IdParseError {
    #[inline]
    fn from(err: DecodeSliceError) -> Self {
        Self {
            kind: IdParseErrorKind::DecodeSliceError(err),
        }
    }
}

impl FromStr for Id {
    type Err = IdParseError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut dest = [0u8; 8];

        let len = ENGINE.decode_slice(s, &mut dest[..])?;

        if len != 8 {
            return Err(IdParseError {
                kind: IdParseErrorKind::InvalidLength(len),
            });
        }

        let id = u64::from_be_bytes(dest);
        Ok(Id::new(id))
    }
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = Id;

            #[inline]
            fn expecting(&self, f: &mut fmt::Formatter) -> std::fmt::Result {
                write!(f, "a base64-encoded u64")
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

#[cfg(feature = "sqll")]
impl BindValue for Id {
    #[inline]
    fn bind_value(&self, stmt: &mut Statement, index: c_int) -> Result<(), sqll::Error> {
        self.as_u64().cast_signed().bind_value(stmt, index)
    }
}

#[cfg(feature = "sqll")]
impl Bind for Id {
    #[inline]
    fn bind(&self, stmt: &mut Statement) -> Result<(), sqll::Error> {
        self.bind_value(stmt, BIND_INDEX)
    }
}

#[cfg(feature = "sqll")]
impl FromColumn<'_> for Id {
    type Type = ty::Integer;

    #[inline]
    fn from_column(stmt: &Statement, index: ty::Integer) -> Result<Self, sqll::Error> {
        let id = i64::from_column(stmt, index)?.cast_unsigned();
        Ok(Id::new(id))
    }
}
