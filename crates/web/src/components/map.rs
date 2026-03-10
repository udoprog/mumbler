use std::collections::{BTreeMap, HashMap, HashSet};

use api::{
    Color, Extent, Id, Key, LocalUpdateBody, Pan, PeerId, RemoteObject, RemotePeerObject,
    Transform, Value, Vec3,
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

use super::render::{self, RenderAvatar, RenderStatic, ViewTransform};
use super::{ObjectSettings, StaticSettings};

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
    fn from_config(properties: &api::Properties) -> Self {
        let mut this = Self::default();

        for (key, value) in properties.iter() {
            this.update(key, value);
        }

        this
    }

    fn update(&mut self, key: Key, value: &Value) -> bool {
        match key {
            Key::WORLD_SCALE => self.zoom.update(value.as_float().unwrap_or(2.0)),
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
            zoom: State::new(1.0),
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
    pub(crate) move_target: Option<Vec3>,
    pub(crate) arrow_target: Option<(f32, f32)>,
}

impl LocalObject {
    fn from_remote(remote: &RemoteObject) -> Self {
        Self {
            data: ObjectData::from_remote(remote),
            move_target: None,
            arrow_target: None,
        }
    }

    fn image(&self) -> Option<Id> {
        self.data.image()
    }
}

pub(crate) struct Avatar {
    pub(crate) look_at: State<Option<Vec3>>,
    pub(crate) image: State<Option<Id>>,
    pub(crate) color: State<Option<Color>>,
    pub(crate) name: State<Option<String>>,
    pub(crate) hidden: State<bool>,
    pub(crate) token_radius: State<f32>,
    pub(crate) speed: State<f32>,
}

impl Avatar {
    fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::LOOK_AT => self.look_at.update(value.as_vec3()),
            Key::IMAGE_ID => self.image.update(value.as_id()),
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.into_string()),
            Key::HIDDEN => self.hidden.update(value.as_bool().unwrap_or(false)),
            Key::TOKEN_RADIUS => self
                .token_radius
                .update(value.as_float().unwrap_or(DEFAULT_TOKEN_RADIUS)),
            Key::SPEED => self.speed.update(value.as_float().unwrap_or(DEFAULT_SPEED)),
            _ => false,
        }
    }
}

pub(crate) struct StaticObject {
    pub(crate) image: State<Option<Id>>,
    pub(crate) color: State<Option<Color>>,
    pub(crate) name: State<Option<String>>,
    pub(crate) hidden: State<bool>,
    pub(crate) width: State<f32>,
    pub(crate) height: State<f32>,
}

impl StaticObject {
    fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::IMAGE_ID => self.image.update(value.as_id()),
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.into_string()),
            Key::HIDDEN => self.hidden.update(value.as_bool().unwrap_or(false)),
            Key::STATIC_WIDTH => self
                .width
                .update(value.as_float().unwrap_or(DEFAULT_STATIC_WIDTH)),
            Key::STATIC_HEIGHT => self
                .height
                .update(value.as_float().unwrap_or(DEFAULT_STATIC_HEIGHT)),
            _ => false,
        }
    }
}

pub(crate) enum ObjectKind {
    Avatar(Avatar),
    Static(StaticObject),
    Unknown,
}

pub(crate) struct ObjectData {
    pub(crate) id: Id,
    pub(crate) transform: State<Transform>,
    pub(crate) kind: ObjectKind,
}

