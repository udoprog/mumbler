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
use std::collections::hash_map::IntoIter;

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

    /// Convert to a transparent css color.
    pub fn to_transparent_rgba(&self, a: f32) -> String {
        format!(
            "rgba({}, {}, {}, {})",
            self.r,
            self.g,
            self.b,
            (self.a as f32 / 255.0) * a.clamp(0.0, 1.0),
        )
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
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateLookAtRequest {
    pub look_at: Option<Vec3>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum ImageSizing {
    /// The crop region determines the sizing.
    Crop,
    /// Square the image padding by average color.
    Square,
}

impl ImageSizing {
    /// If the image should be squared.
    #[inline]
    pub fn is_square(&self) -> bool {
        matches!(self, Self::Square)
    }
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UploadImageRequest {
    /// MIME type of the uploaded image (e.g. "image/png").
    pub content_type: String,
    /// Raw bytes of the image file.
    pub data: Vec<u8>,
    /// The crop region to apply to the source image.
    pub crop: CropRegion,
    /// Requested image sizing.
    pub sizing: ImageSizing,
    /// The requested maximum size.
    pub size: u32,
}

/// A square crop region expressed in the source image's natural pixel space.
#[derive(Debug, Clone, Copy, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct CropRegion {
    pub x1: u32,
    pub y1: u32,
    pub x2: u32,
    pub y2: u32,
}

impl CropRegion {
    /// If the region is the whole image.
    #[inline]
    pub fn is_whole_image(&self, width: u32, height: u32) -> bool {
        self.x1 == 0 && self.y1 == 0 && self.x2 == width && self.y2 == height
    }
}

/// Response returned after successfully uploading an image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UploadImageResponse {
    /// The unique identifier of the uploaded image.
    pub id: Id,
}

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
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

#[derive(Debug, Clone, Copy, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Extent {
    /// Extent along the x axis.
    pub x: Span,
    /// Extent along the y axis.
    pub y: Span,
}

impl Extent {
    /// Returns `true` if the point `(x, y)` lies within the extent.
    #[inline]
    pub fn contains(self, x: f32, y: f32) -> bool {
        self.x.contains(x) && self.y.contains(y)
    }

    /// A zero extent at the origin.
    pub const fn arena() -> Self {
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Encode, Decode)]
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

#[derive(Default, Clone, Debug, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Properties {
    /// Global values.
    values: HashMap<Key, Value>,
}

impl Properties {
    /// Construct a new empty set of properties.
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    /// Iterate over properties.
    pub fn iter(&self) -> impl Iterator<Item = (Key, &Value)> {
        self.values.iter().map(|(&k, v)| (k, v))
    }

    /// Get the value of a property by key.
    pub fn get(&self, key: Key) -> &Value {
        static DEFAULT: Value = Value::empty();
        self.values.get(&key).unwrap_or(&DEFAULT)
    }

    /// Test if the set of properties contains the given key.
    pub fn contains(&self, key: Key) -> bool {
        self.values.contains_key(&key)
    }

    /// Insert or update a property value by key.
    ///
    /// Inserting an [`Value::empty`] value is the equivalent of removing it.
    pub fn insert(&mut self, key: Key, value: Value) -> Value {
        if value.is_empty() {
            return self.remove(key);
        }

        let Some(value) = self.values.insert(key, value) else {
            return Value::empty();
        };

        value
    }

    /// Remove a property by key.
    pub fn remove(&mut self, key: Key) -> Value {
        let Some(value) = self.values.remove(&key) else {
            return Value::empty();
        };

        value
    }
}

impl IntoIterator for Properties {
    type Item = (Key, Value);
    type IntoIter = IntoIter<Key, Value>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.values.into_iter()
    }
}

impl<const N: usize> From<[(Key, Value); N]> for Properties {
    fn from(values: [(Key, Value); N]) -> Self {
        let mut properties = Properties::new();

        for (key, value) in values {
            properties.insert(key, value);
        }

        properties
    }
}

/// Two point coordinates in world space.
#[derive(Debug, Clone, Copy)]
pub struct VecXZ {
    pub x: f32,
    pub z: f32,
}

impl VecXZ {
    /// Construct a new `VecXZ` with the given coordinates.
    #[inline]
    pub fn new(x: f32, z: f32) -> Self {
        Self { x, z }
    }

