#[cfg(feature = "sqll")]
use core::ffi::c_int;
use core::fmt;

use musli_core::{Decode, Encode};
#[cfg(feature = "sqll")]
use sqll::{BIND_INDEX, Bind, BindValue, FromColumn, Statement, ty};

#[derive(Clone, Copy, Encode, Decode)]
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
    ($($name:ident = $value:expr),* $(,)?) => {
        impl Key {
            $(
                pub const $name: Self = Self::new($value);
            )*
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
    AVATAR_IMAGE = 0,
    AVATAR_COLOR = 1,
    AVATAR_TRANSFORM = 2,
    AVATAR_LOOK_AT = 3,
    AVATAR_NAME = 4,
    MUMBLE_ENABLED = 5,
    REMOTE_SERVER = 6,
    REMOTE_ENABLED = 7,
    WORLD_SCALE = 8,
    WORLD_ZOOM = 9,
    WORLD_PAN = 10,
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
