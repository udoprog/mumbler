mod macros;

mod id;
pub use id::Id;

mod peer_id;
pub use peer_id::PeerId;

mod public_key;
pub use public_key::PublicKey;

mod properties;
pub use properties::Properties;

mod remote_id;
pub use remote_id::RemoteId;

mod stable_id;
pub use stable_id::StableId;

mod value;
pub use self::value::{Value, ValueKind, ValueType};

mod ids;
pub use self::ids::{AtomicIds, Ids};

mod vec3;
pub use self::vec3::Vec3;

mod hash;

use core::fmt;
use core::ops::{Add, Sub};

use musli_core::{Decode, Encode};
use musli_web::api::{self, ChannelId};

crate::macros::ids! {
    /// The role of an image.
    pub struct Role {
        /// The token type.
        TOKEN = 1;
        /// The image of a static.
        STATIC = 2;
        /// A background image.
        BACKGROUND = 3;
    }
}

crate::macros::ids! {
    pub struct Type {
        /// The token type.
        TOKEN = 0x10000000;
        /// The static object type.
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

    /// Get the title of the type suitable for use in a UI.
    #[inline]
    pub fn title(&self) -> &'static str {
        match *self {
            Self::TOKEN => "Token",
            Self::STATIC => "Static",
            Self::GROUP => "Group",
            Self::ROOM => "Room",
            _ => "Object",
        }
    }

    /// Get lowercase display for type suitable for use in a human readable
    /// message.
    #[inline]
    pub fn display(&self) -> &'static str {
        match *self {
            Self::TOKEN => "token",
            Self::STATIC => "static",
            Self::GROUP => "group",
            Self::ROOM => "room",
            _ => "object",
        }
    }
}

crate::macros::keys! {
    pub struct Key {
        IMAGE_ID: Id = 0;
        COLOR: Color = 1;
        TRANSFORM: Transform = 2;
        LOOK_AT: Vec3 = 3;
        NAME: String = 4;
        MUMBLE_ENABLED: Boolean = 5;
        REMOTE_SERVER: String = 6;
        REMOTE_ENABLED: Boolean = 7;
        SCALE: Float = 8;
        REMOTE_TLS: Boolean = 11;
        ZOOM: Float = 9;
        PAN: Vec3 = 10;
        ROOM_EXTENT: Extent = 12;
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
        RADIUS: Float = 17;
        /// Per-object movement speed.
        SPEED: Float = 18;
        /// Width of a static object in world units.
        WIDTH: Float = 19;
        /// Height of a static object in world units.
        HEIGHT: Float = 20;
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
        /// The background image for a room.
        ROOM_BACKGROUND: Id = 31;
        /// Whether the grid is visible for a room.
        SHOW_GRID: Boolean = 32;
        /// A move target, if set causes the object to be animated to the move target.
        MOVE_TARGET: Vec3 = 33;
    }
}

impl Key {
    /// Test if the key is remotely exported, making it visible to other peers.
    pub fn is_remote(&self) -> bool {
        matches!(*self, Key::PEER_NAME | Key::ROOM)
    }

