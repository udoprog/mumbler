use musli_core::{Decode, Encode};
use musli_web::api;

use ::api::{ContentType, Id, Key, PeerId, RemoteObject, Value};

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
    /// List of objects owned by peer.
    pub objects: Vec<RemoteObject>,
    /// List of images owned by peer.
    pub images: Vec<RemoteImage>,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct ConnectBodyRef<'a> {
    /// The protocol version of the client.
    pub version: u32,
    /// The context to connect to.
    pub room: &'a [u8],
    /// List of objects owned by peer.
    pub objects: &'a [RemoteObject],
    /// List of images owned by peer.
    pub images: &'a [RemoteImage],
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

/// A remote image associated with a peer.
#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteImage {
    /// The id of the image.
    pub id: Id,
    /// The content type of the image.
    pub content_type: ContentType,
    /// The bytes of the image.
    pub bytes: Box<[u8]>,
    /// The width of the image.
    pub width: u32,
    /// The height of the image.
    pub height: u32,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct JoinBody {
    /// The peer that joined.
    pub peer_id: PeerId,
    /// The key-value pairs that were immediately set for the peer.
    pub objects: Vec<RemoteObject>,
    /// Remote images associated with the peer.
    pub images: Vec<RemoteImage>,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct JoinBodyRef<'a> {
    /// The peer that joined.
    pub peer_id: PeerId,
    /// The objects thare are associated with the peer.
    pub objects: &'a [RemoteObject],
    /// The images that are associated with the peer.
    pub images: &'a [RemoteImage],
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

/// A request to add an object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct AddObjectBody {
    /// The object being added.
    pub object: RemoteObject,
}

/// A request to add a new image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct AddImageBody {
    /// The image being added.
    pub image: RemoteImage,
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

/// Broadcast by the server when a peer adds an image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ImageAddedBody {
    /// The peer that added the image.
    pub peer_id: PeerId,
    /// The image that was added.
    pub image: RemoteImage,
}

/// A request to remove an object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoveObjectBody {
    /// The id of the object being removed.
    pub object_id: Id,
}

/// A request to remove an image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoveImageBody {
    /// The id of the image being removed.
    pub image_id: Id,
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

/// Broadcast by the server when a peer removes an image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ImageRemovedBody {
    /// The peer that removed the image.
    pub peer_id: PeerId,
    /// The id of the image that was removed.
    pub image_id: Id,
}

api::define! {
    pub type Connect;

    impl Broadcast for Connect {
        impl Event for ConnectBody;
        impl Event for ConnectBodyRef<'_>;
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

    pub type AddImage;

    impl Broadcast for AddImage {
        impl Event for AddImageBody;
    }

    pub type ImageAdded;

    impl Broadcast for ImageAdded {
        impl Event for ImageAddedBody;
    }

    pub type RemoveObject;

    impl Broadcast for RemoveObject {
        impl Event for RemoveObjectBody;
    }

    pub type ObjectRemoved;

    impl Broadcast for ObjectRemoved {
        impl Event for ObjectRemovedBody;
    }

    pub type RemoveImage;

    impl Broadcast for RemoveImage {
        impl Event for RemoveImageBody;
    }

    pub type ImageRemoved;

    impl Broadcast for ImageRemoved {
        impl Event for ImageRemovedBody;
    }
}
