use musli_core::{Decode, Encode};
use musli_web::api;

use ::api::{Id, Key, PeerId, RemoteObject, Value};

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Header {
    /// The type of the request.
    pub request: u16,
    /// The type id of the error message.
    pub error: u16,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ConnectBody {
    /// The protocol version of the client.
    pub version: u32,
    /// The context to connect to.
    pub room: Box<[u8]>,
    /// List of remote objects and their properties defined by peer.
    pub objects: Vec<RemoteObject>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ServerHello {
    /// The protocol version of the server.
    pub version: u32,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PingBody {
    /// The payload of the ping that will be sent back in the pong.
    pub payload: u64,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PongBody {
    /// The payload of the pong, which is the same as the ping.
    pub payload: u64,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct JoinBody {
    /// The peer that joined.
    pub peer_id: PeerId,
    /// The key-value pairs that were immediately set for the peer.
    pub objects: Vec<RemoteObject>,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct JoinBodyRef<'a> {
    /// The peer that joined.
    pub peer_id: PeerId,
    /// The key-value pairs that were immediately set for the peer.
    pub objects: &'a [RemoteObject],
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct LeaveBody {
    /// The peer that left the room.
    pub id: PeerId,
}

/// A request to update.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdatePeer {
    /// The id of the object being updated.
    pub object_id: Id,
    /// The key to update.
    pub key: Key,
    /// The value to update.
    pub value: Value,
}

/// A request to update.
///
/// Can only be used to encode.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct UpdatePeerRef<'a> {
    /// The id of the object being updated.
    pub object_id: Id,
    /// The key to update.
    pub key: Key,
    /// The value to update.
    pub value: &'a Value,
}

/// Information that a peer has updated.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdatedPeer {
    /// The peer that updated.
    pub peer_id: PeerId,
    /// The object id being updated.
    pub object_id: Id,
    /// The key that was updated.
    pub key: Key,
    /// The value that was updated.
    pub value: Value,
}

/// Information that a peer has updated.
///
/// Can only be used to encode.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct UpdatedPeerRef<'a> {
    /// The peer that updated.
    pub peer_id: PeerId,
    /// The object id being updated.
    pub object_id: Id,
    /// The key that was updated.
    pub key: Key,
    /// The value that was updated.
    pub value: &'a Value,
}

/// A request to add an object. Sent by the client to the server.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct AddObjectBody {
    /// The object being added.
    pub object: RemoteObject,
}

/// Broadcast by the server when a peer adds an object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ObjectAddedBody {
    /// The peer that added the object.
    pub peer_id: PeerId,
    /// The object that was added.
    pub object: RemoteObject,
}

/// A request to remove an object. Sent by the client to the server.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoveObjectBody {
    /// The id of the object being removed.
    pub object_id: Id,
}

/// Broadcast by the server when a peer removes an object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ObjectRemovedBody {
    /// The peer that removed the object.
    pub peer_id: PeerId,
    /// The id of the object that was removed.
    pub object_id: Id,
}

api::define! {
    pub type Connect;

    impl Broadcast for Connect {
        impl Event for ConnectBody;
    }

    pub type Ping;

    impl Broadcast for Ping {
        impl Event for PingBody;
    }

    pub type Pong;

    impl Broadcast for Pong {
        impl Event for PongBody;
    }

    pub type Join;

    impl Broadcast for Join {
        impl Event for JoinBody;
        impl Event for JoinBodyRef<'_>;
    }

    pub type Leave;

    impl Broadcast for Leave {
        impl Event for LeaveBody;
    }

    pub type Update;

    impl Broadcast for Update {
        impl Event for UpdatePeer;
        impl Event for UpdatePeerRef<'_>;
    }

    pub type Updated;

    impl Broadcast for Updated {
        impl Event for UpdatedPeer;
        impl Event for UpdatedPeerRef<'_>;
    }

    pub type AddObject;

    impl Broadcast for AddObject {
        impl Event for AddObjectBody;
    }

    pub type ObjectAdded;

    impl Broadcast for ObjectAdded {
        impl Event for ObjectAddedBody;
    }

    pub type RemoveObject;

    impl Broadcast for RemoveObject {
        impl Event for RemoveObjectBody;
    }

    pub type ObjectRemoved;

    impl Broadcast for ObjectRemoved {
        impl Event for ObjectRemovedBody;
    }
}
