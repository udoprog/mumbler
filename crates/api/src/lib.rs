mod id;
pub use id::Id;

use musli_core::{Decode, Encode};
use musli_web::api;

#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdatePlayerRequest {
    pub avatar: Avatar,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdatePlayerResponse;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UploadImageRequest {
    /// MIME type of the uploaded image (e.g. "image/png").
    pub content_type: String,
    /// Raw bytes of the image file.
    pub data: Vec<u8>,
}

/// Response returned after successfully uploading an image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UploadImageResponse {
    /// The unique identifier of the uploaded image.
    pub id: Id,
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Span {
    /// Start of the span.
    pub start: f32,
    /// End of the span.
    pub end: f32,
}

impl Span {
    /// Returns `true` if `value` lies within `[start, end]`.
    #[inline]
    pub fn contains(self, value: f32) -> bool {
        self.start <= value && value <= self.end
    }
}

#[derive(Debug, Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Extent2 {
    /// Extent along the x axis.
    pub x: Span,
    /// Extent along the y axis.
    pub y: Span,
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
    /// The zero vector.
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
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
    pub id: Id,
    /// The position of the avatar on the map, in world coordinates.
    pub position: Vec3,
    /// The direction the avatar is facing, as a unit vector in world coordinates (x/z plane).
    pub front: Vec3,
    /// The unique identifier of the avatar image, if any.
    pub image: Option<Id>,
}

/// Event emitted when the API is initialized.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeEvent {
    /// The player avatar.
    pub player: Avatar,
    /// The name of the current user.
    pub name: Option<String>,
    /// List of current avatars.
    pub avatars: Vec<Avatar>,
    /// The configuration of the world.
    pub world: World,
    /// Included images.
    pub images: Vec<Image>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ListSettingsRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Image {
    /// The unique identifier of the image.
    pub id: Id,
    /// The width of the image in pixels.
    pub width: u32,
    /// The height of the image in pixels.
    pub height: u32,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ListSettingsResponse {
    /// The unique identifier of the currently selected avatar image.
    pub selected: Option<Id>,
    /// List of image identifiers currently stored in the database.
    pub images: Vec<Image>,
}

/// Request to select an image for use as the player's avatar.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct SelectImageRequest {
    pub id: Id,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct SelectImageResponse {
    /// The unique identifier of the selected image.
    pub id: Id,
}

/// Request to delete a stored image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteImageRequest {
    pub id: Id,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteImageResponse;

api::define! {
    pub type Initialize;

    impl Endpoint for Initialize {
        impl Request for InitializeRequest;
        type Response<'de> = InitializeEvent;
    }

    pub type UpdatePlayer;

    impl Endpoint for UpdatePlayer {
        impl Request for UpdatePlayerRequest;
        type Response<'de> = UpdatePlayerResponse;
    }

    pub type UploadImage;

    impl Endpoint for UploadImage {
        impl Request for UploadImageRequest;
        type Response<'de> = UploadImageResponse;
    }

    pub type ListSettings;

    impl Endpoint for ListSettings {
        impl Request for ListSettingsRequest;
        type Response<'de> = ListSettingsResponse;
    }

    pub type SelectImage;

    impl Endpoint for SelectImage {
        impl Request for SelectImageRequest;
        type Response<'de> = SelectImageResponse;
    }

    pub type DeleteImage;

    impl Endpoint for DeleteImage {
        impl Request for DeleteImageRequest;
        type Response<'de> = DeleteImageResponse;
    }
}
