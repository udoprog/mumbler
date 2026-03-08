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
    ($($name:ident: $ty:ident = $value:expr),* $(,)?) => {
        impl Key {
            $(
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
    MUMBLE_ENABLED: Boolean = 5,
    REMOTE_SERVER: String = 6,
    REMOTE_ENABLED: Boolean = 7,
    WORLD_SCALE: Float = 8,
    REMOTE_TLS: String = 11,
    WORLD_ZOOM: Float = 9,
    WORLD_PAN: Pan = 10,
    WORLD_EXTENT: Extent = 12,
    WORLD_TOKEN_RADIUS: Float = 13,
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
