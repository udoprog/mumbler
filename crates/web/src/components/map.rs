use core::mem;

use std::collections::{HashMap, HashSet};

use api::{Color, Id, Key, PeerId, RemoteObject, Transform, Value, Vec3, World};
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
use yew_router::prelude::Link;

use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::ws;

use super::navigation::Route;
use super::render::{self, RenderAvatar, ViewTransform};

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const MOVEMENT_SPEED: f32 = 5.0;
const ANIMATION_FPS: u32 = 60;
const HELP: &str = "LMB to move (drag to update) / Shift to look / MMB to pan / Scroll to zoom";

pub(crate) struct PeerObject {
    pub(crate) peer_id: PeerId,
    pub(crate) object: LocalObject,
}

pub(crate) struct LocalObject {
    pub(crate) id: Id,
    pub(crate) transform: Transform,
    pub(crate) look_at: Option<Vec3>,
    pub(crate) image: Option<Id>,
    pub(crate) color: Option<Color>,
    pub(crate) name: Option<String>,
    pub(crate) move_target: Option<Vec3>,
}

impl LocalObject {
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
            move_target: None,
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
    _remote_avatar_listener: ws::Listener,
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
    local_objects: HashMap<Id, LocalObject>,
    /// The list of remote objects.
    remote_avatars: Vec<PeerObject>,
    /// Mouse position at the start of a middle-mouse drag.
    pan_anchor: Option<(f64, f64)>,
    /// Canvas-space position where the left button was pressed.
    start_press: Option<(f32, f32, bool)>,
    /// Canvas-space mouse position once the drag exceeds the arrow threshold.
    arrow_target: Option<(f32, f32)>,
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
}

pub(crate) enum Msg {
    KeyDown(KeyboardEvent),
    KeyUp(KeyboardEvent),
    InitializeMap(Result<Packet<api::InitializeMap>, ws::Error>),
    RemoteAvatarUpdate(Result<Packet<api::RemoteAvatarUpdate>, ws::Error>),
    TransformUpdated(Result<Packet<api::Update>, ws::Error>),
    LookAtUpdated(Result<Packet<api::Update>, ws::Error>),
    WorldUpdated(Result<Packet<api::UpdateWorld>, ws::Error>),
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

        let remote_avatar_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::RemoteAvatarUpdate>(ctx.link().callback(Msg::RemoteAvatarUpdate));

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
            _remote_avatar_listener: remote_avatar_listener,
            _state_change,
            state,
            log,
            _log_handle,
            canvas_sizer: NodeRef::default(),
            canvas_ref: NodeRef::default(),
            world: api::World::zero(),
            selected: None,
            local_objects: HashMap::new(),
            remote_avatars: Vec::new(),
            update_transform_ids: HashSet::new(),
            update_look_at_ids: HashSet::new(),
            update_world: false,
            pan_anchor: None,
            start_press: None,
            arrow_target: None,
            _resize_observer: None,
            images: Images::new(),
            animation_interval: None,
            mouse_world_pos: None,
            _keydown_listener,
            _keyup_listener,
            _create_object: ws::Request::new(),
            _delete_object: ws::Request::new(),
            confirm_delete: None,
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
            && self.local_objects.values().any(|o| o.move_target.is_some())
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

        if let Some(object) = self.selected.and_then(|id| self.local_objects.get(&id)) {
            let p = object.transform.position;
            let f = object.transform.front;

            let position = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", p.x, p.y, p.z);
            let front = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", f.x, f.y, f.z);
            pos = Some(html!(<div class="pre">{position}{" / "}{front}</div>))
        } else {
            pos = None;
        }

