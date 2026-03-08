mod id;
pub use id::Id;

mod peer_id;
pub use peer_id::PeerId;

mod ty;
pub use ty::Type;

mod config;
pub use config::Key;

mod value;
pub use self::value::{Value, ValueKind, ValueType};

use core::fmt;
use std::collections::HashMap;

use musli_core::{Decode, Encode};
use musli_web::api;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Empty;

/// Represents an RGBA color with 8-bit components.
#[derive(Clone, Copy, PartialEq, Eq, Encode, Decode)]
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
    pub const fn neutral() -> Self {
        Self::new(0x66, 0xc5, 0xe5, 255)
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

impl fmt::Debug for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("Color")
            .field(&format_args!(
                "#{:02x}{:02x}{:02x}{:02x}",
                self.r, self.g, self.b, self.a
            ))
            .finish()
    }
}

#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeMapRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateRequest {
    pub object_id: Id,
    pub key: Key,
    pub value: Value,
    /// Whether the update should be broadcasted to the current connection.
    /// Normally this is false, since updates are handled on the frontend.
    pub broadcast_self: bool,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateLookAtRequest {
    pub look_at: Option<Vec3>,
}

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

impl Extent2 {
    /// Returns `true` if the point `(x, y)` lies within the extent.
    #[inline]
    pub fn contains(self, x: f32, y: f32) -> bool {
        self.x.contains(x) && self.y.contains(y)
    }

    /// A zero extent at the origin.
    pub const fn zero() -> Self {
        Self {
            x: Span {
                start: -10.0,
                end: 10.0,
            },
            y: Span {
                start: -10.0,
                end: 10.0,
            },
        }
    }
}

/// Represents a 2D pan offset in canvas pixels.
#[derive(Clone, Copy, Debug, Default, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Pan {
    pub x: f64,
    pub y: f64,
}

impl Pan {
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[inline]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0)
    }

    /// Add a delta to the pan offset.
    #[inline]
    pub fn add(&self, dx: f64, dy: f64) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct World {
    /// The zoom level of the map.
    pub zoom: f32,
    /// The pan offset in canvas pixels.
    pub pan: Pan,
    /// The extent of the world in meters.
    pub extent: Extent2,
    /// The radius of a token in meters.
    pub token_radius: f32,
}

impl World {
    /// A world with default settings.
    pub const fn zero() -> Self {
        Self {
            zoom: 2.0,
            pan: Pan::zero(),
            extent: Extent2::zero(),
            token_radius: 0.5,
        }
    }
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

impl Vec3 {
    /// Convert the vector to an array of three floats.
    #[inline]
    pub fn as_array(&self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// Invert the z coordinate.
    #[inline]
    pub fn invert_z(&self) -> Self {
        Self {
            x: self.x,
            y: self.y,
            z: -self.z,
        }
    }
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
pub struct RemotePeerObject {
    pub peer_id: PeerId,
    pub object: RemoteObject,
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteObject {
    pub id: Id,
    pub properties: HashMap<Key, Value>,
}

/// Event emitted when the map is initialized.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeMapEvent {
    pub objects: Vec<RemoteObject>,
    pub remote_avatars: Vec<RemotePeerObject>,
    pub world: World,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetSettingsRequest;

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
pub struct GetSettingsResponse {
    /// List of objects.
    pub objects: Vec<RemoteObject>,
    /// List of image identifiers currently stored in the database.
    pub images: Vec<Image>,
    /// The remote host[:port] combination.
    pub remote_server: Option<String>,
    /// Whether TLS is enabled for the remote server connection.
    pub remote_server_tls: bool,
}

/// Request to fetch settings for a single object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetObjectSettingsRequest {
    pub id: Id,
}

/// Response containing settings for a single object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetObjectSettingsResponse {
    /// The object, if it exists.
    pub object: Option<RemoteObject>,
    /// List of image identifiers currently stored in the database.
    pub images: Vec<Image>,
}

/// Request to create a new local object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct CreateObjectRequest;

/// Response returned after creating a new object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct CreateObjectResponse {
    /// The newly created object ID.
    pub id: Id,
}

/// Request to delete a local object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteObjectRequest {
    pub id: Id,
}

/// Response after deleting an object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteObjectResponse;

/// Request to delete a stored image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteImageRequest {
    pub id: Id,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteImageResponse;

/// Request to update world settings (pan and zoom).
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateWorldRequest {
    pub pan: Pan,
    pub zoom: f32,
}

/// Request to restart the mumble link connection.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct MumbleRestartRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct MumbleRestartResponse;

