use core::num::NonZeroU32;
use std::collections::{HashMap, HashSet};

use api::{
    AtomicIds, Canvas2, Color, Extent, InitializeMapResponse, Key, PeerId, RemoteId,
    RemoteUpdateBody, Role, StableId, Type, UpdateBody, Value, Vec3,
};
use gloo::events::EventListener;
use gloo::file::callbacks::{FileReader, read_as_bytes};
use gloo::timers::callback::Interval;
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    CanvasRenderingContext2d, DragEvent, HtmlCanvasElement, HtmlImageElement, KeyboardEvent,
    MouseEvent, WheelEvent, Window,
};
use yew::prelude::*;

use crate::components as c;
use crate::components::render::{RenderObject, RenderObjectKind};
use crate::drag_over::DragOver;
use crate::error::Error;
use crate::hierarchy::Hierarchy;
use crate::images::Images;
use crate::log;
use crate::objects::{Object, ObjectKind, ObjectRef, Objects, ObjectsRef};
use crate::peers::Peers;
use crate::state::State;

use super::render::{self, ViewTransform};
use super::{
    AnimationFrame, COMMON_ROOM, ChannelExt, ContextMenuDropdown, DynamicCanvas, GroupSettings,
    HelpModal, Icon, ObjectList, RoomSettings, Rooms, SetupChannel, StaticSettings, TemporaryUrl,
    TokenSettings, UNKNOWN_ROOM,
};

const LEFT_MOUSE_BUTTON: i16 = 0;
const MIDDLE_MOUSE_BUTTON: i16 = 1;

const ZOOM_FACTOR: f32 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const SIMULATION_FPS: u32 = 60;

#[derive(Debug, PartialEq)]
pub(crate) enum Modal {
    Help,
    Rooms,
    Remove { object: ObjectRef },
    Settings { object: ObjectRef },
    Unlock { object: ObjectRef },
}

impl Modal {
    fn title(&self) -> Html {
        match self {
            Modal::Help => html! {
                {"Shortcuts"}
            },
            Modal::Rooms => html! {
                {"Rooms"}
            },
            Modal::Remove { object, .. } => html! {
                <>
                    {"Remove "}
                    {object.name()}
                    {"?"}
                </>
            },
            Modal::Settings { object, .. } => html! {
                <>
                    {object.title()}
                </>
            },
            Modal::Unlock { object, .. } => html! {
                <>
                    {"Object "}
                    <span class="object-bullet">{object.name()}</span>
                    {" is locked!"}
                </>
            },
        }
    }

    fn view(&self, ctx: &Context<Map>) -> Html {
        match *self {
            Modal::Help => html! {
                <HelpModal />
            },
            Modal::Rooms => html! {
                <Rooms
                    onopensettings={ctx.link().callback(Msg::OpenSettings)}
                    onrequestdelete={ctx.link().callback(Msg::ConfirmRemove)}
                    />
            },
            Modal::Remove {
                object: ref object @ ObjectRef { id, .. },
                ..
            } => html! {
                <>
                    <p>
                        {"Are you sure you want to remove "}
                        <span class="object-bullet">{object.name()}</span>
                        {"?"}
                    </p>

                    <div class="btn-group">
                        <button class="btn danger"
                            onclick={ctx.link().callback(move |_| Msg::RemoveObject(id))}>
                            {"Remove"}
                        </button>
                        <button class="btn" onclick={ctx.link().callback(|_| Msg::CloseModal)}>
                            {"Cancel"}
                        </button>
                    </div>
                </>
            },
            Modal::Settings {
                object: ref object @ ObjectRef { id, .. },
            } => html! {
                 {match object.ty {
                    Type::STATIC => {
                        html! { <StaticSettings {id} /> }
                    }
                    Type::GROUP => {
                        html! { <GroupSettings {id} /> }
                    }
                    Type::TOKEN => {
                        html! { <TokenSettings {id} /> }
                    }
                    Type::ROOM => {
                        html! { <RoomSettings {id} /> }
                    }
                    _ => html! { <p class="hint">{"Unknown object type"}</p> },
                }}
            },
            Modal::Unlock {
                object: ObjectRef { id, .. },
                ..
            } => html! {
                <>
                    <div class="btn-group">
                        <button class="btn primary" onclick={ctx.link().callback(|_| Msg::CloseModal)}>
                            {"Ok"}
                        </button>
                        <button class="btn danger" onclick={ctx.link().callback(move |_| Msg::ToggleLocked(id))}>
                            <Icon name="lock-open" />
                            {"Unlock"}
                        </button>
                    </div>
                </>
            },
        }
    }
}

/// We keep some interior state here since sometimes we require a split borrow
/// between it and for example the objects or hierarchy.
///
/// Long term, most local mutable state will end up here.
struct MapState {
    window: Window,
    channel: ws::Channel,
    look_at: HashSet<RemoteId>,
    transforms: HashSet<RemoteId>,
    selected: RemoteId,
    context_menu: Option<ContextMenu>,
    modal: Option<Modal>,
    _toggle_mumble_request: ws::Request,
    redraw: bool,
    update_cache: bool,
    update_config: bool,
    update_view: bool,
    move_target: HashMap<RemoteId, Vec3>,
    arrow_target: HashMap<RemoteId, Vec3>,
    animation_frame: Option<AnimationFrame>,
    object_requests: HashMap<RemoteId, ObjectRequests>,
    zoom: State<f32>,
    pan: State<Vec3>,
    mumble_object: State<RemoteId>,
    mumble_follow: State<bool>,
    room: State<StableId>,
    name: State<String>,
}

impl MapState {
    fn new(window: Window) -> Self {
        Self {
            window,
            channel: ws::Channel::default(),
            look_at: HashSet::default(),
            transforms: HashSet::default(),
            selected: RemoteId::default(),
            context_menu: Option::default(),
            modal: Option::default(),
            _toggle_mumble_request: ws::Request::default(),
            redraw: false,
            update_cache: false,
            update_config: false,
            update_view: false,
            move_target: HashMap::default(),
            arrow_target: HashMap::default(),
            animation_frame: Option::default(),
            object_requests: HashMap::new(),
            zoom: State::new(2.0),
            pan: State::new(Vec3::ZERO),
            mumble_object: State::new(RemoteId::ZERO),
            mumble_follow: State::new(false),
            room: State::new(StableId::ZERO),
            name: State::new(String::new()),
        }
    }

    fn display(&self) -> &str {
        if self.name.is_empty() {
            "You"
        } else {
            &self.name
        }
    }

    fn initialize(&mut self, props: api::Properties) {
        *self.zoom = 2.0;
        *self.pan = Vec3::ZERO;
        *self.mumble_object = RemoteId::ZERO;
        *self.mumble_follow = false;
        *self.room = StableId::ZERO;
        *self.name = String::new();

        for (key, value) in props {
            self.update(key, value);
        }
    }

    fn update(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::SCALE => self.zoom.update(value.as_f32().unwrap_or(2.0)),
            Key::PAN => self.pan.update(value.as_vec3().unwrap_or_else(Vec3::zero)),
            Key::MUMBLE_OBJECT => self.mumble_object.update(RemoteId::local(value.as_id())),
            Key::MUMBLE_FOLLOW => self.mumble_follow.update(value.as_bool()),
            Key::ROOM => self.room.update(*value.as_stable_id()),
            Key::PEER_NAME => self.name.update(value.as_str().to_owned()),
            _ => false,
        }
    }

    fn world_values(&self) -> Vec<(Key, Value)> {
        let mut values = Vec::new();
        values.push((Key::SCALE, Value::from(*self.zoom)));
        values.push((Key::PAN, Value::from(*self.pan)));
        values
    }

    fn look_at(&mut self, objects: &mut ObjectsRef, from: Vec3, to: Vec3) {
        let Some(o) = objects.get_mut(self.selected) else {
            return;
        };

        let Some(transform) = o.as_transform_mut() else {
            return;
        };

        transform.front = from.direction_to(to);

        self.arrow_target.insert(o.id, to);
        self.transforms.insert(o.id);
    }

    fn select_object(&mut self, ctx: &Context<Map>, id: RemoteId, objects: &ObjectsRef) -> bool {
        if self.selected == id {
            return false;
        }

        self.redraw = true;
        self.selected = id;
        self.context_menu = None;

        if self
            .modal
            .as_ref()
            .is_some_and(|m| matches!(m, Modal::Remove { object } if object.id == id))
        {
            self.modal = None;
        }

        if !*self.mumble_follow || *self.mumble_object == id {
            return true;
        }

        if !id.is_zero() && !objects.is_interactive(id) {
            return true;
        }

        *self.mumble_object = id;

        self._toggle_mumble_request = self
            .channel
            .updates(ctx, [(Key::MUMBLE_OBJECT, Value::from(id.id))]);
        true
    }

    fn apply(&mut self, objects: &mut ObjectsRef, action: &mut Action, mouse: Vec3) {
        match action {
            Action::Rotate(r) => {
                let Some(o) = objects.get_mut(r.object_id) else {
                    return;
                };

                let dist = r.center.dist(mouse);

                if dist < ARROW_THRESHOLD {
                    return;
                }

                if r.is_static {
                    // Use the original cursor offset to rotate relative to the initial grab.
                    if let Some(transform) = o.as_transform_mut() {
                        let cursor = mouse - r.center;
                        let angle = cursor.angle_xz() + r.rotation_offset;
                        transform.front = Vec3::new(angle.cos(), transform.front.y, -angle.sin());
                        self.transforms.insert(o.id);
                    }

                    self.arrow_target.insert(r.object_id, mouse);
                } else if let Some(look_at) = o.as_look_at_mut() {
                    **look_at = Some(Vec3::new(mouse.x, 0.0, mouse.z));
                    self.look_at.insert(o.id);
                    self.look_at(objects, r.center, mouse);
                }

                self.redraw = true;
            }
            Action::Translate(t) => {
                let Some(o) = objects.get_mut(t.object_id) else {
                    return;
                };

                if o.is_static() {
                    let Some(transform) = o.as_transform_mut() else {
                        return;
                    };

                    transform.position = mouse + t.offset;
                    self.transforms.insert(o.id);
                    self.redraw = true;
                } else {
                    self.move_target.insert(t.object_id, mouse);
                    self.redraw = true;
                }
            }
            Action::Scale(scale) => {
                let distance = scale.position.dist(mouse);

                if distance > f32::EPSILON {
                    scale.scale = distance / scale.initial_distance;
                    self.redraw = true;
                }
            }
        }
    }

    fn finalize_action(
        &mut self,
        ctx: &Context<Map>,
        objects: &mut ObjectsRef,
        action: Action,
    ) -> bool {
        match action {
            Action::Scale(scale) => self.finalize_scale(ctx, objects, scale),
            _ => true,
        }
    }

    fn finalize_scale(
        &mut self,
        ctx: &Context<Map>,
        objects: &mut ObjectsRef,
        scale: Scale,
    ) -> bool {
        let Some(o) = objects.get_mut(scale.object_id) else {
            return false;
        };

        match &mut o.kind {
            ObjectKind::Static(s) => {
                if s.width.update_epsilon(*s.width * scale.scale) {
                    let requests = self.object_requests.entry(scale.object_id).or_default();

                    requests._scale_width = self.channel.object_updates(
                        ctx,
                        scale.object_id.id,
                        [(Key::STATIC_WIDTH, s.width.value())],
                    );
                }

                if s.height.update_epsilon(*s.height * scale.scale) {
                    let requests = self.object_requests.entry(scale.object_id).or_default();

                    requests._scale_height = self.channel.object_updates(
                        ctx,
                        scale.object_id.id,
                        [(Key::STATIC_HEIGHT, s.height.value())],
                    );
                }
            }
            ObjectKind::Token(t) => {
                if t.token_radius.update_epsilon(*t.token_radius * scale.scale) {
                    let requests = self.object_requests.entry(scale.object_id).or_default();

                    requests._scale_radius = self.channel.object_updates(
                        ctx,
                        scale.object_id.id,
                        [(Key::TOKEN_RADIUS, t.token_radius.value())],
                    );
                }
            }
            _ => {}
        }

        true
    }
}

