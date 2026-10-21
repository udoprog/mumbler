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
        #[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, ::musli_core::Encode, ::musli_core::Decode)]
        #[musli(transparent)]
        $(#[doc = $doc])*
        #[cfg_attr(feature = "sqll", derive(::sqll::BindValue, ::sqll::FromColumn))]
        $vis struct $ty {
            raw: u32,
        }

        impl $ty {
            const fn new(raw: u32) -> Self {
                Self { raw }
            }
        }

        impl $ty {
            $vis const NONE: Self = Self::new(0);

            $(
                $(#[doc = $field_doc])*
                $vis const $name: Self = Self::new($value);
            )*

            $vis const ALL: &'static [$ty] = &[$($ty::$name),*];
        }

        impl ::core::default::Default for $ty {
            #[inline]
            fn default() -> Self {
                Self::NONE
            }
        }

        impl ::core::fmt::Display for $ty {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                #[allow(unreachable_patterns)]
                match self.raw {
                    $($value => write!(f, stringify!($name))),*,
                    0 => write!(f, "NONE"),
                    _ => write!(f, "UNKNOWN({})", self.raw),
                }
            }
        }

        impl ::core::fmt::Debug for $ty {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                #[allow(unreachable_patterns)]
                match self.raw {
                    $($value => write!(f, stringify!($name))),*,
                    0 => write!(f, "NONE"),
                    _ => write!(f, "UNKNOWN({})", self.raw),
                }
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