    pub fn id(&self) -> &'static str {
        match *self {
            Self::IMAGE_ID => "image-id",
            Self::COLOR => "color",
            Self::TRANSFORM => "transform",
            Self::LOOK_AT => "look-at",
            Self::NAME => "name",
            Self::MUMBLE_ENABLED => "mumble-enabled",
            Self::REMOTE_SERVER => "remote-server",
            Self::REMOTE_ENABLED => "remote-enabled",
            Self::SCALE => "scale",
            Self::REMOTE_TLS => "remote-tls",
            Self::ZOOM => "zoom",
            Self::PAN => "pan",
            Self::ROOM_EXTENT => "room-extent",
            Self::MUMBLE_OBJECT => "mumble-object",
            Self::HIDDEN => "hidden",
            Self::LOCAL_HIDDEN => "local-hidden",
            Self::MUMBLE_FOLLOW => "mumble-follow",
            Self::RADIUS => "radius",
            Self::SPEED => "speed",
            Self::WIDTH => "static-width",
            Self::HEIGHT => "static-height",
            Self::RATIO => "ratio",
            Self::LOCKED => "locked",
            Self::SORT => "sort",
            Self::GROUP => "group",
            Self::EXPANDED => "expanded",
            Self::PEER_NAME => "peer-name",
            Self::PEER_SECRET => "peer-secret",
            Self::ROOM => "room",
            Self::ROOM_BACKGROUND => "room-background",
            Self::SHOW_GRID => "show-grid",
            _ => "unknown",
        }
    }

    pub fn label(&self) -> &'static str {
        match *self {
            Self::IMAGE_ID => "Image Id",
            Self::COLOR => "Color",
            Self::TRANSFORM => "Transform",
            Self::LOOK_AT => "Look At",
            Self::NAME => "Name",
            Self::MUMBLE_ENABLED => "Mumble Enabled",
            Self::REMOTE_SERVER => "Remote Server",
            Self::REMOTE_ENABLED => "Remote Enabled",
            Self::SCALE => "Scale",
            Self::REMOTE_TLS => "Remote TLS",
            Self::ZOOM => "Zoom",
            Self::PAN => "Pan",
            Self::ROOM_EXTENT => "Room Extent",
            Self::MUMBLE_OBJECT => "Mumble Object",
            Self::HIDDEN => "Hidden",
            Self::LOCAL_HIDDEN => "Local Hidden",
            Self::MUMBLE_FOLLOW => "Mumble Follow",
            Self::RADIUS => "Radius",
            Self::SPEED => "Speed",
            Self::WIDTH => "Static Width",
            Self::HEIGHT => "Static Height",
            Self::RATIO => "Ratio",
            Self::LOCKED => "Locked",
            Self::SORT => "Sort",
            Self::GROUP => "Group",
            Self::EXPANDED => "Expanded",
            Self::PEER_NAME => "Peer Name",
            Self::PEER_SECRET => "Peer Secret",
            Self::ROOM => "Room",
            Self::ROOM_BACKGROUND => "Room Background",
            Self::SHOW_GRID => "Show Grid",
            _ => "Unknown",
        }
    }

    pub fn placeholder(&self) -> &'static str {
        match *self {
            Self::IMAGE_ID => "Enter Image Id",
            Self::COLOR => "Enter Color",
            Self::TRANSFORM => "Enter Transform",
            Self::LOOK_AT => "Enter Look At",
            Self::NAME => "Enter Name",
            Self::MUMBLE_ENABLED => "Enter Mumble Enabled",
            Self::REMOTE_SERVER => "Enter Remote Server",
            Self::REMOTE_ENABLED => "Enter Remote Enabled",
            Self::SCALE => "Enter Scale",
            Self::REMOTE_TLS => "Enter Remote TLS",
            Self::ZOOM => "Enter Zoom",
            Self::PAN => "Enter Pan",
            Self::ROOM_EXTENT => "Enter Room Extent",
            Self::MUMBLE_OBJECT => "Enter Mumble Object",
            Self::HIDDEN => "Enter Hidden",
            Self::LOCAL_HIDDEN => "Enter Local Hidden",
            Self::MUMBLE_FOLLOW => "Enter Mumble Follow",
            Self::RADIUS => "Enter Radius",
            Self::SPEED => "Enter Speed",
            Self::WIDTH => "Enter Static Width",
            Self::HEIGHT => "Enter Static Height",
            Self::RATIO => "Enter Ratio",
            Self::LOCKED => "Enter Locked",
            Self::SORT => "Enter Sort",
            Self::GROUP => "Enter Group",
            Self::EXPANDED => "Enter Expanded",
            Self::PEER_NAME => "Enter Peer Name",
            Self::PEER_SECRET => "Enter Peer Secret",
            Self::ROOM => "Set Room",
            Self::ROOM_BACKGROUND => "Set Room Background",
            Self::SHOW_GRID => "Toggle Show Grid",
            _ => "Set Unknown",
        }
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

    /// A neutral gray-blue color.
    pub const fn neutral() -> Self {
        Self::new(0x66, 0xc5, 0xe5, u8::MAX)
    }

    /// A neutral background.
    pub const fn neutral_background() -> Self {
        Self::new(0x10, 0x10, 0x10, u8::MAX)
    }

    /// Get the average color value.
    pub const fn factor(self) -> u32 {
        (self.r as u32 + self.g as u32 + self.b as u32) / 3
    }

    /// Test if the color is considered light.
    pub const fn is_light(self) -> bool {
        self.factor() >= 0x80
    }

    /// Darken the current color with the given factor.
    pub const fn darken(self, darkness: f32) -> Self {
        let factor = 1.0 - darkness.clamp(0.0, 1.0);
        let r = ((self.r as f32) * factor) as u8;
        let g = ((self.g as f32) * factor) as u8;
        let b = ((self.b as f32) * factor) as u8;
        Self::new(r, g, b, self.a)
    }

    /// Darken the current color with the given factor.
    pub const fn lighten(self, darkness: f32) -> Self {
        let factor = darkness.clamp(0.0, 1.0);
        let r = self.r + (((u8::MAX - self.r) as f32) * factor) as u8;
        let g = self.g + (((u8::MAX - self.g) as f32) * factor) as u8;
        let b = self.b + (((u8::MAX - self.b) as f32) * factor) as u8;
        Self::new(r, g, b, self.a)
    }

    /// Convert to a CSS color string.
    pub fn to_css_string(&self) -> String {
        if self.a == u8::MAX {
            format!("#{:02x}{:02x}{:02x}", self.r, self.g, self.b)
        } else {
            format!("#{:02x}{:02x}{:02x}{:02x}", self.r, self.g, self.b, self.a)
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
    pub values: Vec<(Key, Value)>,
}

#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct ObjectUpdateBodyRef<'a> {
    pub id: Id,
    pub values: &'a [(Key, Value)],
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdateLookAtRequest {
    pub look_at: Option<Vec3>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode)]
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
    /// The role of the image being uploaded.
    pub role: Role,
    /// The crop region to apply to the source image.
    pub crop: CropRegion,
    /// Requested image sizing.
    pub sizing: ImageSizing,
    /// The requested maximum size.
    pub size: u32,
    /// Raw bytes of the image file.
    pub data: Vec<u8>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UploadImageRequestRef<'a> {
    /// MIME type of the uploaded image (e.g. "image/png").
    pub content_type: &'a str,
    /// The role of the image being uploaded.
    pub role: Role,
    /// The crop region to apply to the source image.
    pub crop: CropRegion,
    /// Requested image sizing.
    pub sizing: ImageSizing,
    /// The requested maximum size.
    pub size: u32,
    /// Raw bytes of the image file.
    pub data: &'a [u8],
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
    /// The uploaded image.
    pub image: Image,
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
    /// The length of the span.
    ///
    /// # Examples
    ///
    /// ```
    /// use api::Span;
    ///
    /// let span = Span { start: 1.0, end: 3.0 };
    /// assert_eq!(span.len(), 2.0);
    ///
    /// let span = Span { start: 3.0, end: 1.0 };
    /// assert_eq!(span.len(), 2.0);
    ///
    /// let span = Span { start: 1.0, end: 1.5 };
    /// assert_eq!(span.len(), 0.5);
    /// ```
    #[inline]
    pub fn len(&self) -> f32 {
        (self.end - self.start).abs()
    }

    /// Get the  midpoint of the span.
    ///
    /// # Examples
    ///
    /// ```
    /// use api::Span;
    ///
    /// let span = Span { start: 1.0, end: 3.0 };
    /// assert_eq!(span.mid(), 2.0);
    ///
    /// let span = Span { start: 3.0, end: 1.0 };
    /// assert_eq!(span.mid(), 2.0);
    ///
    /// let span = Span { start: 1.0, end: 1.5 };
    /// assert_eq!(span.mid(), 1.25);
    /// ```
    #[inline]
    pub fn mid(&self) -> f32 {
        (self.start + self.end) / 2.0
    }

    /// Returns `true` if `value` lies within `[start, end]`.
    ///
    /// # Examples
    ///
    /// ```
    /// use api::Span;
    ///
    /// let span = Span { start: 1.0, end: 3.0 };
    ///
    /// assert!(span.contains(2.0));
    /// assert!(!span.contains(0.5));
    /// ```
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
    pub z: Span,
}

