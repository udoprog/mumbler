use std::collections::{BTreeSet, HashMap, HashSet};

use api::{
    Color, Extent, Id, Key, LocalUpdateBody, Pan, PeerId, RemoteObject, RemotePeerObject,
    RemoteUpdateBody, Transform, Value, Vec3, VecXZ,
};
use gloo::events::EventListener;
use gloo::timers::callback::Interval;
use musli_web::web::Packet;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlElement, KeyboardEvent, MouseEvent,
    ResizeObserver, WheelEvent,
};
use yew::prelude::*;

use crate::components::Icon;
use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::state::State;
use crate::ws;

use super::render::{self, RenderStatic, RenderToken, ViewTransform};
use super::{ObjectSettings, StaticSettings, into_target};

const LEFT_MOUSE_BUTTON: i16 = 0;
const MIDDLE_MOUSE_BUTTON: i16 = 1;

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const DEFAULT_SPEED: f32 = 5.0;
const DEFAULT_STATIC_WIDTH: f32 = 1.0;
const DEFAULT_STATIC_HEIGHT: f32 = 1.0;
const DEFAULT_TOKEN_RADIUS: f32 = 0.25;
const ANIMATION_FPS: u32 = 60;

pub(crate) struct Config {
    pub(crate) zoom: State<f32>,
    pub(crate) pan: State<Pan>,
    pub(crate) extent: State<Extent>,
    pub(crate) mumble_object: State<Option<Id>>,
    pub(crate) mumble_follow_selection: State<bool>,
}

impl Config {
    fn from_config(props: api::Properties) -> Self {
        let mut this = Self::default();

        for (key, value) in props {
            this.update(key, value);
        }

        this
    }

    fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::WORLD_SCALE => self.zoom.update(value.as_f32().unwrap_or(2.0)),
            Key::WORLD_PAN => self.pan.update(value.as_pan().unwrap_or_else(Pan::zero)),
            Key::WORLD_EXTENT => self
                .extent
                .update(value.as_extent().unwrap_or_else(Extent::arena)),
            Key::MUMBLE_OBJECT => self.mumble_object.update(value.as_id()),
            Key::MUMBLE_FOLLOW_SELECTION => self
                .mumble_follow_selection
                .update(value.as_bool().unwrap_or(false)),
            _ => false,
        }
    }

    fn world_values(&self) -> Vec<(Key, Value)> {
        let mut values = Vec::new();
        values.push((Key::WORLD_SCALE, Value::from(*self.zoom)));
        values.push((Key::WORLD_PAN, Value::from(*self.pan)));
        values.push((Key::WORLD_EXTENT, Value::from(*self.extent)));
        values
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            zoom: State::new(2.0),
            pan: State::new(Pan::zero()),
            extent: State::new(Extent::arena()),
            mumble_object: State::new(None),
            mumble_follow_selection: State::new(false),
        }
    }
}

pub(crate) struct PeerObject {
    pub(crate) peer_id: PeerId,
    pub(crate) data: ObjectData,
}

impl PeerObject {
    fn from_peer(remote: &RemotePeerObject) -> Self {
        Self {
            peer_id: remote.peer_id,
            data: ObjectData::from_remote(&remote.object),
        }
    }
}

pub(crate) struct LocalObject {
    pub(crate) data: ObjectData,
    pub(crate) move_target: Option<VecXZ>,
    pub(crate) arrow_target: Option<VecXZ>,
}

impl LocalObject {
    fn from_remote(remote: &RemoteObject) -> Self {
        Self {
            data: ObjectData::from_remote(remote),
            move_target: None,
            arrow_target: None,
        }
    }
}

