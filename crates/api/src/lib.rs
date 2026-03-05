use core::fmt;

use musli_core::{Decode, Encode};
use musli_web::api;

#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeRequest;

#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct AvatarId(u64);

impl AvatarId {
    #[inline]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }
}

impl fmt::Debug for AvatarId {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateAvatarsRequest {
    pub avatars: Vec<Avatar>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateAvatarsResponse;

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Extent2 {
    /// Start of the extent along the x axis.
    pub start_x: f32,
    /// End of the extent along the x axis.
    pub end_x: f32,
    /// Start of the extent along the y axis.
    pub start_y: f32,
    /// End of the extent along the y axis.
    pub end_y: f32,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct World {
    /// The zoom level of the map.
    pub zoom: f32,
    /// The extent of the world in meters.
    pub extent: Extent2,
    /// The radius of a token in meters.
    pub token_radius: f32,
    /// The identifier of the player avatar.
    pub player: AvatarId,
}

#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Vec3 {
    /// The x coordinate in meters from the origin (left / right).
    pub x: f32,
    /// The y coordinate in meters from the origin (up / down).
    pub y: f32,
    /// The z coordinate in meters from the origin (forward / backward).
    pub z: f32,
}

impl Vec3 {
    /// A unit vector pointing forward in the world (negative z direction).
    pub const FORWARD: Self = Self::new(0.0, 0.0, -1.0);

    /// Constructs a new position with the given coordinates.
    #[inline]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Avatar {
    /// The unique identifier of the avatar.
    pub id: AvatarId,
    /// The position of the avatar on the map, in world coordinates.
    pub position: Vec3,
    /// The direction the avatar is facing, as a unit vector in world coordinates (x/z plane).
    pub front: Vec3,
}

/// Event emitted when the API is initialized.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeEvent {
    /// The name of the current user.
    pub name: Option<String>,
    /// List of current avatars.
    pub avatars: Vec<Avatar>,
    /// The configuration of the world.
    pub world: World,
}

api::define! {
    pub type Initialize;

    impl Endpoint for Initialize {
        impl Request for InitializeRequest;
        type Response<'de> = InitializeEvent;
    }

    pub type UpdateAvatars;

    impl Endpoint for UpdateAvatars {
        impl Request for UpdateAvatarsRequest;
        type Response<'de> = UpdateAvatarsResponse;
    }
}