struct Translate {
    object_id: RemoteId,
    offset: Vec3,
}

struct Rotate {
    object_id: RemoteId,
    center: Vec3,
    rotation_offset: f32,
    is_static: bool,
}

struct Scale {
    object_id: RemoteId,
    scale: f32,
    position: Vec3,
    initial_distance: f32,
}

enum Action {
    Translate(Translate),
    Rotate(Rotate),
    Scale(Scale),
}

struct Cache {
    room_id: RemoteId,
    extent: Extent,
    show_grid: bool,
    background: RemoteId,
    room_icon: &'static str,
    room_name: String,
}

impl Cache {
    fn update(&mut self, room_id: RemoteId, objects: &ObjectsRef) {
        tracing::debug!(?room_id, object = ?objects.get(room_id));

        let Some((o, ObjectKind::Room(room))) = objects.get(room_id).map(|o| (o, &o.kind)) else {
            *self = Self::default();
            return;
        };

        let room_icon = 'done: {
            if room_id.is_zero() {
                break 'done "question-mark-circle";
            }

            if room_id.is_local() {
                "home"
            } else {
                "home-modern"
            }
        };

        *self = Self {
            room_id,
            extent: *room.extent,
            show_grid: *room.show_grid,
            background: RemoteId::new(room_id.peer_id, *room.background),
            room_icon,
            room_name: match o.name() {
                "" => UNKNOWN_ROOM.to_string(),
                name => name.to_string(),
            },
        };
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self {
            room_id: RemoteId::ZERO,
            extent: Extent::arena(),
            show_grid: false,
            background: RemoteId::ZERO,
            room_icon: "question-mark-circle",
            room_name: String::from(COMMON_ROOM),
        }
    }
}

struct DropImage {
    id: NonZeroU32,
    _onerror: Closure<dyn FnMut()>,
    _onload: Closure<dyn FnMut()>,
    _img: HtmlImageElement,
    _url: TemporaryUrl,
    bytes: Option<Vec<u8>>,
    content_type: String,
    file_reader: Option<FileReader>,
    pixel_size: Option<(u32, u32)>,
    world_pos: Vec3,
    world_size: Option<(f32, f32)>,
    _upload_image: ws::Request,
    _create_dropped_object: ws::Request,
}

impl DropImage {
    #[inline]
    fn is_ready_for_upload(&self) -> bool {
        self.world_size.is_some() && self.pixel_size.is_some() && self.bytes.is_some()
    }

    #[inline]
    fn compute_world_image_size(width: u32, height: u32) -> (f32, f32) {
        if width == 0 || height == 0 {
            return (2.0, 2.0);
        }

        let width = width as f32;
        let height = height as f32;

        if width >= height {
            (2.0, 2.0 * (height / width))
        } else {
            (2.0 * (width / height), 2.0)
        }
    }
}

/// State for the right-click context menu.
struct ContextMenu {
    object_id: RemoteId,
    position: Canvas2,
    onclose: Callback<()>,
    onsettings: Callback<()>,
    onhidden: Callback<()>,
    onlocalhidden: Callback<()>,
    onmumbleobject: Callback<()>,
    onremove: Callback<()>,
}

#[derive(Default)]
struct ObjectRequests {
    _expanded: ws::Request,
    _scale_height: ws::Request,
    _scale_radius: ws::Request,
    _scale_width: ws::Request,
    _toggle_hidden: ws::Request,
    _toggle_local_hidden: ws::Request,
}

pub(crate) struct Map {
    ids: AtomicIds,
    log: log::Log,
    s: MapState,
    _setup_channel: SetupChannel,
    _create_group: ws::Request,
    _create_object: ws::Request,
    _create_static: ws::Request,
    _remove_object: ws::Request,
    _initialize: ws::Request,
    _keydown_listener: EventListener,
    _keyup_listener: EventListener,
    _config_update: ws::Listener,
    _remote_update: ws::Listener,
    _set_group: ws::Request,
    _set_mumble_follow: ws::Request,
    _set_sort: ws::Request,
    _toggle_locked: ws::Request,
    _update_world: ws::Request,
    simulation_interval: Option<Interval>,
    canvas: Option<HtmlCanvasElement>,
    drag_over: Option<DragOver>,
    dropped_images: Vec<DropImage>,
    images: Images,
    look_at_requests: HashMap<RemoteId, ws::Request>,
    mouse: Option<Vec3>,
    object_ondragend: Callback<RemoteId>,
    object_ondragover: Callback<DragOver>,
    object_onexpandtoggle: Callback<RemoteId>,
    object_onhiddentoggle: Callback<RemoteId>,
    object_onlocalhiddentoggle: Callback<RemoteId>,
    object_onlockedtoggle: Callback<RemoteId>,
    object_onmumbletoggle: Callback<RemoteId>,
    object_onselect: Callback<RemoteId>,
    object_onopen: Callback<RemoteId>,
    objects: Objects,
    order: Hierarchy,
    pan_anchor: Option<Canvas2>,
    peers: Peers,
    cache: Cache,
    action: Option<Action>,
    transform_requests: HashMap<RemoteId, ws::Request>,
    look_ats: Vec<(Vec3, Color)>,
    view: ViewTransform,
}

#[derive(Debug)]
pub(crate) enum Msg {
    Error(Error),
    Channel(Result<ws::Channel, Error>),
    AnimationFrame(Result<f64, Error>),
    SimulationFrame,
    CloseModal,
    CloseContextMenu,
    Update(Result<Packet<api::Updates>, ws::Error>),
    ConfigUpdate(Result<Packet<api::Update>, ws::Error>),
    ConfirmRemove(RemoteId),
    ContextMenu(MouseEvent),
    CreateToken,
    CreateStatic,
    CreateGroup,
    RemoveObject(RemoteId),
    DragEnd(RemoteId),
    DragOver(DragOver),
    CanvasDragOver(DragEvent),
    DropImage(DragEvent),
    DropImageLoaded(NonZeroU32, u32, u32),
    DropImageData(NonZeroU32, Result<Vec<u8>, gloo::file::FileReadError>),
    DropImageUploaded(NonZeroU32, Result<Packet<api::UploadImage>, ws::Error>),
    DropObjectCreated(NonZeroU32, Result<Packet<api::CreateObject>, ws::Error>),
    Initialize(Result<Packet<api::InitializeMap>, ws::Error>),
    KeyDown(KeyboardEvent),
    KeyUp(KeyboardEvent),
    ObjectCreated(Result<Packet<api::CreateObject>, ws::Error>),
    ObjectRemoved(Result<Packet<api::RemoveObject>, ws::Error>),
    OpenSettings(RemoteId),
    PointerDown(PointerEvent),
    PointerLeave(PointerEvent),
    PointerMove(PointerEvent),
    PointerUp(PointerEvent),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    SelectObject(RemoteId),
    OpenObject(RemoteId),
    ToggleFollowMumbleSelection,
    ToggleHidden(RemoteId),
    ToggleLocalHidden(RemoteId),
    ToggleExpanded(RemoteId),
    ToggleLocked(RemoteId),
    ToggleMumbleObject(RemoteId),
    ObjectUpdate(Result<Packet<api::ObjectUpdate>, ws::Error>),
    OpenModal(Modal),
    Wheel(WheelEvent),
    ImageLoaded(Result<(), Error>),
    CanvasLoaded(HtmlCanvasElement),
    CanvasResized((u32, u32)),
}