pub(crate) struct Token {
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

impl Token {
    fn update(&mut self, key: Key, value: Value) -> bool {
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
    fn update(&mut self, key: Key, value: Value) -> bool {
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

pub(crate) enum ObjectKind {
    Token(Token),
    Static(StaticObject),
    Unknown,
}

pub(crate) struct ObjectData {
    pub(crate) id: Id,
    pub(crate) kind: ObjectKind,
}

impl ObjectData {
    fn from_remote(o: &RemoteObject) -> Self {
        let kind = match o.ty {
            api::Type::TOKEN => {
                let token = Token {
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
                };

                ObjectKind::Token(token)
            }
            api::Type::STATIC => {
                let s = StaticObject {
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
                };

                ObjectKind::Static(s)
            }
            _ => ObjectKind::Unknown,
        };

        Self { id: o.id, kind }
    }

    fn update(&mut self, key: Key, value: Value) -> bool {
        match &mut self.kind {
            ObjectKind::Token(this) => this.update(key, value),
            ObjectKind::Static(this) => this.update(key, value),
            ObjectKind::Unknown => false,
        }
    }

    #[inline]
    fn sort_mut(&mut self) -> Option<&mut State<Vec<u8>>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.sort),
            ObjectKind::Static(this) => Some(&mut this.sort),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn sort(&self) -> &[u8] {
        match &self.kind {
            ObjectKind::Token(this) => &this.sort,
            ObjectKind::Static(this) => &this.sort,
            ObjectKind::Unknown => &[],
        }
    }

    #[inline]
    fn as_transform(&self) -> Option<&Transform> {
        match &self.kind {
            ObjectKind::Token(this) => Some(&this.transform),
            ObjectKind::Static(this) => Some(&this.transform),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_transform_mut(&mut self) -> Option<&mut State<Transform>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.transform),
            ObjectKind::Static(this) => Some(&mut this.transform),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_interpolate_mut(
        &mut self,
    ) -> Option<(&mut State<Transform>, Option<&Vec3>, Option<f32>)> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some((
                &mut this.transform,
                this.look_at.as_ref(),
                Some(*this.speed),
            )),
            ObjectKind::Static(this) => Some((&mut this.transform, None, None)),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn click_radius(&self) -> f32 {
        match &self.kind {
            ObjectKind::Token(this) => *this.token_radius,
            ObjectKind::Static(this) => (*this.width).hypot(*this.height) / 2.0,
            ObjectKind::Unknown => 0.5,
        }
    }

    /// Returns `true` if this is a static object (rectangle, snap movement).
    #[inline]
    fn is_static(&self) -> bool {
        matches!(&self.kind, ObjectKind::Static(_))
    }

    #[inline]
    fn look_at(&self) -> Option<&Vec3> {
        match &self.kind {
            ObjectKind::Token(this) => this.look_at.as_ref(),
            ObjectKind::Static(_) | ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_look_at_mut(&mut self) -> Option<&mut State<Option<Vec3>>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.look_at),
            ObjectKind::Static(_) | ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_image_mut(&mut self) -> Option<&mut State<Option<Id>>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.image),
            ObjectKind::Static(this) => Some(&mut this.image),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_hidden_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.hidden),
            ObjectKind::Static(this) => Some(&mut this.hidden),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn name(&self) -> Option<&str> {
        match &self.kind {
            ObjectKind::Token(this) => this.name.as_deref(),
            ObjectKind::Static(this) => this.name.as_deref(),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn image(&self) -> Option<Id> {
        match &self.kind {
            ObjectKind::Token(this) => *this.image,
            ObjectKind::Static(this) => *this.image,
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn is_hidden(&self) -> bool {
        match &self.kind {
            ObjectKind::Token(this) => *this.hidden,
            ObjectKind::Static(this) => *this.hidden,
            ObjectKind::Unknown => false,
        }
    }

    #[inline]
    fn as_locked_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Token(this) => Some(&mut this.locked),
            ObjectKind::Static(this) => Some(&mut this.locked),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn is_locked(&self) -> bool {
        match &self.kind {
            ObjectKind::Token(this) => *this.locked,
            ObjectKind::Static(this) => *this.locked,
            ObjectKind::Unknown => false,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Drag {
    Above,
    Below,
}

impl Drag {
    fn from_event(ev: DragEvent, element: &HtmlElement) -> Self {
        let rect = element.get_bounding_client_rect();
        let offset = ev.client_y() as f64 - rect.top();

        if offset < rect.height() / 2.0 {
            Drag::Above
        } else {
            Drag::Below
        }
    }
}

pub(crate) struct Map {
    _create_object: ws::Request,
    _create_static: ws::Request,
    _delete_object: ws::Request,
    _initialize: ws::Request,
    _keydown_listener: EventListener,
    _keyup_listener: EventListener,
    _local_update_listener: ws::Listener,
    _log_handle: ContextHandle<log::Log>,
    _remote_update_listener: ws::Listener,
    _resize_observer: Option<(ResizeObserver, Closure<dyn FnMut()>)>,
    _set_mumble_follow_selection: ws::Request,
    _set_sort: ws::Request,
    _state_change: ws::StateListener,
    _toggle_locked_request: ws::Request,
    _toggle_mumble_request: ws::Request,
    _update_world: ws::Request,
    animation_interval: Option<Interval>,
    canvas_ref: NodeRef,
    canvas_sizer: NodeRef,
    config: Config,
    context_menu: Option<ContextMenu>,
    delete: Option<Id>,
    drag_over: Option<(Drag, Id)>,
    hide_requests: HashMap<Id, ws::Request>,
    images: Images<Self>,
    log: log::Log,
    look_at_requests: HashMap<Id, ws::Request>,
    mouse_world_pos: Option<VecXZ>,
    objects: HashMap<Id, LocalObject>,
    open_settings: Option<Id>,
    order: BTreeSet<(Vec<u8>, Id)>,
    pan_anchor: Option<(f64, f64)>,
    peers: HashMap<(PeerId, Id), PeerObject>,
    selected: Option<Id>,
    start_press: Option<(VecXZ, bool)>,
    state: ws::State,
    transform_requests: HashMap<Id, ws::Request>,
    update_look_at_ids: HashSet<Id>,
    update_transform_ids: HashSet<Id>,
    update_world: bool,
}

/// State for the right-click context menu.
struct ContextMenu {
    /// Object the menu was opened for.
    object_id: Id,
    /// CSS left position (pixels from the map-sizer left edge).
    x: f64,
    /// CSS top position (pixels from the map-sizer top edge).
    y: f64,
}

pub(crate) enum Msg {
    AnimationFrame,
    CancelDelete,
    CloseContextMenu,
    CloseObjectSettings,
    ConfigResult(Result<Packet<api::UpdateConfig>, ws::Error>),
    ConfigUpdate(Result<Packet<api::ConfigUpdate>, ws::Error>),
    ConfirmDelete(Id),
    ContextMenu(MouseEvent),
    CreateStatic,
    CreateToken,
    DeleteObject(Id),
    DragEnd(Id),
    DragOver(DragEvent, Id),
    DragStart(DragEvent, Id),
    ImageMessage(ImageMessage),
    InitializeMap(Result<Packet<api::InitializeMap>, ws::Error>),
    KeyDown(KeyboardEvent),
    KeyUp(KeyboardEvent),
    LocalUpdate(Result<Packet<api::LocalUpdate>, ws::Error>),
    ObjectCreated(Result<Packet<api::CreateObject>, ws::Error>),
    ObjectDeleted(Result<Packet<api::DeleteObject>, ws::Error>),
    OpenObjectSettings(Id),
    PointerDown(PointerEvent),
    PointerLeave,
    PointerMove(PointerEvent),
    PointerUp(PointerEvent),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    Resized,
    SelectObject(Option<Id>),
    SetLog(log::Log),
    StateChanged(ws::State),
    StaticObjectCreated(Result<Packet<api::CreateObject>, ws::Error>),
    ToggleFollowMumbleSelection,
    ToggleHidden(Id),
    ToggleHiddenResult(Id, Result<Packet<api::Update>, ws::Error>),
    ToggleLocked(Id),
    ToggleMumbleObject(Id),
    UpdateResult(Result<Packet<api::Update>, ws::Error>),
    Wheel(WheelEvent),
}

impl From<ImageMessage> for Msg {
    #[inline]
    fn from(message: ImageMessage) -> Self {
        Msg::ImageMessage(message)
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

impl Component for Map {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::SetLog))
            .expect("ErrorLog context not found");

        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let _config_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::ConfigUpdate>(ctx.link().callback(Msg::ConfigUpdate));

        let _local_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::LocalUpdate>(ctx.link().callback(Msg::LocalUpdate));

        let _remote_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::RemoteUpdate>(ctx.link().callback(Msg::RemoteUpdate));

        let document = web_sys::window()
            .expect("window")
            .document()
            .expect("document");

        let link = ctx.link().clone();
        let _keydown_listener = EventListener::new(&document, "keydown", move |e| {
            if let Some(e) = e.dyn_ref::<KeyboardEvent>() {
                link.send_message(Msg::KeyDown(e.clone()));
            }
        });

        let link = ctx.link().clone();
        let _keyup_listener = EventListener::new(&document, "keyup", move |e| {
            if let Some(e) = e.dyn_ref::<KeyboardEvent>() {
                link.send_message(Msg::KeyUp(e.clone()));
            }
        });

        let mut this = Self {
            _create_object: ws::Request::new(),
            _create_static: ws::Request::new(),
            _delete_object: ws::Request::new(),
            _initialize: ws::Request::new(),
            _keydown_listener,
            _keyup_listener,
            _local_update_listener,
            _log_handle,
            _remote_update_listener,
            _resize_observer: None,
            _set_mumble_follow_selection: ws::Request::new(),
            _set_sort: ws::Request::new(),
            _state_change,
            _toggle_locked_request: ws::Request::new(),
            _toggle_mumble_request: ws::Request::new(),
            _update_world: ws::Request::new(),
            animation_interval: None,
            canvas_ref: NodeRef::default(),
            canvas_sizer: NodeRef::default(),
            config: Config::default(),
            context_menu: None,
            delete: None,
            drag_over: None,
            hide_requests: HashMap::new(),
            images: Images::new(),
            log,
            look_at_requests: HashMap::new(),
            mouse_world_pos: None,
            objects: HashMap::new(),
            open_settings: None,
            order: BTreeSet::new(),
            pan_anchor: None,
            peers: HashMap::new(),
            selected: None,
            start_press: None,
            state,
            transform_requests: HashMap::new(),
            update_look_at_ids: HashSet::new(),
            update_transform_ids: HashSet::new(),
            update_world: false,
        };

        this.refresh(ctx);
        this
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            self.resize_canvas();

            if let Err(error) = self.setup_resizer(ctx) {
                self.log.error("map::setup_resizer", error);
            }
        }

        if let Err(error) = self.redraw() {
            self.log.error("map::redraw", error);
        }
    }

    fn destroy(&mut self, _: &Context<Self>) {
        if let Some((observer, _closure)) = self._resize_observer.take() {
            observer.disconnect();
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let changed = match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("map::update", error);
                false
            }
        };

        if !self.update_transform_ids.is_empty() {
            self.send_transform_updates(ctx);
        }

        if !self.update_look_at_ids.is_empty() {
            self.send_look_at_updates(ctx);
        }

        if self.update_world {
            self.send_world_updates(ctx);
            self.update_world = false;
        }

        if self.animation_interval.is_none()
            && self.objects.values().any(|o| o.move_target.is_some())
        {
            let link = ctx.link().clone();

            let interval = Interval::new(1000 / ANIMATION_FPS, move || {
                link.send_message(Msg::AnimationFrame);
            });

            self.animation_interval = Some(interval);
        }

        changed
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let pos;

        if let Some(o) = self.selected.and_then(|id| self.objects.get(&id))
            && let Some(transform) = o.data.as_transform()
        {
            let p = transform.position;
            let f = transform.front;

            let zoom = *self.config.zoom;

            let position = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", p.x, p.y, p.z);
            let front = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", f.x, f.y, f.z);
            let other = format!("ZOOM:{:.02}", zoom);
            pos = Some(html!(<div class="pre">{position}{" / "}{front}{" / "}{other}</div>))
        } else {
            pos = None;
        }

        let object_list_header = {
            let o = self.selected.and_then(|id| self.objects.get(&id));

            let settings_classes = classes! {
                "btn",
                "square",
                o.is_some().then_some("primary"),
                o.is_none().then_some("disabled"),
            };

            let settings_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::OpenObjectSettings(id))
            });

            let delete_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::ConfirmDelete(id))
            });

            let delete_classes = classes! {
                "btn",
                "square",
                o.is_some().then_some("danger"),
                o.is_none().then_some("disabled"),
            };

            html! {
                <div class="control-group">
                    <button class="btn square primary" title="Add token" onclick={ctx.link().callback(|_| Msg::CreateToken)}>
                        <Icon name="user-plus" title="Add token" />
                    </button>
                    <button class="btn square primary" title="Add static object" onclick={ctx.link().callback(|_| Msg::CreateStatic)}>
                        <Icon name="squares-plus" title="Add static" />
                    </button>
                    <div class="fill"></div>
                    <button class={settings_classes} title="Object settings" onclick={settings_click}>
                        <Icon name="cog" />
                    </button>
                    <button class={delete_classes} title="Delete object" onclick={delete_click}>
                        <Icon name="x-mark" />
                    </button>
                </div>
            }
        };

        let toolbar = {
            let o = self.selected.and_then(|id| self.objects.get(&id));

            let is_hidden = o.map(|o| o.data.is_hidden()).unwrap_or_default();
            let is_locked = o.map(|o| o.data.is_locked()).unwrap_or_default();
            let is_mumble = o
                .map(|o| *self.config.mumble_object == Some(o.data.id))
                .unwrap_or_default();

            let hidden_icon = if is_hidden { "eye-slash" } else { "eye" };
            let locked_icon = if is_locked {
                "lock-closed"
            } else {
                "lock-open"
            };

            let hidden_title = if is_hidden {
                "Hidden from others"
            } else {
                "Visible to others"
            };

            let locked_title = if is_locked { "Locked" } else { "Unlocked" };

            let follow_classes = classes! {
                "btn", "square",
                self.config.mumble_follow_selection.then_some("success"),
            };

            let follow_title = if *self.config.mumble_follow_selection {
                "Disable MumbleLink selection following"
            } else {
                "Enable MumbleLink selection following"
            };

            let mumble_classes = classes! {
                "btn", "square",
                is_mumble.then_some("success"),
                o.is_none().then_some("disabled"),
            };

            let hidden_classes = classes! {
                "btn", "square",
                is_hidden.then_some("danger"),
                o.is_none().then_some("disabled"),
            };

            let locked_classes = classes! {
                "btn", "square",
                is_locked.then_some("danger"),
                o.is_none().then_some("disabled"),
            };

            let mumble_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::ToggleMumbleObject(id))
            });

