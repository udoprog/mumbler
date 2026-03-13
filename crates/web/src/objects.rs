use core::cell::{Cell, Ref, RefCell, RefMut};
use core::ops::{Deref, DerefMut};

use std::collections::HashMap;
use std::rc::Rc;

use api::{
    Color, Id, Key, PeerId, RemoteObject, RemotePeerObject, Transform, Type, Value, Vec3, VecXZ,
};

use crate::state::State;

const DEFAULT_SPEED: f32 = 5.0;
const DEFAULT_STATIC_WIDTH: f32 = 1.0;
const DEFAULT_STATIC_HEIGHT: f32 = 1.0;
const DEFAULT_TOKEN_RADIUS: f32 = 0.25;

pub(crate) struct PeerObject {
    pub(crate) peer_id: PeerId,
    pub(crate) data: ObjectData,
}

impl PeerObject {
    #[inline]
    pub(crate) fn from_peer(remote: &RemotePeerObject) -> Self {
        Self {
            peer_id: remote.peer_id,
            data: ObjectData::from_remote(&remote.object),
        }
    }
}

impl Deref for PeerObject {
    type Target = ObjectData;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for PeerObject {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

pub(crate) struct LocalObject {
    pub(crate) data: ObjectData,
    pub(crate) move_target: Option<VecXZ>,
    pub(crate) arrow_target: Option<VecXZ>,
}

impl LocalObject {
    pub(crate) fn from_remote(remote: &RemoteObject) -> Self {
        Self {
            data: ObjectData::from_remote(remote),
            move_target: None,
            arrow_target: None,
        }
    }
}

impl Deref for LocalObject {
    type Target = ObjectData;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.data
    }
}

impl DerefMut for LocalObject {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.data
    }
}

#[derive(Default)]
struct Inner {
    mutable: RefCell<ObjectsRef>,
    version: Cell<u64>,
}

#[derive(Default)]
pub(crate) struct Objects {
    inner: Rc<Inner>,
    version: u64,
}

impl PartialEq for Objects {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.inner, &other.inner) && self.version == other.inner.version.get()
    }
}

impl Clone for Objects {
    #[inline]
    fn clone(&self) -> Self {
        Self {
            inner: Rc::clone(&self.inner),
            version: self.inner.version.get(),
        }
    }
}

impl Objects {
    #[inline]
    pub(crate) fn borrow(&self) -> Ref<'_, ObjectsRef> {
        self.inner.mutable.borrow()
    }

    #[inline]
    pub(crate) fn borrow_mut(&self) -> RefMut<'_, ObjectsRef> {
        self.inner
            .version
            .set(self.inner.version.get().wrapping_add(1));
        self.inner.mutable.borrow_mut()
    }
}

#[derive(Default)]
pub(crate) struct ObjectsRef {
    values: HashMap<Id, LocalObject>,
}

impl ObjectsRef {
    #[inline]
    pub(crate) fn get(&self, id: Id) -> Option<&LocalObject> {
        self.values.get(&id)
    }

    #[inline]
    pub(crate) fn values(&self) -> impl Iterator<Item = &LocalObject> {
        self.values.values()
    }

    #[inline]
    pub(crate) fn is_interactive(&self, id: Id) -> bool {
        let Some(object) = self.values.get(&id) else {
            return false;
        };

        object.data.is_interactive()
    }

    #[inline]
    pub(crate) fn remove(&mut self, id: Id) -> Option<LocalObject> {
        self.values.remove(&id)
    }

    #[inline]
    pub(crate) fn insert(&mut self, id: Id, object: LocalObject) -> Option<LocalObject> {
        self.values.insert(id, object)
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: Id) -> Option<&mut LocalObject> {
        self.values.get_mut(&id)
    }

    #[inline]
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut LocalObject> {
        self.values.values_mut()
    }

    /// Test if the given group or any of its ancestors is hidden.
    #[inline]
    pub(crate) fn is_group_hidden(&self, group: Id) -> bool {
        let mut current = group;

        while current != Id::ZERO {
            let Some(object) = self.values.get(&current) else {
                break;
            };

            if object.data.is_hidden() {
                return true;
            }

            current = *object.data.group;
        }

        false
    }
}