impl From<Result<Packet<api::ObjectUpdate>, ws::Error>> for Msg {
    #[inline]
    fn from(value: Result<Packet<api::ObjectUpdate>, ws::Error>) -> Self {
        Msg::ObjectUpdate(value)
    }
}

impl From<Result<Packet<api::Updates>, ws::Error>> for Msg {
    #[inline]
    fn from(value: Result<Packet<api::Updates>, ws::Error>) -> Self {
        Msg::Update(value)
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props;

impl Component for Map {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let window = web_sys::window().expect("window not found");

        let document = window.document().expect("document");

        let (log, _) = ctx
            .link()
            .context::<log::Log>(Callback::noop())
            .expect("Log context not found");

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

        Self {
            ids: AtomicIds::new(0),
            log,
            s: MapState::new(window),
            _setup_channel: SetupChannel::new(ctx, ctx.link().callback(Msg::Channel)),
            _create_group: ws::Request::new(),
            _create_object: ws::Request::new(),
            _create_static: ws::Request::new(),
            _remove_object: ws::Request::new(),
            _initialize: ws::Request::new(),
            _keydown_listener,
            _keyup_listener,
            _config_update: ws::Listener::new(),
            _remote_update: ws::Listener::new(),
            _set_group: ws::Request::new(),
            _set_mumble_follow: ws::Request::new(),
            _set_sort: ws::Request::new(),
            _toggle_locked: ws::Request::new(),
            _update_world: ws::Request::new(),
            action: None,
            simulation_interval: None,
            canvas: None,
            drag_over: None,
            dropped_images: Vec::new(),
            images: Images::new(ctx.link().callback(Msg::ImageLoaded)),
            look_at_requests: HashMap::new(),
            look_ats: Vec::new(),
            mouse: None,
            object_ondragend: ctx.link().callback(Msg::DragEnd),
            object_ondragover: ctx.link().callback(Msg::DragOver),
            object_onexpandtoggle: ctx.link().callback(Msg::ToggleExpanded),
            object_onhiddentoggle: ctx.link().callback(Msg::ToggleHidden),
            object_onlocalhiddentoggle: ctx.link().callback(Msg::ToggleLocalHidden),
            object_onlockedtoggle: ctx.link().callback(Msg::ToggleLocked),
            object_onmumbletoggle: ctx.link().callback(Msg::ToggleMumbleObject),
            object_onselect: ctx.link().callback(Msg::SelectObject),
            object_onopen: ctx.link().callback(Msg::OpenObject),
            objects: Objects::default(),
            order: Hierarchy::default(),
            pan_anchor: None,
            peers: Peers::default(),
            cache: Cache::default(),
            transform_requests: HashMap::new(),
            view: ViewTransform::simple(0, 0, 1.0),
        }
    }

    fn rendered(&mut self, _: &Context<Self>, first_render: bool) {
        if first_render && let Err(error) = self.redraw() {
            self.log.error("map::redraw", error);
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        let mut changed = match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("map::update", error);
                false
            }
        };

        if !self.s.transforms.is_empty() {
            self.send_transform_updates(ctx);
        }

        if !self.s.look_at.is_empty() {
            self.send_look_at_updates(ctx);
        }

        if self.s.update_config {
            self._update_world = self.s.channel.updates(ctx, self.s.world_values());
            self.s.update_config = false;
            changed = true;
        }

        if self.s.update_cache {
            let room_id = self.peers.to_remote_id(&self.s.room);
            let objects = self.objects.borrow();
            self.cache.update(room_id, &objects);

            // If the cache is updated, the view needs to change as well since
            // extent might be modified.
            self.s.update_view = true;
            self.s.update_cache = false;
            changed = true;
        }

        if self.s.update_view {
            self.view = ViewTransform::new(
                self.view.width,
                self.view.height,
                *self.s.zoom,
                &self.s.pan,
                &self.cache.extent,
            );

            self.s.update_view = false;
            self.s.redraw = true;
            changed = true;
        }

        if self.s.redraw {
            if self.s.animation_frame.is_none() {
                self.s.animation_frame = Some(AnimationFrame::request(
                    self.s.window.clone(),
                    ctx.link().callback(Msg::AnimationFrame),
                ));
            }

            self.s.redraw = false;
        }

        if self.simulation_interval.is_none() && !self.s.move_target.is_empty() {
            let link = ctx.link().clone();

            let interval = Interval::new(1000 / SIMULATION_FPS, move || {
                link.send_message(Msg::SimulationFrame);
            });

            self.simulation_interval = Some(interval);
        }

        changed
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let objects = self.objects.borrow();
        let footer = self.footer(&objects);

        let object_list_header = {
            let o = objects.get(self.s.selected);

            let settings_classes = classes! {
                "btn",
                "square",
                o.is_some().then_some("primary"),
                (!o.is_some_and(|o| o.id.is_local())).then_some("disabled"),
            };

            let settings_click = o.filter(|o| o.id.is_local()).map(|o| {
                let id = o.id;
                ctx.link().callback(move |_| Msg::OpenSettings(id))
            });

            let remove_click = o.filter(|o| o.id.is_local()).map(|o| {
                let id = o.id;
                ctx.link().callback(move |_| Msg::ConfirmRemove(id))
            });

            let remove_classes = classes! {
                "btn",
                "square",
                o.is_some().then_some("danger"),
                (!o.is_some_and(|o| o.id.is_local())).then_some("disabled"),
            };

            html! {
                <div class="control-group">
                    <button class="btn square primary" title="Add token" onclick={ctx.link().callback(|_| Msg::CreateToken)}>
                        <Icon name="user-plus" title="Add token" />
                    </button>
                    <button class="btn square primary" title="Add static object" onclick={ctx.link().callback(|_| Msg::CreateStatic)}>
                        <Icon name="squares-plus" title="Add static" />
                    </button>
                    <button class="btn square primary" title="Add group" onclick={ctx.link().callback(|_| Msg::CreateGroup)}>
                        <Icon name="folder-plus" title="Add group" />
                    </button>

                    if !self.dropped_images.is_empty() {
                        <button class="btn warning" title="Image upload in progress">
                            <Icon name="photo" />
                            {self.dropped_images.len().to_string()}
                            <span class="loader invert" />
                        </button>
                    }

                    <section class="fill"></section>

                    <button class={settings_classes} title="Object settings" onclick={settings_click}>
                        <Icon name="cog" />
                    </button>
                    <button class={remove_classes} title="Remove object" onclick={remove_click}>
                        <Icon name="x-mark" />
                    </button>
                </div>
            }
        };

        let toolbar = {
            let o = objects.get(self.s.selected);

            let mumble = {
                let is_mumble = o.map(|o| *self.s.mumble_object == o.id).unwrap_or_default();

                let onclick = o.filter(|o| o.is_interactive()).map(|o| {
                    let id = o.id;
                    ctx.link().callback(move |_| Msg::ToggleMumbleObject(id))
                });

                let class = classes! {
                    "btn", "square",
                    is_mumble.then_some("success"),
                    onclick.is_some().then_some("disabled"),
                };

                html! {
                    <button {class} title="Toggle as MumbleLink Source" {onclick}>
                        <Icon name="mumble" />
                    </button>
                }
            };

            let hidden = {
                let is_hidden = o.map(|o| o.is_hidden()).unwrap_or_default();

                let hidden_icon = if is_hidden { "eye-slash" } else { "eye" };

                let class = classes! {
                    "btn", "square",
                    is_hidden.then_some("danger"),
                    o.is_none().then_some("disabled"),
                };

                let title = if is_hidden {
                    "Hidden from others"
                } else {
                    "Visible to others"
                };

                let onclick = o.map(|o| {
                    let id = o.id;
                    ctx.link().callback(move |_| Msg::ToggleHidden(id))
                });

                html! {
                    <button {class} title={title} onclick={onclick}>
                        <Icon name={hidden_icon} />
                    </button>
                }
            };

            let local_hidden = {
                let is_local_hidden = o.map(|o| o.is_local_hidden()).unwrap_or_default();

                let title = if is_local_hidden { "Hidden" } else { "Visible" };

                let class = classes! {
                    "btn", "square",
                    is_local_hidden.then_some("danger"),
                    o.is_none().then_some("disabled"),
                };

                let onclick = o.map(|o| {
                    let id = o.id;
                    ctx.link().callback(move |_| Msg::ToggleLocalHidden(id))
                });

                html! {
                    <button {class} {title} {onclick}>
                        <Icon name="no-symbol" />
                    </button>
                }
            };

            let locked = {
                let is_locked = o.map(|o| o.is_locked()).unwrap_or_default();

                let title = if is_locked { "Locked" } else { "Unlocked" };

                let name = if is_locked {
                    "lock-closed"
                } else {
                    "lock-open"
                };

                let class = classes! {
                    "btn", "square",
                    is_locked.then_some("danger"),
                    o.is_none().then_some("disabled"),
                };

                let onclick = o.map(|o| {
                    let id = o.id;
                    ctx.link().callback(move |_| Msg::ToggleLocked(id))
                });

                html! {
                    <button {class} {title} {onclick}>
                        <Icon {name} />
                    </button>
                }
            };

            let follow = {
                let class = classes! {
                    "btn", "square",
                    self.s.mumble_follow.then_some("success"),
                };

                let title = if *self.s.mumble_follow {
                    "Disable MumbleLink selection following"
                } else {
                    "Enable MumbleLink selection following"
                };

                let onclick = ctx.link().callback(|_| Msg::ToggleFollowMumbleSelection);

                html! {
                    <button {class} {title} {onclick}>
                        <Icon name="cursor-arrow-rays" />
                    </button>
                }
            };

            let help = {
                html! {
                    <button class="btn square" title="Keyboard shortcuts (F1)" onclick={ctx.link().callback(|_| Msg::OpenModal(Modal::Help))}>
                        <Icon name="question-mark-circle" />
                    </button>
                }
            };

            let room_id = self.cache.room_id;

            html! {
                <div class="control-group">
                    {mumble}
                    {hidden}
                    {local_hidden}
                    {locked}

                    <section class="icon-group">
                        <button class="btn" title="Switch room" onclick={ctx.link().callback(|_| Msg::OpenModal(Modal::Rooms))}>
                            <Icon name={self.cache.room_icon} />
                            <span>{self.cache.room_name.clone()}</span>
                        </button>
                    </section>

                    if room_id.is_local() {
                        <button class="btn square" title="Room settings" onclick={ctx.link().callback(move |_| Msg::OpenSettings(room_id))}>
                            <Icon name="cog" />
                        </button>
                    }

                    <section class="fill"></section>

                    {follow}
                    {help}
                </div>
            }
        };

        let players = html! {
            <>
                <div class="control-group">
                    <Icon name="remote" invert={true} />
                    <span>{"Players"}</span>
                </div>

                <div class="list" key="players">
                    <section class="list-content">
                        <Icon name="user" invert={true} small={true} />
                        <span class="list-label">{self.s.display()}</span>
                    </section>

                    {for self.peers.iter().filter(|p| p.in_room).map(|peer| html! {
                        html! {
                            <section class="list-content">
                                <Icon name="user" invert={true} small={true} />
                                <span class="list-label">{peer.display()}</span>
                            </section>
                        }
                    })}
                </div>
            </>
        };

        html! {
            <>
                <div class="row fill">
                    <div class="col-9 rows map-column">
                        {toolbar}

                        <DynamicCanvas
                            id="map"
                            onload={ctx.link().callback(Msg::CanvasLoaded)}
                            onerror={ctx.link().callback(Msg::Error)}
                            onresize={ctx.link().callback(Msg::CanvasResized)}
                            onpointerdown={ctx.link().callback(Msg::PointerDown)}
                            onpointermove={ctx.link().callback(Msg::PointerMove)}
                            onpointerup={ctx.link().callback(Msg::PointerUp)}
                            onpointerleave={ctx.link().callback(Msg::PointerLeave)}
                            onwheel={ctx.link().callback(Msg::Wheel)}
                            oncontextmenu={ctx.link().callback(Msg::ContextMenu)}
                            ondragover={ctx.link().callback(Msg::CanvasDragOver)}
                            ondrop={ctx.link().callback(Msg::DropImage)}
                            />
                    </div>

                    <div class="col-3 rows">
                        {object_list_header}
                        <ContextProvider<Objects> context={self.objects.clone()}>
                            <ContextProvider<Hierarchy> context={self.order.clone()}>
                                <ObjectList
                                    key={format!("{}", RemoteId::ZERO)}
                                    group={RemoteId::ZERO}
                                    drag_over={self.drag_over}
                                    mumble_object={*self.s.mumble_object}
                                    selected={self.s.selected}
                                    onselect={self.object_onselect.clone()}
                                    onopen={self.object_onopen.clone()}
                                    ondragover={self.object_ondragover.clone()}
                                    ondragend={self.object_ondragend.clone()}
                                    onhiddentoggle={self.object_onhiddentoggle.clone()}
                                    onlocalhiddentoggle={self.object_onlocalhiddentoggle.clone()}
                                    onexpandtoggle={self.object_onexpandtoggle.clone()}
                                    onlockedtoggle={self.object_onlockedtoggle.clone()}
                                    onmumbletoggle={self.object_onmumbletoggle.clone()}
                                    />
                            </ContextProvider<Hierarchy>>
                        </ContextProvider<Objects>>

                        {players}
                    </div>
                </div>

                {footer}

                if let Some(menu) = &self.s.context_menu {
                    <ContextMenuDropdown
                        position={menu.position}
                        object_id={menu.object_id}
                        is_hidden={objects.get(menu.object_id).map(|o| o.is_hidden()).unwrap_or_default()}
                        mumble_object={*self.s.mumble_object}
                        onclose={menu.onclose.clone()}
                        onsettings={menu.onsettings.clone()}
                        onhidden={menu.onhidden.clone()}
                        onlocalhidden={menu.onlocalhidden.clone()}
                        onmumbleobject={menu.onmumbleobject.clone()}
                        onremove={menu.onremove.clone()}
                        />
                }

                if let Some(modal) = &self.s.modal {
                    <c::Modal title={modal.title()} class="rows" onclose={ctx.link().callback(|_| Msg::CloseModal)}>
                        {modal.view(ctx)}
                    </c::Modal>
                }

            </>
        }
    }
}

