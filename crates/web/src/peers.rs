use std::collections::HashMap;

use api::{Key, PeerId, Properties, PublicKey, RemoteId, RemotePeer, StableId, Value};

/// State assocaited with a peer.
#[derive(Default)]
pub(crate) struct Peer {
    pub(crate) id: PeerId,
    pub(crate) props: Properties,
    pub(crate) public_key: PublicKey,
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
        let name = self.props.get(Key::PEER_NAME).as_str();

        if name.is_empty() {
            return self.id.to_string();
        }

        name.to_string()
    }
}

#[derive(Default)]
pub(crate) struct Peers {
    peers: HashMap<PeerId, Peer>,
    public_keys: HashMap<PublicKey, PeerId>,
    pub(crate) public_key: PublicKey,
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
        self.public_keys.clear();
    }

    /// Get a peer by id.
    pub(crate) fn get(&self, id: PeerId) -> Option<&Peer> {
        self.peers.get(&id)
    }

    /// Get a peer by public key.
    pub(crate) fn by_public_key(&self, key: &PublicKey) -> Option<&Peer> {
        let peer_id = self.public_keys.get(key)?;
        self.peers.get(peer_id)
    }

    /// Remove all objects associated with a peer.
    pub(crate) fn remove_peer(&mut self, id: PeerId) {
        if let Some(peer) = self.peers.remove(&id) {
            self.public_keys.remove(&peer.public_key);
        }
    }

    /// Get a mutable reference to a peer.
    pub(crate) fn get_mut(&mut self, id: PeerId) -> Option<&mut Peer> {
        self.peers.get_mut(&id)
    }

    /// Insert a new peer.
    pub(crate) fn insert(&mut self, p: RemotePeer, room: &StableId) {
        if let Some(old) = self.public_keys.insert(p.public_key, p.id) {
            self.peers.remove(&old);
        }

        let in_room = *p.props.get(Key::ROOM).as_stable_id() == *room;

        let peer = Peer {
            id: p.id,
            props: p.props,
            public_key: p.public_key,
            in_room,
        };

        if let Some(old) = self.peers.insert(p.id, peer) {
            self.public_keys.remove(&old.public_key);
        }
    }

    /// Translates a stable id to a remote id.
    ///
    /// RemoteId are temporary identifiers for peers. A StableId is globally
    /// unique based on their key.
    pub(crate) fn to_remote_id(&self, id: &StableId) -> RemoteId {
        if id.public_key == self.public_key {
            return RemoteId::local(id.id);
        }

        let Some(peer_id) = self.public_keys.get(&id.public_key) else {
            return RemoteId::ZERO;
        };

        RemoteId::new(*peer_id, id.id)
    }

    /// Translates a remote id to a stable id.
    pub(crate) fn to_stable_id(&self, id: &RemoteId) -> StableId {
        if id.is_local() {
            return StableId::new(self.public_key, id.id);
        }

        let Some(peer_id) = self.peers.get(&id.peer_id) else {
            return StableId::ZERO;
        };

        StableId::new(peer_id.public_key, id.id)
    }
}
