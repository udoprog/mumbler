use core::mem;

use std::collections::{HashMap, HashSet};

use api::{
    Color, Config, Extent, Id, Key, LocalUpdateBody, Pan, PeerId, RemoteObject, RemotePeerObject,
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

use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::ws;

use super::ObjectSettings;
use super::render::{self, RenderAvatar, ViewTransform};

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const MOVEMENT_SPEED: f32 = 5.0;
const ANIMATION_FPS: u32 = 60;
const HELP: &str = "LMB to move (drag to update) / Shift to look / MMB to pan / Scroll to zoom";

pub(crate) struct World {
    pub(crate) zoom: f32,
    pub(crate) pan: Pan,
    pub(crate) extent: Extent,
    pub(crate) token_radius: f32,
}

impl World {
    fn from_globals(globals: &Config) -> Self {
        let mut this = Self::default();

        for (key, value) in &globals.values {
            this.update(*key, value);
        }

        this
    }

    fn update(&mut self, key: Key, value: &Value) {
        match key {
            Key::WORLD_SCALE => {
                self.zoom = value.as_float().unwrap_or(2.0);
            }
            Key::WORLD_PAN => {
                self.pan = value.as_pan().unwrap_or_else(Pan::zero);
            }
            Key::WORLD_EXTENT => {
                self.extent = value.as_extent().unwrap_or_else(Extent::arena);
            }
            Key::WORLD_TOKEN_RADIUS => {
                self.token_radius = value.as_float().unwrap_or(0.5);
            }
            _ => {}
        }
    }

    fn values(&self) -> Vec<(Key, Value)> {
        let mut values = Vec::new();
        values.push((Key::WORLD_SCALE, Value::from(self.zoom)));
        values.push((Key::WORLD_PAN, Value::from(self.pan)));
        values.push((Key::WORLD_EXTENT, Value::from(self.extent)));
        values.push((Key::WORLD_TOKEN_RADIUS, Value::from(self.token_radius)));
        values
    }
}

impl Default for World {
    fn default() -> Self {
        Self {
            zoom: 10.0,
            pan: Pan::zero(),
            extent: Extent::arena(),
            token_radius: 0.5,
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
}

pub(crate) struct ObjectData {
    pub(crate) id: Id,
    pub(crate) transform: Transform,
    pub(crate) look_at: Option<Vec3>,
    pub(crate) image: Option<Id>,
    pub(crate) color: Option<Color>,
    pub(crate) name: Option<String>,
}

impl ObjectData {
    fn from_remote(remote: &RemoteObject) -> Self {
        Self {
            id: remote.id,
            transform: remote
                .properties
                .get(&Key::TRANSFORM)
                .and_then(|v| v.as_transform())
                .unwrap_or_else(Transform::origin),
            look_at: remote
                .properties
                .get(&Key::LOOK_AT)
                .and_then(|v| v.as_vec3()),
            image: remote
                .properties
                .get(&Key::IMAGE_ID)
                .and_then(|v| v.as_id()),
            color: remote
                .properties
                .get(&Key::COLOR)
                .and_then(|v| v.as_color()),
            name: remote
                .properties
                .get(&Key::NAME)
                .and_then(|v| v.as_string().map(str::to_owned)),
        }
    }

    fn update(&mut self, key: Key, value: Value) {
        match key {
            Key::TRANSFORM => {
                self.transform = value.as_transform().unwrap_or_else(Transform::origin);
            }
            Key::LOOK_AT => {
                self.look_at = value.as_vec3();
            }
            Key::IMAGE_ID => {
                self.image = value.as_id();
            }
            Key::COLOR => {
                self.color = value.as_color();
            }
            Key::NAME => {
                self.name = value.into_string();
            }
            _ => {}
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
    world: World,
    /// Object IDs whose transforms need to be sent to the server.
    update_transform_ids: HashSet<Id>,
    /// Object IDs whose look-at needs to be sent to the server.
    update_look_at_ids: HashSet<Id>,
    /// Whether world settings need to be updated.
    update_world: bool,
    /// The selected object.
    selected: Option<Id>,
    /// The list of local objects.
    objects: HashMap<Id, LocalObject>,
    /// The list of remote objects.
    peers: HashMap<(PeerId, Id), PeerObject>,
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
    /// In-flight delete-object request.
    _delete_object: ws::Request,
    /// Index of the object pending delete confirmation.
    confirm_delete: Option<Id>,
    /// Object whose settings modal is currently open.
    open_settings: Option<Id>,
}

pub(crate) enum Msg {
    KeyDown(KeyboardEvent),
    KeyUp(KeyboardEvent),
    InitializeMap(Result<Packet<api::InitializeMap>, ws::Error>),
    LocalUpdate(Result<Packet<api::LocalUpdate>, ws::Error>),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    TransformUpdated(Result<Packet<api::Update>, ws::Error>),
    LookAtUpdated(Result<Packet<api::Update>, ws::Error>),
    WorldUpdated(Result<Packet<api::UpdateConfig>, ws::Error>),
    ObjectCreated(Result<Packet<api::CreateObject>, ws::Error>),
    StateChanged(ws::State),
    Resized,
    ImageMessage(ImageMessage),
    MouseDown(MouseEvent),
    MouseMove(MouseEvent),
    MouseUp(MouseEvent),
    MouseLeave,
    Wheel(WheelEvent),
    AnimationFrame,
    SelectObject(Id),
    CreateObject,
    ConfirmDelete(Id),
    CancelDelete,
    DeleteObject(Id),
    ObjectDeleted(Id, Result<Packet<api::DeleteObject>, ws::Error>),
    OpenObjectSettings(Id),
    CloseObjectSettings,
    SetLog(log::Log),
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
            world: World::default(),
            selected: None,
            objects: HashMap::new(),
            peers: HashMap::new(),
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
            _delete_object: ws::Request::new(),
            confirm_delete: None,
            open_settings: None,
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
                self.log.error("map::update", &error);
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

            let position = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", p.x, p.y, p.z);
            let front = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", f.x, f.y, f.z);
            pos = Some(html!(<div class="pre">{position}{" / "}{front}</div>))
        } else {
            pos = None;
        }

        html! {
            <>
            <div class="row">
                <div class="col-10 rows">
                    <div class="pre">{HELP}</div>

                    <div class="map-sizer" ref={self.canvas_sizer.clone()}>
                        <canvas id="map" ref={self.canvas_ref.clone()}
                            onmousedown={ctx.link().callback(Msg::MouseDown)}
                            onmousemove={ctx.link().callback(Msg::MouseMove)}
                            onmouseup={ctx.link().callback(Msg::MouseUp)}
                            onmouseleave={ctx.link().callback(|_| Msg::MouseLeave)}
                            onwheel={ctx.link().callback(Msg::Wheel)}
                        ></canvas>
                    </div>

                    {pos}
                </div>

                <div class="col-2 rows">
                    <div class="object-list-header">
                        <strong>{"Objects"}</strong>
                        <button class="btn sm square" title="Add object"
                            onclick={ctx.link().callback(|_| Msg::CreateObject)}>
                            {"+"}
                        </button>
                    </div>

                    <div class="object-list">
                        {for self.objects.values().map(|o| {
                            let object_id = o.data.id;

                            let selected = self.selected == Some(object_id);
                            let on_click = ctx.link().callback(move |_| Msg::SelectObject(object_id));
                            let classes = classes!("object-list-item", selected.then_some("selected"));
                            let label = o.data.name.as_deref().unwrap_or("");

                            if self.confirm_delete == Some(object_id) {
                                html! {
                                    <div class={classes}>
                                        <span class="object-list-item-label">{"Delete?"}</span>
                                        <button class="btn sm square danger" title="Confirm delete"
                                            onclick={ctx.link().callback(move |_| Msg::DeleteObject(object_id))}>
                                            <span class="icon x-mark" />
                                        </button>
                                        <button class="btn sm square" title="Cancel"
                                            onclick={ctx.link().callback(|_| Msg::CancelDelete)}>
                                            <span class="icon check" />
                                        </button>
                                    </div>
                                }
                            } else {
                                html! {
                                    <div class={classes} onclick={on_click}>
                                        <span class="object-list-item-label">{label}</span>
                                        <button class="btn sm square object-list-item-action" title="Object settings"
                                            onclick={ctx.link().callback(move |e: MouseEvent| {
                                                e.stop_propagation();
                                                Msg::OpenObjectSettings(object_id)
                                            })}>
                                            <span class="icon cog" />
                                        </button>
                                        <button class="btn sm square danger object-list-item-action" title="Delete object"
                                            onclick={ctx.link().callback(move |e: MouseEvent| {
                                                e.stop_propagation();
                                                Msg::ConfirmDelete(object_id)
                                            })}>
                                            <span class="icon x-mark" />
                                        </button>
                                    </div>
                                }
                            }
                        })}
                    </div>
                </div>
            </div>

            if let Some(settings_id) = self.open_settings {
                <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::CloseObjectSettings)}>
                    <div class="modal" onclick={|e: MouseEvent| e.stop_propagation()}>
                        <div class="modal-header">
                            <h2>{"Object Settings"}</h2>
                            <button class="btn sm square" title="Close"
                                onclick={ctx.link().callback(|_| Msg::CloseObjectSettings)}>
                                <span class="icon x-mark" />
                            </button>
                        </div>
                        <div class="modal-body">
                            <ObjectSettings ws={ctx.props().ws.clone()} id={settings_id} />
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
                self.open_settings = Some(id);
                Ok(true)
            }
            Msg::CloseObjectSettings => {
                self.open_settings = None;
                Ok(true)
            }
            Msg::SetLog(log) => {
                self.log = log;
                Ok(false)
            }
            Msg::InitializeMap(result) => {
                let body = result?;
                let body = body.decode()?;

                tracing::debug!(?body, "Initialize");

                self.world = World::from_globals(&body.globals);

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
            Msg::LocalUpdate(body) => {
                let body = body?;

                let LocalUpdateBody {
                    object_id,
                    key,
                    value,
                } = body.decode()?;

                if let Some(a) = self.objects.get_mut(&object_id) {
                    if key == Key::IMAGE_ID {
                        let new = value.as_id();

                        let old = mem::replace(&mut a.data.image, new);

                        if let Some(new) = new {
                            self.images.load(ctx, new);
                        }

                        if let Some(old) = old {
                            self.images.remove(old);
                        }
                    }

                    a.data.update(key, value);
                }

                self.redraw()?;
                Ok(false)
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

                            if let Some(id) = data.image {
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
                    } => {
                        if let Some(a) = self.peers.get_mut(&(peer_id, object_id)) {
                            if key == Key::IMAGE_ID {
                                let new = value.as_id();

                                let old = mem::replace(&mut a.data.image, new);

                                if let Some(new) = new {
                                    self.images.load(ctx, new);
                                }

                                if let Some(old) = old {
                                    self.images.remove(old);
                                }
                            }

                            a.data.update(key, value);
                        }
                    }
                    api::RemoteUpdateBody::ObjectAdded { peer_id, object } => {
                        let data = ObjectData::from_remote(&object);

                        if let Some(id) = data.image {
                            self.images.load(ctx, id);
                        }

                        self.peers
                            .insert((peer_id, data.id), PeerObject { peer_id, data });
                    }
                    api::RemoteUpdateBody::ObjectRemoved { peer_id, object_id } => {
                        if let Some(removed) = self.peers.remove(&(peer_id, object_id)) {
                            if let Some(id) = removed.data.image {
                                self.images.remove(id);
                            }
                        }
                    }
                }

                self.redraw()?;
                Ok(false)
            }
            Msg::TransformUpdated(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(false)
            }
            Msg::LookAtUpdated(body) => {
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
                self.on_mouse_down(e)?;
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
            Msg::SelectObject(i) => {
                self.selected = Some(i);
                self.confirm_delete = None;
                Ok(true)
            }
            Msg::CreateObject => {
                self._create_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest)
                    .on_packet(ctx.link().callback(Msg::ObjectCreated))
                    .send();
                Ok(false)
            }
            Msg::ObjectCreated(result) => {
                let result = result?;
                let response = result.decode()?;

                let object = LocalObject {
                    data: ObjectData {
                        id: response.id,
                        transform: api::Transform::origin(),
                        look_at: None,
                        image: None,
                        color: None,
                        name: None,
                    },
                    move_target: None,
                    arrow_target: None,
                };

                self.selected = Some(object.data.id);
                self.objects.insert(object.data.id, object);
                Ok(true)
            }
            Msg::ConfirmDelete(id) => {
                self.confirm_delete = Some(id);
                Ok(true)
            }
            Msg::CancelDelete => {
                self.confirm_delete = None;
                Ok(true)
            }
            Msg::DeleteObject(id) => {
                self._delete_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::DeleteObjectRequest { id })
                    .on_packet(ctx.link().callback(move |r| Msg::ObjectDeleted(id, r)))
                    .send();

                self.confirm_delete = None;
                Ok(false)
            }
            Msg::ObjectDeleted(id, result) => {
                let result = result?;
                _ = result.decode()?;

                self.objects.remove(&id);

                if self.selected == Some(id) {
                    self.selected = None;
                }

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

        let object_id = o.data.id;
        o.data.look_at = None;
        o.arrow_target = None;

        self.update_look_at_ids.insert(object_id);
        self.start_press = None;
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
        o.data.look_at = Some(Vec3::new(mx, 0.0, my));

        let (px, py) = (o.data.transform.position.x, o.data.transform.position.z);
        self.start_press = Some((px, py, true));
        self.look_at(px, py, mx, my);
        self.update_look_at_ids.insert(object_id);
        self.redraw()?;
        Ok(())
    }

    fn interpolate_movement(&mut self) {
        for o in self.objects.values_mut() {
            let p = o.data.transform.position;

            'move_done: {
                let Some(target) = o.move_target else {
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

                let step = MOVEMENT_SPEED / ANIMATION_FPS as f32;
                let move_distance = step.min(distance);
                let ratio = move_distance / distance;

                o.data.transform.position.x += dx * ratio;
                o.data.transform.position.z += dz * ratio;

                // Face the movement direction unless a look_at target is active.
                if o.data.look_at.is_none() {
                    let angle_rad = dz.atan2(dx);
                    o.data.transform.front = Vec3::new(angle_rad.cos(), 0.0, angle_rad.sin());
                }

                self.update_transform_ids.insert(o.data.id);
            };

            'look_done: {
                let Some(target) = o.data.look_at else {
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

            let req = ctx
                .props()
                .ws
                .request()
                .body(api::UpdateRequest {
                    object_id: id,
                    key: Key::TRANSFORM,
                    value: Value::from(o.data.transform),
                    broadcast_self: false,
                })
                .on_packet(ctx.link().callback(Msg::TransformUpdated))
                .send();

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

            let req = ctx
                .props()
                .ws
                .request()
                .body(api::UpdateRequest {
                    object_id: id,
                    key: Key::LOOK_AT,
                    value: Value::from(o.data.look_at),
                    broadcast_self: false,
                })
                .on_packet(ctx.link().callback(Msg::LookAtUpdated))
                .send();

            self.look_at_requests.insert(id, req);
        }
    }

    fn send_world_updates(&mut self, ctx: &Context<Self>) {
        self._update_world = ctx
            .props()
            .ws
            .request()
            .body(api::UpdateConfigRequest {
                values: self.world.values(),
            })
            .on_packet(ctx.link().callback(Msg::WorldUpdated))
            .send();
    }

    fn on_mouse_down(&mut self, e: MouseEvent) -> Result<(), Error> {
        let needs_redraw = 'out: {
            match e.button() {
                0 => {
                    let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
                        break 'out false;
                    };

                    let t = ViewTransform::new(&canvas, &self.world);
                    let (ex, ey) = (e.offset_x() as f64, e.offset_y() as f64);
                    let (ex, ey) = t.canvas_to_world(ex, ey);

                    let r = self.world.token_radius;

                    let hit = self
                        .objects
                        .values()
                        .find(|o| {
                            let p = o.data.transform.position;
                            let dx = p.x - ex;
                            let dz = p.z - ey;
                            dx * dx + dz * dz <= r * r
                        })
                        .map(|o| o.data.id);

                    if let Some(hit_id) = hit {
                        if self.selected != Some(hit_id) {
                            self.selected = Some(hit_id);
                            self.confirm_delete = None;
                            break 'out true;
                        }
                    }

                    let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) else {
                        break 'out hit.is_some();
                    };

                    o.arrow_target = None;

                    let object_id = o.data.id;

                    if e.shift_key() {
                        let (px, py) = (o.data.transform.position.x, o.data.transform.position.z);

                        self.start_press = Some((px, py, true));
                        o.data.look_at = Some(Vec3::new(ex, 0.0, ey));
                        self.look_at(px, py, ex, ey);
                        self.update_look_at_ids.insert(object_id);
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

        let v = ViewTransform::new(&canvas, &self.world);

        let mut needs_redraw = false;

        if let Some((ax, ay)) = self.pan_anchor {
            let dx = e.client_x() as f64 - ax;
            let dy = e.client_y() as f64 - ay;
            self.world.pan = self.world.pan.add(dx, dy);
            self.pan_anchor = Some((e.client_x() as f64, e.client_y() as f64));
            self.update_world = true;
            needs_redraw = true;
        }

        let (mx, my) = v.canvas_to_world(e.offset_x() as f64, e.offset_y() as f64);
        self.mouse_world_pos = Some((mx, my));

        if let Some((px, py, shift_key)) = self.start_press {
            if let Some(o) = self.selected.and_then(|id| self.objects.get_mut(&id)) {
                if shift_key {
                    let dist = (mx - px).hypot(my - py);
                    if dist >= ARROW_THRESHOLD {
                        self.update_look_at_ids.insert(o.data.id);
                        o.data.look_at = Some(Vec3::new(mx, 0.0, my));
                        self.look_at(px, py, mx, my);
                        needs_redraw = true;
                    }
                } else {
                    o.move_target = Some(Vec3::new(mx, 0.0, my));
                    needs_redraw = true;
                }
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

        let t_before = ViewTransform::new(&canvas, &self.world);
        let (wx, wz) = t_before.canvas_to_world(mx, my);

        self.world.zoom = (self.world.zoom * delta).clamp(0.1, 20.0);

        let t_after = ViewTransform::new(&canvas, &self.world);
        let (cx2, cy2) = t_after.world_to_canvas(wx, wz);
        self.world.pan.x += mx - cx2;
        self.world.pan.y += my - cy2;

        self.update_world = true;
        self.redraw()?;
        Ok(())
    }

    /// Resize the canvas according to its parent sizer element.
    fn load_initialize_images(&mut self, ctx: &Context<Self>) {
        self.images.clear();

        let ids = self.objects.values().filter_map(|o| o.data.image);
        let ids = ids.chain(self.peers.values().filter_map(|a| a.data.image));

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

        let t = ViewTransform::new(&canvas, &self.world);

        let token_radius = self.world.token_radius as f64 * t.scale;

        render::draw_grid(&cx, &t, &self.world.extent, self.world.zoom);

        let selected = self.selected;

        let renders = || {
            let remotes = self
                .peers
                .values()
                .map(|a| RenderAvatar::from_data(&a.data));

            let locals = self.objects.values().map(move |a| {
                let mut avatar = RenderAvatar::from_data(&a.data);
                avatar.player = true;
                avatar.selected = selected == Some(a.data.id);
                avatar
            });

            remotes.chain(locals)
        };

        let selected_arrow = self
            .selected
            .and_then(|id| self.objects.get(&id))
            .and_then(|o| o.arrow_target);

        for a in renders() {
            let arrow = a.selected.then_some(selected_arrow).flatten();

            render::draw_avatar_token(&cx, &t, &a, token_radius, arrow, |id| {
                self.images.get(id).cloned()
            })?;
        }

        for a in renders() {
            if let Some(target) = a.look_at {
                render::draw_look_at(&cx, &t, target, &a.color, self.world.zoom as f64)?;
            }
        }

        Ok(())
    }
}