impl Map {
    fn footer(&self, objects: &ObjectsRef) -> Html {
        let mut parts = Vec::with_capacity(5);

        if let Some(mouse) = self.mouse {
            parts.push(format!(
                "MOUSE: X:{:.02}, Y:{:.02}, Z:{:.02}",
                mouse.x, mouse.y, mouse.z
            ));
        }

        let zoom = *self.s.zoom;
        parts.push(format!("ZOOM:{:.02}", zoom));

        let pan = *self.s.pan;
        parts.push(format!(
            "PAN: X:{:.02}, Y:{:.02}, Z:{:.02}",
            pan.x, pan.y, pan.z
        ));

        if let Some(o) = objects.get(self.s.selected)
            && let Some(transform) = o.as_transform()
        {
            let position = &transform.position;
            parts.push(format!(
                "POSITION: X:{:.02}, Y:{:.02}, Z:{:.02}",
                position.x, position.y, position.z
            ));

            let front = &transform.front;
            parts.push(format!(
                "FRONT: X:{:.02}, Y:{:.02}, Z:{:.02}",
                front.x, front.y, front.z
            ));
        }

        let mut html_parts = Vec::new();

        let iter = parts.into_iter();
        let mut first = true;

        for part in iter {
            if !first {
                html_parts.push(html!(<span class="separator">{" | "}</span>));
            }

            html_parts.push(html!({ part }));
            first = false;
        }

        html! {
            <div class="row">
                <div class="col-12 footer">{ for html_parts }</div>
            </div>
        }
    }

    fn on_drop_image(&mut self, ctx: &Context<Self>, ev: DragEvent) -> Result<bool, Error> {
        let world_pos = self.view.to_world(ev.offset());

        let Some(dt) = ev.data_transfer() else {
            return Ok(false);
        };

        let Some(files) = dt.files() else {
            return Ok(false);
        };

        let mut update = false;

        for n in 0..files.length() {
            let Some(file) = files.get(n) else {
                continue;
            };

            let content_type = file.type_();

            if !content_type.starts_with("image/") {
                continue;
            }

            let Some(id) = self.ids.next() else {
                continue;
            };

            let url = TemporaryUrl::create(&file, ctx.link().callback(Msg::Error))?;

            let img = HtmlImageElement::new()?;
            let link = ctx.link().clone();
            let img_clone = img.clone();

            let onload = Closure::<dyn FnMut()>::new(move || {
                let w = img_clone.natural_width();
                let h = img_clone.natural_height();
                link.send_message(Msg::DropImageLoaded(id, w, h));
            });

            let error_link = ctx.link().clone();

            let onerror = Closure::<dyn FnMut()>::new(move || {
                error_link.send_message(Msg::DropImageLoaded(id, 0, 0));
            });

            img.set_onload(Some(onload.as_ref().unchecked_ref()));
            img.set_onerror(Some(onerror.as_ref().unchecked_ref()));
            img.set_src(&url);

            let gloo_file = gloo::file::File::from(file);

            let link = ctx.link().clone();

            let file_reader = read_as_bytes(&gloo_file, move |result| {
                link.send_message(Msg::DropImageData(id, result));
            });

            self.dropped_images.push(DropImage {
                id,
                _onerror: onerror,
                _onload: onload,
                _img: img,
                _url: url,
                bytes: None,
                content_type,
                file_reader: Some(file_reader),
                pixel_size: None,
                world_pos,
                world_size: None,
                _upload_image: ws::Request::new(),
                _create_dropped_object: ws::Request::new(),
            });

            update = true;
        }

        Ok(update)
    }

