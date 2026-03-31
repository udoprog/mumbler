use core::error::Error;
use core::fmt;
use core::str::FromStr;

use musli_core::{Decode, Encode};
use serde_core::{Deserialize, Deserializer, de};

use crate::id::ParseIdError;
use crate::public_key::ParsePublicKeyError;
use crate::{Id, PublicKey};

/// Globally identifies a room.
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct StableId {
    /// The public key that owns the stable identifier.
    pub public_key: PublicKey,
    /// The identifier of the room.
    pub id: Id,
}

impl StableId {
    /// The zero stable identifier.
    pub const ZERO: Self = Self {
        public_key: PublicKey::ZERO,
        id: Id::ZERO,
    };

    /// Construct a new room.
    #[inline]
    pub fn new(public_key: PublicKey, id: Id) -> Self {
        Self { public_key, id }
    }

    /// Whether the stable identifier is zero.
    #[inline]
    pub const fn is_zero(&self) -> bool {
        self.public_key.is_zero() && self.id.is_zero()
    }
}

impl fmt::Debug for StableId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.public_key, self.id)
    }
}

impl fmt::Display for StableId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.public_key, self.id)
    }
}

#[cfg(feature = "yew")]
impl From<StableId> for yew::virtual_dom::Key {
    #[inline]
    fn from(id: StableId) -> Self {
        yew::virtual_dom::Key::from(id.to_string())
    }
}

impl<'de> Deserialize<'de> for StableId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = StableId;

            #[inline]
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                write!(f, "a remote identifier")
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

pub struct ParseStableIdError {
    kind: ParseStableIdErrorKind,
}

#[derive(Debug)]
enum ParseStableIdErrorKind {
    MissingSeparator,
    PublicKey(ParsePublicKeyError),
    Id(ParseIdError),
}

impl fmt::Display for ParseStableIdErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeparator => write!(f, "missing separator"),
            Self::PublicKey(..) => write!(f, "invalid public key"),
            Self::Id(..) => write!(f, "invalid id"),
        }
    }
}

impl fmt::Display for ParseStableIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl fmt::Debug for ParseStableIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl Error for ParseStableIdError {
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ParseStableIdErrorKind::PublicKey(err) => Some(err),
            ParseStableIdErrorKind::Id(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ParseStableIdErrorKind> for ParseStableIdError {
    #[inline]
    fn from(kind: ParseStableIdErrorKind) -> Self {
        Self { kind }
    }
}

impl FromStr for StableId {
    type Err = ParseStableIdError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((public_key, id)) = s.split_once("::") else {
            return Err(ParseStableIdError::from(
                ParseStableIdErrorKind::MissingSeparator,
            ));
        };

        let public_key = public_key
            .parse()
            .map_err(ParseStableIdErrorKind::PublicKey)?;
        let id = id.parse().map_err(ParseStableIdErrorKind::Id)?;
        Ok(Self { public_key, id })
    }
}
