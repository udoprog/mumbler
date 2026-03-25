use core::fmt;

use base64::display::Base64Display;
use base64::engine::GeneralPurpose;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use musli_core::{Decode, Encode};
use musli_web::api;

use ::api::{ContentType, Id, Key, PeerId, Properties, PublicKey, RemoteObject, Role, Value};

/// The engine used for base64.
static ENGINE: GeneralPurpose = URL_SAFE_NO_PAD;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Header {
    /// The type of the request.
    pub request: u16,
    /// The type id of the error message.
    pub error: u16,
}

/// A signature.
#[derive(Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Signature {
    raw: [u8; 64],
}

impl Signature {
    /// Construct a raw signature.
    #[inline]
    pub fn new(raw: [u8; 64]) -> Self {
        Self { raw }
    }

    /// The raw bytes of the signature.
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.raw
    }
}

impl fmt::Display for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this = Base64Display::new(&self.raw, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

impl fmt::Debug for Signature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let this = Base64Display::new(&self.raw, &ENGINE);
        fmt::Display::fmt(&this, f)
    }
}

/// Sent by the server in response to [`HelloBodyRef`]; the client must sign the
/// nonce with its private key and respond with a [`ConnectBodyRef`].
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ChallengeBody {
    /// Random server-generated nonce (32 bytes).
    pub nonce: Box<[u8]>,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct ChallengeBodyRef<'a> {
    /// Random server-generated nonce (32 bytes).
    pub nonce: &'a [u8],
}

/// Sent by the client to announce its presence. Once received, the server will
/// send a challenge to authenticate the client.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct HelloBody {
    /// The protocol version of the client.
    pub version: u32,
}

/// Sent by the client in response to a [`ChallengeBody`] to authenticate.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ConnectBody {
    /// The requested public key.
    pub public_key: PublicKey,
    /// The signature over the server challenge nonce to prove that the peer
    /// controls the private component of the provided public key.
    pub signature: Signature,
    /// List of objects owned by peer.
    pub objects: Vec<RemoteObject>,
    /// List of images owned by peer.
    pub images: Vec<RemoteImage>,
    /// Properties of the client.
    pub props: Properties,
}

/// Sent by the client in response to a [`ChallengeBody`] to authenticate.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct ConnectBodyRef<'a> {
    /// The requested public key.
    pub public_key: PublicKey,
    /// The signature over the server challenge nonce to prove that the peer
    /// controls the private component of the provided public key.
    pub signature: Signature,
    /// List of objects owned by peer.
    pub objects: &'a [RemoteObject],
    /// List of images owned by peer.
    pub images: &'a [RemoteImage],
    /// Properties of the client.
    pub props: &'a Properties,
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
    /// The role of the image.
    pub role: Role,
    /// The width of the image.
    pub width: u32,
    /// The height of the image.
    pub height: u32,
    /// The bytes of the image.
    pub bytes: Box<[u8]>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PeerConnectedBody {
    /// The public key of the peer that connected.
    pub public_key: PublicKey,
    /// The peer that connected.
    pub peer_id: PeerId,
    /// Properties of the peer.
    pub props: Properties,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct PeerConnectedBodyRef<'a> {
    /// The public key of the peer that connected.
    pub public_key: PublicKey,
    /// The peer that connected.
    pub peer_id: PeerId,
    /// Properties of the peer.
    pub props: &'a Properties,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PeerJoinBody {
    /// The peer that joined.
    pub peer_id: PeerId,
    /// The key-value pairs that were immediately set for the peer.
    pub objects: Vec<RemoteObject>,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct PeerJoinBodyRef<'a> {
    /// The peer that joined.
    pub peer_id: PeerId,
    /// The objects that are associated with the peer.
    pub objects: &'a [RemoteObject],
}

/// A request to update a peer.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PeerUpdateBody {
    /// The key to update.
    pub key: Key,
    /// The value to update.
    pub value: Value,
}

/// A request to update a peer.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct PeerUpdateBodyRef<'a> {
    /// The key to update.
    pub key: Key,
    /// The value to update.
    pub value: &'a Value,
}

/// Information that a peer has updated.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PeerUpdatedBody {
    /// The peer that updated.
    pub peer_id: PeerId,
    /// The key that was updated.
    pub key: Key,
    /// The value that was updated.
    pub value: Value,
}

