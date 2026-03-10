#[cfg(feature = "sqll")]
use core::ffi::c_int;
use core::fmt;

use musli_core::{Decode, Encode};
#[cfg(feature = "sqll")]
use sqll::{BIND_INDEX, Bind, BindValue, FromColumn, Statement, ty};

use crate::ValueType;

#[derive(Clone, Copy, Encode, Decode, PartialEq, Eq, Hash)]
#[musli(crate = musli_core, transparent)]
pub struct Key {
    raw: u32,
}

impl Key {
    const fn new(raw: u32) -> Self {
        Self { raw }
    }
}

macro_rules! keys {
    ($(
        $(#[doc = $doc:literal])*
        $name:ident: $ty:ident = $value:expr
    ),* $(,)?) => {
        impl Key {
            $(
                $(#[doc = $doc])*
                pub const $name: Self = Self::new($value);
            )*

            /// The value type of a key.
            pub fn ty(&self) -> Option<ValueType> {
                match self.raw {
                    $($value => Some(ValueType::$ty)),*,
                    _ => None,
                }
            }
        }

        impl fmt::Display for Key {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.raw {
                    $($value => write!(f, stringify!($name))),*,
                    _ => write!(f, "UNKNOWN({})", self.raw),
                }
            }
        }

        impl fmt::Debug for Key {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                match self.raw {
                    $($value => write!(f, stringify!($name))),*,
                    _ => write!(f, "UNKNOWN({})", self.raw),
                }
            }
        }
    };
}

keys! {
    IMAGE_ID: Id = 0,
    COLOR: Color = 1,
    TRANSFORM: Transform = 2,
    LOOK_AT: Vec3 = 3,
    NAME: String = 4,
    /// Manual list order for an object. When defined, determines sorting.
    ORDER: Integer = 21,
    MUMBLE_ENABLED: Boolean = 5,
    REMOTE_SERVER: String = 6,
    REMOTE_ENABLED: Boolean = 7,
    WORLD_SCALE: Float = 8,
    REMOTE_TLS: Boolean = 11,
    WORLD_ZOOM: Float = 9,
    WORLD_PAN: Pan = 10,
    WORLD_EXTENT: Extent = 12,
    /// The object which is used for mumble link.
    MUMBLE_OBJECT: Id = 14,
    /// Whether the object is hidden from remote peers.
    HIDDEN: Boolean = 15,
    /// Whether selecting an object automatically sets it as the MumbleLink
    /// source.
    MUMBLE_FOLLOW_SELECTION: Boolean = 16,
    /// Per-object token radius.
    TOKEN_RADIUS: Float = 17,
    /// Per-object movement speed.
    SPEED: Float = 18,
    /// Width of a static object in world units.
    STATIC_WIDTH: Float = 19,
    /// Height of a static object in world units.
    STATIC_HEIGHT: Float = 20,
    /// Whether to maintain a fixed aspect ratio when resizing a static object.
    RATIO: Float = 23,
    /// An object is locked from further interaction. This prevents clicking on
    /// it in the map.
    LOCKED: Boolean = 22,
    /// Image bytes associated with an object.
    IMAGE_BYTES: Bytes = 0x1000,
}

#[cfg(feature = "sqll")]
impl BindValue for Key {
    #[inline]
    fn bind_value(&self, stmt: &mut Statement, index: c_int) -> Result<(), sqll::Error> {
        self.raw.bind_value(stmt, index)
    }
}

#[cfg(feature = "sqll")]
impl Bind for Key {
    #[inline]
    fn bind(&self, stmt: &mut Statement) -> Result<(), sqll::Error> {
        self.bind_value(stmt, BIND_INDEX)
    }
}

#[cfg(feature = "sqll")]
impl FromColumn<'_> for Key {
    type Type = ty::Integer;

    #[inline]
    fn from_column(stmt: &Statement, index: ty::Integer) -> Result<Self, sqll::Error> {
        let id = u32::from_column(stmt, index)?;
        Ok(Key::new(id))
    }
}