impl FromIterator<LocalObject> for Objects {
    #[inline]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = LocalObject>,
    {
        let mutable = ObjectsRef {
            values: iter.into_iter().map(|o| (o.data.id, o)).collect(),
        };

        let inner = Inner {
            mutable: RefCell::new(mutable),
            version: Cell::new(0),
        };

        Self {
            inner: Rc::new(inner),
            version: 0,
        }
    }
}

pub(crate) struct TokenObject {
    pub(crate) transform: State<Transform>,
    pub(crate) locked: State<bool>,
    pub(crate) look_at: State<Option<Vec3>>,
    pub(crate) image: State<Option<Id>>,
    pub(crate) color: State<Option<Color>>,
    pub(crate) name: State<Option<String>>,
    pub(crate) hidden: State<bool>,
    pub(crate) token_radius: State<f32>,
    pub(crate) speed: State<f32>,
    pub(crate) sort: State<Vec<u8>>,
}

impl TokenObject {
    pub(crate) fn from_remote(o: &RemoteObject) -> Self {
        Self {
            transform: State::new(
                o.props
                    .get(Key::TRANSFORM)
                    .as_transform()
                    .unwrap_or_else(Transform::origin),
            ),
            locked: State::new(o.props.get(Key::LOCKED).as_bool().unwrap_or(false)),
            look_at: State::new(o.props.get(Key::LOOK_AT).as_vec3()),
            image: State::new(o.props.get(Key::IMAGE_ID).as_id()),
            color: State::new(o.props.get(Key::COLOR).as_color()),
            name: State::new(o.props.get(Key::NAME).as_str().map(str::to_owned)),
            hidden: State::new(o.props.get(Key::HIDDEN).as_bool().unwrap_or(false)),
            token_radius: State::new(
                o.props
                    .get(Key::TOKEN_RADIUS)
                    .as_f32()
                    .unwrap_or(DEFAULT_TOKEN_RADIUS),
            ),
            speed: State::new(o.props.get(Key::SPEED).as_f32().unwrap_or(DEFAULT_SPEED)),
            sort: State::new(
                o.props
                    .get(Key::SORT)
                    .as_bytes()
                    .unwrap_or_default()
                    .to_vec(),
            ),
        }
    }

    pub(crate) fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::TRANSFORM => self
                .transform
                .update(value.as_transform().unwrap_or_else(Transform::origin)),
            Key::LOCKED => self.locked.update(value.as_bool().unwrap_or(false)),
            Key::LOOK_AT => self.look_at.update(value.as_vec3()),
            Key::IMAGE_ID => self.image.update(value.as_id()),
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.into_string()),
            Key::HIDDEN => self.hidden.update(value.as_bool().unwrap_or(false)),
            Key::TOKEN_RADIUS => self
                .token_radius
                .update(value.as_f32().unwrap_or(DEFAULT_TOKEN_RADIUS)),
            Key::SPEED => self.speed.update(value.as_f32().unwrap_or(DEFAULT_SPEED)),
            Key::SORT => self
                .sort
                .update(value.as_bytes().unwrap_or_default().to_vec()),
            _ => false,
        }
    }
}

pub(crate) struct StaticObject {
    pub(crate) transform: State<Transform>,
    pub(crate) locked: State<bool>,
    pub(crate) image: State<Option<Id>>,
    pub(crate) color: State<Option<Color>>,
    pub(crate) name: State<Option<String>>,
    pub(crate) hidden: State<bool>,
    pub(crate) width: State<f32>,
    pub(crate) height: State<f32>,
    pub(crate) sort: State<Vec<u8>>,
}

