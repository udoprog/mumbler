#[cfg(feature = "sqll")]
use core::ffi::c_int;
use core::fmt;

use base64::Engine as _;
use base64::display::Base64Display;
use musli_core::{Decode, Encode};
use serde_core::{Deserialize, Deserializer, de};
#[cfg(feature = "sqll")]
use sqll::{BIND_INDEX, Bind, BindValue, FromColumn, Statement, ty};

/// The engine used for base64.
static ENGINE: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// A base64-encoded u64, used for identifiers in the API.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct PeerId {
    raw: u64,
}

impl PeerId {
    /// Create a new identifier from a u64.
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self { raw: id }
    }

    /// Get the inner u64 value of the identifier.
    #[inline]
    pub const fn get(self) -> u64 {
        self.raw
    }
}

impl fmt::Display for PeerId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.raw.to_be_bytes();
        let this = Base64Display::new(&bytes, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl fmt::Debug for PeerId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.raw.to_be_bytes();
        let this = Base64Display::new(&bytes, &ENGINE);
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
                write!(f, "a base64-encoded u64")
            }

            #[inline]
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let mut dest = [0u8; 8];

                let len = ENGINE
                    .decode_slice(v, &mut dest[..])
                    .map_err(de::Error::custom)?;

                if len != 8 {
                    return Err(de::Error::custom(format!(
                        "invalid length: expected 8 bytes, got {len}"
                    )));
                }

                let id = u64::from_be_bytes(dest);
                Ok(PeerId::new(id))
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}

#[cfg(feature = "sqll")]
impl BindValue for PeerId {
    #[inline]
    fn bind_value(&self, stmt: &mut Statement, index: c_int) -> Result<(), sqll::Error> {
        self.raw.cast_signed().bind_value(stmt, index)
    }
}

#[cfg(feature = "sqll")]
impl Bind for PeerId {
    #[inline]
    fn bind(&self, stmt: &mut Statement) -> Result<(), sqll::Error> {
        self.bind_value(stmt, BIND_INDEX)
    }
}

#[cfg(feature = "sqll")]
impl FromColumn<'_> for PeerId {
    type Type = ty::Integer;

    #[inline]
    fn from_column(stmt: &Statement, index: ty::Integer) -> Result<Self, sqll::Error> {
        let id = i64::from_column(stmt, index)?.cast_unsigned();
        Ok(PeerId::new(id))
    }
}
