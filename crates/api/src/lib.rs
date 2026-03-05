use musli_core::{Decode, Encode};
use musli_web::api;

#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Empty;

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct World {
    /// The width of the world in meters.
    pub width: f32,
    /// The height of the world in meters.
    pub height: f32,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Vec3 {
    /// The x coordinate in meters from the origin (forward / backward).
    pub x: f32,
    /// The y coordinate in meters from the origin (up / down).
    pub y: f32,
    /// The z coordinate in meters from the origin (left / right).
    pub z: f32,
}

impl Vec3 {
    /// Constructs a new position with the given coordinates.
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Avatar {
    /// The unique identifier of the avatar.
    pub id: u64,
    /// The position of the avatar on the map, in world coordinates.
    pub position: Vec3,
}

/// Event emitted when the API is initialized.
#[derive(Debug, Clone, Encode, Decode)]
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
        impl Request for Empty;
        type Response<'de> = InitializeEvent;
    }
}