            let hidden_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::ToggleHidden(id))
            });

            let locked_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::ToggleLocked(id))
            });

            html! {
                <div class="control-group">
                    <button class={mumble_classes} title="Toggle as MumbleLink Source" onclick={mumble_click}>
                        <Icon name="mumble" />
                    </button>
                    <button class={hidden_classes} title={hidden_title} onclick={hidden_click}>
                        <Icon name={hidden_icon} />
                    </button>
                    <button class={locked_classes} title={locked_title} onclick={locked_click}>
                        <Icon name={locked_icon} />
                    </button>
                    <div class="fill"></div>
                    <button class={follow_classes} title={follow_title} onclick={ctx.link().callback(|_| Msg::ToggleFollowMumbleSelection)}>
                        <Icon name="cursor-arrow-rays" />
                    </button>
                </div>
            }
        };

        html! {
            <>
            <div class="row">
                <div class="col-9 rows">
                    {toolbar}

                    <div class="map-sizer" ref={self.canvas_sizer.clone()}>
                        <canvas id="map" ref={self.canvas_ref.clone()}
                            onpointerdown={ctx.link().callback(Msg::PointerDown)}
                            onpointermove={ctx.link().callback(Msg::PointerMove)}
                            onpointerup={ctx.link().callback(Msg::PointerUp)}
                            onpointerleave={ctx.link().callback(|_| Msg::PointerLeave)}
                            onwheel={ctx.link().callback(Msg::Wheel)}
                            oncontextmenu={ctx.link().callback(Msg::ContextMenu)}
                        ></canvas>

                        if let Some(menu) = &self.context_menu {
                            {self.render_context_menu(ctx, menu)}
                        }
                    </div>

                    {pos}
                </div>

                <div class="col-3 rows">
                    {object_list_header}

                    <div class="object-list">
                        {for self.order.iter().flat_map(|(_, id)| {
                            let o = self.objects.get(id)?;

                            let id = o.data.id;
                            let selected = self.selected == Some(id);
                            let on_click = ctx.link().callback(move |_| Msg::SelectObject(Some(id)));
                            let classes = classes!("object-list-item", selected.then_some("selected"));
                            let label = o.data.name().unwrap_or("");

                            let is_hidden = o.data.is_hidden();
                            let is_locked = o.data.is_locked();
                            let hidden_icon = if is_hidden { "eye-slash" } else { "eye" };
                            let hidden_title = if is_hidden { "Hidden from others" } else { "Visible to others" };
                            let locked_icon = if is_locked { "lock-closed" } else { "lock-open" };
                            let locked_title = if is_locked { "Locked" } else { "Unlocked" };
                            let is_mumble = *self.config.mumble_object == Some(id);

                            let mumble_classes = classes! {
                                "btn", "sm", "square", "object-list-item-action",
                                is_mumble.then_some("success"),
                                is_mumble.then_some("active"),
                            };

                            let hidden_classes = classes! {
                                "btn", "sm", "square", "object-list-item-action",
                                is_hidden.then_some("danger"),
                                is_hidden.then_some("active"),
                            };

                            let locked_classes = classes! {
                                "btn", "sm", "square", "object-list-item-action",
                                is_locked.then_some("danger"),
                                is_locked.then_some("active"),
                            };

                            let icon_name = match o.data.kind {
                                ObjectKind::Token(..) => "user",
                                _ => "squares-2x2",
                            };

                            // Drag-and-drop handlers
                            let on_drag_end = ctx.link().callback(move |_| Msg::DragEnd(id));
                            let on_drag_start = ctx.link().callback(move |ev| Msg::DragStart(ev, id));
                            let on_drag_over = ctx.link().callback(move |ev| Msg::DragOver(ev, id));

                            let node = html! {
                                <div
                                    key={id.to_string()}
                                    class={classes}
                                    onclick={on_click}
                                    draggable={true}
                                    ondragstart={on_drag_start}
                                    ondragend={on_drag_end}
                                    ondragover={on_drag_over}
                                >
                                    <Icon name={icon_name} invert={true} />

                                    <span class="object-list-item-label">{label}</span>

                                    <button class={mumble_classes}
                                        title="Toggle as MumbleLink Source"
                                        onclick={ctx.link().callback(move |ev: MouseEvent| {
                                            ev.stop_propagation();
                                            Msg::ToggleMumbleObject(id)
                                        })}>
                                        <Icon name="mumble" />
                                    </button>
                                    <button class={hidden_classes}
                                        title={hidden_title}
                                        onclick={ctx.link().callback(move |ev: MouseEvent| {
                                            ev.stop_propagation();
                                            Msg::ToggleHidden(id)
                                        })}>
                                        <Icon name={hidden_icon} />
                                    </button>
                                    <button class={locked_classes}
                                        title={locked_title}
                                        onclick={ctx.link().callback(move |ev: MouseEvent| {
                                            ev.stop_propagation();
                                            Msg::ToggleLocked(id)
                                        })}>
                                        <Icon name={locked_icon} />
                                    </button>
                                </div>
                            };

                            let drop_above;
                            let drop_below;

                            match self.drag_over {
                                Some((Drag::Above, id)) if id == o.data.id => {
                                    drop_above = Some(html!(<div key={format!("above-{id}")} class="object-list-drop" />));
                                    drop_below = None;
                                }
                                Some((Drag::Below, id)) if id == o.data.id => {
                                    drop_above = None;
                                    drop_below = Some(html!(<div key={format!("below-{id}")} class="object-list-drop" />));
                                }
                                _ => {
                                    drop_above = None;
                                    drop_below = None;
                                },
                            }

                            Some(html! {
                                <>
                                    {for drop_above}
                                    {node}
                                    {for drop_below}
                                </>
                            })
                        })}
                    </div>
                </div>
            </div>

            if let Some(id) = self.delete {
                <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                    <div class="modal" onclick={|ev: MouseEvent| ev.stop_propagation()}>
                        <div class="modal-header">
                            <h2>{"Confirm Deletion"}</h2>
                            <button class="btn sm square danger" title="Cancel"
                                onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                                <Icon name="x-mark" />
                            </button>
                        </div>
                        <div class="modal-body rows">
                            <p>{format!("Remove \"{}\"?", self.objects.get(&id).and_then(|o| o.data.name()).unwrap_or("unnamed"))}</p>
                            <div class="btn-group">
                                <button class="btn danger"
                                    onclick={ctx.link().callback(move |_| Msg::DeleteObject(id))}>
                                    {"Delete"}
                                </button>
                                <button class="btn"
                                    onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                                    {"Cancel"}
                                </button>
                            </div>
                        </div>
                    </div>
                </div>
            }

            if let Some(id) = self.open_settings {
                <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::CloseObjectSettings)}>
                    <div class="modal" onclick={|ev: MouseEvent| ev.stop_propagation()}>
                        <div class="modal-header">
                            <h2>{"Object Settings"}</h2>
                            <button class="btn sm square danger" title="Close"
                                onclick={ctx.link().callback(|_| Msg::CloseObjectSettings)}>
                                <Icon name="x-mark" />
                            </button>
                        </div>
                        <div class="modal-body">
                            if self.objects.get(&id).is_some_and(|o| o.data.is_static()) {
                                <StaticSettings ws={ctx.props().ws.clone()} {id} />
                            } else {
                                <ObjectSettings ws={ctx.props().ws.clone()} {id} />
                            }
                        </div>
                    </div>
                </div>
            }
            </>
        }
    }
}

