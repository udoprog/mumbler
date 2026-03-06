use core::fmt;

use base64::Engine as _;
use base64::display::Base64Display;
use musli_core::{Decode, Encode};
use serde_core::{Deserialize, Deserializer, de};

/// The engine used for base64.
static ENGINE: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// A base64-encoded u64, used for identifiers in the API.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Id(u64);

impl Id {
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    #[inline]
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0.to_be_bytes();
        let this = Base64Display::new(&bytes, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl fmt::Debug for Id {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let bytes = self.0.to_be_bytes();
        let this = Base64Display::new(&bytes, &ENGINE);
        fmt::Display::fmt(&this, f)
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
                Ok(Id(id))
            }
        }

        deserializer.deserialize_str(Visitor)
    }
}
