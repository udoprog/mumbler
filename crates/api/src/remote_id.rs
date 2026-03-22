use core::error::Error;
use core::fmt;
use core::str::FromStr;

use musli_core::{Decode, Encode};
use serde_core::{Deserialize, Deserializer, de};

use crate::id::ParseIdError;
use crate::peer_id::ParsePeerIdError;
use crate::{Id, PeerId};

/// Globally identifies a room.
#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteId {
    /// The peer that defined the room.
    pub peer_id: PeerId,
    /// The identifier of the room.
    pub id: Id,
}

impl RemoteId {
    /// The zero remote identifier.
    pub const ZERO: Self = Self {
        peer_id: PeerId::ZERO,
        id: Id::ZERO,
    };

    /// A local identifier.
    #[inline]
    pub fn local(id: Id) -> Self {
        Self {
            peer_id: PeerId::ZERO,
            id,
        }
    }

    /// Construct a new room.
    #[inline]
    pub fn new(peer_id: PeerId, id: Id) -> Self {
        Self { peer_id, id }
    }

    /// Check if the remote identifier is zero.
    #[inline]
    pub fn is_zero(&self) -> bool {
        self.peer_id.is_zero() && self.id.is_zero()
    }

    #[inline]
    pub fn as_non_zero(&self) -> Option<Self> {
        if self.is_zero() { None } else { Some(*self) }
    }
}

impl fmt::Debug for RemoteId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.peer_id, self.id)
    }
}

impl fmt::Display for RemoteId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.peer_id, self.id)
    }
}

#[cfg(feature = "yew")]
impl From<RemoteId> for yew::virtual_dom::Key {
    #[inline]
    fn from(id: RemoteId) -> Self {
        yew::virtual_dom::Key::from(id.to_string())
    }
}

impl<'de> Deserialize<'de> for RemoteId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Visitor;

        impl<'de> de::Visitor<'de> for Visitor {
            type Value = RemoteId;

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

pub struct ParseRemoteIdError {
    kind: ParseRemoteIdErrorKind,
}

#[derive(Debug)]
enum ParseRemoteIdErrorKind {
    MissingSeparator,
    PeerId(ParsePeerIdError),
    Id(ParseIdError),
}

impl fmt::Display for ParseRemoteIdErrorKind {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingSeparator => write!(f, "missing separator"),
            Self::PeerId(..) => write!(f, "invalid peer id"),
            Self::Id(..) => write!(f, "invalid id"),
        }
    }
}

impl fmt::Display for ParseRemoteIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl fmt::Debug for ParseRemoteIdError {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

impl Error for ParseRemoteIdError {
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match &self.kind {
            ParseRemoteIdErrorKind::PeerId(err) => Some(err),
            ParseRemoteIdErrorKind::Id(err) => Some(err),
            _ => None,
        }
    }
}

impl From<ParseRemoteIdErrorKind> for ParseRemoteIdError {
    #[inline]
    fn from(kind: ParseRemoteIdErrorKind) -> Self {
        Self { kind }
    }
}

impl FromStr for RemoteId {
    type Err = ParseRemoteIdError;

    #[inline]
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let Some((peer_id, id)) = s.split_once("::") else {
            return Err(ParseRemoteIdError::from(
                ParseRemoteIdErrorKind::MissingSeparator,
            ));
        };

        let peer_id = peer_id.parse().map_err(ParseRemoteIdErrorKind::PeerId)?;
        let id = id.parse().map_err(ParseRemoteIdErrorKind::Id)?;
        Ok(Self { peer_id, id })
    }
}
