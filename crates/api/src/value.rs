use core::fmt;

use musli_core::{Decode, Encode};

use crate::{Color, Extent, Id, Pan, Transform, Vec3};

#[derive(Debug, Clone, Copy, Encode, Decode, PartialEq, Eq, Hash)]
#[musli(crate = musli_core)]
#[non_exhaustive]
pub enum ValueType {
    Boolean,
    String,
    Float,
    Id,
    Pan,
    Extent,
    Transform,
    Vec3,
    Color,
    Bytes,
}

#[derive(Clone, Encode, Decode)]
#[musli(crate = musli_core, transparent)]
pub struct Value {
    kind: ValueKind,
}

impl Value {
    #[inline]
    pub fn into_kind(self) -> ValueKind {
        self.kind
    }

    #[inline]
    pub fn empty() -> Self {
        Self {
            kind: ValueKind::Empty,
        }
    }

    #[inline]
    pub fn as_id(&self) -> Option<Id> {
        match &self.kind {
            ValueKind::Id(id) => Some(*id),
            _ => None,
        }
    }

    #[inline]
    pub fn as_bool(&self) -> Option<bool> {
        match &self.kind {
            ValueKind::Boolean(b) => Some(*b),
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
    pub fn as_float(&self) -> Option<f32> {
        match &self.kind {
            ValueKind::Float(f) => Some(*f),
            _ => None,
        }
    }

    #[inline]
    pub fn as_pan(&self) -> Option<Pan> {
        match &self.kind {
            ValueKind::Pan(pan) => Some(*pan),
            _ => None,
        }
    }

    #[inline]
    pub fn as_extent(&self) -> Option<Extent> {
        match &self.kind {
            ValueKind::Extent(extent) => Some(*extent),
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
    pub fn into_transform_mut(&mut self) -> &mut Transform {
        if !matches!(self.kind, ValueKind::Transform(_)) {
            self.kind = ValueKind::Transform(Transform::origin());
        }

        if let ValueKind::Transform(transform) = &mut self.kind {
            return transform;
        }

        unreachable!()
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

    #[inline]
    pub fn into_vec3_mut(&mut self) -> &mut Vec3 {
        if !matches!(self.kind, ValueKind::Vec3(_)) {
            self.kind = ValueKind::Vec3(Vec3::default());
        }

        if let ValueKind::Vec3(vec) = &mut self.kind {
            return vec;
        }

        unreachable!()
    }
}

impl Default for Value {
    #[inline]
    fn default() -> Self {
        Self {
            kind: ValueKind::Empty,
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
pub enum ValueKind {
    Id(Id),
    Float(f32),
    Boolean(bool),
    String(String),
    Bytes(Vec<u8>),
    Transform(Transform),
    Color(Color),
    Vec3(Vec3),
    Pan(Pan),
    Extent(Extent),
    Empty,
}

impl From<Id> for Value {
    #[inline]
    fn from(value: Id) -> Self {
        Self {
            kind: ValueKind::Id(value),
        }
    }
}

impl From<f32> for Value {
    #[inline]
    fn from(value: f32) -> Self {
        Self {
            kind: ValueKind::Float(value),
        }
    }
}

impl From<bool> for Value {
    #[inline]
    fn from(value: bool) -> Self {
        Self {
            kind: ValueKind::Boolean(value),
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

impl From<Pan> for Value {
    #[inline]
    fn from(value: Pan) -> Self {
        Self {
            kind: ValueKind::Pan(value),
        }
    }
}

impl From<Extent> for Value {
    #[inline]
    fn from(value: Extent) -> Self {
        Self {
            kind: ValueKind::Extent(value),
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
                kind: ValueKind::Empty,
            },
        }
    }
}
