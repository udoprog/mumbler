mod id;
pub use id::Id;

use core::fmt;

use musli_core::{Decode, Encode};
use musli_web::api;

/// Represents an RGBA color with 8-bit components.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    /// Create a new color from RGBA components.
    pub const fn new(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// A nice neutral gray color (default).
    pub const fn neutral_gray() -> Self {
        Self::new(107, 114, 128, 255)
    }

    /// Convert to a CSS color string.
    pub fn to_css_string(&self) -> String {
        if self.a == 255 {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!(
                "rgba({}, {}, {}, {})",
                self.r,
                self.g,
                self.b,
                self.a as f32 / 255.0
            )
        }
    }

    /// Parse a color from a CSS hex string (e.g., "#6B7280" or "#6B7280FF").
    pub fn from_hex(hex: &str) -> Option<Self> {
        let hex = hex.strip_prefix('#')?;

        match hex.len() {
            6 => {
                let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
                let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
                let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
                Some(Self::new(r, g, b, 255))
            }
            8 => {
                let r = u8::from_str_radix(hex.get(0..2)?, 16).ok()?;
                let g = u8::from_str_radix(hex.get(2..4)?, 16).ok()?;
                let b = u8::from_str_radix(hex.get(4..6)?, 16).ok()?;
                let a = u8::from_str_radix(hex.get(6..8)?, 16).ok()?;
                Some(Self::new(r, g, b, a))
            }
            _ => None,
        }
    }
}

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

#[derive(Clone, Copy, Default, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Vec3 {
    /// The x coordinate in meters from the origin (left / right).
    pub x: f32,
    /// The y coordinate in meters from the origin (up / down).
    pub y: f32,
    /// The z coordinate in meters from the origin (forward / backward).
    pub z: f32,
}

impl fmt::Debug for Vec3 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Vec3")
            .field(&self.x)
            .field(&self.y)
            .field(&self.z)
            .finish()
    }
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

/// Represents a position and orientation in 3D space.
#[derive(Debug, Clone, Copy, Default, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Transform {
    /// The position in world coordinates.
    pub position: Vec3,
    /// The direction facing as a unit vector.
    pub front: Vec3,
}

impl Transform {
    /// Creates a new transform with the given position and front direction.
    pub const fn new(position: Vec3, front: Vec3) -> Self {
        Self { position, front }
    }

    /// A transform at the origin facing forward.
    pub const fn origin() -> Self {
        Self::new(Vec3::ZERO, Vec3::FORWARD)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteAvatar {
    /// The identifier of the remote avatar.
    pub id: Id,
    /// The transform (position and orientation) of the avatar.
    pub transform: Transform,
    /// Indicates if the remote avatar has an image.
    pub image: Option<Id>,
    /// The custom color for the avatar.
    pub color: Color,
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Avatar {
    /// The transform (position and orientation) of the avatar.
    pub transform: Transform,
    /// The unique identifier of the avatar image, if any.
    pub image: Option<Id>,
    /// The custom color for the avatar.
    pub color: Color,
}

/// Event emitted when the API is initialized.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeEvent {
    /// The player avatar.
    pub player: Avatar,
    /// The name of the current user.
    pub name: Option<String>,
    /// List of remote avatars.
    pub remote_avatars: Vec<RemoteAvatar>,
    /// The configuration of the world.
    pub world: World,
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
    /// The selected color for the avatar.
    pub color: Color,
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

/// Request to select a custom color for the player's avatar.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct SelectColorRequest {
    pub color: Color,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct SelectColorResponse {
    /// The selected color.
    pub color: Color,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum RemoteAvatarUpdateBody {
    RemoteLost,
    Join { peer_id: Id },
    Leave { peer_id: Id },
    Move { peer_id: Id, transform: Transform },
    ImageUpdated { peer_id: Id, image: Option<Id> },
    ColorUpdated { peer_id: Id, color: Color },
}

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

    pub type SelectColor;

    impl Endpoint for SelectColor {
        impl Request for SelectColorRequest;
        type Response<'de> = SelectColorResponse;
    }

    pub type RemoteAvatarUpdate;

    impl Broadcast for RemoteAvatarUpdate {
        impl Event for RemoteAvatarUpdateBody;
    }
}
