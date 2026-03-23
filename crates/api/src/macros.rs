macro_rules! __ids {
    (
        $(#[doc = $doc:literal])*
        $vis:vis struct $ty:ident {
            $(
                $(#[doc = $field_doc:literal])*
                $name:ident = $value:literal;
            )*
        }
    ) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, ::musli_core::Encode, ::musli_core::Decode)]
        #[musli(crate = ::musli_core, transparent)]
        $(#[doc = $doc])*
        $vis struct $ty {
            raw: u32,
        }

        impl $ty {
            const fn new(raw: u32) -> Self {
                Self { raw }
            }
        }


        impl $ty {
            $(
                $(#[doc = $field_doc])*
                $vis const $name: Self = Self::new($value);
            )*
        }

        impl ::core::fmt::Display for $ty {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self.raw {
                    $($value => write!(f, stringify!($name))),*,
                    _ => write!(f, "UNKNOWN({})", self.raw),
                }
            }
        }

        impl ::core::fmt::Debug for $ty {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                match self.raw {
                    $($value => write!(f, stringify!($name))),*,
                    _ => write!(f, "UNKNOWN({})", self.raw),
                }
            }
        }

        #[cfg(feature = "sqll")]
        impl ::sqll::BindValue for $ty {
            #[inline]
            fn bind_value(&self, stmt: &mut ::sqll::Statement, index: ::core::ffi::c_int) -> Result<(), ::sqll::Error> {
                ::sqll::BindValue::bind_value(&self.raw, stmt, index)
            }
        }

        #[cfg(feature = "sqll")]
        impl ::sqll::Bind for $ty {
            #[inline]
            fn bind(&self, stmt: &mut ::sqll::Statement) -> Result<(), ::sqll::Error> {
                ::sqll::BindValue::bind_value(self, stmt, ::sqll::BIND_INDEX)
            }
        }

        #[cfg(feature = "sqll")]
        impl ::sqll::FromColumn<'_> for $ty {
            type Type = ::sqll::ty::Integer;

            #[inline]
            fn from_column(stmt: &::sqll::Statement, index: ::sqll::ty::Integer) -> Result<Self, ::sqll::Error> {
                let id = u32::from_column(stmt, index)?;
                Ok($ty::new(id))
            }
        }
    };
}

macro_rules! __keys {
    (
        $vis:vis struct $ty:ident {
            $(
                $(#[doc = $doc:literal])*
                $const:ident: $const_type:ident = $value:literal;
            )* $(,)?
        }
    ) => {
        $crate::macros::ids! {
            $vis struct $ty {
                $(
                    $(#[doc = $doc])*
                    $const = $value;
                )*
            }
        }

        impl $ty {
            /// The value type of a key.
            $vis fn ty(&self) -> Option<crate::ValueType> {
                match self.raw {
                    $($value => Some(crate::ValueType::$const_type)),*,
                    _ => None,
                }
            }
        }
    };
}

pub(crate) use __ids as ids;
pub(crate) use __keys as keys;