impl Extent {
    /// Returns `true` if the point `(x, z)` lies within the extent.
    #[inline]
    pub fn contains(self, x: f32, z: f32) -> bool {
        self.x.contains(x) && self.z.contains(z)
    }

    /// A zero extent at the origin.
    pub const fn arena() -> Self {
        Self {
            x: Span {
                start: -5.0,
                end: 5.0,
            },
            z: Span {
                start: -5.0,
                end: 5.0,
            },
        }
    }
}

/// Represents a 2 dimensional pixel position in canvas space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Canvas2 {
    pub x: f64,
    pub y: f64,
}

impl Canvas2 {
    #[inline]
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }

    #[inline]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0)
    }
}

impl Add for Canvas2 {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self::new(self.x + rhs.x, self.y + rhs.y)
    }
}

impl Sub for Canvas2 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self::new(self.x - rhs.x, self.y - rhs.y)
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
    pub id: PeerId,
    pub public_key: PublicKey,
    pub props: Properties,
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

#[derive(Debug, Encode, Decode)]
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
    pub peer_objects: Vec<(PeerId, RemoteObject)>,
}

#[derive(Debug, Encode, Decode)]
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
    /// List of objects associated with each peer.
    pub peer_objects: Vec<(PeerId, RemoteObject)>,
}

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct GetConfigRequest;

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct Image {
    /// The unique identifier of the image.
    pub id: RemoteId,
    /// The content type of the image.
    pub content_type: ContentType,
    /// The role of the image.
    pub role: Role,
    /// The width of the image in pixels.
    pub width: u32,
    /// The height of the image in pixels.
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct ImageWithData {
    /// The unique identifier of the image.
    pub id: RemoteId,
    /// The content type of the image.
    pub content_type: ContentType,
    /// The role of the image.
    pub role: Role,
    /// The width of the image in pixels.
    pub width: u32,
    /// The height of the image in pixels.
    pub height: u32,
    /// The raw bytes of the image file.
    pub bytes: Vec<u8>,
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
    /// The public key associated with the local peer.
    pub public_key: PublicKey,
}

/// Request to fetch images..
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeImageUploadRequest;

/// Response when fetching images.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct InitializeImageUploadResponse {
    /// The images currently stored in the database.
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

#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct CreateObjectResponse {
    /// The remote object that was just created.
    pub object: RemoteObject,
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
pub struct RemoveImageRequest {
    pub id: Id,
}

/// Request to update config.
#[derive(Debug, Encode, Decode)]
#[musli(crate = musli_core)]
pub struct UpdatesRequest {
    pub values: Vec<(Key, Value)>,
}

/// Request to update config.
#[derive(Debug, Encode)]
#[musli(crate = musli_core)]
pub struct UpdatesRequestRef<'a> {
    pub values: &'a [(Key, Value)],
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
    Config {
        channel: ChannelId,
        key: Key,
        value: Value,
    },
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
        /// The channel from which this update originated.
        channel: ChannelId,
        id: RemoteId,
        key: Key,
        value: Value,
    },
    ObjectCreated {
        channel: ChannelId,
        id: RemoteId,
        object: RemoteObject,
    },
    ObjectRemoved {
        channel: ChannelId,
        id: RemoteId,
    },
    ImageCreated {
        image: Image,
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
        impl Request for ObjectUpdateBodyRef<'_>;
        type Response<'de> = Empty;
    }

    pub type UploadImage;

    impl Endpoint for UploadImage {
        impl Request for UploadImageRequest;
        impl Request for UploadImageRequestRef<'_>;
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

    pub type InitializeImageUpload;

    impl Endpoint for InitializeImageUpload {
        impl Request for InitializeImageUploadRequest;
        type Response<'de> = InitializeImageUploadResponse;
    }

    pub type CreateObject;

    impl Endpoint for CreateObject {
        impl Request for CreateObjectRequest;
        type Response<'de> = CreateObjectResponse;
    }

    pub type RemoveObject;

    impl Endpoint for RemoveObject {
        impl Request for RemoveObjectRequest;
        type Response<'de> = Empty;
    }

    pub type RemoveImage;

    impl Endpoint for RemoveImage {
        impl Request for RemoveImageRequest;
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
        impl Request for UpdatesRequestRef<'_>;
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