impl Map {
    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._initialize = ctx
                .props()
                .ws
                .request()
                .body(api::InitializeMapRequest)
                .on_packet(ctx.link().callback(Msg::InitializeMap))
                .send();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::DragStart(ev, id) => {
                let element = into_target!(ev, HtmlElement);
                let drag = Drag::from_event(ev, &element);
                tracing::warn!(?drag, ?id, "drag start");
                self.drag_over = Some((drag, id));
                Ok(true)
            }
            Msg::DragEnd(id) => {
                let Some((drag, target)) = self.drag_over.take() else {
                    return Ok(false);
                };

                let new = {
                    let Some(this) = self.objects.get(&target).map(|o| o.data.sort()) else {
                        return Ok(true);
                    };

                    let next = match drag {
                        Drag::Below => self
                            .order
                            .iter()
                            .skip_while(|(_, id)| *id != target)
                            .nth(1)
                            .map(|(_, id)| *id),
                        Drag::Above => self
                            .order
                            .iter()
                            .rev()
                            .skip_while(|(_, id)| *id != target)
                            .nth(1)
                            .map(|(_, id)| *id),
                    };

                    let next = next.and_then(|id| Some(self.objects.get(&id)?.data.sort()));

                    match (drag, next) {
                        (Drag::Above, Some(next)) => sorting::midpoint(next, this),
                        (Drag::Above, _) => sorting::before(this),
                        (Drag::Below, Some(next)) => sorting::midpoint(this, next),
                        (Drag::Below, _) => sorting::after(this),
                    }
                };

                let Some(sort) = self.objects.get_mut(&id).and_then(|o| o.data.sort_mut()) else {
                    return Ok(true);
                };

                let Some(sort) = sort.replace(new.clone()) else {
                    return Ok(true);
                };

                self._set_sort = self::update(ctx, id, Key::SORT, Value::from(new.clone()));
                self.order.remove(&(sort, id));
                self.order.insert((new, id));
                Ok(true)
            }
            Msg::DragOver(ev, id) => {
                let element = into_target!(ev, HtmlElement);
                let drag = Drag::from_event(ev, &element);
                tracing::warn!(?drag, ?id, "drag over");
                self.drag_over = Some((drag, id));
                Ok(true)
            }
            // Removed misplaced enum variants
            Msg::OpenObjectSettings(id) => {
                self.context_menu = None;
                self.open_settings = Some(id);
                Ok(true)
            }
            Msg::CloseObjectSettings => {
                self.open_settings = None;
                Ok(true)
            }
            Msg::ToggleMumbleObject(id) => {
                self.context_menu = None;

                let update = if *self.config.mumble_object == Some(id) {
                    None
                } else {
                    Some(id)
                };

                *self.config.mumble_object = update;

                self._toggle_mumble_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateConfigRequest {
                        values: vec![(Key::MUMBLE_OBJECT, Value::from(update))],
                    })
                    .on_packet(ctx.link().callback(Msg::ConfigResult))
                    .send();