    /// Calculate the direction from `self` to `other` as a unit vector.
    pub fn direction_to(&self, other: Self) -> Self {
        let angle_rad = (other.z - self.z).atan2(other.x - self.x);
        let dir_x = angle_rad.cos();
        let dir_z = angle_rad.sin();
        Self::new(dir_x, dir_z)
    }

    /// Calculate the distance between this and another point in the xz plane.
    #[inline]
    pub fn dist(&self, other: Self) -> f32 {
        (other.x - self.x).hypot(other.z - self.z)
    }

    /// Calculate the angle at which the XZ vector is facing in the xz plane
    /// where 0 degrees means facing in the positive x direction.
    #[inline]
    pub fn angle(&self) -> f32 {
        (-self.z).atan2(self.x)
    }

    /// Swizzle into a three component vector.
    #[inline]
    pub fn xyz(&self, y: f32) -> Vec3 {
        Vec3::new(self.x, y, self.z)
    }
}

#[derive(Clone, Copy, Default, PartialEq, Encode, Decode)]
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

    /// Extract the x and z coordinates.
    #[inline]
    pub fn xz(&self) -> VecXZ {
        VecXZ::new(self.x, self.z)
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
#[derive(Debug, Clone, Copy, Default, PartialEq, Encode, Decode)]
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

// The definition of a remote object.
#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteObject {
    /// The type of the object.
    pub ty: Type,
    /// The identifier of the object.
    pub id: Id,
    /// The properties of the object.
    pub props: Properties,
}

/// Event emitted when the map is initialized.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeMapEvent {
    pub objects: Vec<RemoteObject>,
    pub remote_objects: Vec<RemotePeerObject>,
    pub config: Properties,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetConfigRequest;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Image {
    /// The unique identifier of the image.
    pub id: Id,
    /// The width of the image in pixels.
    pub width: u32,
    /// The height of the image in pixels.
    pub height: u32,
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
    pub object: RemoteObject,
    /// List of image identifiers currently stored in the database.
    pub images: Vec<Image>,
}

/// Request to create a new local object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct CreateObjectRequest {
    /// The type of object to create.
    pub ty: Type,
    /// The initial properties of the object.
    pub props: Properties,
}

/// Request to delete a local object.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteObjectRequest {
    pub id: Id,
}

/// Request to delete a stored image.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct DeleteImageRequest {
    pub id: Id,
}

/// Request to update config.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateConfigRequest {
    pub values: Vec<(Key, Value)>,
}

/// Request to restart the mumble link connection.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct MumbleRestartRequest;

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

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemoteRestartRequest;

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum ServerNotificationBody {
    Info { component: String, message: String },
    Error { component: String, message: String },
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ConfigUpdateBody {
    pub key: Key,
    pub value: Value,
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum LocalUpdateBody {
    Update {
        object_id: Id,
        key: Key,
        value: Value,
    },
    Delete {
        object_id: Id,
    },
    Create {
        object: RemoteObject,
    },
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

    pub type GetConfig;

    impl Endpoint for GetConfig {
        impl Request for GetConfigRequest;
        type Response<'de> = Properties;
    }

    pub type GetObjectSettings;

    impl Endpoint for GetObjectSettings {
        impl Request for GetObjectSettingsRequest;
        type Response<'de> = GetObjectSettingsResponse;
    }

    pub type CreateObject;

    impl Endpoint for CreateObject {
        impl Request for CreateObjectRequest;
        type Response<'de> = Empty;
    }

    pub type DeleteObject;

    impl Endpoint for DeleteObject {
        impl Request for DeleteObjectRequest;
        type Response<'de> = Empty;
    }

    pub type DeleteImage;

    impl Endpoint for DeleteImage {
        impl Request for DeleteImageRequest;
        type Response<'de> = Empty;
    }

    pub type UpdateConfig;

    impl Endpoint for UpdateConfig {
        impl Request for UpdateConfigRequest;
        type Response<'de> = Empty;
    }

    pub type MumbleRestart;

    impl Endpoint for MumbleRestart {
        impl Request for MumbleRestartRequest;
        type Response<'de> = Empty;
    }

    pub type RemoteRestart;

    impl Endpoint for RemoteRestart {
        impl Request for RemoteRestartRequest;
        type Response<'de> = Empty;
    }

    pub type ConfigUpdate;

    impl Broadcast for ConfigUpdate {
        impl Event for ConfigUpdateBody;
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
