use std::collections::HashMap;

use musli_core::{Decode, Encode};
use musli_web::api;

use ::api::{Id, Key, Value};

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
    /// The key-value pairs to immediately set for the peer.
    pub values: HashMap<Key, Value>,
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
    /// The peer that joined the room.
    pub id: Id,
    /// The key-value pairs that were immediately set for the peer.
    pub values: HashMap<Key, Value>,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct JoinBodyRef<'a> {
    /// The peer that joined the room.
    pub id: Id,
    /// The key-value pairs that were immediately set for the peer.
    pub values: &'a HashMap<Key, Value>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct LeaveBody {
    /// The peer that left the room.
    pub id: Id,
}

/// A request to update.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdatePeer {
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
    pub id: Id,
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
    pub id: Id,
    /// The key that was updated.
    pub key: Key,
    /// The value that was updated.
    pub value: &'a Value,
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
}