impl StaticObject {
    pub(crate) fn from_remote(o: &RemoteObject) -> Self {
        Self {
            transform: State::new(
                o.props
                    .get(Key::TRANSFORM)
                    .as_transform()
                    .unwrap_or_else(Transform::origin),
            ),
            locked: State::new(o.props.get(Key::LOCKED).as_bool().unwrap_or(false)),
            image: State::new(o.props.get(Key::IMAGE_ID).as_id()),
            color: State::new(o.props.get(Key::COLOR).as_color()),
            name: State::new(o.props.get(Key::NAME).as_str().map(str::to_owned)),
            hidden: State::new(o.props.get(Key::HIDDEN).as_bool().unwrap_or(false)),
            width: State::new(
                o.props
                    .get(Key::STATIC_WIDTH)
                    .as_f32()
                    .unwrap_or(DEFAULT_STATIC_WIDTH),
            ),
            height: State::new(
                o.props
                    .get(Key::STATIC_HEIGHT)
                    .as_f32()
                    .unwrap_or(DEFAULT_STATIC_HEIGHT),
            ),
            sort: State::new(
                o.props
                    .get(Key::SORT)
                    .as_bytes()
                    .unwrap_or_default()
                    .to_vec(),
            ),
        }
    }

    pub(crate) fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::TRANSFORM => self
                .transform
                .update(value.as_transform().unwrap_or_else(Transform::origin)),
            Key::LOCKED => self.locked.update(value.as_bool().unwrap_or(false)),
            Key::IMAGE_ID => self.image.update(value.as_id()),
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.into_string()),
            Key::HIDDEN => self.hidden.update(value.as_bool().unwrap_or(false)),
            Key::STATIC_WIDTH => self
                .width
                .update(value.as_f32().unwrap_or(DEFAULT_STATIC_WIDTH)),
            Key::STATIC_HEIGHT => self
                .height
                .update(value.as_f32().unwrap_or(DEFAULT_STATIC_HEIGHT)),
            Key::SORT => self
                .sort
                .update(value.as_bytes().unwrap_or_default().to_vec()),
            _ => false,
        }
    }
}

pub(crate) struct GroupObject {
    pub(crate) locked: State<bool>,
    pub(crate) name: State<Option<String>>,
    pub(crate) hidden: State<bool>,
    pub(crate) sort: State<Vec<u8>>,
}

impl GroupObject {
    pub(crate) fn from_remote(o: &RemoteObject) -> Self {
        Self {
            locked: State::new(o.props.get(Key::LOCKED).as_bool().unwrap_or(false)),
            name: State::new(o.props.get(Key::NAME).as_str().map(str::to_owned)),
            hidden: State::new(o.props.get(Key::HIDDEN).as_bool().unwrap_or(false)),
            sort: State::new(
                o.props
                    .get(Key::SORT)
                    .as_bytes()
                    .unwrap_or_default()
                    .to_vec(),
            ),
        }
    }

    pub(crate) fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::LOCKED => self.locked.update(value.as_bool().unwrap_or(false)),
            Key::NAME => self.name.update(value.into_string()),
            Key::HIDDEN => self.hidden.update(value.as_bool().unwrap_or(false)),
            Key::SORT => self
                .sort
                .update(value.as_bytes().unwrap_or_default().to_vec()),
            _ => false,
        }
    }
}

pub(crate) enum ObjectKind {
    Token(TokenObject),
    Static(StaticObject),
    Group(GroupObject),
    Unknown,
}

pub(crate) struct ObjectData {
    pub(crate) id: Id,
    pub(crate) group: State<Id>,
    pub(crate) kind: ObjectKind,
}

impl ObjectData {
    #[inline]
    pub(crate) fn from_remote(o: &RemoteObject) -> Self {
        let kind = match o.ty {
            Type::TOKEN => ObjectKind::Token(TokenObject::from_remote(o)),
            Type::STATIC => ObjectKind::Static(StaticObject::from_remote(o)),
            Type::GROUP => ObjectKind::Group(GroupObject::from_remote(o)),
            _ => ObjectKind::Unknown,
        };

        let group = o.props.get(Key::GROUP).as_id().unwrap_or(Id::ZERO);
        let group = State::new(group);

        Self {
            id: o.id,
            group,
            kind,
        }
    }