    fn try_create_dropped_object(
        &mut self,
        ctx: &Context<Self>,
        id: NonZeroU32,
    ) -> Result<bool, Error> {
        let Some(drop_image) = self.dropped_images.iter_mut().find(|d| d.id == id) else {
            return Ok(false);
        };

        if !drop_image.is_ready_for_upload() {
            return Ok(false);
        }

        let Some((image_width, image_height)) = drop_image.pixel_size else {
            return Ok(false);
        };

        let Some(data) = drop_image.bytes.take() else {
            return Ok(false);
        };

        drop_image._upload_image = self
            .s
            .channel
            .request()
            .body(api::UploadImageRequestRef {
                content_type: &drop_image.content_type,
                role: Role::STATIC,
                crop: api::CropRegion {
                    x1: 0,
                    y1: 0,
                    x2: image_width,
                    y2: image_height,
                },
                sizing: api::ImageSizing::Crop,
                size: 512,
                data: &data,
            })
            .on_packet(
                ctx.link()
                    .callback(move |result| Msg::DropImageUploaded(id, result)),
            )
            .send();

        Ok(false)
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Error(error) => {
                self.log.error("map", error);
                Ok(false)
            }
            Msg::DragOver(drag_over) => {
                self.drag_over = Some(drag_over);
                Ok(true)
            }
            Msg::DragEnd(id) => self.drag_end(ctx, id),
            Msg::OpenSettings(id) => {
                self.s.context_menu = None;

                let objects = self.objects.borrow();

                let Some(o) = objects.get(id) else {
                    return Ok(false);
                };

                let modal = if objects.is_locked(o.id) {
                    Modal::Unlock { object: o.as_ref() }
                } else {
                    Modal::Settings { object: o.as_ref() }
                };

                self.s.modal = Some(modal);
                Ok(true)
            }
            Msg::OpenModal(modal) => {
                if self.s.modal.as_ref() != Some(&modal) {
                    self.s.modal = Some(modal);
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Msg::Update(result) => {
                result?;
                Ok(false)
            }
            Msg::ToggleMumbleObject(id) => Ok(self.toggle_mumble_object(ctx, id)),
            Msg::ToggleLocked(id) => Ok(self.toggle_locked(ctx, id)),
            Msg::ToggleHidden(id) => Ok(self.toggle_hidden(ctx, id)),
            Msg::ToggleLocalHidden(id) => Ok(self.toggle_local_hidden(ctx, id)),
            Msg::ToggleExpanded(id) => Ok(self.toggle_expanded(ctx, id)),
            Msg::Channel(channel) => {
                self.s.channel = channel?;

                if self.s.channel.id() == ChannelId::NONE {
                    self.dropped_images.clear();
                    return Ok(false);
                }

                self._config_update = self
                    .s
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::ConfigUpdate));

                self._remote_update = self
                    .s
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::RemoteUpdate));

                self._initialize = self
                    .s
                    .channel
                    .request()
                    .body(api::InitializeMapRequest)
                    .on_packet(ctx.link().callback(Msg::Initialize))
                    .send();

