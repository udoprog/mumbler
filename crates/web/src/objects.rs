use core::cell::{Cell, Ref, RefCell, RefMut};
use core::ops::{Deref, DerefMut};

use std::collections::HashMap;
use std::rc::Rc;

use api::{Color, Id, Key, PeerId, RemoteObject, Transform, Type, Value, Vec3};

use crate::components::render::Visibility;
use crate::state::State;

const DEFAULT_SPEED: f32 = 5.0;
const DEFAULT_STATIC_WIDTH: f32 = 1.0;
const DEFAULT_STATIC_HEIGHT: f32 = 1.0;
const DEFAULT_TOKEN_RADIUS: f32 = 0.25;

enum Shape {
    Circle { radius: f32 },
    Rectangle { width: f32, height: f32 },
}

pub(crate) struct Geometry<'a> {
    transform: &'a Transform,
    shape: Shape,
}

impl Geometry<'_> {
    pub(crate) fn intersects(&self, point: Vec3) -> bool {
        match self.shape {
            Shape::Circle { radius } => self.transform.position.dist(point) <= radius,
            Shape::Rectangle { width, height } => {
                // Do a fast path and check if point is within bounding circle first.
                let radius = (width * width + height * height).sqrt() / 2.0;

                if self.transform.position.dist(point) > radius {
                    return false;
                }

                // Transform point to object space.
                let local = self.transform.transform_point(point);

                // Check if point is within bounds.
                local.x.abs() < width / 2.0 && local.z.abs() < height / 2.0
            }
        }
    }
}

pub(crate) struct LocalObject {
    pub(crate) move_target: Option<Vec3>,
    pub(crate) arrow_target: Option<Vec3>,
    pub(crate) data: ObjectData,
}

impl LocalObject {
    pub(crate) fn from_remote(remote: &RemoteObject) -> Option<Self> {
        Some(Self {
            data: ObjectData::new(PeerId::ZERO, remote)?,
            move_target: None,
            arrow_target: None,
        })
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
    pub(crate) fn visibility(&self, group: Id) -> Visibility {
        let mut hidden = Visibility::Remote;
        let mut current = group;

        while current != Id::ZERO {
            let Some(object) = self.values.get(&current) else {
                break;
            };

            hidden = hidden.max(object.data.visibility());
            current = *object.data.group;
        }

        hidden
    }

    /// Test if the given group or any of its ancestors is locked.
    #[inline]
    pub(crate) fn is_locked(&self, group: Id) -> bool {
        let mut current = group;

        while current != Id::ZERO {
            let Some(object) = self.values.get(&current) else {
                break;
            };

            if object.data.is_locked() {
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
    pub(crate) image: State<Id>,
    pub(crate) color: State<Option<Color>>,
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
    pub(crate) image: State<Id>,
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
            name: State::new(o.props.get(Key::OBJECT_NAME).as_str().map(str::to_owned)),
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
            Key::OBJECT_NAME => self.name.update(value.into_string()),
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
    pub(crate) sort: State<Vec<u8>>,
    pub(crate) expanded: State<bool>,
}

impl GroupObject {
    pub(crate) fn from_remote(o: &RemoteObject) -> Self {
        Self {
            locked: State::new(o.props.get(Key::LOCKED).as_bool().unwrap_or(false)),
            sort: State::new(
                o.props
                    .get(Key::SORT)
                    .as_bytes()
                    .unwrap_or_default()
                    .to_vec(),
            ),
            expanded: State::new(o.props.get(Key::EXPANDED).as_bool().unwrap_or_default()),
        }
    }

    pub(crate) fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::LOCKED => self.locked.update(value.as_bool().unwrap_or(false)),
            Key::SORT => self
                .sort
                .update(value.as_bytes().unwrap_or_default().to_vec()),
            Key::EXPANDED => self.expanded.update(value.as_bool().unwrap_or_default()),
            _ => false,
        }
    }

    #[inline]
    pub(crate) fn is_expanded(&self) -> bool {
        *self.expanded
    }
}

pub(crate) enum ObjectKind {
    Token(TokenObject),
    Static(StaticObject),
    Group(GroupObject),
}

pub(crate) struct ObjectData {
    pub(crate) peer_id: PeerId,
    pub(crate) id: Id,
    pub(crate) kind: ObjectKind,
    pub(crate) group: State<Id>,
    pub(crate) name: State<Option<String>>,
    pub(crate) hidden: State<bool>,
    pub(crate) local_hidden: State<bool>,
}

impl ObjectData {
    #[inline]
    pub(crate) fn new(peer_id: PeerId, o: &RemoteObject) -> Option<Self> {
        let kind = match o.ty {
            Type::TOKEN => ObjectKind::Token(TokenObject::from_remote(o)),
            Type::STATIC => ObjectKind::Static(StaticObject::from_remote(o)),
            Type::GROUP => ObjectKind::Group(GroupObject::from_remote(o)),
            _ => return None,
        };

        Some(Self {
            peer_id,
            id: o.id,
            kind,
            group: State::new(o.props.get(Key::GROUP).as_id()),
            name: State::new(o.props.get(Key::OBJECT_NAME).as_str().map(str::to_owned)),
            hidden: State::new(o.props.get(Key::HIDDEN).as_bool().unwrap_or(false)),
            local_hidden: State::new(o.props.get(Key::LOCAL_HIDDEN).as_bool().unwrap_or(false)),
        })
    }

    #[inline]
    pub(crate) fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::OBJECT_NAME => self.name.update(value.into_string()),
            Key::HIDDEN => self.hidden.update(value.as_bool().unwrap_or(false)),
            _ => match &mut self.kind {
                ObjectKind::Token(this) => this.update(key, value),
                ObjectKind::Static(this) => this.update(key, value),
                ObjectKind::Group(this) => this.update(key, value),
            },
        }
    }

