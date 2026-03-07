use core::fmt;

use musli_core::{Decode, Encode};

use crate::{Color, Id, Transform, Vec3};

#[derive(Clone, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Value {
    kind: ValueKind,
}

impl Value {
    #[inline]
    pub fn as_id(&self) -> Option<Id> {
        match &self.kind {
            ValueKind::Id(id) => Some(*id),
            _ => None,
        }
    }

    #[inline]
    pub fn as_string(&self) -> Option<&str> {
        match &self.kind {
            ValueKind::String(s) => Some(s),
            _ => None,
        }
    }

    #[inline]
    pub fn into_string(self) -> Option<String> {
        match self.kind {
            ValueKind::String(s) => Some(s),
            _ => None,
        }
    }

    #[inline]
    pub fn as_bytes(&self) -> Option<&[u8]> {
        match &self.kind {
            ValueKind::Bytes(b) => Some(b),
            _ => None,
        }
    }

    #[inline]
    pub fn into_bytes(self) -> Option<Vec<u8>> {
        match self.kind {
            ValueKind::Bytes(b) => Some(b),
            _ => None,
        }
    }

    #[inline]
    pub fn as_transform(&self) -> Option<Transform> {
        match &self.kind {
            ValueKind::Transform(transform) => Some(*transform),
            _ => None,
        }
    }

    #[inline]
    pub fn as_color(&self) -> Option<Color> {
        match &self.kind {
            ValueKind::Color(color) => Some(*color),
            _ => None,
        }
    }

    #[inline]
    pub fn as_vec3(&self) -> Option<Vec3> {
        match &self.kind {
            ValueKind::Vec3(vec) => Some(*vec),
            _ => None,
        }
    }
}

impl Default for Value {
    #[inline]
    fn default() -> Self {
        Self {
            kind: ValueKind::None,
        }
    }
}

impl fmt::Debug for Value {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

#[derive(Debug, Clone, Encode, Decode)]
#[musli(crate = musli_core)]
enum ValueKind {
    Id(Id),
    String(String),
    Bytes(Vec<u8>),
    Transform(Transform),
    Color(Color),
    Vec3(Vec3),
    /// Empty value.
    None,
}

impl From<Id> for Value {
    #[inline]
    fn from(value: Id) -> Self {
        Self {
            kind: ValueKind::Id(value),
        }
    }
}

impl From<String> for Value {
    #[inline]
    fn from(value: String) -> Self {
        Self {
            kind: ValueKind::String(value),
        }
    }
}

impl From<Vec<u8>> for Value {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Self {
            kind: ValueKind::Bytes(value),
        }
    }
}

impl From<Transform> for Value {
    #[inline]
    fn from(value: Transform) -> Self {
        Self {
            kind: ValueKind::Transform(value),
        }
    }
}

impl From<Color> for Value {
    #[inline]
    fn from(value: Color) -> Self {
        Self {
            kind: ValueKind::Color(value),
        }
    }
}

impl From<Vec3> for Value {
    #[inline]
    fn from(value: Vec3) -> Self {
        Self {
            kind: ValueKind::Vec3(value),
        }
    }
}

impl<T> From<Option<T>> for Value
where
    Value: From<T>,
{
    #[inline]
    fn from(value: Option<T>) -> Self {
        match value {
            Some(value) => Self::from(value),
            None => Self {
                kind: ValueKind::None,
            },
        }
    }
}
