use core::cell::{Cell, Ref, RefCell, RefMut};

use std::collections::HashMap;
use std::rc::Rc;

use api::{Color, Extent, Id, Key, PeerId, RemoteId, RemoteObject, Transform, Type, Value, Vec3};
use yew::{Html, html};

use crate::components::Visibility;
use crate::state::State;

const DEFAULT_SPEED: f32 = 2.5;
const DEFAULT_STATIC_WIDTH: f32 = 1.0;
const DEFAULT_STATIC_HEIGHT: f32 = 1.0;
const DEFAULT_TOKEN_RADIUS: f32 = 0.25;

enum Shape {
    Empty,
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
            Shape::Empty => false,
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
    values: HashMap<RemoteId, Object>,
}

impl ObjectsRef {
    #[inline]
    pub(crate) fn get(&self, id: RemoteId) -> Option<&Object> {
        self.values.get(&id)
    }

    #[inline]
    pub(crate) fn values(&self) -> impl Iterator<Item = &Object> {
        self.values.values()
    }

    #[inline]
    pub(crate) fn is_interactive(&self, id: RemoteId) -> bool {
        let Some(object) = self.values.get(&id) else {
            return false;
        };

        object.is_interactive()
    }

    #[inline]
    pub(crate) fn retain<F>(&mut self, mut f: F)
    where
        F: FnMut(&RemoteId, bool) -> bool,
    {
        self.values.retain(move |id, o| {
            let global = o.is_global();
            f(id, global)
        });
    }

    #[inline]
    pub(crate) fn remove(&mut self, id: RemoteId) -> Option<Object> {
        self.values.remove(&id)
    }

    #[inline]
    pub(crate) fn insert(&mut self, object: Object) -> Option<Object> {
        self.values.insert(object.id, object)
    }

    #[inline]
    pub(crate) fn get_mut(&mut self, id: RemoteId) -> Option<&mut Object> {
        self.values.get_mut(&id)
    }

    #[inline]
    pub(crate) fn values_mut(&mut self) -> impl Iterator<Item = &mut Object> {
        self.values.values_mut()
    }

    /// Test if the given group or any of its ancestors is hidden.
    #[inline]
    pub(crate) fn visibility(&self, group: RemoteId) -> Visibility {
        let mut hidden = Visibility::Remote;
        let mut current = group;

        while current != RemoteId::ZERO {
            let Some(object) = self.values.get(&current) else {
                break;
            };

            hidden = hidden.max(object.visibility());
            current = *object.group;
        }

        hidden
    }

    /// Test if the given group or any of its ancestors is locked.
    #[inline]
    pub(crate) fn is_locked(&self, id: RemoteId) -> bool {
        let mut current = id;

        while current != RemoteId::ZERO {
            let Some(object) = self.values.get(&current) else {
                break;
            };

            if object.is_locked() {
                return true;
            }

            current = *object.group;
        }

        false
    }
}

