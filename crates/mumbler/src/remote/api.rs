use core::fmt;

use musli_core::{Decode, Encode};
use musli_web::api;

use ::api::Vec3;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct PeerId {
    raw: u64,
}

impl PeerId {
    /// Generate a random peer id.
    pub fn random() -> Self {
        Self {
            raw: rand::random(),
        }
    }
}

impl fmt::Debug for PeerId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.raw.fmt(f)
    }
}

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
    pub id: PeerId,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct LeaveBody {
    /// The peer that left the room.
    pub id: PeerId,
}

/// A request to move.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct MoveToBody {
    /// The position of the peer.
    pub position: Vec3,
    /// The front of the peer.
    pub front: Vec3,
}

/// Information that a peer moved.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct MovedToBody {
    /// The peer that moved.
    pub id: PeerId,
    /// The position of the peer.
    pub position: Vec3,
    /// The front of the peer.
    pub front: Vec3,
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
    }

    pub type Leave;

    impl Broadcast for Leave {
        impl Event for LeaveBody;
    }

    pub type Move;

    impl Broadcast for Move {
        impl Event for MoveToBody;
    }

    pub type Moved;

    impl Broadcast for Moved {
        impl Event for MovedToBody;
    }
}