    #[inline]
    pub(crate) fn is_expanded(&self) -> bool {
        match &self.kind {
            ObjectKind::Group(o) => o.is_expanded(),
            _ => false,
        }
    }

    /// Test if object is a group.
    #[inline]
    pub(crate) fn is_group(&self) -> bool {
        matches!(self.kind, ObjectKind::Group(_))
    }

    /// Test if object is interactive.
    #[inline]
    pub(crate) fn is_interactive(&self) -> bool {
        matches!(self.kind, ObjectKind::Token(_))
    }

    #[inline]
    pub(crate) fn icon(&self) -> &'static str {
        match &self.kind {
            ObjectKind::Token(..) => "user",
            ObjectKind::Static(..) => "squares-2x2",
            ObjectKind::Group(..) => "folder",
        }
    }

    #[inline]
    pub(crate) fn sort_mut(&mut self) -> Option<(&mut State<Id>, &mut State<Vec<u8>>)> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some((&mut self.group, &mut this.sort)),
            ObjectKind::Static(this) => Some((&mut self.group, &mut this.sort)),
            ObjectKind::Group(this) => Some((&mut self.group, &mut this.sort)),
        }
    }

    #[inline]
    pub(crate) fn sort(&self) -> &[u8] {
        match &self.kind {
            ObjectKind::Token(this) => &this.sort,
            ObjectKind::Static(this) => &this.sort,
            ObjectKind::Group(this) => &this.sort,
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
    pub(crate) fn as_click_geometry(&self) -> Option<Geometry<'_>> {
        let (transform, shape) = match &self.kind {
            ObjectKind::Token(this) => (
                &*this.transform,
                Shape::Circle {
                    radius: *this.token_radius,
                },
            ),
            ObjectKind::Static(this) => (
                &*this.transform,
                Shape::Rectangle {
                    width: *this.width,
                    height: *this.height,
                },
            ),
            _ => return None,
        };

        Some(Geometry { transform, shape })
    }

    /// Returns `true` if this is a token.
    #[inline]
    pub(crate) fn is_token(&self) -> bool {
        matches!(&self.kind, ObjectKind::Token(_))
    }

    /// Returns `true` if this is a static.
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
    pub(crate) fn as_expanded_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Group(this) => Some(&mut this.expanded),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn visibility(&self) -> Visibility {
        if *self.local_hidden {
            return Visibility::None;
        }

        if *self.hidden {
            return Visibility::Local;
        }

        Visibility::Remote
    }

    #[inline]
    pub(crate) fn is_hidden(&self) -> bool {
        *self.hidden
    }

    #[inline]
    pub(crate) fn is_local_hidden(&self) -> bool {
        *self.local_hidden
    }

    #[inline]
    pub(crate) fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    #[inline]
    pub(crate) fn as_locked_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.locked),
            ObjectKind::Static(this) => Some(&mut this.locked),
            ObjectKind::Group(this) => Some(&mut this.locked),
        }
    }

    #[inline]
    pub(crate) fn is_locked(&self) -> bool {
        match &self.kind {
            ObjectKind::Token(this) => *this.locked,
            ObjectKind::Static(this) => *this.locked,
            ObjectKind::Group(this) => *this.locked,
        }
    }
}