/// Information that a peer has updated.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct PeerUpdatedBodyRef<'a> {
    /// The peer that updated.
    pub peer_id: PeerId,
    /// The key that was updated.
    pub key: Key,
    /// The value that was updated.
    pub value: &'a Value,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PeerDisconnectBody {
    /// The peer that disconnected.
    pub id: PeerId,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PeerLeaveBody {
    /// The peer that left the room.
    pub id: PeerId,
}

/// A request to update.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ObjectUpdateBody {
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
pub struct ObjectUpdateBodyRef<'a> {
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
pub struct ObjectUpdatedBody {
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
pub struct ObjectUpdatedBodyRef<'a> {
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
pub struct ObjectCreateBody {
    /// The object being added.
    pub object: RemoteObject,
}

/// A request to add a new image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ImageCreateBody {
    /// The image being added.
    pub image: RemoteImage,
}

/// Broadcast by the server when a peer adds an object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ObjectCreatedBody {
    /// The peer that added the object.
    pub peer_id: PeerId,
    /// The object that was added.
    pub object: RemoteObject,
}

/// Broadcast by the server when a peer adds an object.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct ObjectCreatedBodyRef<'a> {
    /// The peer that added the object.
    pub peer_id: PeerId,
    /// The object that was added.
    pub object: &'a RemoteObject,
}

/// Broadcast by the server when a peer adds an image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ImageCreatedBody {
    /// The peer that added the image.
    pub peer_id: PeerId,
    /// The image that was added.
    pub image: RemoteImage,
}

/// Broadcast by the server when a peer adds an image.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct ImageCreatedBodyRef<'a> {
    /// The peer that added the image.
    pub peer_id: PeerId,
    /// The image that was added.
    pub image: &'a RemoteImage,
}

/// A request to remove an object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ObjectRemoveBody {
    /// The id of the object being removed.
    pub object_id: Id,
}

/// A request to remove an image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ImageRemoveBody {
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
    pub type Challenge;

    impl Broadcast for Challenge {
        impl Event for ChallengeBody;
        impl Event for ChallengeBodyRef<'_>;
    }

    pub type Hello;

    impl Broadcast for Hello {
        impl Event for HelloBody;
    }

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

    pub type PeerConnected;

    impl Broadcast for PeerConnected {
        impl Event for PeerConnectedBody;
        impl Event for PeerConnectedBodyRef<'_>;
    }

    pub type PeerJoin;

    impl Broadcast for PeerJoin {
        impl Event for PeerJoinBody;
        impl Event for PeerJoinBodyRef<'_>;
    }

    pub type PeerDisconnect;

    impl Broadcast for PeerDisconnect {
        impl Event for PeerDisconnectBody;
    }

    pub type PeerLeave;

    impl Broadcast for PeerLeave {
        impl Event for PeerLeaveBody;
    }

    pub type PeerUpdate;

    impl Broadcast for PeerUpdate {
        impl Event for PeerUpdateBody;
        impl Event for PeerUpdateBodyRef<'_>;
    }

    pub type PeerUpdated;

    impl Broadcast for PeerUpdated {
        impl Event for PeerUpdatedBody;
        impl Event for PeerUpdatedBodyRef<'_>;
    }

    pub type ObjectUpdate;

    impl Broadcast for ObjectUpdate {
        impl Event for ObjectUpdateBody;
        impl Event for ObjectUpdateBodyRef<'_>;
    }

    pub type ObjectUpdated;

    impl Broadcast for ObjectUpdated {
        impl Event for ObjectUpdatedBody;
        impl Event for ObjectUpdatedBodyRef<'_>;
    }

    pub type ObjectCreate;

    impl Broadcast for ObjectCreate {
        impl Event for ObjectCreateBody;
    }

    pub type ObjectCreated;

    impl Broadcast for ObjectCreated {
        impl Event for ObjectCreatedBody;
        impl Event for ObjectCreatedBodyRef<'_>;
    }

    pub type ImageCreate;

    impl Broadcast for ImageCreate {
        impl Event for ImageCreateBody;
    }

    pub type ImageCreated;

    impl Broadcast for ImageCreated {
        impl Event for ImageCreatedBody;
        impl Event for ImageCreatedBodyRef<'_>;
    }

    pub type ObjectRemove;

    impl Broadcast for ObjectRemove {
        impl Event for ObjectRemoveBody;
    }

    pub type ObjectRemoved;

    impl Broadcast for ObjectRemoved {
        impl Event for ObjectRemovedBody;
    }

    pub type ImageRemove;

    impl Broadcast for ImageRemove {
        impl Event for ImageRemoveBody;
    }

    pub type ImageRemoved;

    impl Broadcast for ImageRemoved {
        impl Event for ImageRemovedBody;
    }
}