impl FromIterator<Object> for Objects {
    #[inline]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Object>,
    {
        let mutable = ObjectsRef {
            values: iter.into_iter().map(|o| (o.id, o)).collect(),
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

#[derive(Debug, PartialEq)]
pub(crate) struct TokenObject {
    pub(crate) transform: State<Transform>,
    pub(crate) locked: State<bool>,
    pub(crate) look_at: State<Option<Vec3>>,
    pub(crate) image: State<Id>,
    pub(crate) color: State<Option<Color>>,
    pub(crate) token_radius: State<f32>,
    pub(crate) speed: State<f32>,
}

impl TokenObject {
    pub(crate) fn new(o: &RemoteObject) -> Self {
        Self {
            transform: State::new(
                o.props
                    .get(Key::TRANSFORM)
                    .as_transform()
                    .unwrap_or_else(Transform::origin),
            ),
            locked: State::new(o.props.get(Key::LOCKED).as_bool()),
            look_at: State::new(o.props.get(Key::LOOK_AT).as_vec3()),
            image: State::new(o.props.get(Key::IMAGE_ID).as_id()),
            color: State::new(o.props.get(Key::COLOR).as_color()),
            token_radius: State::new(
                o.props
                    .get(Key::RADIUS)
                    .as_f32()
                    .unwrap_or(DEFAULT_TOKEN_RADIUS),
            ),
            speed: State::new(o.props.get(Key::SPEED).as_f32().unwrap_or(DEFAULT_SPEED)),
        }
    }

    pub(crate) fn update(&mut self, key: Key, value: &Value) -> bool {
        match key {
            Key::TRANSFORM => self
                .transform
                .update(value.as_transform().unwrap_or_else(Transform::origin)),
            Key::LOCKED => self.locked.update(value.as_bool()),
            Key::LOOK_AT => self.look_at.update(value.as_vec3()),
            Key::IMAGE_ID => self.image.update(value.as_id()),
            Key::COLOR => self.color.update(value.as_color()),
            Key::RADIUS => self
                .token_radius
                .update(value.as_f32().unwrap_or(DEFAULT_TOKEN_RADIUS)),
            Key::SPEED => self.speed.update(value.as_f32().unwrap_or(DEFAULT_SPEED)),
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct StaticObject {
    pub(crate) transform: State<Transform>,
    pub(crate) locked: State<bool>,
    pub(crate) image: State<Id>,
    pub(crate) color: State<Option<Color>>,
    pub(crate) name: State<String>,
    pub(crate) hidden: State<bool>,
    pub(crate) width: State<f32>,
    pub(crate) height: State<f32>,
}

impl StaticObject {
    pub(crate) fn new(o: &RemoteObject) -> Self {
        Self {
            transform: State::new(
                o.props
                    .get(Key::TRANSFORM)
                    .as_transform()
                    .unwrap_or_else(Transform::origin),
            ),
            locked: State::new(o.props.get(Key::LOCKED).as_bool()),
            image: State::new(o.props.get(Key::IMAGE_ID).as_id()),
            color: State::new(o.props.get(Key::COLOR).as_color()),
            name: State::new(o.props.get(Key::NAME).as_str().to_owned()),
            hidden: State::new(o.props.get(Key::HIDDEN).as_bool()),
            width: State::new(
                o.props
                    .get(Key::WIDTH)
                    .as_f32()
                    .unwrap_or(DEFAULT_STATIC_WIDTH),
            ),
            height: State::new(
                o.props
                    .get(Key::HEIGHT)
                    .as_f32()
                    .unwrap_or(DEFAULT_STATIC_HEIGHT),
            ),
        }
    }

    pub(crate) fn update(&mut self, key: Key, v: &Value) -> bool {
        match key {
            Key::TRANSFORM => self
                .transform
                .update(v.as_transform().unwrap_or_else(Transform::origin)),
            Key::LOCKED => self.locked.update(v.as_bool()),
            Key::IMAGE_ID => self.image.update(v.as_id()),
            Key::COLOR => self.color.update(v.as_color()),
            Key::NAME => self.name.update(v.as_str().to_owned()),
            Key::HIDDEN => self.hidden.update(v.as_bool()),
            Key::WIDTH => self
                .width
                .update(v.as_f32().unwrap_or(DEFAULT_STATIC_WIDTH)),
            Key::HEIGHT => self
                .height
                .update(v.as_f32().unwrap_or(DEFAULT_STATIC_HEIGHT)),
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct GroupObject {
    pub(crate) locked: State<bool>,
    pub(crate) expanded: State<bool>,
}

impl GroupObject {
    pub(crate) fn new(o: &RemoteObject) -> Self {
        Self {
            locked: State::new(o.props.get(Key::LOCKED).as_bool()),
            expanded: State::new(o.props.get(Key::EXPANDED).as_bool()),
        }
    }

    pub(crate) fn update(&mut self, key: Key, value: &Value) -> bool {
        match key {
            Key::LOCKED => self.locked.update(value.as_bool()),
            Key::EXPANDED => self.expanded.update(value.as_bool()),
            _ => false,
        }
    }

    #[inline]
    pub(crate) fn is_expanded(&self) -> bool {
        *self.expanded
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct RoomObject {
    pub(crate) background: State<Id>,
    pub(crate) extent: State<Extent>,
    pub(crate) show_grid: State<bool>,
    pub(crate) color: State<Color>,
}

impl RoomObject {
    pub(crate) fn new(o: &RemoteObject) -> Self {
        Self {
            background: State::new(o.props.get(Key::ROOM_BACKGROUND).as_id()),
            extent: State::new(
                o.props
                    .get(Key::ROOM_EXTENT)
                    .as_extent()
                    .unwrap_or_else(Extent::arena),
            ),
            show_grid: State::new(o.props.get(Key::SHOW_GRID).as_bool()),
            color: State::new(
                o.props
                    .get(Key::COLOR)
                    .as_color()
                    .unwrap_or_else(Color::neutral_background),
            ),
        }
    }

    pub(crate) fn update(&mut self, key: Key, value: &Value) -> bool {
        match key {
            Key::ROOM_BACKGROUND => self.background.update(value.as_id()),
            Key::ROOM_EXTENT => self
                .extent
                .update(value.as_extent().unwrap_or_else(Extent::arena)),
            Key::SHOW_GRID => self.show_grid.update(value.as_bool()),
            Key::COLOR => self
                .color
                .update(value.as_color().unwrap_or_else(Color::neutral_background)),
            _ => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum ObjectKind {
    Token(TokenObject),
    Static(StaticObject),
    Group(GroupObject),
    Room(RoomObject),
}

#[derive(Debug, PartialEq)]
pub(crate) struct Object {
    pub(crate) id: RemoteId,
    pub(crate) group: State<RemoteId>,
    pub(crate) name: State<String>,
    pub(crate) hidden: State<bool>,
    pub(crate) local_hidden: State<bool>,
    pub(crate) sort: State<Vec<u8>>,
    pub(crate) kind: ObjectKind,
}

impl Object {
    #[inline]
    pub(crate) fn new(peer_id: PeerId, o: &RemoteObject) -> Option<Self> {
        let kind = match o.ty {
            Type::TOKEN => ObjectKind::Token(TokenObject::new(o)),
            Type::STATIC => ObjectKind::Static(StaticObject::new(o)),
            Type::GROUP => ObjectKind::Group(GroupObject::new(o)),
            Type::ROOM => ObjectKind::Room(RoomObject::new(o)),
            _ => return None,
        };

        Some(Self {
            id: RemoteId::new(peer_id, o.id),
            group: State::new(RemoteId::new(peer_id, o.props.get(Key::GROUP).as_id())),
            name: State::new(o.props.get(Key::NAME).as_str().to_owned()),
            hidden: State::new(o.props.get(Key::HIDDEN).as_bool()),
            local_hidden: State::new(o.props.get(Key::LOCAL_HIDDEN).as_bool()),
            sort: State::new(o.props.get(Key::SORT).as_bytes().to_vec()),
            kind,
        })
    }

    /// Get a view for this object.
    #[inline]
    pub(crate) fn as_ref(&self) -> ObjectRef {
        ObjectRef {
            ty: self.ty(),
            name: self.name.clone(),
            id: self.id,
        }
    }

    /// Get the type of this object.
    #[inline]
    pub(crate) fn ty(&self) -> Type {
        match self.kind {
            ObjectKind::Token(_) => Type::TOKEN,
            ObjectKind::Static(_) => Type::STATIC,
            ObjectKind::Group(_) => Type::GROUP,
            ObjectKind::Room(_) => Type::ROOM,
        }
    }

    #[inline]
    pub(crate) fn update(&mut self, key: Key, value: &Value) -> bool {
        match key {
            Key::NAME => self.name.update_str(value.as_str()),
            Key::HIDDEN => self.hidden.update(value.as_bool()),
            Key::LOCAL_HIDDEN => self.local_hidden.update(value.as_bool()),
            Key::GROUP => self
                .group
                .update(RemoteId::new(self.id.peer_id, value.as_id())),
            Key::SORT => self.sort.update(value.as_bytes().to_vec()),
            _ => match &mut self.kind {
                ObjectKind::Token(this) => this.update(key, value),
                ObjectKind::Static(this) => this.update(key, value),
                ObjectKind::Group(this) => this.update(key, value),
                ObjectKind::Room(this) => this.update(key, value),
            },
        }
    }

    #[inline]
    pub(crate) fn is_global(&self) -> bool {
        matches!(self.kind, ObjectKind::Room(_))
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
            ObjectKind::Group(g) => {
                if *g.expanded {
                    "folder-open"
                } else {
                    "folder"
                }
            }
            ObjectKind::Room(..) => "home",
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
    pub(crate) fn as_click_geometry(&self) -> Geometry<'_> {
        const ORIGIN: Transform = Transform::origin();

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
            _ => {
                return Geometry {
                    transform: &ORIGIN,
                    shape: Shape::Empty,
                };
            }
        };

        Geometry { transform, shape }
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
    pub(crate) fn as_locked_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.locked),
            ObjectKind::Static(this) => Some(&mut this.locked),
            ObjectKind::Group(this) => Some(&mut this.locked),
            ObjectKind::Room(_) => None,
        }
    }

    #[inline]
    pub(crate) fn is_locked(&self) -> bool {
        match &self.kind {
            ObjectKind::Token(this) => *this.locked,
            ObjectKind::Static(this) => *this.locked,
            ObjectKind::Group(this) => *this.locked,
            ObjectKind::Room(_) => false,
        }
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct ObjectRef {
    pub(crate) ty: Type,
    pub(crate) name: State<String>,
    pub(crate) id: RemoteId,
}

impl ObjectRef {
    #[inline]
    pub(crate) fn update(&mut self, object: &Object) -> bool {
        self.name.update(object.name.as_str().to_owned())
    }

    /// Get a view for this object.
    #[inline]
    pub(crate) fn name(&self) -> Html {
        if !self.name.is_empty() {
            return html! {
                <span class="object">
                    <span class="object-name">{self.name.as_str()}</span>
                </span>
            };
        }

        html! {
            <span class="object">
                <span class="object-type">{self.ty.display()}</span>
                <span class="object-id">{self.id.to_string()}</span>
            </span>
        }
    }

    /// Get a title view for this object.
    #[inline]
    pub(crate) fn title(&self) -> Html {
        if !self.name.is_empty() {
            return html! {
                <span class="object">
                    <span class="object-type">{self.ty.title()}</span>
                    <span class="object-name">{self.name.as_str()}</span>
                </span>
            };
        }

        html! {
            <span class="object">
                <span class="object-type">{self.ty.title()}</span>
                <span class="object-id">{self.id.to_string()}</span>
            </span>
        }
    }
}