impl ObjectData {
    fn from_remote(remote: &RemoteObject) -> Self {
        let kind = match remote.ty {
            api::Type::AVATAR => {
                let avatar = Avatar {
                    look_at: State::new(remote.properties.get(Key::LOOK_AT).as_vec3()),
                    image: State::new(remote.properties.get(Key::IMAGE_ID).as_id()),
                    color: State::new(remote.properties.get(Key::COLOR).as_color()),
                    name: State::new(
                        remote
                            .properties
                            .get(Key::NAME)
                            .as_string()
                            .map(str::to_owned),
                    ),
                    hidden: State::new(
                        remote
                            .properties
                            .get(Key::HIDDEN)
                            .as_bool()
                            .unwrap_or(false),
                    ),
                    token_radius: State::new(
                        remote
                            .properties
                            .get(Key::TOKEN_RADIUS)
                            .as_float()
                            .unwrap_or(DEFAULT_TOKEN_RADIUS),
                    ),
                    speed: State::new(
                        remote
                            .properties
                            .get(Key::SPEED)
                            .as_float()
                            .unwrap_or(DEFAULT_SPEED),
                    ),
                };

                ObjectKind::Avatar(avatar)
            }
            api::Type::STATIC => {
                let s = StaticObject {
                    image: State::new(remote.properties.get(Key::IMAGE_ID).as_id()),
                    color: State::new(remote.properties.get(Key::COLOR).as_color()),
                    name: State::new(
                        remote
                            .properties
                            .get(Key::NAME)
                            .as_string()
                            .map(str::to_owned),
                    ),
                    hidden: State::new(
                        remote
                            .properties
                            .get(Key::HIDDEN)
                            .as_bool()
                            .unwrap_or(false),
                    ),
                    width: State::new(
                        remote
                            .properties
                            .get(Key::STATIC_WIDTH)
                            .as_float()
                            .unwrap_or(DEFAULT_STATIC_WIDTH),
                    ),
                    height: State::new(
                        remote
                            .properties
                            .get(Key::STATIC_HEIGHT)
                            .as_float()
                            .unwrap_or(DEFAULT_STATIC_HEIGHT),
                    ),
                };

                ObjectKind::Static(s)
            }
            _ => ObjectKind::Unknown,
        };

        Self {
            id: remote.id,
            transform: State::new(
                remote
                    .properties
                    .get(Key::TRANSFORM)
                    .as_transform()
                    .unwrap_or_else(Transform::origin),
            ),
            kind,
        }
    }

    fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::TRANSFORM => self
                .transform
                .update(value.as_transform().unwrap_or_else(Transform::origin)),
            key => match &mut self.kind {
                ObjectKind::Avatar(avatar) => avatar.update(key, value),
                ObjectKind::Static(s) => s.update(key, value),
                ObjectKind::Unknown => false,
            },
        }
    }

    #[inline]
    fn click_radius(&self) -> f32 {
        match &self.kind {
            ObjectKind::Avatar(avatar) => *avatar.token_radius,
            ObjectKind::Static(s) => (*s.width).hypot(*s.height) / 2.0,
            ObjectKind::Unknown => 0.5,
        }
    }

    /// Returns `true` if this is a static object (rectangle, snap movement).
    #[inline]
    fn is_static(&self) -> bool {
        matches!(&self.kind, ObjectKind::Static(_))
    }

    #[inline]
    fn look_at(&self) -> Option<&Option<Vec3>> {
        match &self.kind {
            ObjectKind::Avatar(avatar) => Some(&*avatar.look_at),
            ObjectKind::Static(_) | ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_look_at_mut(&mut self) -> Option<&mut State<Option<Vec3>>> {
        match &mut self.kind {
            ObjectKind::Avatar(avatar) => Some(&mut avatar.look_at),
            ObjectKind::Static(_) | ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_image_mut(&mut self) -> Option<&mut State<Option<Id>>> {
        match &mut self.kind {
            ObjectKind::Avatar(avatar) => Some(&mut avatar.image),
            ObjectKind::Static(s) => Some(&mut s.image),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn as_hidden_mut(&mut self) -> Option<&mut State<bool>> {
        match &mut self.kind {
            ObjectKind::Avatar(avatar) => Some(&mut avatar.hidden),
            ObjectKind::Static(s) => Some(&mut s.hidden),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn speed(&self) -> Option<f32> {
        match &self.kind {
            ObjectKind::Avatar(avatar) => Some(*avatar.speed),
            ObjectKind::Static(_) | ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn name(&self) -> Option<&str> {
        match &self.kind {
            ObjectKind::Avatar(avatar) => avatar.name.as_deref(),
            ObjectKind::Static(s) => s.name.as_deref(),
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn image(&self) -> Option<Id> {
        match &self.kind {
            ObjectKind::Avatar(avatar) => *avatar.image,
            ObjectKind::Static(s) => *s.image,
            ObjectKind::Unknown => None,
        }
    }

    #[inline]
    fn is_hidden(&self) -> bool {
        match &self.kind {
            ObjectKind::Avatar(avatar) => *avatar.hidden,
            ObjectKind::Static(s) => *s.hidden,
            ObjectKind::Unknown => false,
        }
    }
}

pub(crate) struct Map {
    _initialize: ws::Request,
    /// Per-object in-flight transform update requests.
    transform_requests: HashMap<Id, ws::Request>,
    /// Per-object in-flight look-at update requests.
    look_at_requests: HashMap<Id, ws::Request>,
    _update_world: ws::Request,
    _state_change: ws::StateListener,
    _local_update_listener: ws::Listener,
    _remote_update_listener: ws::Listener,
    state: ws::State,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    canvas_sizer: NodeRef,
    canvas_ref: NodeRef,
    /// World configuration.
    config: Config,
    /// Object IDs whose transforms need to be sent to the server.
    update_transform_ids: HashSet<Id>,
    /// Object IDs whose look-at needs to be sent to the server.
    update_look_at_ids: HashSet<Id>,
    /// Whether world settings need to be updated.
    update_world: bool,
    /// The selected object.
    selected: Option<Id>,
    /// The list of local objects.
    objects: BTreeMap<Id, LocalObject>,
    /// The list of remote objects.
    peers: BTreeMap<(PeerId, Id), PeerObject>,
    /// Mouse position at the start of a middle-mouse drag.
    pan_anchor: Option<(f64, f64)>,
    /// Canvas-space position where the left button was pressed.
    start_press: Option<(f32, f32, bool)>,
    /// Keeps the ResizeObserver and its closure alive for the component's lifetime.
    _resize_observer: Option<(ResizeObserver, Closure<dyn FnMut()>)>,
    /// Loaded avatar images, keyed by image id.
    /// The closure must be kept alive alongside the element for the onload callback to fire.
    images: Images<Self>,
    /// Animation interval for smooth movement.
    animation_interval: Option<Interval>,
    /// Current world-space mouse position while cursor is over the canvas.
    mouse_world_pos: Option<(f32, f32)>,
    /// Keeps the document keydown listener alive.
    _keydown_listener: EventListener,
    /// Keeps the document keyup listener alive.
    _keyup_listener: EventListener,
    /// In-flight create-object request.
    _create_object: ws::Request,
    /// In-flight create-static-object request.
    _create_static_object: ws::Request,
    /// In-flight delete-object request.
    _delete_object: ws::Request,
    /// Index of the object pending delete confirmation.
    delete: HashSet<Id>,
    /// Object whose settings modal is currently open.
    open_settings: Option<Id>,
    _set_mumble_object: ws::Request,
    _set_mumble_follow_selection: ws::Request,
    /// In-flight visibility toggle requests, per object.
    hide_requests: HashMap<Id, ws::Request>,
    /// Position and target of the right-click context menu, if visible.
    context_menu: Option<ContextMenu>,
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
    KeyDown(KeyboardEvent),
    KeyUp(KeyboardEvent),
    InitializeMap(Result<Packet<api::InitializeMap>, ws::Error>),
    ConfigUpdate(Result<Packet<api::ConfigUpdate>, ws::Error>),
    LocalUpdate(Result<Packet<api::LocalUpdate>, ws::Error>),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    UpdateResult(Result<Packet<api::Update>, ws::Error>),
    WorldUpdated(Result<Packet<api::UpdateConfig>, ws::Error>),
    ObjectCreated(Result<Packet<api::CreateObject>, ws::Error>),
    StaticObjectCreated(Result<Packet<api::CreateObject>, ws::Error>),
    StateChanged(ws::State),
    Resized,
    ImageMessage(ImageMessage),
    MouseDown(MouseEvent),
    MouseMove(MouseEvent),
    MouseUp(MouseEvent),
    MouseLeave,
    Wheel(WheelEvent),
    AnimationFrame,
    SelectObject(Option<Id>),
    CreateObject,
    CreateStaticObject,
    ConfirmDelete(Id),
    CancelDelete,
    DeleteObject(Id),
    ObjectDeleted(Result<Packet<api::DeleteObject>, ws::Error>),
    OpenObjectSettings(Id),
    CloseObjectSettings,
    ToggleMumbleObject(Id),
    SetConfig(Result<Packet<api::UpdateConfig>, ws::Error>),
    ToggleFollowMumbleSelection,
    ToggleHidden(Id),
    ToggleHiddenResult(Id, Result<Packet<api::Update>, ws::Error>),
    SetLog(log::Log),
    ContextMenu(MouseEvent),
    CloseContextMenu,
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
            _initialize: ws::Request::new(),
            transform_requests: HashMap::new(),
            look_at_requests: HashMap::new(),
            _update_world: ws::Request::new(),
            _local_update_listener,
            _remote_update_listener,
            _state_change,
            state,
            log,
            _log_handle,
            canvas_sizer: NodeRef::default(),
            canvas_ref: NodeRef::default(),
            config: Config::default(),
            selected: None,
            objects: BTreeMap::new(),
            peers: BTreeMap::new(),
            update_transform_ids: HashSet::new(),
            update_look_at_ids: HashSet::new(),
            update_world: false,
            pan_anchor: None,
            start_press: None,
            _resize_observer: None,
            images: Images::new(),
            animation_interval: None,
            mouse_world_pos: None,
            _keydown_listener,
            _keyup_listener,
            _create_object: ws::Request::new(),
            _create_static_object: ws::Request::new(),
            _delete_object: ws::Request::new(),
            delete: HashSet::new(),
            open_settings: None,
            _set_mumble_object: ws::Request::new(),
            _set_mumble_follow_selection: ws::Request::new(),
            hide_requests: HashMap::new(),
            context_menu: None,
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

        if let Some(o) = self.selected.and_then(|id| self.objects.get(&id)) {
            let p = o.data.transform.position;
            let f = o.data.transform.front;
            let zoom = *self.config.zoom;

            let position = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", p.x, p.y, p.z);
            let front = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", f.x, f.y, f.z);
            let other = format!("ZOOM:{:.02}", zoom);
            pos = Some(html!(<div class="pre">{position}{" / "}{front}{" / "}{other}</div>))
        } else {
            pos = None;
        }

        let object_list_header = {
            html! {
                <div class="control-group">
                    <button class="btn square primary" title="Add avatar" onclick={ctx.link().callback(|_| Msg::CreateObject)}>
                        <Icon name="user-plus" title="Add avatar" />
                    </button>
                    <button class="btn square" title="Add static object" onclick={ctx.link().callback(|_| Msg::CreateStaticObject)}>
                        <Icon name="square-2-stack" title="Add static" />
                    </button>
                </div>
            }
        };

        let toolbar = {
            let o = self.selected.and_then(|id| self.objects.get(&id));

            let is_hidden = o.map(|o| o.data.is_hidden()).unwrap_or_default();
            let is_mumble = o
                .map(|o| *self.config.mumble_object == Some(o.data.id))
                .unwrap_or_default();

            let eye_icon = if is_hidden { "eye-slash" } else { "eye" };
            let eye_title = if is_hidden {
                "Hidden from others"
            } else {
                "Visible to others"
            };

            let follow_classes = classes! {
                "btn", "square",
                self.config.mumble_follow_selection.then_some("success"),
            };

            let follow_title = if *self.config.mumble_follow_selection {
                "Disable MumbleLink selection following"
            } else {
                "Enable MumbleLink selection following"
            };

            let settings_classes = classes! {
                "btn",
                "square",
                o.is_some().then_some("primary"),
                o.is_none().then_some("disabled"),
            };

            let delete_classes = classes! {
                "btn",
                "square",
                "right",
                o.is_some().then_some("danger"),
                o.is_none().then_some("disabled"),
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

            let settings_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::OpenObjectSettings(id))
            });

            let delete_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::ConfirmDelete(id))
            });

            let mumble_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::ToggleMumbleObject(id))
            });

            let hidden_click = o.map(|o| {
                let id = o.data.id;
                ctx.link().callback(move |_| Msg::ToggleHidden(id))
            });

            html! {
                <div class="control-group">
                    <button class={settings_classes} title="Object settings" onclick={settings_click}>
                        <Icon name="cog" />
                    </button>
                    <button class={mumble_classes} title="Toggle as MumbleLink Source" onclick={mumble_click}>
                        <Icon name="mumble" />
                    </button>
                    <button class={follow_classes} title={follow_title} onclick={ctx.link().callback(|_| Msg::ToggleFollowMumbleSelection)}>
                        <Icon name="cursor-arrow-rays" />
                    </button>
                    <button class={hidden_classes} title={eye_title} onclick={hidden_click}>
                        <Icon name={eye_icon} />
                    </button>
                    <button class={delete_classes} title="Delete object" onclick={delete_click}>
                        <Icon name="x-mark" />
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
                            onmousedown={ctx.link().callback(Msg::MouseDown)}
                            onmousemove={ctx.link().callback(Msg::MouseMove)}
                            onmouseup={ctx.link().callback(Msg::MouseUp)}
                            onmouseleave={ctx.link().callback(|_| Msg::MouseLeave)}
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
                        {for self.objects.values().map(|o| {
                            let object_id = o.data.id;
                            let selected = self.selected == Some(object_id);
                            let on_click = ctx.link().callback(move |_| Msg::SelectObject(Some(object_id)));
                            let classes = classes!("object-list-item", selected.then_some("selected"));
                            let label = o.data.name().unwrap_or("");

                            let is_hidden = o.data.is_hidden();
                            let eye_icon = if is_hidden { "eye-slash" } else { "eye" };
                            let eye_title = if is_hidden { "Hidden from others" } else { "Visible to others" };
                            let is_mumble = *self.config.mumble_object == Some(object_id);

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

                            html! {
                                <div class={classes} onclick={on_click}>
                                    <span class="object-list-item-label">{label}</span>
                                    <button class={mumble_classes}
                                        title="Toggle as MumbleLink Source"
                                        onclick={ctx.link().callback(move |e: MouseEvent| {
                                            e.stop_propagation();
                                            Msg::ToggleMumbleObject(object_id)
                                        })}>
                                        <Icon name="mumble" />
                                    </button>
                                    <button class={hidden_classes}
                                        title={eye_title}
                                        onclick={ctx.link().callback(move |e: MouseEvent| {
                                            e.stop_propagation();
                                            Msg::ToggleHidden(object_id)
                                        })}>
                                        <Icon name={eye_icon} />
                                    </button>
                                </div>
                            }
                        })}
                    </div>
                </div>
            </div>

            if let Some(id) = self.delete.iter().next().copied() {
                <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                    <div class="modal" onclick={|e: MouseEvent| e.stop_propagation()}>
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
                    <div class="modal" onclick={|e: MouseEvent| e.stop_propagation()}>
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

                self._set_mumble_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateConfigRequest {
                        values: vec![(Key::MUMBLE_OBJECT, Value::from(update))],
                    })
                    .on_packet(ctx.link().callback(Msg::SetConfig))
                    .send();

                Ok(true)
            }
            Msg::SetConfig(result) => {
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

                self.config = Config::from_config(&body.config);

                self.objects = body
                    .objects
                    .iter()
                    .map(LocalObject::from_remote)
                    .map(|o| (o.data.id, (o)))
                    .collect();

                self.peers = body
                    .remote_avatars
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

                let changed = self.config.update(body.key, &body.value);

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
                        self.objects
                            .insert(object.id, LocalObject::from_remote(&object));

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
                            let Some(a) = self.objects.get_mut(&object_id) else {
                                break 'done false;
                            };

                            match key {
                                // Don't support local updates of transform and
                                // look at because they cause feedback loops
                                // which are laggy.
                                Key::TRANSFORM | Key::LOOK_AT => {
                                    break 'done false;
                                }
                                Key::IMAGE_ID => {
                                    let new = value.as_id();

                                    let Some(image) = a.data.as_image_mut() else {
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
                                }
                                _ => {}
                            }

                            a.data.update(key, value)
                        }
                    }
                };

                self.redraw()?;
                Ok(update)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                tracing::debug!(?body, "Remote avatar update");

                match body {
                    api::RemoteUpdateBody::RemoteLost => {
                        self.peers.clear();
                    }
                    api::RemoteUpdateBody::Join {
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
                    api::RemoteUpdateBody::Leave { peer_id } => {
                        self.peers.retain(|&(pid, _), _| pid != peer_id);
                    }
                    api::RemoteUpdateBody::Update {
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
                    api::RemoteUpdateBody::ObjectAdded { peer_id, object } => {
                        let data = ObjectData::from_remote(&object);

                        if let Some(id) = data.image() {
                            self.images.load(ctx, id);
                        }

                        self.peers
                            .insert((peer_id, data.id), PeerObject { peer_id, data });
                    }
                    api::RemoteUpdateBody::ObjectRemoved { peer_id, object_id } => {
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
            Msg::WorldUpdated(body) => {
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
            Msg::MouseDown(e) => {
                self.on_mouse_down(ctx, e)?;
                Ok(true)
            }
            Msg::MouseMove(e) => {
                self.on_mouse_move(e)?;
                Ok(true)
            }
            Msg::MouseUp(e) => {
                self.on_mouse_up(e)?;
                Ok(true)
            }
            Msg::MouseLeave => {
                self.on_mouse_leave()?;
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
            Msg::KeyDown(e) => {
                self.on_key_down(e)?;
                Ok(false)
            }
            Msg::KeyUp(e) => {
                self.on_key_up(e)?;
                Ok(false)
            }
            Msg::SelectObject(id) => {
                self.selected = id;
                self.context_menu = None;

                if let Some(id) = id {
                    self.delete.remove(&id);
                }

                if *self.config.mumble_follow_selection && *self.config.mumble_object != id {
                    *self.config.mumble_object = id;

                    self._set_mumble_object = ctx
                        .props()
                        .ws
                        .request()
                        .body(api::UpdateConfigRequest {
                            values: vec![(Key::MUMBLE_OBJECT, Value::from(id))],
                        })
                        .on_packet(ctx.link().callback(Msg::SetConfig))
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
                    .on_packet(ctx.link().callback(Msg::SetConfig))
                    .send();

                Ok(true)
            }
            Msg::CreateObject => {
                self._create_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: api::Type::AVATAR,
                        properties: api::Properties::from([
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
            Msg::CreateStaticObject => {
                self._create_static_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: api::Type::STATIC,
                        properties: api::Properties::from([
                            (Key::NAME, Value::from("Object")),
                            (Key::HIDDEN, Value::from(false)),
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
                self.delete.clear();
                self.delete.insert(id);
                Ok(true)
            }
            Msg::CancelDelete => {
                self.delete.clear();
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

                self.delete.remove(&id);
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

    fn on_key_up(&mut self, e: KeyboardEvent) -> Result<(), Error> {
        if e.key() != "Shift" {
            return Ok(());
        }

        let Some((_, _, true)) = self.start_press else {
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

    fn on_key_down(&mut self, e: KeyboardEvent) -> Result<(), Error> {
        if e.key() != "Shift" || self.start_press.is_some() {
            return Ok(());
        }

        let Some((mx, my)) = self.mouse_world_pos else {
            return Ok(());
        };

        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
            return Ok(());
        };

        let object_id = o.data.id;

        if let Some(look_at) = o.data.as_look_at_mut() {
            **look_at = Some(Vec3::new(mx, 0.0, my));
            self.update_look_at_ids.insert(object_id);
        }

        let (px, py) = (o.data.transform.position.x, o.data.transform.position.z);
        self.start_press = Some((px, py, true));
        self.look_at(px, py, mx, my);
        self.redraw()?;
        Ok(())
    }

    fn interpolate_movement(&mut self) {
        for o in self.objects.values_mut() {
            let p = o.data.transform.position;

            'move_done: {
                let (Some(target), Some(speed)) = (o.move_target, o.data.speed()) else {
                    break 'move_done;
                };

                let dx = target.x - p.x;
                let dz = target.z - p.z;
                let distance = (dx * dx + dz * dz).sqrt();

                if distance < 0.01 {
                    o.data.transform.position = target;
                    o.move_target = None;
                    self.update_transform_ids.insert(o.data.id);
                    break 'move_done;
                }

                let step = speed / ANIMATION_FPS as f32;
                let move_distance = step.min(distance);
                let ratio = move_distance / distance;

                o.data.transform.position.x += dx * ratio;
                o.data.transform.position.z += dz * ratio;

                // Face the movement direction unless a look_at target is active.
                if o.data.look_at().is_none() {
                    let angle_rad = dz.atan2(dx);
                    o.data.transform.front = Vec3::new(angle_rad.cos(), 0.0, angle_rad.sin());
                }

                self.update_transform_ids.insert(o.data.id);
            };

            'look_done: {
                let Some(target) = o.data.look_at().and_then(|look_at| *look_at) else {
                    break 'look_done;
                };

                let angle_radian = (target.z - p.z).atan2(target.x - p.x);

                o.arrow_target = Some((target.x, target.z));
                o.data.transform.front = Vec3::new(angle_radian.cos(), 0.0, angle_radian.sin());

                self.update_transform_ids.insert(o.data.id);
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

            let req = send_update(ctx, id, Key::TRANSFORM, *o.data.transform);
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

            let req = send_update(ctx, id, Key::LOOK_AT, *look_at);
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
            .on_packet(ctx.link().callback(Msg::WorldUpdated))
            .send();
    }

    fn on_context_menu(&mut self, _ctx: &Context<Self>, e: MouseEvent) -> Result<(), Error> {
        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let t = ViewTransform::new(&canvas, &self.config);
        let (wx, wz) = t.canvas_to_world(e.offset_x() as f64, e.offset_y() as f64);

        let hit = self
            .objects
            .values()
            .find(|o| {
                let p = o.data.transform.position;
                let r = o.data.click_radius();
                let dx = p.x - wx;
                let dz = p.z - wz;
                dx * dx + dz * dz <= r * r
            })
            .map(|o| o.data.id);

        if let Some(object_id) = hit {
            self.selected = Some(object_id);
            self.context_menu = Some(ContextMenu {
                object_id,
                x: e.offset_x() as f64,
                y: e.offset_y() as f64,
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
                <div class="context-menu" {style} onclick={|e: MouseEvent| e.stop_propagation()}>
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

    fn on_mouse_down(&mut self, ctx: &Context<Self>, e: MouseEvent) -> Result<(), Error> {
        self.context_menu = None;

        let needs_redraw = 'out: {
            match e.button() {
                0 => {
                    let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
                        break 'out false;
                    };

                    let t = ViewTransform::new(&canvas, &self.config);
                    let (ex, ey) = (e.offset_x() as f64, e.offset_y() as f64);
                    let (ex, ey) = t.canvas_to_world(ex, ey);

                    let hit = self
                        .objects
                        .values()
                        .find(|o| {
                            let p = o.data.transform.position;
                            let r = o.data.click_radius();
                            let dx = p.x - ex;
                            let dz = p.z - ey;
                            dx * dx + dz * dz <= r * r
                        })
                        .map(|o| o.data.id);

                    if let Some(hit_id) = hit
                        && self.selected != Some(hit_id)
                    {
                        ctx.link().send_message(Msg::SelectObject(Some(hit_id)));
                        self.delete.remove(&hit_id);
                        break 'out true;
                    }

                    let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
                        break 'out hit.is_some();
                    };

                    o.arrow_target = None;

                    let object_id = o.data.id;

                    if e.shift_key() {
                        let (px, py) = (o.data.transform.position.x, o.data.transform.position.z);

                        self.start_press = Some((px, py, true));

                        if o.data.is_static() {
                            // Shift-drag on a static object rotates it.
                            self.look_at(px, py, ex, ey);
                        } else if let Some(look_at) = o.data.as_look_at_mut() {
                            **look_at = Some(Vec3::new(ex, 0.0, ey));
                            self.look_at(px, py, ex, ey);
                            self.update_look_at_ids.insert(object_id);
                        }
                    } else if o.data.is_static() {
                        // Static objects snap immediately to where they are dropped.
                        self.start_press = Some((ex, ey, false));
                        o.data.transform.position = Vec3::new(ex, 0.0, ey);
                        self.update_transform_ids.insert(object_id);
                    } else {
                        self.start_press = Some((ex, ey, false));
                        o.move_target = Some(Vec3::new(ex, 0.0, ey));
                    }

                    true
                }
                1 => {
                    e.prevent_default();
                    self.pan_anchor = Some((e.client_x() as f64, e.client_y() as f64));
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

    fn on_mouse_move(&mut self, e: MouseEvent) -> Result<(), Error> {
        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let v = ViewTransform::new(&canvas, &self.config);

        let mut needs_redraw = false;

        if let Some((ax, ay)) = self.pan_anchor {
            let dx = e.client_x() as f64 - ax;
            let dy = e.client_y() as f64 - ay;
            *self.config.pan = self.config.pan.add(dx, dy);
            self.pan_anchor = Some((e.client_x() as f64, e.client_y() as f64));
            self.update_world = true;
            needs_redraw = true;
        }

        let (mx, my) = v.canvas_to_world(e.offset_x() as f64, e.offset_y() as f64);
        self.mouse_world_pos = Some((mx, my));

        if let Some((px, py, shift_key)) = self.start_press
            && let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id))
        {
            if shift_key {
                let dist = (mx - px).hypot(my - py);

                if dist >= ARROW_THRESHOLD {
                    if o.data.is_static() {
                        // Shift-drag rotates a static object.
                        self.look_at(px, py, mx, my);
                    } else if let Some(look_at) = o.data.as_look_at_mut() {
                        **look_at = Some(Vec3::new(mx, 0.0, my));
                        self.update_look_at_ids.insert(o.data.id);
                        self.look_at(px, py, mx, my);
                    }

                    needs_redraw = true;
                }
            } else if o.data.is_static() {
                // Static objects snap immediately while dragging.
                o.data.transform.position = Vec3::new(mx, 0.0, my);
                self.update_transform_ids.insert(o.data.id);
                needs_redraw = true;
            } else {
                o.move_target = Some(Vec3::new(mx, 0.0, my));
                needs_redraw = true;
            }
        }

        if needs_redraw {
            self.redraw()?;
        }

        Ok(())
    }

    fn look_at(&mut self, px: f32, py: f32, mx: f32, my: f32) {
        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
            return;
        };

        let id = o.data.id;
        o.arrow_target = Some((mx, my));
        let angle_rad = (my - py).atan2(mx - px);
        let dir_x = angle_rad.cos();
        let dir_z = angle_rad.sin();
        o.data.transform.front = Vec3::new(dir_x, 0.0, dir_z);
        self.update_transform_ids.insert(id);
    }

    fn on_mouse_up(&mut self, e: MouseEvent) -> Result<(), Error> {
        let needs_redraw = {
            match e.button() {
                0 => {
                    self.start_press = None;
                    if let Some(object) = self.selected.and_then(|id| self.objects.get_mut(&id)) {
                        object.arrow_target = None;
                    }
                    true
                }
                1 => {
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

    fn on_mouse_leave(&mut self) -> Result<(), Error> {
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

    fn on_wheel(&mut self, e: WheelEvent) -> Result<(), Error> {
        e.prevent_default();

        let delta = if e.delta_y() < 0.0 {
            ZOOM_FACTOR
        } else {
            1.0 / ZOOM_FACTOR
        } as f32;

        let canvas = self
            .canvas_ref
            .cast::<HtmlCanvasElement>()
            .ok_or("missing canvas")?;

        let mx = e.offset_x() as f64;
        let my = e.offset_y() as f64;

        let t_before = ViewTransform::new(&canvas, &self.config);
        let (wx, wz) = t_before.canvas_to_world(mx, my);

        *self.config.zoom = (*self.config.zoom * delta).clamp(0.1, 20.0);

        let t_after = ViewTransform::new(&canvas, &self.config);
        let (cx2, cy2) = t_after.world_to_canvas(wx, wz);
        self.config.pan.x += mx - cx2;
        self.config.pan.y += my - cy2;

        self.update_world = true;
        self.redraw()?;
        Ok(())
    }

    /// Resize the canvas according to its parent sizer element.
    fn load_initialize_images(&mut self, ctx: &Context<Self>) {
        self.images.clear();

        let ids = self.objects.values().filter_map(|o| o.image());
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
        let canvas = self
            .canvas_ref
            .cast::<HtmlCanvasElement>()
            .ok_or("missing canvas")?;

        let cx = canvas.get_context("2d")?.ok_or("missing canvas context")?;

        let cx = cx
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "invalid canvas context")?;

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        let t = ViewTransform::new(&canvas, &self.config);

        render::draw_grid(&cx, &t, &self.config.extent, *self.config.zoom);

        let selected = self.selected;

        // Draw static objects first (behind avatars).
        for o in self.objects.values() {
            if let Some(mut s) = RenderStatic::from_data(&o.data) {
                s.selected = selected == Some(o.data.id);
                render::draw_static_token(&cx, &t, &s, |id| self.images.get(id).cloned())?;
            }
        }
        // Draw remote static objects.
        for o in self.peers.values() {
            if let Some(s) = RenderStatic::from_data(&o.data) {
                if !s.hidden {
                    render::draw_static_token(&cx, &t, &s, |id| self.images.get(id).cloned())?;
                }
            }
        }

        let renders = || {
            let remotes = self
                .peers
                .values()
                .flat_map(|a| RenderAvatar::from_data(&a.data))
                .filter(|a| !a.hidden);

            let locals = self.objects.values().flat_map(move |a| {
                let mut avatar = RenderAvatar::from_data(&a.data)?;
                avatar.player = true;
                avatar.selected = selected == Some(a.data.id);
                Some(avatar)
            });

            remotes.chain(locals)
        };

        let selected_arrow = self
            .selected
            .and_then(|id| self.objects.get(&id))
            .and_then(|o| o.arrow_target);

        for a in renders() {
            let arrow = a.selected.then_some(selected_arrow).flatten();
            render::draw_avatar_token(&cx, &t, &a, arrow, |id| self.images.get(id).cloned())?;
        }

        for a in renders() {
            if let Some(target) = a.look_at {
                render::draw_look_at(&cx, &t, target, &a.color, *self.config.zoom as f64)?;
            }
        }

        Ok(())
    }
}

fn send_update(
    ctx: &Context<Map>,
    object_id: Id,
    key: Key,
    value: impl Into<Value>,
) -> ws::Request {
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
