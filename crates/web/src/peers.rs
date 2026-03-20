use std::collections::HashMap;

use api::{Id, Key, PeerId, Properties, Value};

use crate::components::render::Visibility;
use crate::objects::PeerObject;

/// State assocaited with a peer.
pub(crate) struct Peer {
    pub(crate) peer_id: PeerId,
    pub(crate) props: Properties,
    objects: HashMap<Id, PeerObject>,
}

impl Peer {
    /// Insert a new object.
    pub(crate) fn insert(&mut self, object_id: Id, peer: PeerObject) {
        self.objects.insert(object_id, peer);
    }

    /// Update a peer property.
    pub(crate) fn update(&mut self, key: Key, value: Value) {
        self.props.insert(key, value);
    }
}

#[derive(Default)]
pub(crate) struct Peers {
    peers: HashMap<PeerId, Peer>,
}

impl Peers {
    /// Clear remote peers.
    pub(crate) fn clear(&mut self) {
        self.peers.clear();
    }

    /// Remove all objects associated with a peer.
    pub(crate) fn remove_peer(&mut self, peer_id: PeerId) {
        self.peers.remove(&peer_id);
    }

    /// Remove a peer.
    pub(crate) fn remove(&mut self, peer_id: PeerId, object_id: Id) {
        let Some(peer) = self.peers.get_mut(&peer_id) else {
            return;
        };

        peer.objects.remove(&object_id);
    }

    /// Get a mutable reference to a peer.
    pub(crate) fn get_mut(&mut self, peer_id: PeerId) -> Option<&mut Peer> {
        self.peers.get_mut(&peer_id)
    }

    /// Get a mutable reference to a peer object.
    pub(crate) fn get_object_mut(
        &mut self,
        peer_id: PeerId,
        object_id: Id,
    ) -> Option<&mut PeerObject> {
        let peer = self.peers.get_mut(&peer_id)?;
        peer.objects.get_mut(&object_id)
    }

    /// Iterate over peers.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &PeerObject> {
        self.peers.values().flat_map(|peer| peer.objects.values())
    }

    /// Insert a new peer.
    pub(crate) fn create(&mut self, peer_id: PeerId, props: Properties) -> &mut Peer {
        self.peers.entry(peer_id).or_insert_with(move || Peer {
            peer_id,
            props,
            objects: HashMap::new(),
        })
    }

    /// Test if the given group or any of its ancestors is hidden.
    #[inline]
    pub(crate) fn visibility(&self, peer_id: PeerId, group: Id) -> Visibility {
        let Some(peer) = self.peers.get(&peer_id) else {
            return Visibility::None;
        };

        let mut hidden = Visibility::Remote;

        let mut current = group;

        while current != Id::ZERO {
            let Some(o) = peer.objects.get(&current) else {
                break;
            };

            hidden = hidden.max(o.visibility());
            current = *o.group;
        }

        hidden
    }
}
