use core::fmt;

use musli_core::{Decode, Encode};

use crate::{Canvas2, Color, Extent, Id, PeerId, StableId, Transform, Vec3};

#[derive(Debug, Clone, Copy, Encode, Decode, PartialEq, Eq, Hash)]
#[musli(crate = musli_core)]
pub enum ValueType {
    Boolean,
    Bytes,
    Color,
    Extent,
    Float,
    Id,
    Integer,
    Canvas2,
    PeerId,
    StableId,
    String,
    Transform,
    Vec3,
}

#[derive(Clone, PartialEq, Encode, Decode)]
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
    pub const fn empty() -> Self {
        Self {
            kind: ValueKind::Empty,
        }
    }

    /// Check if the value is empty.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        matches!(self.kind, ValueKind::Empty)
    }

    #[inline]
    pub fn as_id(&self) -> Id {
        match &self.kind {
            ValueKind::Id(id) => *id,
            _ => Id::ZERO,
        }
    }

    #[inline]
    pub fn as_peer_id(&self) -> &PeerId {
        match &self.kind {
            ValueKind::PeerId(peer_id) => peer_id,
            _ => &PeerId::ZERO,
        }
    }

    #[inline]
    pub fn as_stable_id(&self) -> &StableId {
        match &self.kind {
            ValueKind::StableId(stable_id) => stable_id,
            _ => &StableId::ZERO,
        }
    }

    #[inline]
    pub fn as_bool(&self) -> bool {
        match &self.kind {
            ValueKind::Boolean(b) => *b,
            _ => false,
        }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        match &self.kind {
            ValueKind::String(s) => s,
            _ => "",
        }
    }

    #[inline]
    pub fn as_f32(&self) -> Option<f32> {
        match &self.kind {
            ValueKind::Float(f) => Some(*f as f32),
            _ => None,
        }
    }

    #[inline]
    pub fn as_f64(&self) -> Option<f64> {
        match &self.kind {
            ValueKind::Float(f) => Some(*f),
            _ => None,
        }
    }

    #[inline]
    pub fn as_u32(&self) -> Option<u32> {
        match &self.kind {
            ValueKind::Integer(i) => u32::try_from(*i).ok(),
            _ => None,
        }
    }

    #[inline]
    pub fn as_i64(&self) -> Option<i64> {
        match &self.kind {
            ValueKind::Integer(i) => Some(*i),
            _ => None,
        }
    }

    #[inline]
    pub fn as_canvas2(&self) -> Option<Canvas2> {
        match &self.kind {
            ValueKind::Canvas2(pan) => Some(*pan),
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
    pub fn into_string(self) -> String {
        match self.kind {
            ValueKind::String(s) => s,
            _ => String::new(),
        }
    }

    #[inline]
    pub fn as_bytes(&self) -> &[u8] {
        match &self.kind {
            ValueKind::Bytes(b) => b,
            _ => &[],
        }
    }

    #[inline]
    pub fn into_bytes(self) -> Vec<u8> {
        match self.kind {
            ValueKind::Bytes(b) => b,
            _ => Vec::new(),
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
        match &self.kind {
            ValueKind::Boolean(value) => value.fmt(f),
            ValueKind::Bytes(value) => value.fmt(f),
            ValueKind::Color(value) => value.fmt(f),
            ValueKind::Empty => f.write_str("Empty"),
            ValueKind::Extent(value) => value.fmt(f),
            ValueKind::Float(value) => value.fmt(f),
            ValueKind::Id(value) => value.fmt(f),
            ValueKind::Integer(value) => value.fmt(f),
            ValueKind::Canvas2(value) => value.fmt(f),
            ValueKind::PeerId(value) => value.fmt(f),
            ValueKind::StableId(value) => value.fmt(f),
            ValueKind::String(value) => value.fmt(f),
            ValueKind::Transform(value) => value.fmt(f),
            ValueKind::Vec3(value) => value.fmt(f),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Encode, Decode)]
#[musli(crate = musli_core)]
pub enum ValueKind {
    Boolean(bool),
    Bytes(Vec<u8>),
    Color(Color),
    Empty,
    Extent(Extent),
    Float(f64),
    Id(Id),
    Integer(i64),
    Canvas2(Canvas2),
    PeerId(PeerId),
    StableId(StableId),
    String(String),
    Transform(Transform),
    Vec3(Vec3),
}

impl From<Id> for Value {
    #[inline]
    fn from(value: Id) -> Self {
        Self {
            kind: if value.is_zero() {
                ValueKind::Empty
            } else {
                ValueKind::Id(value)
            },
        }
    }
}

impl From<f32> for Value {
    #[inline]
    fn from(value: f32) -> Self {
        Self {
            kind: ValueKind::Float(value as f64),
        }
    }
}

impl From<f64> for Value {
    #[inline]
    fn from(value: f64) -> Self {
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

impl From<&str> for Value {
    #[inline]
    fn from(value: &str) -> Self {
        Self {
            kind: ValueKind::String(value.to_string()),
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

impl From<&[u8]> for Value {
    #[inline]
    fn from(value: &[u8]) -> Self {
        Self::from(value.to_vec())
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

impl From<Canvas2> for Value {
    #[inline]
    fn from(value: Canvas2) -> Self {
        Self {
            kind: ValueKind::Canvas2(value),
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

impl From<StableId> for Value {
    #[inline]
    fn from(value: StableId) -> Self {
        Self {
            kind: ValueKind::StableId(value),
        }
    }
}

impl From<PeerId> for Value {
    #[inline]
    fn from(value: PeerId) -> Self {
        Self {
            kind: ValueKind::PeerId(value),
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

impl From<i64> for Value {
    #[inline]
    fn from(value: i64) -> Self {
        Self {
            kind: ValueKind::Integer(value),
        }
    }
}

impl From<i32> for Value {
    #[inline]
    fn from(value: i32) -> Self {
        Self {
            kind: ValueKind::Integer(value as i64),
        }
    }
}