                Ok(false)
            }
            Msg::Initialize(body) => {
                let body = body?;
                let body = body.decode()?;
                Ok(self.initialize(ctx, body))
            }
            Msg::ConfigUpdate(body) => {
                let body = body?;
                let body = body.decode()?;
                self.config_update(body)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;
                Ok(self.remote_update(ctx, body))
            }
            Msg::ObjectUpdate(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(false)
            }
            Msg::CanvasDragOver(ev) => {
                ev.prevent_default();
                Ok(false)
            }
            Msg::DropImage(ev) => {
                ev.prevent_default();
                self.on_drop_image(ctx, ev)
            }
            Msg::DropImageLoaded(id, width, height) => {
                let Some(drop_image) = self.dropped_images.iter_mut().find(|d| d.id == id) else {
                    return Ok(false);
                };

                drop_image.world_size = Some(DropImage::compute_world_image_size(width, height));
                drop_image.pixel_size = Some((width, height));
                self.try_create_dropped_object(ctx, id)
            }
            Msg::DropImageData(id, result) => match result {
                Ok(data) => {
                    let Some(drop_image) = self.dropped_images.iter_mut().find(|d| d.id == id)
                    else {
                        return Ok(false);
                    };

                    drop_image.bytes = Some(data);
                    drop_image.file_reader = None;
                    self.try_create_dropped_object(ctx, id)
                }
                Err(err) => {
                    self.dropped_images.retain(|d| d.id != id);

                    self.log.error(
                        "drop image",
                        Error::from(anyhow::anyhow!("file read error: {err}")),
                    );
                    Ok(true)
                }
            },
            Msg::DropImageUploaded(id, result) => {
                let body = result?;
                let body = body.decode()?;

                let Some(drop_image) = self.dropped_images.iter_mut().find(|d| d.id == id) else {
                    return Ok(false);
                };

                let world_pos = drop_image.world_pos;
                let (width, height) = drop_image.world_size.unwrap_or((2.0, 2.0));

                let transform = api::Transform::new(world_pos, api::Vec3::FORWARD);

                let body = api::CreateObjectRequest {
                    ty: Type::STATIC,
                    props: api::Properties::from_iter([
                        (Key::HIDDEN, Value::from(true)),
                        (Key::IMAGE_ID, Value::from(body.image.id.id)),
                        (Key::TRANSFORM, Value::from(transform)),
                        (Key::STATIC_WIDTH, Value::from(width)),
                        (Key::STATIC_HEIGHT, Value::from(height)),
                    ]),
                };

                drop_image._create_dropped_object = self
                    .s
                    .channel
                    .request()
                    .body(body)
                    .on_packet(
                        ctx.link()
                            .callback(move |packet| Msg::DropObjectCreated(id, packet)),
                    )
                    .send();

                Ok(true)
            }
            Msg::DropObjectCreated(id, body) => {
                let body = body?;
                let _ = body.decode()?;
                self.dropped_images.retain(|d| d.id != id);
                Ok(true)
            }
            Msg::PointerDown(ev) => {
                self.on_pointer_down(ctx, ev)?;
                Ok(true)
            }
            Msg::PointerMove(ev) => {
                self.on_pointer_move(ev)?;
                Ok(true)
            }
            Msg::PointerUp(ev) => self.on_pointer_up(ctx, ev),
            Msg::PointerLeave(ev) => self.on_pointer_leave(ev),
            Msg::Wheel(e) => {
                self.on_wheel(e)?;
                Ok(true)
            }
            Msg::AnimationFrame(result) => {
                let _ = result?;
                self.s.animation_frame = None;
                self.redraw()?;
                Ok(false)
            }
            Msg::SimulationFrame => {
                self.simulation_frame();
                self.s.redraw = true;
                Ok(true)
            }
            Msg::KeyDown(ev) => self.on_key_down(ctx, ev),
            Msg::KeyUp(ev) => self.on_key_up(ctx, ev),
            Msg::SelectObject(id) => {
                self.cancel_action();

                let objects = self.objects.borrow();

                let update = self.s.select_object(ctx, id, &objects);

                Ok(update)
            }
            Msg::OpenObject(id) => {
                let objects = self.objects.borrow();

                let Some(o) = objects.get(id) else {
                    return Ok(false);
                };

                match &o.kind {
                    ObjectKind::Group(..) => {
                        ctx.link().send_message(Msg::ToggleExpanded(id));
                        Ok(false)
                    }
                    _ => {
                        ctx.link().send_message(Msg::OpenSettings(id));
                        Ok(false)
                    }
                }
            }
            Msg::ToggleFollowMumbleSelection => {
                *self.s.mumble_follow = !*self.s.mumble_follow;

                self._set_mumble_follow = self.s.channel.updates(
                    ctx,
                    [(Key::MUMBLE_FOLLOW, Value::from(*self.s.mumble_follow))],
                );
                Ok(true)
            }
            Msg::CreateToken => {
                self._create_object =
                    self.create_remote_object(ctx, Type::TOKEN, [(Key::HIDDEN, Value::from(true))]);

                Ok(false)
            }
            Msg::CreateStatic => {
                self._create_static = self.create_remote_object(
                    ctx,
                    Type::STATIC,
                    [
                        (Key::HIDDEN, Value::from(true)),
                        (Key::STATIC_WIDTH, Value::from(1.0_f32)),
                        (Key::STATIC_HEIGHT, Value::from(1.0_f32)),
                    ],
                );

                Ok(false)
            }
            Msg::CreateGroup => {
                self._create_group = self.create_remote_object(
                    ctx,
                    Type::GROUP,
                    [
                        (Key::HIDDEN, Value::from(false)),
                        (Key::EXPANDED, Value::from(true)),
                    ],
                );

                Ok(false)
            }
            Msg::ObjectCreated(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(true)
            }
            Msg::ConfirmRemove(id) => {
                self.s.context_menu = None;

                let objects = self.objects.borrow();

                let Some(o) = objects.get(id) else {
                    return Ok(false);
                };

                let modal = if objects.is_locked(o.id) {
                    Modal::Unlock { object: o.as_ref() }
                } else {
                    Modal::Remove { object: o.as_ref() }
                };

                self.s.modal = Some(modal);
                Ok(true)
            }
            Msg::CloseModal => {
                self.s.modal = None;
                Ok(true)
            }
            Msg::RemoveObject(id) => Ok(self.remove_object_remote(ctx, id)),
            Msg::ObjectRemoved(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
            Msg::ContextMenu(ev) => {
                ev.prevent_default();
                self.on_context_menu(ctx, ev)?;
                Ok(true)
            }
            Msg::CloseContextMenu => {
                self.s.context_menu = None;
                Ok(true)
            }
            Msg::ImageLoaded(result) => {
                result?;
                self.s.redraw = true;
                Ok(false)
            }
            Msg::CanvasLoaded(canvas) => {
                self.canvas = Some(canvas);
                self.s.redraw = true;
                Ok(false)
            }
            Msg::CanvasResized((width, height)) => {
                self.view = ViewTransform::new(
                    width,
                    height,
                    *self.s.zoom,
                    &self.s.pan,
                    &self.cache.extent,
                );

                self.s.redraw = true;
                Ok(true)
            }
        }
    }

    fn initialize(&mut self, ctx: &Context<Self>, body: InitializeMapResponse) -> bool {
        tracing::debug!(?body, "Initialize");

        self.peers.public_key = body.public_key;
        self.s.initialize(body.props);

        self.objects = body
            .objects
            .iter()
            .filter_map(|object| Object::new(PeerId::ZERO, object))
            .collect();

        let mut order = self.order.borrow_mut();
        let mut objects = self.objects.borrow_mut();

        order.extend(objects.values());

        self.peers.clear();

        for peer in body.peers {
            self.peers.insert(peer, &self.s.room);
        }

        for (peer_id, object) in body.peer_objects {
            let Some(object) = Object::new(peer_id, &object) else {
                continue;
            };

            order.insert(&object);
            objects.insert(object);
        }

        self.images.clear();

        for id in body.images {
            self.images
                .load_id(&id, ctx.link().callback(Msg::ImageLoaded));
        }

        self.s.update_cache = true;
        self.s.redraw = true;
        true
    }

    fn config_update(&mut self, body: UpdateBody) -> Result<bool, Error> {
        tracing::debug!(?body, "Config update");

        match body {
            UpdateBody::Config {
                channel,
                key,
                value,
            } => {
                if self.s.channel.id() == channel {
                    return Ok(false);
                }

                let changed = self.s.update(key, value);

                if changed {
                    if matches!(key, Key::ROOM) {
                        for peer in self.peers.iter_mut() {
                            peer.update_config(&self.s.room);
                        }

                        self.s.update_cache = true;
                    }

                    self.s.update_view |= matches!(key, Key::ZOOM | Key::PAN);
                }

                self.s.redraw = changed;
                Ok(changed)
            }
            UpdateBody::PublicKey { public_key } => {
                self.peers.public_key = public_key;
                Ok(true)
            }
        }
    }

    fn remote_update(&mut self, ctx: &Context<Self>, body: RemoteUpdateBody) -> bool {
        tracing::debug!(?body, "Remote update");

        match body {
            RemoteUpdateBody::RemoteLost => {
                let mut objects = self.objects.borrow_mut();
                let mut order = self.order.borrow_mut();

                objects.retain(|id, _| id.peer_id == PeerId::ZERO);
                order.retain(|peer_id| peer_id == PeerId::ZERO);
                self.images.retain(|peer_id| peer_id == PeerId::ZERO);
                self.peers.clear();
                self.s.update_cache = true;
                self.s.redraw = true;
                true
            }
            RemoteUpdateBody::PeerConnected { peer } => {
                self.peers.insert(peer, &self.s.room);
                true
            }
            RemoteUpdateBody::PeerJoin {
                peer_id,
                objects: objs,
            } => {
                let mut objects = self.objects.borrow_mut();
                let mut order = self.order.borrow_mut();

                for object in objs {
                    let Some(object) = Object::new(peer_id, &object) else {
                        continue;
                    };

                    order.insert(&object);
                    objects.insert(object);
                }

                self.s.redraw = true;
                true
            }
            RemoteUpdateBody::PeerLeave { peer_id } => {
                let mut objects = self.objects.borrow_mut();
                let mut order = self.order.borrow_mut();

                // When a peer leaves our room, we remove all of their
                // local objects.
                objects.retain(|id, global| global || id.peer_id != peer_id);
                order.retain(|this| this != peer_id);

                self.s.redraw = true;
                true
            }
            RemoteUpdateBody::PeerDisconnect { peer_id } => {
                let mut objects = self.objects.borrow_mut();
                let mut order = self.order.borrow_mut();

                // We remove all objects associated with a peer when
                // that peer disconnects.
                objects.retain(|id, _| id.peer_id != peer_id);
                order.retain(|this| this != peer_id);
                self.peers.remove_peer(peer_id);

                self.s.redraw = true;
                true
            }
            RemoteUpdateBody::PeerUpdate {
                peer_id,
                key,
                value,
            } => {
                let Some(peer) = self.peers.get_mut(peer_id) else {
                    return false;
                };

                peer.update(key, value, &self.s.room);
                true
            }
            RemoteUpdateBody::ObjectCreated { id, object, .. } => self.create_object(id, object),
            RemoteUpdateBody::ObjectRemoved { channel, id } => {
                if self.s.channel.id() == channel {
                    return false;
                }

                self.remove_object(ctx, id)
            }
            RemoteUpdateBody::ObjectUpdated {
                channel,
                id,
                key,
                value,
            } => {
                let mut objects = self.objects.borrow_mut();
                let mut order = self.order.borrow_mut();

                if self.s.channel.id() == channel {
                    return false;
                }

                let Some(o) = objects.get_mut(id) else {
                    return false;
                };

                let mut update = match key {
                    Key::SORT => 'done: {
                        let new = value.as_bytes().to_vec();

                        let Some(old_sort) = o.sort.replace(new) else {
                            break 'done false;
                        };

                        order.reorder(*o.group, &old_sort, *o.group, &o.sort, o.id)
                    }
                    Key::GROUP => 'done: {
                        let new = RemoteId::new(o.id.peer_id, value.as_id());

                        let Some(old_group) = o.group.replace(new) else {
                            break 'done false;
                        };

                        order.reorder(old_group, &o.sort, *o.group, &o.sort, o.id)
                    }
                    _ => false,
                };

                self.s.update_cache = o.ty() == Type::ROOM;
                update |= o.update(key, value);
                self.s.redraw |= update;
                update
            }
            RemoteUpdateBody::ImageCreated { image } => {
                self.images
                    .load_id(&image.id, ctx.link().callback(Msg::ImageLoaded));
                self.s.redraw = true;
                true
            }
            RemoteUpdateBody::ImageRemoved { id } => {
                self.images.remove(&id);
                self.s.redraw = true;
                true
            }
        }
    }

    fn create_remote_object(
        &mut self,
        ctx: &Context<Self>,
        ty: Type,
        props: impl IntoIterator<Item = (Key, Value)>,
    ) -> ws::Request {
        let body = api::CreateObjectRequest {
            ty,
            props: api::Properties::from_iter(props),
        };

        self.s
            .channel
            .request()
            .body(body)
            .on_packet(ctx.link().callback(Msg::ObjectCreated))
            .send()
    }

    fn create_object(&mut self, id: RemoteId, object: api::RemoteObject) -> bool {
        let mut objects = self.objects.borrow_mut();
        let mut order = self.order.borrow_mut();

        let Some(object) = Object::new(id.peer_id, &object) else {
            return false;
        };

        order.insert(&object);
        objects.insert(object);
        self.s.update_cache = true;
        self.s.redraw = true;
        true
    }

    fn remove_object_remote(&mut self, ctx: &Context<Self>, id: RemoteId) -> bool {
        self._remove_object = self
            .s
            .channel
            .request()
            .body(api::RemoveObjectRequest { id: id.id })
            .on_packet(ctx.link().callback(Msg::ObjectRemoved))
            .send();

        self.remove_object(ctx, id)
    }

    fn remove_object(&mut self, ctx: &Context<Map>, id: RemoteId) -> bool {
        let mut objects = self.objects.borrow_mut();
        let mut order = self.order.borrow_mut();

        self.s.arrow_target.remove(&id);
        self.s.move_target.remove(&id);

        let Some(o) = objects.remove(id) else {
            return false;
        };

        order.remove(&o);

        if self.s.selected == id {
            self.s.select_object(ctx, RemoteId::ZERO, &objects);
        }

        self.s.modal = None;
        self.s.object_requests.remove(&id);
        self.s.update_cache = true;
        self.s.redraw = true;
        true
    }

    fn on_key_up(&mut self, _ctx: &Context<Self>, ev: KeyboardEvent) -> Result<bool, Error> {
        let key = ev.key();

        match key.as_str() {
            "Shift" if self.s.modal.is_none() => {
                let Some(Action::Rotate(r)) = self.action.take() else {
                    return Ok(false);
                };

                let mut objects = self.objects.borrow_mut();

                let Some(o) = objects.get_mut(r.object_id) else {
                    return Ok(false);
                };

                self.s.arrow_target.remove(&r.object_id);

                if let Some(look_at) = o.as_look_at_mut() {
                    **look_at = None;
                    self.s.look_at.insert(r.object_id);
                }

                self.s.redraw = true;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn cancel_action(&mut self) -> bool {
        if self.action.take().is_some() {
            self.s.redraw = true;
            return true;
        }

        false
    }

    fn start_translate_on_hit(&mut self, ctx: &Context<Self>) -> bool {
        let Some(mouse) = self.mouse else {
            return false;
        };

        let new_selected;
        let start_translate;
        let mut update = false;

        {
            let order = self.order.borrow();
            let objects = self.objects.borrow();

            let hit = order
                .walk()
                .flat_map(|id| objects.get(id))
                .find(|o| o.as_click_geometry().intersects(mouse));

            let selected = objects.get(self.s.selected);

            let s_is_token = selected.is_some_and(|o| o.is_token());
            let s_id = selected.map(|o| o.id);
            let h_id = hit.map(|o| o.id);
            let h_is_not_token = hit.is_some_and(|o| !o.is_token());

            new_selected = (s_id != h_id && !s_is_token).then_some(h_id.unwrap_or(RemoteId::ZERO));
            start_translate = s_id == h_id || s_is_token || h_is_not_token;

            if let Some(id) = new_selected {
                update = self.s.select_object(ctx, id, &objects);
            }
        }

        if start_translate {
            update |= self.start_translate(ctx, self.s.selected);
        }

        update
    }

    fn start_translate(&mut self, ctx: &Context<Self>, id: RemoteId) -> bool {
        if id.is_zero() || !id.is_local() {
            return false;
        }

        let Some(mouse) = self.mouse else {
            return false;
        };

        let mut objects = self.objects.borrow_mut();

        if objects.is_locked(id) {
            return false;
        }

        let Some(o) = objects.get_mut(id) else {
            return false;
        };

        self.s.arrow_target.remove(&id);

        let is_static = o.is_static();

        let offset = if is_static {
            let Some(transform) = o.as_transform() else {
                return false;
            };

            // Keep the cursor's offset relative to the object's origin while
            // dragging. The stored vector is applied to the world cursor
            // position on move.
            transform.position - mouse
        } else {
            self.s.move_target.insert(id, mouse);
            mouse
        };

        if let Some(action) = self.action.take() {
            self.s.finalize_action(ctx, &mut objects, action);
        }

        let action = self.action.insert(Action::Translate(Translate {
            object_id: id,
            offset,
        }));

        self.s.apply(&mut objects, action, mouse);
        true
    }

    fn start_scale(&mut self, ctx: &Context<Self>) -> bool {
        let Some(mouse) = self.mouse else {
            return false;
        };

        if self.s.selected.is_zero() || !self.s.selected.is_local() {
            return false;
        }

        let mut objects = self.objects.borrow_mut();

        if objects.is_locked(self.s.selected) {
            return false;
        };

        let Some(o) = objects.get_mut(self.s.selected) else {
            return false;
        };

        let Some(position) = o.as_transform().map(|t| t.position) else {
            return false;
        };

        let initial_distance = position.dist(mouse).max(0.01);

        self.s.move_target.remove(&self.s.selected);

        if let Some(action) = self.action.take() {
            self.s.finalize_action(ctx, &mut objects, action);
        }

        let action = self.action.insert(Action::Scale(Scale {
            object_id: self.s.selected,
            scale: 1.0,
            position,
            initial_distance,
        }));

        self.s.apply(&mut objects, action, mouse);
        true
    }

    fn finalize_action(&mut self, ctx: &Context<Self>) -> bool {
        let Some(action) = self.action.take() else {
            return false;
        };

        let mut objects = self.objects.borrow_mut();
        self.s.finalize_action(ctx, &mut objects, action)
    }

    fn on_key_down(&mut self, ctx: &Context<Self>, ev: KeyboardEvent) -> Result<bool, Error> {
        let key = ev.key();

        match key.as_str() {
            "Escape" => Ok(self.cancel()),
            "Delete" => {
                if self.s.modal.as_ref().is_some_and(
                    |m| matches!(m, Modal::Remove { object } if object.id == self.s.selected),
                ) {
                    return Ok(false);
                }

                ctx.link().send_message(Msg::ConfirmRemove(self.s.selected));
                Ok(false)
            }
            "F1" | "?" => {
                self.s.modal = match self.s.modal {
                    Some(Modal::Help) => None,
                    _ => Some(Modal::Help),
                };

                Ok(true)
            }
            "Enter" if self.s.modal.is_none() => Ok(self.accept(ctx)),
            "s" | "S" if self.s.modal.is_none() => Ok(self.start_scale(ctx)),
            "t" | "T" if self.s.modal.is_none() => Ok(self.toggle_locked(ctx, self.s.selected)),
            "r" | "R" if self.s.modal.is_none() => Ok(self.start_rotation(ctx)),
            "g" | "G" if self.s.modal.is_none() => Ok(self.start_translate(ctx, self.s.selected)),
            "Shift" if self.s.modal.is_none() => Ok(self.start_rotation(ctx)),
            _ => Ok(false),
        }
    }

    fn accept(&mut self, ctx: &Context<Self>) -> bool {
        let Some(modal) = self.s.modal.take() else {
            return false;
        };

        if let Modal::Remove { object } = modal {
            self.remove_object_remote(ctx, object.id);
        }

        true
    }

    fn cancel(&mut self) -> bool {
        if self.cancel_action() {
            return false;
        }

        if self.s.modal.is_some() {
            self.s.modal = None;
            return true;
        }

        if self.s.context_menu.is_some() {
            self.s.context_menu = None;
            return true;
        }

        if self.s.selected != RemoteId::ZERO {
            self.s.selected = RemoteId::ZERO;
            self.s.redraw = true;
            return true;
        }

        false
    }

    fn start_rotation(&mut self, ctx: &Context<Self>) -> bool {
        if self.s.selected.is_zero() || !self.s.selected.is_local() {
            return false;
        }

        let Some(mouse) = self.mouse else {
            return false;
        };

        let mut objects = self.objects.borrow_mut();

        if objects.is_locked(self.s.selected) {
            return false;
        }

        let Some(o) = objects.get(self.s.selected) else {
            return false;
        };

        let object_id = o.id;

        let Some(transform) = o.as_transform() else {
            return false;
        };

        let center = transform.position;

        let rotation_offset = if o.is_static() {
            let cursor = mouse - center;
            transform.front.angle_xz() - cursor.angle_xz()
        } else {
            0.0
        };

        let is_static = o.is_static();

        if let Some(action) = self.action.take() {
            self.s.finalize_action(ctx, &mut objects, action);
        }

        let action = self.action.insert(Action::Rotate(Rotate {
            object_id,
            center,
            rotation_offset,
            is_static,
        }));

        self.s.apply(&mut objects, action, mouse);
        true
    }

    /// Routine responsible for simulating objects that are in motion.
    ///
    /// This is called at a rate of [`SIMULATION_FPS`].
    fn simulation_frame(&mut self) {
        let mut objects = self.objects.borrow_mut();

        for o in objects.values_mut() {
            let id = o.id;

            let Some((transform, look_at, speed)) = o.as_interpolate_mut() else {
                continue;
            };

            let p = transform.position;

            'move_done: {
                let (Some(target), Some(speed)) = (self.s.move_target.get(&id), speed) else {
                    break 'move_done;
                };

                let dx = target.x - p.x;
                let dy = target.y - p.y;
                let dz = target.z - p.z;
                let distance = (dx * dx + dy * dy + dz * dz).sqrt();

                if distance < 0.01 {
                    transform.position = *target;
                    self.s.move_target.remove(&id);
                    self.s.transforms.insert(id);
                    break 'move_done;
                }

                let step = speed / SIMULATION_FPS as f32;
                let move_distance = step.min(distance);
                let ratio = move_distance / distance;

                transform.position.x += dx * ratio;
                transform.position.y += dy * ratio;
                transform.position.z += dz * ratio;

                // Face the movement direction unless a look_at target is active.
                if look_at.is_none() {
                    transform.front = p.direction_to(*target);
                }

                self.s.transforms.insert(id);
            };

            'look_done: {
                let Some(t) = look_at else {
                    break 'look_done;
                };

                transform.front = p.direction_to(*t);

                self.s.arrow_target.insert(id, *t);
                self.s.transforms.insert(id);
            };
        }

        if self.s.move_target.is_empty() {
            self.simulation_interval = None;
        }
    }

    fn send_transform_updates(&mut self, ctx: &Context<Self>) {
        if self.s.channel.id() == ChannelId::NONE {
            self.s.transforms.clear();
            return;
        }

        let objects = self.objects.borrow();

        for id in self.s.transforms.drain() {
            let Some(o) = objects.get(id) else {
                continue;
            };

            let Some(transform) = o.as_transform() else {
                continue;
            };

            let req =
                self.s
                    .channel
                    .object_updates(ctx, id.id, [(Key::TRANSFORM, (*transform).into())]);

            self.transform_requests.insert(id, req);
        }
    }

    fn send_look_at_updates(&mut self, ctx: &Context<Self>) {
        if self.s.channel.id() == ChannelId::NONE {
            self.s.look_at.clear();
            return;
        }

        let objects = self.objects.borrow();

        for id in self.s.look_at.drain() {
            let Some(o) = objects.get(id) else {
                continue;
            };

            let req = self.s.channel.object_updates(
                ctx,
                id.id,
                [(Key::LOOK_AT, o.look_at().copied().into())],
            );

            self.look_at_requests.insert(id, req);
        }
    }

    fn on_context_menu(&mut self, ctx: &Context<Self>, ev: MouseEvent) -> Result<(), Error> {
        let mouse = self.view.to_world(ev.offset());

        let objects = self.objects.borrow();

        let hit = objects
            .values()
            .find(|o| o.as_click_geometry().intersects(mouse))
            .map(|o| o.id);

        if let Some(object_id) = hit {
            self.s.selected = object_id;

            self.s.context_menu = Some(ContextMenu {
                object_id,
                position: ev.client(),
                onclose: ctx.link().callback(|_| Msg::CloseContextMenu),
                onsettings: ctx.link().callback(move |_| Msg::OpenSettings(object_id)),
                onhidden: ctx.link().callback(move |_| Msg::ToggleHidden(object_id)),
                onlocalhidden: ctx
                    .link()
                    .callback(move |_| Msg::ToggleLocalHidden(object_id)),
                onmumbleobject: ctx
                    .link()
                    .callback(move |_| Msg::ToggleMumbleObject(object_id)),
                onremove: ctx.link().callback(move |_| Msg::ConfirmRemove(object_id)),
            });

            self.s.redraw = true;
        } else {
            self.s.context_menu = None;
        }

        Ok(())
    }

    fn on_pointer_down(&mut self, ctx: &Context<Self>, ev: PointerEvent) -> Result<(), Error> {
        ev.prevent_default();

        self.s.context_menu = None;

        match ev.button() {
            LEFT_MOUSE_BUTTON if ev.shift_key() => {
                let e = self.view.to_world(ev.offset());

                self.mouse = Some(e);

                if self.start_rotation(ctx) {
                    ev.prevent_default();
                    return Ok(());
                }
            }
            LEFT_MOUSE_BUTTON => {
                if self.finalize_action(ctx) {
                    self.s.redraw = true;
                    return Ok(());
                }

                let e = self.view.to_world(ev.offset());

                self.mouse = Some(e);
                self.start_translate_on_hit(ctx);
            }
            MIDDLE_MOUSE_BUTTON => {
                ev.prevent_default();
                self.pan_anchor = Some(ev.client());
            }
            _ => {}
        }

        Ok(())
    }

    fn on_pointer_move(&mut self, ev: PointerEvent) -> Result<(), Error> {
        ev.prevent_default();

        if let Some(a) = self.pan_anchor {
            let d = self.view.to_world(ev.client()) - self.view.to_world(a);

            *self.s.pan = *self.s.pan - d;
            self.pan_anchor = Some(ev.client());

            self.s.update_view = true;
            self.s.update_config = true;
            self.s.redraw = true;
        }

        let mouse = self.view.to_world(ev.offset());

        self.mouse = Some(mouse);

        let mut objects = self.objects.borrow_mut();

        if let Some(action) = &mut self.action {
            self.s.apply(&mut objects, action, mouse);
        }

        Ok(())
    }

    fn on_pointer_up(&mut self, ctx: &Context<Self>, ev: PointerEvent) -> Result<bool, Error> {
        let mut update = false;

        match ev.button() {
            LEFT_MOUSE_BUTTON => {
                ev.prevent_default();
                update = self.finalize_action(ctx);
                self.s.redraw |= self.s.arrow_target.remove(&self.s.selected).is_some();
            }
            MIDDLE_MOUSE_BUTTON => {
                self.pan_anchor = None;
            }
            _ => {}
        }

        Ok(update)
    }

    fn on_pointer_leave(&mut self, ev: PointerEvent) -> Result<bool, Error> {
        ev.prevent_default();

        self.s.redraw |= self.action.take().is_some();
        self.pan_anchor = None;
        self.mouse = None;
        self.s.redraw |= self.s.arrow_target.remove(&self.s.selected).is_some();
        Ok(false)
    }

    fn on_wheel(&mut self, ev: WheelEvent) -> Result<(), Error> {
        ev.prevent_default();

        let delta = if ev.delta_y() < 0.0 {
            ZOOM_FACTOR
        } else {
            1.0 / ZOOM_FACTOR
        };

        let zoom = (*self.s.zoom * delta).clamp(0.1, 20.0);

        let after = ViewTransform::new(
            self.view.width,
            self.view.height,
            zoom,
            &self.s.pan,
            &self.cache.extent,
        );

        let mut update = false;
        update |= self.s.zoom.update(zoom);
        update |= self
            .s
            .pan
            .update(*self.s.pan + (self.view.to_world(ev.offset()) - after.to_world(ev.offset())));

        if !update {
            return Ok(());
        }

        self.s.update_view = true;
        self.s.update_config = true;
        self.s.redraw = true;
        Ok(())
    }

    fn drag_end(&mut self, ctx: &Context<Self>, id: RemoteId) -> Result<bool, Error> {
        let Some(drag_over) = self.drag_over.take() else {
            return Ok(false);
        };

        // Refuse to drag zero or non-local objects.
        if id.is_zero() || !id.is_local() {
            return Ok(true);
        }

        let mut objects = self.objects.borrow_mut();
        let mut order = self.order.borrow_mut();

        if objects.is_locked(id)
            && let Some(o) = objects.get(id)
        {
            self.s.modal = Some(Modal::Unlock { object: o.as_ref() });
            return Ok(true);
        }

        let new_group = drag_over.target_group();

        // We have to refuse to drag a group into itself or to drag into a
        // non-local group.
        if id == new_group || !new_group.is_local() {
            return Ok(true);
        }

        let Some(new_sort) = drag_over.new_sort(&objects, &order) else {
            return Ok(true);
        };

        let Some(o) = objects.get_mut(id) else {
            return Ok(true);
        };

        let old_group = o.group.replace(new_group);
        let old_sort = o.sort.replace(new_sort);

        if old_group.is_some() {
            self._set_group =
                self.s
                    .channel
                    .object_updates(ctx, id.id, [(Key::GROUP, o.group.id.into())]);
        }

        if old_sort.is_some() {
            self._set_sort =
                self.s
                    .channel
                    .object_updates(ctx, id.id, [(Key::SORT, o.sort[..].into())]);
        }

        if old_sort.is_some() || old_group.is_some() {
            let old_group = old_group.unwrap_or(*o.group);

            let old_sort = match &old_sort {
                Some(old) => old,
                None => &o.sort[..],
            };

            order.reorder(old_group, old_sort, *o.group, &o.sort[..], id);
        }

        self.s.redraw = true;
        Ok(true)
    }

    fn toggle_mumble_object(&mut self, ctx: &Context<Self>, id: RemoteId) -> bool {
        if id.is_zero() || !id.is_local() {
            return false;
        }

        self.s.context_menu = None;

        let update = if *self.s.mumble_object == id {
            RemoteId::ZERO
        } else {
            id
        };

        *self.s.mumble_object = update;

        self.s._toggle_mumble_request = self
            .s
            .channel
            .updates(ctx, [(Key::MUMBLE_OBJECT, Value::from(update.id))]);

        true
    }

    fn toggle_locked(&mut self, ctx: &Context<Self>, id: RemoteId) -> bool {
        if id.is_zero() || !id.is_local() {
            return false;
        }

        let mut objects = self.objects.borrow_mut();

        let Some(object) = objects.get_mut(id) else {
            return false;
        };

        let Some(locked) = object.as_locked_mut() else {
            return false;
        };

        let new = !**locked;
        **locked = new;

        self._toggle_locked =
            self.s
                .channel
                .object_updates(ctx, id.id, [(Key::LOCKED, new.into())]);

        self.s.redraw = true;
        true
    }

    fn toggle_hidden(&mut self, ctx: &Context<Self>, id: RemoteId) -> bool {
        if id.is_zero() || !id.is_local() {
            return false;
        }

        self.s.context_menu = None;

        let mut objects = self.objects.borrow_mut();

        let Some(object) = objects.get_mut(id) else {
            return false;
        };

        let new_hidden = !*object.hidden;
        *object.hidden = new_hidden;

        let requests = self.s.object_requests.entry(id).or_default();
        requests._toggle_hidden =
            self.s
                .channel
                .object_updates(ctx, id.id, [(Key::HIDDEN, new_hidden.into())]);
        self.s.redraw = true;
        true
    }

    fn toggle_local_hidden(&mut self, ctx: &Context<Self>, id: RemoteId) -> bool {
        if id.is_zero() || !id.is_local() {
            return false;
        }

        self.s.context_menu = None;

        let mut objects = self.objects.borrow_mut();

        let Some(object) = objects.get_mut(id) else {
            return false;
        };

        let new_local_hidden = !*object.local_hidden;
        *object.local_hidden = new_local_hidden;

        let requests = self.s.object_requests.entry(id).or_default();

        requests._toggle_local_hidden = self.s.channel.object_updates(
            ctx,
            id.id,
            [(Key::LOCAL_HIDDEN, new_local_hidden.into())],
        );

        self.s.redraw = true;
        true
    }

    fn toggle_expanded(&mut self, ctx: &Context<Self>, id: RemoteId) -> bool {
        if id.is_zero() || !id.is_local() {
            return false;
        }

        self.s.context_menu = None;

        let mut objects = self.objects.borrow_mut();

        let Some(object) = objects.get_mut(id) else {
            return false;
        };

        let Some(expanded) = object.as_expanded_mut() else {
            return false;
        };

        let new_expanded = !**expanded;
        **expanded = new_expanded;

        let requests = self.s.object_requests.entry(id).or_default();

        requests._expanded =
            self.s
                .channel
                .object_updates(ctx, id.id, [(Key::EXPANDED, new_expanded.into())]);

        true
    }

    #[tracing::instrument(skip_all)]
    fn redraw(&mut self) -> Result<(), Error> {
        let Some(canvas) = &self.canvas else {
            return Ok(());
        };

        let Some(cx) = canvas.get_context("2d")? else {
            return Ok(());
        };

        let Ok(cx) = cx.dyn_into::<CanvasRenderingContext2d>() else {
            return Ok(());
        };

        let order = self.order.borrow();
        let objects = self.objects.borrow();

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        if let Some(image) = self.images.get_id(&self.cache.background) {
            render::draw_background(&cx, &self.view, &self.cache.extent, &image)?;
        }

        if self.cache.show_grid {
            render::draw_grid(&cx, &self.view, &self.cache.extent, *self.s.zoom);
        }

        let selected = self.s.selected;

        for id in order.walk().rev() {
            let Some(o) = objects.get(id) else {
                continue;
            };

            let selected = selected == o.id;
            let arrow_target = selected.then(|| self.s.arrow_target.get(&id)).flatten();

            let Some(mut render) =
                RenderObject::from_data(o, arrow_target, |id| objects.visibility(id))
            else {
                continue;
            };

            if render.base.visibility.is_none()
                || !o.id.is_local() && !render.base.visibility.is_remote()
            {
                continue;
            }

            if let Some(Action::Scale(s)) = &self.action
                && s.object_id == o.id
            {
                render.apply_scale(s.scale);
            }

            render.base.selected = selected;

            match &render.kind {
                RenderObjectKind::Static(this) => {
                    render::draw_static(&cx, &self.view, &render.base, this, &self.images)?;
                }
                RenderObjectKind::Token(this) => {
                    render::draw_token(&cx, &self.view, &render.base, this, &self.images)?;

                    if let Some(look_at) = this.look_at {
                        self.look_ats.push((*look_at, this.color));
                    }
                }
            }
        }

        for (look_at, color) in self.look_ats.drain(..) {
            render::draw_look_at(&cx, &self.view, look_at, color)?;
        }

        Ok(())
    }
}

trait MouseEventExt {
    /// The offset read-only property of the MouseEvent interface provides the
    /// offset in the coordinate of the mouse pointer between that event and the
    /// padding edge of the target node.
    fn offset(&self) -> Canvas2;

    /// The client read-only property of the MouseEvent interface provides the
    /// coordinate within the application's viewport at which the event occurred
    /// (as opposed to the coordinate within the page).
    fn client(&self) -> Canvas2;
}

impl MouseEventExt for MouseEvent {
    #[inline]
    fn offset(&self) -> Canvas2 {
        Canvas2::new(self.offset_x() as f64, self.offset_y() as f64)
    }

    #[inline]
    fn client(&self) -> Canvas2 {
        Canvas2::new(self.client_x() as f64, self.client_y() as f64)
    }
}