    #[inline]
    pub(crate) fn update(&mut self, key: Key, value: Value) -> bool {
        match &mut self.kind {
            ObjectKind::Token(this) => this.update(key, value),
            ObjectKind::Static(this) => this.update(key, value),
            ObjectKind::Group(this) => this.update(key, value),
            ObjectKind::Unknown => false,
        }
    }

    #[inline]
    pub(crate) fn is_interactive(&self) -> bool {
        matches!(self.kind, ObjectKind::Token(_) | ObjectKind::Static(_))
    }

    #[inline]
    pub(crate) fn sort_mut(&mut self) -> Option<(&mut State<Id>, &mut State<Vec<u8>>)> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some((&mut self.group, &mut this.sort)),
            ObjectKind::Static(this) => Some((&mut self.group, &mut this.sort)),
            ObjectKind::Group(this) => Some((&mut self.group, &mut this.sort)),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    pub(crate) fn sort(&self) -> &[u8] {
        match &self.kind {
            ObjectKind::Token(this) => &this.sort,
            ObjectKind::Static(this) => &this.sort,
            ObjectKind::Group(this) => &this.sort,
            ObjectKind::Unknown => &[],
        }
    }

    #[inline]
    pub(crate) fn as_transform(&self) -> Option<&Transform> {
        match &self.kind {
            ObjectKind::Token(this) => Some(&this.transform),
            ObjectKind::Static(this) => Some(&this.transform),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn as_transform_mut(&mut self) -> Option<&mut State<Transform>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.transform),
            ObjectKind::Static(this) => Some(&mut this.transform),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn as_interpolate_mut(
        &mut self,
    ) -> Option<(&mut State<Transform>, Option<&Vec3>, Option<f32>)> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some((
                &mut this.transform,
                this.look_at.as_ref(),
                Some(*this.speed),
            )),
            ObjectKind::Static(this) => Some((&mut this.transform, None, None)),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn as_click_geometry(&self) -> Option<(&Transform, f32)> {
        match &self.kind {
            ObjectKind::Token(this) => Some((&this.transform, *this.token_radius)),
            ObjectKind::Static(this) => {
                Some((&this.transform, (*this.width).hypot(*this.height) / 2.0))
            }
            _ => None,
        }
    }

    /// Returns `true` if this is a static object (rectangle, snap movement).
    #[inline]
    pub(crate) fn is_static(&self) -> bool {
        matches!(&self.kind, ObjectKind::Static(_))
    }

    #[inline]
    pub(crate) fn look_at(&self) -> Option<&Vec3> {
        match &self.kind {
            ObjectKind::Token(this) => this.look_at.as_ref(),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn as_look_at_mut(&mut self) -> Option<&mut State<Option<Vec3>>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.look_at),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn as_hidden_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.hidden),
            ObjectKind::Static(this) => Some(&mut this.hidden),
            ObjectKind::Group(this) => Some(&mut this.hidden),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    pub(crate) fn is_hidden(&self) -> bool {
        match &self.kind {
            ObjectKind::Token(this) => *this.hidden,
            ObjectKind::Static(this) => *this.hidden,
            ObjectKind::Group(this) => *this.hidden,
            ObjectKind::Unknown => false,
        }
    }

    #[inline]
    pub(crate) fn name(&self) -> Option<&str> {
        match &self.kind {
            ObjectKind::Token(this) => this.name.as_deref(),
            ObjectKind::Static(this) => this.name.as_deref(),
            ObjectKind::Group(this) => this.name.as_deref(),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    pub(crate) fn as_locked_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.locked),
            ObjectKind::Static(this) => Some(&mut this.locked),
            ObjectKind::Group(this) => Some(&mut this.locked),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    pub(crate) fn is_locked(&self) -> bool {
        match &self.kind {
            ObjectKind::Token(this) => *this.locked,
            ObjectKind::Static(this) => *this.locked,
            ObjectKind::Group(this) => *this.locked,
            ObjectKind::Unknown => false,
        }
    }
}
