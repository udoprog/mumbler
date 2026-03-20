use core::fmt;

use musli_core::{Decode, Encode};

use crate::{Id, PeerId};

/// Globally identifies a room.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteId {
    /// The peer that defined the room.
    pub peer_id: PeerId,
    /// The identifier of the room.
    pub id: Id,
}

impl RemoteId {
    /// Construct a new room.
    pub fn new(peer_id: PeerId, id: Id) -> Self {
        Self { peer_id, id }
    }
}

impl fmt::Debug for RemoteId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.peer_id, self.id)
    }
}

impl fmt::Display for RemoteId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}::{}", self.peer_id, self.id)
    }
}

#[cfg(feature = "yew")]
impl From<RemoteId> for yew::virtual_dom::Key {
    #[inline]
    fn from(id: RemoteId) -> Self {
        yew::virtual_dom::Key::from(id.to_string())
    }
}