        html! {
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
                        {for self.local_objects.values().map(|object| {
                            let object_id = object.id;

                            let selected = self.selected == Some(object_id);
                            let on_click = ctx.link().callback(move |_| Msg::SelectObject(object_id));
                            let classes = classes!("object-list-item", selected.then_some("selected"));
                            let label = object.name.as_deref().unwrap_or("object");

                            if self.confirm_delete == Some(object_id) {
                                html! {
                                    <div class={classes}>
                                        <span class="object-list-label">{"Delete?"}</span>
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
                                        <span class="object-list-label">{label}</span>
                                        <Link<Route> to={Route::ObjectSettings { id: object_id }}>
                                            <button class="btn sm square object-list-action" title="Object settings"
                                                onclick={|e: MouseEvent| e.stop_propagation()}>
                                                <span class="icon cog" />
                                            </button>
                                        </Link<Route>>
                                        <button class="btn sm square danger object-list-action" title="Delete object"
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
            Msg::SetLog(log) => {
                self.log = log;
                Ok(false)
            }
            Msg::InitializeMap(result) => {
                let body = result?;
                let body = body.decode()?;

                tracing::debug!(?body, "Initialize");

                self.world = body.world;
                self.local_objects = body
                    .objects
                    .iter()
                    .map(|object| (object.id, LocalObject::from_remote(object)))
                    .collect();

                self.remote_avatars = body
                    .remote_avatars
                    .iter()
                    .map(|p| PeerObject {
                        peer_id: p.peer_id,
                        object: LocalObject::from_remote(&p.object),
                    })
                    .collect();

                self.load_initialize_images(ctx);
                self.redraw()?;
                Ok(true)
            }
            Msg::RemoteAvatarUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                tracing::debug!(?body, "Remote avatar update");

                match body {
                    api::RemoteAvatarUpdateBody::RemoteLost => {
                        self.remote_avatars.clear();
                    }
                    api::RemoteAvatarUpdateBody::Join {
                        peer_id, objects, ..
                    } => {
                        for object in objects {
                            let local = LocalObject::from_remote(&object);

                            if let Some(id) = local.image {
                                self.images.load(ctx, id);
                            }

                            self.remote_avatars.push(PeerObject {
                                peer_id,
                                object: local,
                            });
                        }
                    }
                    api::RemoteAvatarUpdateBody::Leave { peer_id } => {
                        self.remote_avatars.retain(|a| a.peer_id != peer_id);
                    }
                    api::RemoteAvatarUpdateBody::Update {
                        object_id,
                        peer_id,
                        key,
                        value,
                    } => {
                        if let Some(a) = self
                            .remote_avatars
                            .iter_mut()
                            .find(|a| a.peer_id == peer_id && a.object.id == object_id)
                        {
                            if key == Key::IMAGE_ID {
                                let new = value.as_id();

                                let old = mem::replace(&mut a.object.image, new);

                                if let Some(old) = old {
                                    self.images.remove(old);
                                }

                                if let Some(new) = new {
                                    self.images.load(ctx, new);
                                }
                            }

                            a.object.update(key, value);
                        }
                    }
                    api::RemoteAvatarUpdateBody::ObjectAdded { peer_id, object } => {
                        let local = LocalObject::from_remote(&object);

                        if let Some(id) = local.image {
                            self.images.load(ctx, id);
                        }

                        self.remote_avatars.push(PeerObject {
                            peer_id,
                            object: local,
                        });
                    }
                    api::RemoteAvatarUpdateBody::ObjectRemoved { peer_id, object_id } => {
                        if let Some(pos) = self
                            .remote_avatars
                            .iter()
                            .position(|a| a.peer_id == peer_id && a.object.id == object_id)
                        {
                            let removed = self.remote_avatars.remove(pos);

                            if let Some(id) = removed.object.image {
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
                    id: response.id,
                    transform: api::Transform::origin(),
                    look_at: None,
                    image: None,
                    color: None,
                    name: None,
                    move_target: None,
                };

                self.selected = Some(object.id);
                self.local_objects.insert(object.id, object);
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

                self.local_objects.remove(&id);

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

        let Some(object) = self.selected.and_then(|id| self.local_objects.get_mut(&id)) else {
            return Ok(());
        };

        let id = object.id;
        object.look_at = None;
        self.update_look_at_ids.insert(id);
        self.start_press = None;
        self.arrow_target = None;
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

        let Some(object) = self.selected.and_then(|id| self.local_objects.get_mut(&id)) else {
            return Ok(());
        };

        let (px, py) = (object.transform.position.x, object.transform.position.z);
        let obj_id = object.id;
        self.start_press = Some((px, py, true));
        object.look_at = Some(Vec3::new(mx, 0.0, my));
        self.look_at(px, py, mx, my);
        self.update_look_at_ids.insert(obj_id);
        self.redraw()?;
        Ok(())
    }

    fn interpolate_movement(&mut self) {
        let selected = self.selected;

        for object in self.local_objects.values_mut() {
            let p = object.transform.position;

            'move_done: {
                let Some(target) = object.move_target else {
                    break 'move_done;
                };

                let dx = target.x - p.x;
                let dz = target.z - p.z;
                let distance = (dx * dx + dz * dz).sqrt();

                if distance < 0.01 {
                    object.transform.position = target;
                    object.move_target = None;
                    self.update_transform_ids.insert(object.id);
                    break 'move_done;
                }

                let step = MOVEMENT_SPEED / ANIMATION_FPS as f32;
                let move_distance = step.min(distance);
                let ratio = move_distance / distance;

                object.transform.position.x += dx * ratio;
                object.transform.position.z += dz * ratio;

                // Face the movement direction unless a look_at target is active.
                if object.look_at.is_none() {
                    let angle_rad = dz.atan2(dx);
                    object.transform.front = Vec3::new(angle_rad.cos(), 0.0, angle_rad.sin());
                }

                self.update_transform_ids.insert(object.id);
            };

            'look_done: {
                let Some(target) = object.look_at else {
                    break 'look_done;
                };

                if selected == Some(object.id) {
                    self.arrow_target = Some((target.x, target.z));
                }

                let angle_rad = (target.z - p.z).atan2(target.x - p.x);
                object.transform.front = Vec3::new(angle_rad.cos(), 0.0, angle_rad.sin());
                self.update_transform_ids.insert(object.id);
            };
        }

        if self.local_objects.values().all(|o| o.move_target.is_none()) {
            self.animation_interval = None;
        }
    }

    fn send_transform_updates(&mut self, ctx: &Context<Self>) {
        if !matches!(self.state, ws::State::Open) {
            self.update_transform_ids.clear();
            return;
        }

        for id in self.update_transform_ids.drain() {
            let Some(object) = self.local_objects.get(&id) else {
                continue;
            };

            let req = ctx
                .props()
                .ws
                .request()
                .body(api::UpdateRequest {
                    id,
                    key: Key::TRANSFORM,
                    value: Value::from(object.transform),
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
            let Some(object) = self.local_objects.get(&id) else {
                continue;
            };

            let req = ctx
                .props()
                .ws
                .request()
                .body(api::UpdateRequest {
                    id,
                    key: Key::LOOK_AT,
                    value: Value::from(object.look_at),
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
            .body(api::UpdateWorldRequest {
                pan: self.world.pan,
                zoom: self.world.zoom,
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

                    let Some(object) = self.selected.and_then(|id| self.local_objects.get_mut(&id))
                    else {
                        break 'out false;
                    };

                    self.arrow_target = None;

                    let t = ViewTransform::new(&canvas, &self.world);

                    let (ex, ey) = (e.offset_x() as f64, e.offset_y() as f64);
                    let (ex, ey) = t.canvas_to_world(ex, ey);

                    let obj_id = object.id;
                    if e.shift_key() {
                        let (px, py) = (object.transform.position.x, object.transform.position.z);
                        self.start_press = Some((px, py, true));
                        object.look_at = Some(Vec3::new(ex, 0.0, ey));
                        self.look_at(px, py, ex, ey);
                        self.update_look_at_ids.insert(obj_id);
                    } else {
                        self.start_press = Some((ex, ey, false));
                        object.move_target = Some(Vec3::new(ex, 0.0, ey));
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
            if let Some(object) = self.selected.and_then(|id| self.local_objects.get_mut(&id)) {
                if shift_key {
                    let dist = (mx - px).hypot(my - py);
                    if dist >= ARROW_THRESHOLD {
                        self.update_look_at_ids.insert(object.id);
                        object.look_at = Some(Vec3::new(mx, 0.0, my));
                        self.look_at(px, py, mx, my);
                        needs_redraw = true;
                    }
                } else {
                    object.move_target = Some(Vec3::new(mx, 0.0, my));
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
        let Some(object) = self.selected.and_then(|id| self.local_objects.get_mut(&id)) else {
            return;
        };

        let id = object.id;
        self.arrow_target = Some((mx, my));
        let angle_rad = (my - py).atan2(mx - px);
        let dir_x = angle_rad.cos();
        let dir_z = angle_rad.sin();
        object.transform.front = Vec3::new(dir_x, 0.0, dir_z);
        self.update_transform_ids.insert(id);
    }

    fn on_mouse_up(&mut self, e: MouseEvent) -> Result<(), Error> {
        let needs_redraw = {
            match e.button() {
                0 => {
                    self.start_press = None;
                    self.arrow_target = None;
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
        let needs_redraw = self.arrow_target.is_some() || self.start_press.is_some();

        self.pan_anchor = None;
        self.start_press = None;
        self.arrow_target = None;
        self.mouse_world_pos = None;

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

        let ids = self.local_objects.values().filter_map(|o| o.image);
        let ids = ids.chain(self.remote_avatars.iter().filter_map(|a| a.object.image));

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

        let avatars = || {
            let remotes = self
                .remote_avatars
                .iter()
                .map(|a| RenderAvatar::from_remote(&a.object));

            let locals = self.local_objects.values().map(move |a| {
                let mut avatar = RenderAvatar::from_player(a);
                avatar.selected = selected == Some(a.id);
                avatar
            });

            remotes.chain(locals)
        };

        for a in avatars() {
            render::draw_avatar_token(&cx, &t, &a, token_radius, self.arrow_target, |id| {
                self.images.get(id).cloned()
            })?;
        }

        for a in avatars() {
            if let Some(target) = a.look_at {
                render::draw_look_at(&cx, &t, target, &a.color, self.world.zoom as f64)?;
            }
        }

        Ok(())
    }
}
