mod macros;

mod id;
pub use id::Id;

mod peer_id;
pub use peer_id::PeerId;

mod public_key;
pub use public_key::PublicKey;

mod remote_id;
pub use remote_id::RemoteId;

mod stable_id;
pub use stable_id::StableId;

mod value;
pub use self::value::{Value, ValueKind, ValueType};

use core::fmt;
use core::ops::{Add, Sub};
use std::collections::HashMap;
use std::collections::hash_map::IntoIter;

use musli_core::{Decode, Encode};
use musli_web::api;

crate::macros::ids! {
    pub struct Type {
        /// The token type.
        TOKEN = 0x10000000;
        /// The static object type (furniture, props, etc.).
        STATIC = 0x10000002;
        /// The group type.
        GROUP = 0x10000003;
        /// A saved room bookmark.
        ROOM = 0x80000001;
    }
}

impl Type {
    /// Test if the object is a global object. That means it will be sent to all
    /// peers regardless of room.
    #[inline]
    pub fn is_global(&self) -> bool {
        self.raw & 0x80000000 != 0
    }
}

crate::macros::keys! {
    pub struct Key {
        IMAGE_ID: Id = 0;
        COLOR: Color = 1;
        TRANSFORM: Transform = 2;
        LOOK_AT: Vec3 = 3;
        OBJECT_NAME: String = 4;
        MUMBLE_ENABLED: Boolean = 5;
        REMOTE_SERVER: String = 6;
        REMOTE_ENABLED: Boolean = 7;
        WORLD_SCALE: Float = 8;
        REMOTE_TLS: Boolean = 11;
        WORLD_ZOOM: Float = 9;
        WORLD_PAN: Pan = 10;
        WORLD_EXTENT: Extent = 12;
        /// The object which is used for mumble link.
        MUMBLE_OBJECT: Id = 14;
        /// Whether the object is hidden from remote peers.
        HIDDEN: Boolean = 15;
        /// Whether the object is hidden locally.
        LOCAL_HIDDEN: Boolean = 27;
        /// Whether selecting an object automatically sets it as the MumbleLink
        /// source.
        MUMBLE_FOLLOW: Boolean = 16;
        /// Per-object token radius.
        TOKEN_RADIUS: Float = 17;
        /// Per-object movement speed.
        SPEED: Float = 18;
        /// Width of a static object in world units.
        STATIC_WIDTH: Float = 19;
        /// Height of a static object in world units.
        STATIC_HEIGHT: Float = 20;
        /// Whether to maintain a fixed aspect ratio when resizing a static object.
        RATIO: Float = 23;
        /// An object is locked from further interaction. This prevents clicking on
        /// it in the map.
        LOCKED: Boolean = 22;
        /// Key for how this object is sorted.
        SORT: Bytes = 24;
        /// The group this object belongs to.
        GROUP: Id = 25;
        /// Whether a group is expanded in the UI.
        EXPANDED: Boolean = 26;
        /// The name of a peer.
        PEER_NAME: String = 28;
        /// A secret used to derive the peer's identity keypair.
        PEER_SECRET: String = 29;
        /// The name of the room to connect to on the remote server.
        ROOM: StableId = 30;
    }
}

impl Key {
    /// Test if the key is remotely exported, making it visible to other peers.
    pub fn is_remote(&self) -> bool {
        matches!(*self, Key::PEER_NAME | Key::ROOM)
    }
}

crate::macros::ids! {
    pub struct ContentType {
        PNG = 0;
    }
}

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

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ObjectUpdateBody {
    pub id: Id,
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

#[derive(Default, Clone, Encode, Decode)]
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

impl fmt::Debug for Properties {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.values.iter()).finish()
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

#[derive(Clone, Copy, Default, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
#[repr(C)]
pub struct Vec3 {
    /// The x coordinate in meters from the origin (left / right).
    pub x: f32,
    /// The y coordinate in meters from the origin (up / down).
    pub y: f32,
    /// The z coordinate in meters from the origin (forward / backward).
    pub z: f32,
}

impl Vec3 {
    /// A unit vector pointing up in the world positive y direction.
    pub const Y: Self = Self::new(0.0, 1.0, 0.0);

    /// Calculate the cross product of `self` and `other`.
    pub fn cross(&self, other: &Self) -> Self {
        Self::new(
            self.y * other.z - self.z * other.y,
            self.z * other.x - self.x * other.z,
            self.x * other.y - self.y * other.x,
        )
    }

    /// Calculate the dot product of `self` and `other`.
    pub fn dot(&self, other: &Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    /// Normalize the vector to a unit vector.
    pub fn normalize(&self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z).sqrt();
        Self::new(self.x / len, self.y / len, self.z / len)
    }

    /// Coerce into an array of floats.
    #[inline]
    pub fn as_array(&self) -> &[f32; 3] {
        // SAFETY: This struct is repr(C), which guarantees the layout.
        unsafe { &*(self as *const Self as *const [f32; 3]) }
    }

    /// Calculate the distance from `self` to `other`.
    ///
    /// ```
    /// use api::Vec3;
    ///
    /// let a = Vec3::new(1.0, 2.0, 3.0);
    /// let b = Vec3::new(4.0, 6.0, 8.0);
    ///
    /// assert!((a.dist(b) - 7.0710678118654755).abs() < 1e-6);
    /// ```
    #[inline]
    pub fn dist(&self, other: Self) -> f32 {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        let dz = other.z - self.z;
        (dx * dx + dy * dy + dz * dz).sqrt()
    }

    /// Calculate the direction from `self` to `other` as a unit vector.
    #[inline]
    pub fn direction_to(&self, other: Self) -> Self {
        let dx = other.x - self.x;
        let dy = other.y - self.y;
        let dz = other.z - self.z;
        let len = (dx * dx + dy * dy + dz * dz).sqrt();
        Self::new(dx / len, dy / len, dz / len)
    }

    /// Calculate the angle at which the XZ vector is facing in the xz plane
    /// where 0 degrees means facing in the positive x direction.
    #[inline]
    pub fn angle_xz(&self) -> f32 {
        (-self.z).atan2(self.x)
    }
}

impl Sub for Vec3 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self::Output {
        Self::new(self.x - rhs.x, self.y - rhs.y, self.z - rhs.z)
    }
}

