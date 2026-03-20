use std::collections::HashMap;

use api::{Id, Key, PeerId, Properties, Value};

use crate::components::render::Visibility;
use crate::objects::ObjectData;

/// State assocaited with a peer.
pub(crate) struct Peer {
    pub(crate) id: PeerId,
    pub(crate) props: Properties,
    objects: HashMap<Id, ObjectData>,
}

impl Peer {
    /// Insert a new object.
    pub(crate) fn insert(&mut self, object_id: Id, data: ObjectData) {
        self.objects.insert(object_id, data);
    }

    /// Update a peer property.
    pub(crate) fn update(&mut self, key: Key, value: Value) {
        self.props.insert(key, value);
    }

    /// Iterate over objects.
    pub(crate) fn objects(&self) -> impl Iterator<Item = &ObjectData> {
        self.objects.values()
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

    /// Clear remote peers.
    pub(crate) fn clear(&mut self) {
        self.peers.clear();
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
    pub(crate) fn create(&mut self, id: PeerId, props: Properties) -> &mut Peer {
        self.peers.entry(id).or_insert_with(move || Peer {
            id,
            props,
            objects: HashMap::new(),
        })
    }
}