                Ok(true)
            }
            Msg::ToggleLocked(id) => {
                let Some(object) = self.objects.get_mut(&id) else {
                    return Ok(false);
                };

                let Some(locked) = object.data.as_locked_mut() else {
                    return Ok(false);
                };

                if self.selected == Some(id) {
                    self.selected = None;
                }

                let new = !**locked;
                **locked = new;

                self._toggle_locked_request = update(ctx, id, Key::LOCKED, Value::from(new));
                Ok(true)
            }
            Msg::ConfigResult(result) => {
                result?;
                Ok(false)
            }
            Msg::ToggleHidden(id) => {
                self.context_menu = None;

                let Some(object) = self.objects.get_mut(&id) else {
                    return Ok(false);
                };

                let Some(hidden) = object.data.as_hidden_mut() else {
                    return Ok(false);
                };

                let new_hidden = !**hidden;
                **hidden = new_hidden;

                let req = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateRequest {
                        object_id: id,
                        key: Key::HIDDEN,
                        value: Value::from(new_hidden),
                    })
                    .on_packet(ctx.link().callback(move |r| Msg::ToggleHiddenResult(id, r)))
                    .send();

                self.hide_requests.insert(id, req);
                Ok(true)
            }
            Msg::ToggleHiddenResult(id, result) => {
                self.hide_requests.remove(&id);
                result?;
                Ok(false)
            }
            Msg::SetLog(log) => {
                self.log = log;
                Ok(false)
            }
            Msg::InitializeMap(result) => {
                let body = result?;
                let body = body.decode()?;

                tracing::debug!(?body, "Initialize");

                self.config = Config::from_config(body.config);

                self.objects = body
                    .objects
                    .iter()
                    .map(LocalObject::from_remote)
                    .map(|o| (o.data.id, (o)))
                    .collect();

                self.order.extend(
                    self.objects
                        .values()
                        .map(|o| (o.data.sort().to_vec(), o.data.id)),
                );

                self.peers = body
                    .remote_objects
                    .iter()
                    .map(PeerObject::from_peer)
                    .map(|peer| ((peer.peer_id, peer.data.id), peer))
                    .collect();

                self.load_initialize_images(ctx);
                self.redraw()?;
                Ok(true)
            }
            Msg::ConfigUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = self.config.update(body.key, body.value);

                if changed {
                    self.redraw()?;
                }

                Ok(changed)
            }
            Msg::LocalUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let update = match body {
                    LocalUpdateBody::Create { object } => {
                        let o = LocalObject::from_remote(&object);
                        self.order.insert((o.data.sort().to_vec(), o.data.id));
                        self.objects.insert(o.data.id, o);
                        true
                    }
                    LocalUpdateBody::Delete { object_id } => {
                        if let Some(removed) = self.objects.remove(&object_id)
                            && let Some(id) = removed.data.image()
                        {
                            self.images.remove(id);
                        }

                        if self.selected == Some(object_id) {
                            ctx.link().send_message(Msg::SelectObject(None));
                        }

                        true
                    }
                    LocalUpdateBody::Update {
                        object_id,
                        key,
                        value,
                    } => {
                        'done: {
                            let Some(o) = self.objects.get_mut(&object_id) else {
                                break 'done false;
                            };

                            let update = match key {
                                // Don't support local updates of transform and
                                // look at because they cause feedback loops
                                // which are laggy.
                                Key::TRANSFORM | Key::LOOK_AT => {
                                    break 'done false;
                                }
                                Key::IMAGE_ID => {
                                    let new = value.as_id();

                                    let Some(image) = o.data.as_image_mut() else {
                                        break 'done false;
                                    };

                                    let Some(old) = image.replace(new) else {
                                        break 'done false;
                                    };

                                    if let Some(new) = new {
                                        self.images.load(ctx, new);
                                    }

                                    if let Some(old) = old {
                                        self.images.remove(old);
                                    }

                                    false
                                }
                                Key::SORT => {
                                    let sort = value.as_bytes().unwrap_or_default().to_vec();
                                    self.order.retain(|&(_, id)| id != o.data.id);
                                    self.order.insert((sort, o.data.id));
                                    true
                                }
                                _ => false,
                            };

                            o.data.update(key, value) || update
                        }
                    }
                };

                self.redraw()?;
                Ok(update)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                tracing::debug!(?body, "Remote update");

                match body {
                    RemoteUpdateBody::RemoteLost => {
                        self.peers.clear();
                    }
                    RemoteUpdateBody::Join {
                        peer_id, objects, ..
                    } => {
                        for object in objects {
                            let data = ObjectData::from_remote(&object);

                            if let Some(id) = data.image() {
                                self.images.load(ctx, id);
                            }

                            self.peers
                                .insert((peer_id, data.id), PeerObject { peer_id, data });
                        }
                    }
                    RemoteUpdateBody::Leave { peer_id } => {
                        self.peers.retain(|&(pid, _), _| pid != peer_id);
                    }
                    RemoteUpdateBody::Update {
                        object_id,
                        peer_id,
                        key,
                        value,
                    } => 'done: {
                        let Some(a) = self.peers.get_mut(&(peer_id, object_id)) else {
                            break 'done;
                        };

                        match key {
                            Key::IMAGE_ID => {
                                let new = value.as_id();

                                let Some(image) = a.data.as_image_mut() else {
                                    break 'done;
                                };

                                let Some(old) = image.replace(new) else {
                                    break 'done;
                                };

                                if let Some(new) = new {
                                    self.images.load(ctx, new);
                                }

                                if let Some(old) = old {
                                    self.images.remove(old);
                                }
                            }
                            _ => {}
                        }

                        a.data.update(key, value);
                    }
                    RemoteUpdateBody::ObjectAdded { peer_id, object } => {
                        let data = ObjectData::from_remote(&object);

                        if let Some(id) = data.image() {
                            self.images.load(ctx, id);
                        }

                        self.peers
                            .insert((peer_id, data.id), PeerObject { peer_id, data });
                    }
                    RemoteUpdateBody::ObjectRemoved { peer_id, object_id } => {
                        if let Some(removed) = self.peers.remove(&(peer_id, object_id))
                            && let Some(id) = removed.data.image()
                        {
                            self.images.remove(id);
                        }
                    }
                }

                self.redraw()?;
                Ok(false)
            }
            Msg::UpdateResult(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(false)
            }
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::Resized => {
                self.resize_canvas();
                self.redraw()?;
                Ok(false)
            }
            Msg::ImageMessage(msg) => {
                self.images.update(msg);
                self.redraw()?;
                Ok(false)
            }
            Msg::PointerDown(ev) => {
                self.on_pointer_down(ctx, ev)?;
                Ok(true)
            }
            Msg::PointerMove(ev) => {
                self.on_pointer_move(ev)?;
                Ok(true)
            }
            Msg::PointerUp(ev) => {
                self.on_pointer_up(ev)?;
                Ok(true)
            }
            Msg::PointerLeave => {
                self.on_pointer_leave()?;
                Ok(true)
            }
            Msg::Wheel(e) => {
                self.on_wheel(e)?;
                Ok(true)
            }
            Msg::AnimationFrame => {
                self.interpolate_movement();
                self.redraw()?;
                Ok(false)
            }
            Msg::KeyDown(ev) => {
                self.on_key_down(ev)?;
                Ok(false)
            }
            Msg::KeyUp(ev) => {
                self.on_key_up(ev)?;
                Ok(false)
            }
            Msg::SelectObject(id) => {
                self.selected = id;
                self.context_menu = None;

                if id == self.delete {
                    self.delete = None;
                }

                if *self.config.mumble_follow_selection && *self.config.mumble_object != id {
                    *self.config.mumble_object = id;

                    self._toggle_mumble_request = ctx
                        .props()
                        .ws
                        .request()
                        .body(api::UpdateConfigRequest {
                            values: vec![(Key::MUMBLE_OBJECT, Value::from(id))],
                        })
                        .on_packet(ctx.link().callback(Msg::ConfigResult))
                        .send();
                }

                Ok(true)
            }
            Msg::ToggleFollowMumbleSelection => {
                *self.config.mumble_follow_selection = !*self.config.mumble_follow_selection;

                self._set_mumble_follow_selection = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateConfigRequest {
                        values: vec![(
                            Key::MUMBLE_FOLLOW_SELECTION,
                            Value::from(*self.config.mumble_follow_selection),
                        )],
                    })
                    .on_packet(ctx.link().callback(Msg::ConfigResult))
                    .send();

                Ok(true)
            }
            Msg::CreateToken => {
                self._create_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: api::Type::TOKEN,
                        props: api::Properties::from([
                            (Key::NAME, Value::from("Owlbear")),
                            (Key::HIDDEN, Value::from(true)),
                        ]),
                    })
                    .on_packet(ctx.link().callback(Msg::ObjectCreated))
                    .send();

                Ok(false)
            }
            Msg::ObjectCreated(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(true)
            }
            Msg::CreateStatic => {
                self._create_static = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: api::Type::STATIC,
                        props: api::Properties::from([
                            (Key::NAME, Value::from("Object")),
                            (Key::HIDDEN, Value::from(true)),
                            (Key::STATIC_WIDTH, Value::from(1.0_f32)),
                            (Key::STATIC_HEIGHT, Value::from(1.0_f32)),
                        ]),
                    })
                    .on_packet(ctx.link().callback(Msg::StaticObjectCreated))
                    .send();

                Ok(false)
            }
            Msg::StaticObjectCreated(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(true)
            }
            Msg::ConfirmDelete(id) => {
                self.context_menu = None;
                self.delete = Some(id);
                Ok(true)
            }
            Msg::CancelDelete => {
                self.delete = None;
                Ok(true)
            }
            Msg::DeleteObject(id) => {
                self._delete_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::DeleteObjectRequest { id })
                    .on_packet(ctx.link().callback(Msg::ObjectDeleted))
                    .send();

                self.delete = None;
                Ok(false)
            }
            Msg::ObjectDeleted(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
            Msg::ContextMenu(e) => {
                e.prevent_default();
                self.on_context_menu(ctx, e)?;
                Ok(true)
            }
            Msg::CloseContextMenu => {
                self.context_menu = None;
                Ok(true)
            }
        }
    }

    fn on_key_up(&mut self, ev: KeyboardEvent) -> Result<(), Error> {
        if ev.key() != "Shift" {
            return Ok(());
        }

        let Some((_, true)) = self.start_press else {
            return Ok(());
        };

        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
            return Ok(());
        };

        o.arrow_target = None;
        self.start_press = None;

        let object_id = o.data.id;

        if let Some(look_at) = o.data.as_look_at_mut() {
            **look_at = None;
            self.update_look_at_ids.insert(object_id);
        }

        self.redraw()?;
        Ok(())
    }

    fn on_key_down(&mut self, ev: KeyboardEvent) -> Result<(), Error> {
        if ev.key() != "Shift" || self.start_press.is_some() {
            return Ok(());
        }

        let Some(m) = self.mouse_world_pos else {
            return Ok(());
        };

        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
            return Ok(());
        };

        let object_id = o.data.id;

        if let Some(look_at) = o.data.as_look_at_mut() {
            **look_at = Some(Vec3::new(m.x, 0.0, m.z));
            self.update_look_at_ids.insert(object_id);
        }

        if let Some(transform) = o.data.as_transform() {
            let p = transform.position.xz();
            self.start_press = Some((p, true));
            self.look_at(p, m);
        }

        self.redraw()?;
        Ok(())
    }

    fn interpolate_movement(&mut self) {
        for o in self.objects.values_mut() {
            let id = o.data.id;

            let Some((transform, look_at, speed)) = o.data.as_interpolate_mut() else {
                continue;
            };

            let p = transform.position.xz();

            'move_done: {
                let (Some(target), Some(speed)) = (o.move_target, speed) else {
                    break 'move_done;
                };

                let dx = target.x - p.x;
                let dz = target.z - p.z;
                let distance = (dx * dx + dz * dz).sqrt();

                if distance < 0.01 {
                    transform.position = target.xyz(0.0);
                    o.move_target = None;
                    self.update_transform_ids.insert(id);
                    break 'move_done;
                }

                let step = speed / ANIMATION_FPS as f32;
                let move_distance = step.min(distance);
                let ratio = move_distance / distance;

                transform.position.x += dx * ratio;
                transform.position.z += dz * ratio;

                // Face the movement direction unless a look_at target is active.
                if look_at.is_none() {
                    transform.front = p.direction_to(target).xyz(0.0);
                }

                self.update_transform_ids.insert(id);
            };

            'look_done: {
                let Some(t) = look_at else {
                    break 'look_done;
                };

                o.arrow_target = Some(t.xz());
                transform.front = p.direction_to(t.xz()).xyz(0.0);
                self.update_transform_ids.insert(id);
            };
        }

        if self.objects.values().all(|o| o.move_target.is_none()) {
            self.animation_interval = None;
        }
    }

    fn send_transform_updates(&mut self, ctx: &Context<Self>) {
        if !matches!(self.state, ws::State::Open) {
            self.update_transform_ids.clear();
            return;
        }

        for id in self.update_transform_ids.drain() {
            let Some(o) = self.objects.get(&id) else {
                continue;
            };

            let Some(transform) = o.data.as_transform() else {
                continue;
            };

            let req = update(ctx, id, Key::TRANSFORM, *transform);
            self.transform_requests.insert(id, req);
        }
    }

    fn send_look_at_updates(&mut self, ctx: &Context<Self>) {
        if !matches!(self.state, ws::State::Open) {
            self.update_look_at_ids.clear();
            return;
        }

        for id in self.update_look_at_ids.drain() {
            let Some(o) = self.objects.get(&id) else {
                continue;
            };

            let Some(look_at) = o.data.look_at() else {
                continue;
            };

            let req = update(ctx, id, Key::LOOK_AT, *look_at);
            self.look_at_requests.insert(id, req);
        }
    }

    fn send_world_updates(&mut self, ctx: &Context<Self>) {
        self._update_world = ctx
            .props()
            .ws
            .request()
            .body(api::UpdateConfigRequest {
                values: self.config.world_values(),
            })
            .on_packet(ctx.link().callback(Msg::ConfigResult))
            .send();
    }

    fn on_context_menu(&mut self, _ctx: &Context<Self>, ev: MouseEvent) -> Result<(), Error> {
        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let t = ViewTransform::new(&canvas, &self.config);
        let w = t.canvas_to_world(ev.offset_x() as f64, ev.offset_y() as f64);

        let hit = self
            .objects
            .values()
            .find(|o| {
                let Some(transform) = o.data.as_transform() else {
                    return false;
                };

                transform.position.xz().dist(w) < o.data.click_radius() && !o.data.is_locked()
            })
            .map(|o| o.data.id);

        if let Some(object_id) = hit {
            self.selected = Some(object_id);
            self.context_menu = Some(ContextMenu {
                object_id,
                x: ev.offset_x() as f64,
                y: ev.offset_y() as f64,
            });
        } else {
            self.context_menu = None;
        }

        Ok(())
    }

    fn render_context_menu(&self, ctx: &Context<Self>, menu: &ContextMenu) -> Html {
        let object_id = menu.object_id;
        let style = format!("left: {}px; top: {}px;", menu.x, menu.y);

        let Some(o) = self.objects.get(&object_id) else {
            return html! {};
        };

        let is_hidden = o.data.is_hidden();
        let eye_label = if is_hidden { "Show" } else { "Hide" };
        let eye_icon = if is_hidden { "eye" } else { "eye-slash" };
        let is_mumble = *self.config.mumble_object == Some(object_id);
        let mumble_label = if is_mumble {
            "Unset MumbleLink"
        } else {
            "Set as MumbleLink"
        };

        html! {
            <div class="context-menu-backdrop" onclick={ctx.link().callback(|_| Msg::CloseContextMenu)}>
                <div class="context-menu" {style} onclick={|ev: MouseEvent| ev.stop_propagation()}>
                    <button class="context-menu-item"
                        onclick={ctx.link().callback(move |_| Msg::OpenObjectSettings(object_id))}>
                        <Icon name="cog" invert={true} />
                        {"Settings"}
                    </button>
                    <button class="context-menu-item"
                        onclick={ctx.link().callback(move |_| Msg::ToggleHidden(object_id))}>
                        <Icon name={eye_icon} invert={true} />
                        {eye_label}
                    </button>
                    <button class="context-menu-item"
                        onclick={ctx.link().callback(move |_| Msg::ToggleMumbleObject(object_id))}>
                        <Icon name="mumble" invert={true} />
                        {mumble_label}
                    </button>
                    <div class="context-menu-separator" />
                    <button class="context-menu-item danger"
                        onclick={ctx.link().callback(move |_| Msg::ConfirmDelete(object_id))}>
                        <Icon name="x-mark" invert={true} />
                        {"Delete"}
                    </button>
                </div>
            </div>
        }
    }

    fn on_pointer_down(&mut self, ctx: &Context<Self>, ev: PointerEvent) -> Result<(), Error> {
        ev.prevent_default();

        self.context_menu = None;

        let needs_redraw = 'out: {
            match ev.button() {
                LEFT_MOUSE_BUTTON => {
                    let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
                        break 'out false;
                    };

                    let t = ViewTransform::new(&canvas, &self.config);
                    let e = t.canvas_to_world(ev.offset_x() as f64, ev.offset_y() as f64);

                    let mut hit_something = false;

                    if !ev.shift_key() {
                        let hit = self
                            .objects
                            .values()
                            .find(|o| {
                                let Some(transform) = o.data.as_transform() else {
                                    return false;
                                };

                                transform.position.xz().dist(e) < o.data.click_radius()
                                    && !o.data.is_locked()
                            })
                            .map(|o| o.data.id);

                        if let Some(hit_id) = hit
                            && self.selected != Some(hit_id)
                        {
                            ctx.link().send_message(Msg::SelectObject(Some(hit_id)));
                            self.delete = None;
                            break 'out true;
                        }

                        hit_something = hit.is_some();
                    }

                    let Some(object) = self.selected.and_then(|id| self.objects.get_mut(&id))
                    else {
                        break 'out hit_something;
                    };

                    object.arrow_target = None;

                    let object_id = object.data.id;
                    let is_static = object.data.is_static();

                    let Some(transform) = object.data.as_transform_mut() else {
                        break 'out hit_something;
                    };

                    if ev.shift_key() {
                        let p = transform.position.xz();

                        self.start_press = Some((p, true));

                        if is_static {
                            // Shift-drag on a static object rotates it.
                            self.look_at(p, e);
                        } else if let Some(look_at) = object.data.as_look_at_mut() {
                            **look_at = Some(e.xyz(0.0));
                            self.look_at(p, e);
                            self.update_look_at_ids.insert(object_id);
                        }
                    } else if is_static {
                        // Static objects snap immediately to where they are dropped.
                        transform.position = e.xyz(0.0);
                        self.start_press = Some((e, false));
                        self.update_transform_ids.insert(object_id);
                    } else {
                        self.start_press = Some((e, false));
                        object.move_target = Some(e);
                    }

                    true
                }
                MIDDLE_MOUSE_BUTTON => {
                    ev.prevent_default();
                    self.pan_anchor = Some((ev.client_x() as f64, ev.client_y() as f64));
                    true
                }
                _ => false,
            }
        };

        if needs_redraw {
            self.redraw()?;
        }

        Ok(())
    }

    fn on_pointer_move(&mut self, ev: PointerEvent) -> Result<(), Error> {
        ev.prevent_default();

        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let v = ViewTransform::new(&canvas, &self.config);

        let mut needs_redraw = false;

        if let Some((ax, ay)) = self.pan_anchor {
            let dx = ev.client_x() as f64 - ax;
            let dy = ev.client_y() as f64 - ay;
            *self.config.pan = self.config.pan.add(dx, dy);
            self.pan_anchor = Some((ev.client_x() as f64, ev.client_y() as f64));
            self.update_world = true;
            needs_redraw = true;
        }

        let m = v.canvas_to_world(ev.offset_x() as f64, ev.offset_y() as f64);
        self.mouse_world_pos = Some(m);

        'done: {
            let Some((p, shift_key)) = self.start_press else {
                break 'done;
            };

            let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
                break 'done;
            };

            if shift_key {
                let dist = p.dist(m);

                if dist < ARROW_THRESHOLD {
                    break 'done;
                };

                if o.data.is_static() {
                    self.look_at(p, m);
                } else if let Some(look_at) = o.data.as_look_at_mut() {
                    **look_at = Some(Vec3::new(m.x, 0.0, m.z));
                    self.update_look_at_ids.insert(o.data.id);
                    self.look_at(p, m);
                }

                needs_redraw = true;
                break 'done;
            }

            if o.data.is_static()
                && let Some(transform) = o.data.as_transform_mut()
            {
                // Static objects snap immediately while dragging.
                transform.position = m.xyz(0.0);
                self.update_transform_ids.insert(o.data.id);
                needs_redraw = true;
                break 'done;
            }

            o.move_target = Some(m);
            needs_redraw = true;
        }

        if needs_redraw {
            self.redraw()?;
        }

        Ok(())
    }

    fn look_at(&mut self, p: VecXZ, m: VecXZ) {
        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
            return;
        };

        let Some(transform) = o.data.as_transform_mut() else {
            return;
        };

        o.arrow_target = Some(m);
        transform.front = p.direction_to(m).xyz(0.0);
        self.update_transform_ids.insert(o.data.id);
    }

    fn on_pointer_up(&mut self, ev: PointerEvent) -> Result<(), Error> {
        let needs_redraw = {
            match ev.button() {
                LEFT_MOUSE_BUTTON => {
                    self.start_press = None;

                    if let Some(object) = self.selected.and_then(|id| self.objects.get_mut(&id)) {
                        object.arrow_target = None;
                    }

                    true
                }
                MIDDLE_MOUSE_BUTTON => {
                    self.pan_anchor = None;
                    false
                }
                _ => false,
            }
        };

        if needs_redraw {
            self.redraw()?;
        }

        Ok(())
    }

    fn on_pointer_leave(&mut self) -> Result<(), Error> {
        let selected_arrow = self
            .selected
            .and_then(|id| self.objects.get(&id))
            .and_then(|o| o.arrow_target);

        let needs_redraw = selected_arrow.is_some() || self.start_press.is_some();

        self.pan_anchor = None;
        self.start_press = None;
        self.mouse_world_pos = None;

        if let Some(object) = self.selected.and_then(|id| self.objects.get_mut(&id)) {
            object.arrow_target = None;
        }

        if needs_redraw {
            self.redraw()?;
        }

        Ok(())
    }

    fn on_wheel(&mut self, ev: WheelEvent) -> Result<(), Error> {
        ev.prevent_default();

        let delta = if ev.delta_y() < 0.0 {
            ZOOM_FACTOR
        } else {
            1.0 / ZOOM_FACTOR
        } as f32;

        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let mx = ev.offset_x() as f64;
        let my = ev.offset_y() as f64;

        let t_before = ViewTransform::new(&canvas, &self.config);
        let w = t_before.canvas_to_world(mx, my);

        *self.config.zoom = (*self.config.zoom * delta).clamp(0.1, 20.0);

        let t_after = ViewTransform::new(&canvas, &self.config);
        let c2 = t_after.world_to_canvas(w.x, w.z);
        self.config.pan.x += mx - c2.x;
        self.config.pan.y += my - c2.y;

        self.update_world = true;
        self.redraw()?;
        Ok(())
    }

    /// Resize the canvas according to its parent sizer element.
    fn load_initialize_images(&mut self, ctx: &Context<Self>) {
        self.images.clear();

        let ids = self.objects.values().filter_map(|o| o.data.image());
        let ids = ids.chain(self.peers.values().filter_map(|a| a.data.image()));

        for id in ids {
            self.images.load(ctx, id);
        }
    }

    fn resize_canvas(&self) {
        let Some(sizer) = self.canvas_sizer.cast::<HtmlElement>() else {
            return;
        };

        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return;
        };

        // The sizer element is a block element, and has a width we want to
        // adjust to.
        let width = sizer.client_width() as u32;

        canvas.set_width(width);
        canvas.set_height(width);
    }

    fn setup_resizer(&mut self, ctx: &Context<Self>) -> Result<(), Error> {
        let sizer = self
            .canvas_sizer
            .cast::<HtmlElement>()
            .ok_or("missing canvas sizer element")?;

        let link = ctx.link().clone();
        let closure = Closure::<dyn FnMut()>::new(move || {
            link.send_message(Msg::Resized);
        });

        let observer = ResizeObserver::new(closure.as_ref().unchecked_ref())?;

        observer.observe(&sizer);

        if let Some((o, _closure)) = self._resize_observer.replace((observer, closure)) {
            o.disconnect();
            drop(_closure);
        }

        Ok(())
    }

    fn redraw(&self) -> Result<(), Error> {
        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let Some(cx) = canvas.get_context("2d")? else {
            return Ok(());
        };

        let Ok(cx) = cx.dyn_into::<CanvasRenderingContext2d>() else {
            return Ok(());
        };

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        let t = ViewTransform::new(&canvas, &self.config);

        render::draw_grid(&cx, &t, &self.config.extent, *self.config.zoom);

        let selected = self.selected;

        // Draw static objects first (behind tokens).
        for o in self.objects.values() {
            if let Some(mut s) = RenderStatic::from_data(&o.data) {
                s.selected = selected == Some(o.data.id);
                render::draw_static_token(&cx, &t, &s, |id| self.images.get(id).cloned())?;
            }
        }
        // Draw remote static objects.
        for o in self.peers.values() {
            if let Some(s) = RenderStatic::from_data(&o.data)
                && !s.hidden
            {
                render::draw_static_token(&cx, &t, &s, |id| self.images.get(id).cloned())?;
            }
        }

        let renders = || {
            let remotes = self
                .peers
                .values()
                .flat_map(|peer| RenderToken::from_data(&peer.data))
                .filter(|render| !render.hidden);

            let locals = self.order.iter().flat_map(move |(_, id)| {
                let data = &self.objects.get(id)?.data;
                let mut token = RenderToken::from_data(data)?;
                token.player = true;
                token.selected = selected == Some(data.id);
                Some(token)
            });

            remotes.chain(locals)
        };

        let selected_arrow = self
            .selected
            .and_then(|id| self.objects.get(&id))
            .and_then(|o| o.arrow_target);

        for token in renders() {
            let arrow = token.selected.then_some(selected_arrow).flatten();
            render::draw_token_token(&cx, &t, &token, arrow, |id| self.images.get(id).cloned())?;
        }

        for token in renders() {
            if let Some(target) = token.look_at {
                render::draw_look_at(&cx, &t, target, &token.color, *self.config.zoom as f64)?;
            }
        }

        Ok(())
    }
}

fn update(ctx: &Context<Map>, object_id: Id, key: Key, value: impl Into<Value>) -> ws::Request {
    ctx.props()
        .ws
        .request()
        .body(api::UpdateRequest {
            object_id,
            key,
            value: value.into(),
        })
        .on_packet(ctx.link().callback(Msg::UpdateResult))
        .send()
}
