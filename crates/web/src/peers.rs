use std::collections::HashMap;

use api::{Id, Key, PeerId, Properties, RemoteId, Value};

use crate::components::render::Visibility;
use crate::objects::ObjectData;

/// State assocaited with a peer.
#[derive(Default)]
pub(crate) struct Peer {
    pub(crate) id: PeerId,
    pub(crate) props: Properties,
    objects: HashMap<Id, ObjectData>,
    pub(crate) in_room: bool,
}

impl Peer {
    /// Update a peer property.
    pub(crate) fn update(&mut self, key: Key, value: Value, room: &RemoteId) {
        match key {
            Key::ROOM => {
                self.in_room = *value.as_remote_id() == *room;
            }
            _ => {}
        }

        self.props.insert(key, value);
    }

    /// Update peer based on configuration.
    pub(crate) fn update_config(&mut self, room: &RemoteId) {
        self.in_room = *self.props.get(Key::ROOM).as_remote_id() == *room;
    }

    /// Insert a new object.
    pub(crate) fn insert(&mut self, object_id: Id, data: ObjectData) {
        self.objects.insert(object_id, data);
    }

    /// Iterate over objects.
    pub(crate) fn objects(&self) -> impl Iterator<Item = &ObjectData> {
        self.objects.values()
    }

    /// The name of the peer.
    pub(crate) fn display(&self) -> String {
        let Some(name) = self.props.get(Key::PEER_NAME).as_str() else {
            return self.id.to_string();
        };

        name.to_string()
    }

    /// Test if the given group or any of its ancestors is hidden.
    #[inline]
    pub(crate) fn visibility(&self, group: Id) -> Visibility {
        let mut hidden = Visibility::Remote;

        let mut current = group;

        while current != Id::ZERO {
            let Some(o) = self.objects.get(&current) else {
                break;
            };

            hidden = hidden.max(o.visibility());
            current = *o.group;
        }

        hidden
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
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };

        peer.objects.clear();
    }

    /// Remove all objects associated with a peer.
    pub(crate) fn remove_peer(&mut self, id: PeerId) {
        self.peers.remove(&id);
    }

    /// Remove a peer.
    pub(crate) fn remove(&mut self, id: PeerId, object_id: Id) {
        let Some(peer) = self.peers.get_mut(&id) else {
            return;
        };

        peer.objects.remove(&object_id);
    }

    /// Get a mutable reference to a peer.
    pub(crate) fn get_mut(&mut self, id: PeerId) -> Option<&mut Peer> {
        self.peers.get_mut(&id)
    }

    /// Get a mutable reference to a peer object.
    pub(crate) fn get_object_mut(&mut self, id: PeerId, object_id: Id) -> Option<&mut ObjectData> {
        let peer = self.peers.get_mut(&id)?;
        peer.objects.get_mut(&object_id)
    }

    /// Insert a new peer.
    pub(crate) fn create(&mut self, id: PeerId, props: Properties, room: &RemoteId) -> &mut Peer {
        let in_room = *props.get(Key::ROOM).as_remote_id() == *room;

        let peer = self.peers.entry(id).or_default();
        peer.id = id;
        peer.props = props;
        peer.objects.clear();
        peer.in_room = in_room;
        peer
    }
}