/// Request to toggle mumble integration enabled/disabled.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct MumbleToggleRequest {
    pub enabled: bool,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct MumbleToggleResponse {
    pub enabled: bool,
}

/// Request to get the mumble status.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetMumbleStatusRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetMumbleStatusResponse {
    pub enabled: bool,
}

// Remote server management.

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetRemoteStatusRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetRemoteStatusResponse {
    pub enabled: bool,
    pub server: Option<String>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteRestartRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteRestartResponse;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteToggleRequest {
    pub enabled: bool,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteToggleResponse {
    pub enabled: bool,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct SetRemoteServerRequest {
    pub server: Option<String>,
    pub tls: bool,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct SetRemoteServerResponse {
    pub server: Option<String>,
    pub tls: bool,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum ServerNotificationBody {
    Info { component: String, message: String },
    Error { component: String, message: String },
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct LocalUpdateBody {
    pub object_id: Id,
    pub key: Key,
    pub value: Value,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum RemoteUpdateBody {
    RemoteLost,
    Join {
        peer_id: PeerId,
        objects: Vec<RemoteObject>,
    },
    Leave {
        peer_id: PeerId,
    },
    Update {
        peer_id: PeerId,
        object_id: Id,
        key: Key,
        value: Value,
    },
    ObjectAdded {
        peer_id: PeerId,
        object: RemoteObject,
    },
    ObjectRemoved {
        peer_id: PeerId,
        object_id: Id,
    },
}

api::define! {
    pub type InitializeMap;

    impl Endpoint for InitializeMap {
        impl Request for InitializeMapRequest;
        type Response<'de> = InitializeMapEvent;
    }

    pub type Update;

    impl Endpoint for Update {
        impl Request for UpdateRequest;
        type Response<'de> = Empty;
    }

    pub type UploadImage;

    impl Endpoint for UploadImage {
        impl Request for UploadImageRequest;
        type Response<'de> = UploadImageResponse;
    }

    pub type GetSettings;

    impl Endpoint for GetSettings {
        impl Request for GetSettingsRequest;
        type Response<'de> = GetSettingsResponse;
    }

    pub type GetObjectSettings;

    impl Endpoint for GetObjectSettings {
        impl Request for GetObjectSettingsRequest;
        type Response<'de> = GetObjectSettingsResponse;
    }

    pub type CreateObject;

    impl Endpoint for CreateObject {
        impl Request for CreateObjectRequest;
        type Response<'de> = CreateObjectResponse;
    }

    pub type DeleteObject;

    impl Endpoint for DeleteObject {
        impl Request for DeleteObjectRequest;
        type Response<'de> = DeleteObjectResponse;
    }

    pub type DeleteImage;

    impl Endpoint for DeleteImage {
        impl Request for DeleteImageRequest;
        type Response<'de> = DeleteImageResponse;
    }

    pub type UpdateWorld;

    impl Endpoint for UpdateWorld {
        impl Request for UpdateWorldRequest;
        type Response<'de> = Empty;
    }

    pub type MumbleRestart;

    impl Endpoint for MumbleRestart {
        impl Request for MumbleRestartRequest;
        type Response<'de> = MumbleRestartResponse;
    }

    pub type MumbleToggle;

    impl Endpoint for MumbleToggle {
        impl Request for MumbleToggleRequest;
        type Response<'de> = MumbleToggleResponse;
    }

    pub type GetMumbleStatus;

    impl Endpoint for GetMumbleStatus {
        impl Request for GetMumbleStatusRequest;
        type Response<'de> = GetMumbleStatusResponse;
    }

    pub type GetRemoteStatus;

    impl Endpoint for GetRemoteStatus {
        impl Request for GetRemoteStatusRequest;
        type Response<'de> = GetRemoteStatusResponse;
    }

    pub type RemoteRestart;

    impl Endpoint for RemoteRestart {
        impl Request for RemoteRestartRequest;
        type Response<'de> = RemoteRestartResponse;
    }

    pub type RemoteToggle;

    impl Endpoint for RemoteToggle {
        impl Request for RemoteToggleRequest;
        type Response<'de> = RemoteToggleResponse;
    }

    pub type SetRemoteServer;

    impl Endpoint for SetRemoteServer {
        impl Request for SetRemoteServerRequest;
        type Response<'de> = SetRemoteServerResponse;
    }

    pub type LocalUpdate;

    impl Broadcast for LocalUpdate {
        impl Event for LocalUpdateBody;
    }

    pub type RemoteUpdate;

    impl Broadcast for RemoteUpdate {
        impl Event for RemoteUpdateBody;
    }

    pub type ServerNotification;

    impl Broadcast for ServerNotification {
        impl Event for ServerNotificationBody;
    }
}