impl Add for Vec3 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self::Output {
        Self::new(self.x + rhs.x, self.y + rhs.y, self.z + rhs.z)
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

    /// Transforms a world-space point into this transform's local space.
    pub fn transform_point(&self, point: Vec3) -> Vec3 {
        let right = self.front.cross(&Vec3::Y).normalize();
        let up = right.cross(&self.front).normalize();

        // Translate point into the transform's local origin.
        let delta = point - self.position;

        // Project onto each local axis to get local coordinates.
        Vec3::new(delta.dot(&right), delta.dot(&up), delta.dot(&self.front))
    }
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RemotePeer {
    pub peer_id: PeerId,
    pub public_key: PublicKey,
    pub props: Properties,
    pub objects: Vec<RemoteObject>,
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

#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeMapRequest;

/// Response when the map view is initialized.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeMapResponse {
    pub public_key: PublicKey,
    pub props: Properties,
    pub objects: Vec<RemoteObject>,
    pub images: Vec<RemoteId>,
    pub peers: Vec<RemotePeer>,
}

#[derive(Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeRoomsRequest;

/// Response when the rooms view is initialized.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeRoomsResponse {
    /// The public key associated with the local peer.
    pub public_key: PublicKey,
    /// List of image identifiers currently stored in the database.
    pub props: Properties,
    /// List of local rooms on the server.
    pub local: Vec<RemoteObject>,
    /// List of remote rooms associated with peers.
    pub peers: Vec<RemotePeer>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetConfigRequest;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Image {
    /// The unique identifier of the image.
    pub id: Id,
    /// The content type of the image.
    pub content_type: ContentType,
    /// The width of the image in pixels.
    pub width: u32,
    /// The height of the image in pixels.
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ImageWithData {
    /// The unique identifier of the image.
    pub id: Id,
    /// The content type of the image.
    pub content_type: ContentType,
    /// The raw bytes of the image file.
    pub bytes: Vec<u8>,
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
    /// The public key associated with the local peer.
    pub public_key: PublicKey,
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
pub struct RemoveObjectRequest {
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
pub struct UpdatesRequest {
    pub values: Vec<(Key, Value)>,
}

/// Information about a single room on the remote server.
#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct RoomInfo {
    pub room: RemoteId,
    pub name: String,
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

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum NotificationBody {
    Info { component: String, message: String },
    Error { component: String, message: String },
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum UpdateBody {
    /// A configuration change has occured.
    Config { key: Key, value: Value },
    /// The local peer id has been updated, likely because of a configuration
    /// change.
    PublicKey { public_key: PublicKey },
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum RemoteUpdateBody {
    /// Indicates that the remote connection has been lost and all local state
    /// should be cleared.
    RemoteLost,
    /// Indicates that a new peer has connected.
    PeerConnected {
        peer: RemotePeer,
    },
    /// Indicates that a new peer has joined.
    PeerJoin {
        peer_id: PeerId,
        objects: Vec<RemoteObject>,
        images: Vec<Id>,
    },
    /// A property update.
    PeerUpdate {
        peer_id: PeerId,
        key: Key,
        value: Value,
    },
    /// Indicates that a peer has fully disconnected.
    PeerDisconnect {
        peer_id: PeerId,
    },
    /// Indicates that a peer left your room.
    PeerLeave {
        peer_id: PeerId,
    },
    ObjectUpdated {
        id: RemoteId,
        key: Key,
        value: Value,
    },
    ObjectCreated {
        id: RemoteId,
        object: RemoteObject,
    },
    ObjectRemoved {
        id: RemoteId,
    },
    ImageAdded {
        id: RemoteId,
    },
    ImageRemoved {
        id: RemoteId,
    },
}

api::define! {
    pub type InitializeMap;

    impl Endpoint for InitializeMap {
        impl Request for InitializeMapRequest;
        type Response<'de> = InitializeMapResponse;
    }

    pub type InitializeRooms;

    impl Endpoint for InitializeRooms {
        impl Request for InitializeRoomsRequest;
        type Response<'de> = InitializeRoomsResponse;
    }

    pub type ObjectUpdate;

    impl Endpoint for ObjectUpdate {
        impl Request for ObjectUpdateBody;
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

    pub type RemoveObject;

    impl Endpoint for RemoveObject {
        impl Request for RemoveObjectRequest;
        type Response<'de> = Empty;
    }

    pub type DeleteImage;

    impl Endpoint for DeleteImage {
        impl Request for DeleteImageRequest;
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

    pub type Updates;

    impl Endpoint for Updates {
        impl Request for UpdatesRequest;
        type Response<'de> = Empty;
    }

    pub type Update;

    impl Broadcast for Update {
        impl Event for UpdateBody;
    }

    pub type RemoteUpdate;

    impl Broadcast for RemoteUpdate {
        impl Event for RemoteUpdateBody;
    }

    pub type Notification;

    impl Broadcast for Notification {
        impl Event for NotificationBody;
    }
}
