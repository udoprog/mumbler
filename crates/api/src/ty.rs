#[cfg(feature = "sqll")]
use core::ffi::c_int;
use core::fmt;

use musli_core::{Decode, Encode};
#[cfg(feature = "sqll")]
use sqll::{BIND_INDEX, Bind, BindValue, FromColumn, Statement, ty};

/// A base64-encoded u64, used for identifiers in the API.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Type {
    raw: u64,
}

impl Type {
    /// The token type.
    pub const TOKEN: Self = Self::new(0x1000);
    /// The static object type (furniture, props, etc.).
    pub const STATIC: Self = Self::new(0x1001);

    #[inline]
    const fn new(raw: u64) -> Self {
        Self { raw }
    }
}

impl fmt::Display for Type {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TOKEN => write!(f, "AVATAR"),
            Self::STATIC => write!(f, "STATIC"),
            _ => write!(f, "UNKNOWN({:08x})", self.raw),
        }
    }
}

impl fmt::Debug for Type {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::TOKEN => write!(f, "AVATAR"),
            Self::STATIC => write!(f, "STATIC"),
            _ => write!(f, "UNKNOWN({:08x})", self.raw),
        }
    }
}

#[cfg(feature = "sqll")]
impl BindValue for Type {
    #[inline]
    fn bind_value(&self, stmt: &mut Statement, index: c_int) -> Result<(), sqll::Error> {
        self.raw.cast_signed().bind_value(stmt, index)
    }
}

#[cfg(feature = "sqll")]
impl Bind for Type {
    #[inline]
    fn bind(&self, stmt: &mut Statement) -> Result<(), sqll::Error> {
        self.bind_value(stmt, BIND_INDEX)
    }
}

#[cfg(feature = "sqll")]
impl FromColumn<'_> for Type {
    type Type = ty::Integer;

    #[inline]
    fn from_column(stmt: &Statement, index: ty::Integer) -> Result<Self, sqll::Error> {
        let id = i64::from_column(stmt, index)?.cast_unsigned();
        Ok(Type::new(id))
    }
}
