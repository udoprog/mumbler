use core::error::Error;
#[cfg(feature = "sqll")]
use core::ffi::c_int;
use core::fmt;
use core::num::NonZeroU32;
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
#[repr(transparent)]
pub struct Id {
    repr: u32,
}

impl Id {
    /// The zero id.
    pub const ZERO: Self = Self { repr: 0 };

    /// Construct a new identifier.
    #[inline]
    pub const fn new(repr: u32) -> Self {
        Self { repr }
    }

    /// Test if this is the zero id.
    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.repr == 0
    }

    #[inline]
    pub const fn as_non_zero(&self) -> Option<Self> {
        if self.is_zero() { None } else { Some(*self) }
    }

    /// Get the inner u64 value of the identifier.
    #[inline]
    pub const fn get(self) -> u32 {
        self.repr
    }

    /// Coerce into an arbitrary byte vector that represents the id.
    #[inline]
    pub fn to_vec(&self) -> Vec<u8> {
        self.repr.to_le_bytes().to_vec()
    }

    #[inline]
    pub fn to_non_zero_u32(&self) -> Option<NonZeroU32> {
        NonZeroU32::new(self.repr)
    }
}

impl fmt::Display for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.is_zero() {
            return f.write_str("0");
        }

        let bytes = self.repr.to_le_bytes();
        let this = Base64Display::new(&bytes, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl fmt::Debug for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

#[cfg(feature = "yew")]
impl From<Id> for yew::virtual_dom::Key {
    #[inline]
    fn from(id: Id) -> Self {
        yew::virtual_dom::Key::from(id.to_string())
    }
}

/// An error raised by parsing an Id as a string.
pub struct ParseIdError {
    kind: ParseIdErrorKind,
}

#[derive(Debug)]
enum ParseIdErrorKind {
    DecodeSliceError(DecodeSliceError),
    InvalidLength(usize),
}

impl fmt::Display for ParseIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl fmt::Display for ParseIdErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseIdErrorKind::DecodeSliceError(err) => write!(f, "base64 decode error: {err}"),
            ParseIdErrorKind::InvalidLength(len) => {
                write!(f, "invalid length: expected 4 bytes, got {len}")
            }
        }
    }
}

impl fmt::Debug for ParseIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl Error for ParseIdError {}

impl From<DecodeSliceError> for ParseIdError {
    #[inline]
    fn from(err: DecodeSliceError) -> Self {
        Self {
            kind: ParseIdErrorKind::DecodeSliceError(err),
        }
    }
}

impl FromStr for Id {
    type Err = ParseIdError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s == "0" {
            return Ok(Self::ZERO);
        }

        let mut dest = [0u8; 4];

        let len = ENGINE.decode_slice(s, &mut dest[..])?;

        if len != 4 {
            return Err(ParseIdError {
                kind: ParseIdErrorKind::InvalidLength(len),
            });
        }

        let id = u32::from_le_bytes(dest);
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
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "an opaque identifier")
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
        self.repr.bind_value(stmt, index)
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
        let repr = u32::from_column(stmt, index)?;
        Ok(Id::new(repr))
    }
}
