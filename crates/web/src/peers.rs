use std::collections::HashMap;

use api::{Key, PeerId, Properties, StableId, Value};

/// State assocaited with a peer.
#[derive(Default)]
pub(crate) struct Peer {
    pub(crate) id: PeerId,
    pub(crate) props: Properties,
    pub(crate) in_room: bool,
}

impl Peer {
    /// Update a peer property.
    pub(crate) fn update(&mut self, key: Key, value: Value, room: &StableId) {
        match key {
            Key::ROOM => {
                self.in_room = *value.as_stable_id() == *room;
            }
            _ => {}
        }

        self.props.insert(key, value);
    }

    /// Update peer based on configuration.
    pub(crate) fn update_config(&mut self, room: &StableId) {
        self.in_room = *self.props.get(Key::ROOM).as_stable_id() == *room;
    }

    /// The name of the peer.
    pub(crate) fn display(&self) -> String {
        let Some(name) = self.props.get(Key::PEER_NAME).as_str() else {
            return self.id.to_string();
        };

        name.to_string()
    }
}

#[derive(Default)]
pub(crate) struct Peers {
    peers: HashMap<PeerId, Peer>,
}

impl Peers {
    /// Iterate over peers.
    pub(crate) fn iter(&self) -> impl Iterator<Item = &Peer> {
        self.peers.values()
    }

    /// Iterate mutably over peers.
    pub(crate) fn iter_mut(&mut self) -> impl Iterator<Item = &mut Peer> {
        self.peers.values_mut()
    }

    /// Clear remote peers.
    pub(crate) fn clear(&mut self) {
        self.peers.clear();
    }

    /// The given peer left our room.
    pub(crate) fn leave(&mut self, id: PeerId) {
        _ = id;
    }

    /// Remove all objects associated with a peer.
    pub(crate) fn remove_peer(&mut self, id: PeerId) {
        self.peers.remove(&id);
    }

    /// Get a mutable reference to a peer.
    pub(crate) fn get_mut(&mut self, id: PeerId) -> Option<&mut Peer> {
        self.peers.get_mut(&id)
    }

    /// Insert a new peer.
    pub(crate) fn create(&mut self, id: PeerId, props: Properties, room: &StableId) -> &mut Peer {
        let in_room = *props.get(Key::ROOM).as_stable_id() == *room;

        let peer = self.peers.entry(id).or_default();
        peer.id = id;
        peer.props = props;
        peer.in_room = in_room;
        peer
    }
}
