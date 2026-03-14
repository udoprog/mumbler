use std::collections::HashMap;

use api::{Id, PeerId};

use crate::components::render::Hidden;
use crate::objects::PeerObject;

#[derive(Default)]
pub(crate) struct Peers {
    values: HashMap<(PeerId, Id), PeerObject>,
}

impl Peers {
    /// Clear remote peers.
    pub(crate) fn clear(&mut self) {
        self.values.clear();
    }

    /// Remove all objects associated with a peer.
    pub(crate) fn remove_peer(&mut self, peer_id: PeerId) {
        self.values.retain(|&(pid, _), _| pid != peer_id);
    }

    /// Remove a peer.
    pub(crate) fn remove(&mut self, peer_id: PeerId, object_id: Id) {
        self.values.remove(&(peer_id, object_id));
    }

    /// Get a mutable reference to a peer.
    pub(crate) fn get_mut(&mut self, peer_id: PeerId, object_id: Id) -> Option<&mut PeerObject> {
        self.values.get_mut(&(peer_id, object_id))
    }

    /// Iterate over peers.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &PeerObject> {
        self.values.values()
    }

    /// Insert a new peer.
    pub(crate) fn insert(&mut self, peer_id: PeerId, object_id: Id, peer: PeerObject) {
        self.values.insert((peer_id, object_id), peer);
    }

    /// Test if the given group or any of its ancestors is hidden.
    #[inline]
    pub(crate) fn as_hidden(&self, peer_id: PeerId, group: Id) -> Hidden {
        let mut hidden = Hidden::Visible;

        let mut current = group;

        while current != Id::ZERO {
            let Some(peer) = self.values.get(&(peer_id, current)) else {
                break;
            };

            hidden = hidden.max(peer.as_hidden());
            current = *peer.group;
        }

        hidden
    }
}

impl FromIterator<PeerObject> for Peers {
    #[inline]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = PeerObject>,
    {
        Self {
            values: iter
                .into_iter()
                .map(|peer| ((peer.peer_id, peer.data.id), peer))
                .collect(),
        }
    }
}
