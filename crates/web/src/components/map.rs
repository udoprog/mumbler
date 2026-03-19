use std::collections::{HashMap, HashSet};

use api::{Color, Extent, Id, Key, LocalUpdateBody, Pan, RemoteUpdateBody, Value, Vec3};
use gloo::events::EventListener;
use gloo::file::callbacks::{FileReader, read_as_bytes};
use gloo::timers::callback::Interval;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    CanvasRenderingContext2d, DragEvent, HtmlCanvasElement, HtmlElement, HtmlImageElement,
    KeyboardEvent, MouseEvent, ResizeObserver, Url, WheelEvent,
};
use yew::prelude::*;

use crate::components::Icon;
use crate::components::render::{Canvas2, RenderObject, RenderObjectKind};
use crate::drag_over::DragOver;
use crate::error::Error;
use crate::hierarchy::Hierarchy;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::objects::{LocalObject, ObjectData, ObjectKind, Objects, ObjectsRef, PeerObject};
use crate::peers::Peers;
use crate::state::State;

use super::render::{self, ViewTransform};
use super::{GroupSettings, HelpModal, ObjectList, StaticSettings, TokenSettings};

const LEFT_MOUSE_BUTTON: i16 = 0;
const MIDDLE_MOUSE_BUTTON: i16 = 1;

const ZOOM_FACTOR: f32 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const ANIMATION_FPS: u32 = 60;

#[derive(Default)]
struct Updates {
    look_at: HashSet<Id>,
    transforms: HashSet<Id>,
    selected: Id,
    context_menu: Option<ContextMenu>,
    delete: Id,
    _toggle_mumble_request: ws::Request,
}

impl Updates {
    fn look_at(&mut self, objects: &mut ObjectsRef, p: Vec3, m: Vec3) {
        let Some(o) = objects.get_mut(self.selected) else {
            return;
        };

        let Some(transform) = o.data.as_transform_mut() else {
            return;
        };

        o.arrow_target = Some(m);
        transform.front = p.direction_to(m);
        self.transforms.insert(o.id);
    }

    fn select_object(
        &mut self,
        ctx: &Context<Map>,
        id: Id,
        config: &mut Config,
        objects: &ObjectsRef,
    ) {
        self.selected = id;
        self.context_menu = None;

        if self.delete == id {
            self.delete = Id::ZERO;
        }

        if !*config.mumble_follow || *config.mumble_object == id {
            return;
        }

        if !id.is_zero() && !objects.is_interactive(id) {
            return;
        }

        *config.mumble_object = id;

        self._toggle_mumble_request =
            update_config(ctx, vec![(Key::MUMBLE_OBJECT, Value::from(id))]);
    }
}

struct ScaleState {
    id: Id,
    scale: f32,
    position: Vec3,
    initial_distance: f32,
}

pub(crate) struct Config {
    pub(crate) zoom: State<f32>,
    pub(crate) pan: State<Pan>,
    pub(crate) extent: State<Extent>,
    pub(crate) mumble_object: State<Id>,
    pub(crate) mumble_follow: State<bool>,
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
            Key::MUMBLE_FOLLOW => self.mumble_follow.update(value.as_bool().unwrap_or(false)),
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
            mumble_object: State::new(Id::ZERO),
            mumble_follow: State::new(false),
        }
    }
}

struct DropImage {
    _onerror: Closure<dyn FnMut()>,
    _onload: Closure<dyn FnMut()>,
    _img: HtmlImageElement,
    bytes: Option<Vec<u8>>,
    content_type: String,
    file_reader: Option<FileReader>,
    pixel_size: Option<(u32, u32)>,
    url: String,
    world_pos: Vec3,
    world_size: Option<(f32, f32)>,
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

impl Drop for DropImage {
    #[inline]
    fn drop(&mut self) {
        let _ = Url::revoke_object_url(&self.url);
    }
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
    _create_dropped_object: ws::Request,
    _create_group: ws::Request,
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
    _set_group: ws::Request,
    _set_mumble_follow: ws::Request,
    _set_sort: ws::Request,
    _state_change: ws::StateListener,
    _toggle_locked: ws::Request,
    _update_world: ws::Request,
    _upload_image: ws::Request,
    animation_interval: Option<Interval>,
    canvas_ref: NodeRef,
    canvas_sizer: NodeRef,
    config: Config,
    drag_over: Option<DragOver>,
    drop_image: Option<DropImage>,
    images: Images<Self>,
    log: log::Log,
    look_at_requests: HashMap<Id, ws::Request>,
    mouse_position: Option<Vec3>,
    needs_redraw: bool,
    object_ondragend: Callback<Id>,
    object_ondragover: Callback<DragOver>,
    object_onexpandtoggle: Callback<Id>,
    object_onhiddentoggle: Callback<Id>,
    object_onlocalhiddentoggle: Callback<Id>,
    object_onlockedtoggle: Callback<Id>,
    object_onmumbletoggle: Callback<Id>,
    object_onselect: Callback<Id>,
    object_requests: HashMap<Id, ObjectRequests>,
    objects: Objects,
    open_help: bool,
    open_settings: Option<Id>,
    order: Hierarchy,
    pan_anchor: Option<(f64, f64)>,
    peers: Peers,
    scaling: Option<ScaleState>,
    start_press: Option<(Vec3, bool)>,
    rotation_offset: f32,
    state: ws::State,
    transform_requests: HashMap<Id, ws::Request>,
    update_world: bool,
    updates: Updates,
    look_ats: Vec<(Vec3, Color)>,
}

pub(crate) enum Msg {
    AnimationFrame,
    CancelDelete,
    CloseContextMenu,
    CloseSettings,
    OpenHelp,
    CloseHelp,
    ConfigResult(Result<Packet<api::UpdateConfig>, ws::Error>),
    ConfigUpdate(Result<Packet<api::ConfigUpdate>, ws::Error>),
    ConfirmDelete(Id),
    ContextMenu(MouseEvent),
    CreateToken,
    CreateStatic,
    CreateGroup,
    DeleteObject(Id),
    DragEnd(Id),
    DragOver(DragOver),
    CanvasDragOver(DragEvent),
    DropImage(DragEvent),
    DropImageLoaded(u32, u32),
    DropImageData(Result<Vec<u8>, gloo::file::FileReadError>),
    DropImageUploaded(Result<Packet<api::UploadImage>, ws::Error>),
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
    SelectObject(Id),
    SetLog(log::Log),
    StateChanged(ws::State),
    ToggleFollowMumbleSelection,
    ToggleHidden(Id),
    ToggleLocalHidden(Id),
    ToggleExpanded(Id),
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
            _create_dropped_object: ws::Request::new(),
            _create_group: ws::Request::new(),
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
            _set_group: ws::Request::new(),
            _set_mumble_follow: ws::Request::new(),
            _set_sort: ws::Request::new(),
            _state_change,
            _toggle_locked: ws::Request::new(),
            _update_world: ws::Request::new(),
            _upload_image: ws::Request::new(),
            animation_interval: None,
            canvas_ref: NodeRef::default(),
            canvas_sizer: NodeRef::default(),
            config: Config::default(),
            drag_over: None,
            drop_image: None,
            images: Images::new(),
            log,
            look_at_requests: HashMap::new(),
            mouse_position: None,
            needs_redraw: false,
            object_ondragend: ctx.link().callback(Msg::DragEnd),
            object_ondragover: ctx.link().callback(Msg::DragOver),
            object_onexpandtoggle: ctx.link().callback(Msg::ToggleExpanded),
            object_onhiddentoggle: ctx.link().callback(Msg::ToggleHidden),
            object_onlocalhiddentoggle: ctx.link().callback(Msg::ToggleLocalHidden),
            object_onlockedtoggle: ctx.link().callback(Msg::ToggleLocked),
            object_onmumbletoggle: ctx.link().callback(Msg::ToggleMumbleObject),
            object_onselect: ctx.link().callback(Msg::SelectObject),
            object_requests: HashMap::new(),
            objects: Objects::default(),
            open_help: false,
            open_settings: None,
            order: Hierarchy::default(),
            pan_anchor: None,
            peers: Peers::default(),
            scaling: None,
            start_press: None,
            rotation_offset: 0.0,
            state,
            transform_requests: HashMap::new(),
            update_world: false,
            updates: Updates::default(),
            look_ats: Vec::new(),
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

        if !self.updates.transforms.is_empty() {
            self.send_transform_updates(ctx);
        }

        if !self.updates.look_at.is_empty() {
            self.send_look_at_updates(ctx);
        }

        if self.update_world {
            self._update_world = update_config(ctx, self.config.world_values());
            self.update_world = false;
        }

        if self.needs_redraw {
            if let Err(error) = self.redraw() {
                self.log.error("map::redraw", error);
            }

            self.needs_redraw = false;
        }

        let objects = self.objects.borrow();

        if self.animation_interval.is_none() && objects.values().any(|o| o.move_target.is_some()) {
            let link = ctx.link().clone();

            let interval = Interval::new(1000 / ANIMATION_FPS, move || {
                link.send_message(Msg::AnimationFrame);
            });

            self.animation_interval = Some(interval);
        }

        changed
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let ws = &ctx.props().ws;
        let objects = self.objects.borrow();

        let pos;

        if let Some(o) = objects.get(self.updates.selected)
            && let Some(transform) = o.as_transform()
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
            let o = objects.get(self.updates.selected);

            let settings_classes = classes! {
                "btn",
                "square",
                o.is_some().then_some("primary"),
                o.is_none().then_some("disabled"),
            };

            let settings_click = o.map(|o| {
                let id = o.id;
                ctx.link().callback(move |_| Msg::OpenObjectSettings(id))
            });

            let delete_click = o.map(|o| {
                let id = o.id;
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
                    <button class="btn square primary" title="Add group" onclick={ctx.link().callback(|_| Msg::CreateGroup)}>
                        <Icon name="folder-plus" title="Add group" />
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
            let o = objects.get(self.updates.selected);

            let mumble = {
                let is_mumble = o
                    .map(|o| *self.config.mumble_object == o.id)
                    .unwrap_or_default();

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

                let hidden_icon = if is_hidden { "link-slash" } else { "link" };

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

                let name = if is_local_hidden { "eye-slash" } else { "eye" };

                let title = if is_local_hidden {
                    "Hidden from everyone"
                } else {
                    "Visible to everyone"
                };

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
                        <Icon {name} />
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
                    self.config.mumble_follow.then_some("success"),
                };

                let title = if *self.config.mumble_follow {
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
                    <button class="btn square" title="Keyboard shortcuts (F1)" onclick={ctx.link().callback(|_| Msg::OpenHelp)}>
                        <Icon name="question-mark-circle" />
                    </button>
                }
            };

            html! {
                <div class="control-group">
                    {mumble}
                    {hidden}
                    {local_hidden}
                    {locked}
                    <div class="fill"></div>
                    {follow}
                    {help}
                </div>
            }
        };

        html! {
            <ContextProvider<Objects> context={self.objects.clone()}>
                <ContextProvider<Hierarchy> context={self.order.clone()}>
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
                                    ondragover={ctx.link().callback(|ev: DragEvent| { ev.prevent_default(); Msg::CanvasDragOver(ev) })}
                                    ondrop={ctx.link().callback(|ev: DragEvent| { ev.prevent_default(); Msg::DropImage(ev) })}
                                ></canvas>

                                if let Some(menu) = &self.updates.context_menu {
                                    {self.render_context_menu(ctx, menu)}
                                }
                            </div>

                            {pos}
                        </div>

                        <div class="col-3 rows">
                            {object_list_header}

                            <ObjectList
                                key={format!("{}", Id::ZERO)}
                                group={Id::ZERO}
                                drag_over={self.drag_over}
                                mumble_object={*self.config.mumble_object}
                                selected={self.updates.selected}
                                onselect={self.object_onselect.clone()}
                                ondragover={self.object_ondragover.clone()}
                                ondragend={self.object_ondragend.clone()}
                                onhiddentoggle={self.object_onhiddentoggle.clone()}
                                onlocalhiddentoggle={self.object_onlocalhiddentoggle.clone()}
                                onexpandtoggle={self.object_onexpandtoggle.clone()}
                                onlockedtoggle={self.object_onlockedtoggle.clone()}
                                onmumbletoggle={self.object_onmumbletoggle.clone()}
                                />
                        </div>
                    </div>

                    if let Some(id) = self.updates.delete.as_non_zero() {
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
                                    <p>{format!("Remove \"{}\"?", objects.get(id).and_then(|o| o.name()).unwrap_or("unnamed"))}</p>
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
                        <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::CloseSettings)}>
                            <div class="modal" onclick={|ev: MouseEvent| ev.stop_propagation()}>
                                <div class="modal-header">
                                    <h2>{"Settings"}</h2>
                                    <button class="btn sm square danger" title="Close"
                                        onclick={ctx.link().callback(|_| Msg::CloseSettings)}>
                                        <Icon name="x-mark" />
                                    </button>
                                </div>
                                <div class="modal-body">
                                    {match objects.get(id).map(|o| &o.kind).unwrap_or(&ObjectKind::Unknown) {
                                        ObjectKind::Static(..) => {
                                            html! { <StaticSettings {ws} {id} /> }
                                        }
                                        ObjectKind::Group(..) => {
                                            html! { <GroupSettings {ws} {id} /> }
                                        }
                                        ObjectKind::Token(..) => {
                                            html! { <TokenSettings {ws} {id} /> }
                                        }
                                        _ => html! { <p class="hint">{"Unknown object type"}</p> },
                                    }}
                                </div>
                            </div>
                        </div>
                    }

                    if self.open_help {
                        <HelpModal onclose={ctx.link().callback(|_| Msg::CloseHelp)} />
                    }
                </ContextProvider<Hierarchy>>
            </ContextProvider<Objects>>
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

    fn on_drop_image(&mut self, ctx: &Context<Self>, ev: DragEvent) -> Result<bool, Error> {
        ev.prevent_default();

        // Already processing a drop.
        if self.drop_image.is_some() {
            return Ok(false);
        }

        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(false);
        };

        let t = ViewTransform::new(&canvas, &self.config);
        let world_pos = Canvas2::new(ev.offset_x() as f64, ev.offset_y() as f64);
        let world_pos = t.to_world(world_pos);

        let Some(dt) = ev.data_transfer() else {
            return Ok(false);
        };

        let Some(files) = dt.files() else {
            return Ok(false);
        };

        let Some(file) = files.get(0) else {
            return Ok(false);
        };

        let content_type = file.type_();

        if !content_type.starts_with("image/") {
            return Ok(false);
        }

        let Ok(url) = Url::create_object_url_with_blob(&file) else {
            return Ok(false);
        };

        let img = HtmlImageElement::new()?;
        let link = ctx.link().clone();
        let img_clone = img.clone();

        let onload = Closure::<dyn FnMut()>::new(move || {
            let w = img_clone.natural_width();
            let h = img_clone.natural_height();
            link.send_message(Msg::DropImageLoaded(w, h));
        });

        let error_link = ctx.link().clone();

        let onerror = Closure::<dyn FnMut()>::new(move || {
            tracing::warn!("failed to load dropped image");
            error_link.send_message(Msg::DropImageLoaded(0, 0));
        });

        img.set_onload(Some(onload.as_ref().unchecked_ref()));
        img.set_onerror(Some(onerror.as_ref().unchecked_ref()));
        img.set_src(&url);

        let gloo_file = gloo::file::File::from(file);

        let link = ctx.link().clone();

        let file_reader = read_as_bytes(&gloo_file, move |res| {
            link.send_message(Msg::DropImageData(res));
        });

        self.drop_image = Some(DropImage {
            _onerror: onerror,
            _onload: onload,
            _img: img,
            bytes: None,
            content_type,
            file_reader: Some(file_reader),
            pixel_size: None,
            url,
            world_pos,
            world_size: None,
        });

        Ok(false)
    }

    fn try_create_dropped_object(&mut self, ctx: &Context<Self>) -> Result<bool, Error> {
        if !self
            .drop_image
            .as_ref()
            .is_some_and(|image| image.is_ready_for_upload())
        {
            return Ok(false);
        }

        let Some(drop_image) = &mut self.drop_image else {
            return Ok(false);
        };

        let Some((image_width, image_height)) = drop_image.pixel_size else {
            return Ok(false);
        };

        let Some(data) = drop_image.bytes.take() else {
            return Ok(false);
        };

        self._upload_image = ctx
            .props()
            .ws
            .request()
            .body(api::UploadImageRequest {
                content_type: drop_image.content_type.clone(),
                data,
                crop: api::CropRegion {
                    x1: 0,
                    y1: 0,
                    x2: image_width,
                    y2: image_height,
                },
                sizing: api::ImageSizing::Crop,
                size: 512,
            })
            .on_packet(ctx.link().callback(Msg::DropImageUploaded))
            .send();

        Ok(false)
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::DragOver(drag_over) => {
                self.drag_over = Some(drag_over);
                Ok(true)
            }
            Msg::DragEnd(object_id) => self.drag_end(ctx, object_id),
            // Removed misplaced enum variants
            Msg::OpenObjectSettings(id) => {
                self.updates.context_menu = None;
                self.open_settings = Some(id);
                Ok(true)
            }
            Msg::CloseSettings => {
                self.open_settings = None;
                Ok(true)
            }
            Msg::OpenHelp => {
                self.open_help = true;
                Ok(true)
            }
            Msg::CloseHelp => {
                self.open_help = false;
                Ok(true)
            }
            Msg::ToggleMumbleObject(id) => {
                self.updates.context_menu = None;

                let update = if *self.config.mumble_object == id {
                    Id::ZERO
                } else {
                    id
                };

                *self.config.mumble_object = update;

                self.updates._toggle_mumble_request =
                    update_config(ctx, vec![(Key::MUMBLE_OBJECT, Value::from(update))]);
                Ok(true)
            }
            Msg::ToggleLocked(id) => self.toggle_locked(ctx, id),
            Msg::ConfigResult(result) => {
                result?;
                Ok(false)
            }
            Msg::ToggleHidden(id) => {
                self.updates.context_menu = None;

                let mut objects = self.objects.borrow_mut();

                let Some(object) = objects.get_mut(id) else {
                    return Ok(false);
                };

                let new_hidden = !*object.hidden;
                *object.hidden = new_hidden;

                let requests = self.object_requests.entry(id).or_default();
                requests._toggle_hidden = update(ctx, id, Key::HIDDEN, new_hidden);
                Ok(true)
            }
            Msg::ToggleLocalHidden(id) => {
                self.updates.context_menu = None;

                let mut objects = self.objects.borrow_mut();

                let Some(object) = objects.get_mut(id) else {
                    return Ok(false);
                };

                let new_local_hidden = !*object.local_hidden;
                *object.local_hidden = new_local_hidden;

                let requests = self.object_requests.entry(id).or_default();
                requests._toggle_local_hidden =
                    update(ctx, id, Key::LOCAL_HIDDEN, new_local_hidden);
                Ok(true)
            }
            Msg::ToggleExpanded(id) => {
                self.updates.context_menu = None;

                let mut objects = self.objects.borrow_mut();

                let Some(object) = objects.get_mut(id) else {
                    return Ok(false);
                };

                let Some(expanded) = object.as_expanded_mut() else {
                    return Ok(false);
                };

                let new_expanded = !**expanded;
                **expanded = new_expanded;

                let requests = self.object_requests.entry(id).or_default();
                requests._expanded = update(ctx, id, Key::EXPANDED, new_expanded);
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

                self.config = Config::from_config(body.config);

                self.objects = body.objects.iter().map(LocalObject::from_remote).collect();
                self.order
                    .borrow_mut()
                    .extend(self.objects.borrow().values());

                self.peers = body
                    .remote_objects
                    .iter()
                    .map(PeerObject::from_peer)
                    .collect();

                self.images.clear();

                for image_id in body.images {
                    self.images.load(ctx, image_id);
                }

                for image_id in body.remote_images {
                    self.images.load(ctx, image_id);
                }

                self.needs_redraw = true;
                Ok(true)
            }
            Msg::ConfigUpdate(body) => {
                let body = body?;
                let body = body.decode()?;
                let changed = self.config.update(body.key, body.value);
                self.needs_redraw = changed;
                Ok(changed)
            }
            Msg::LocalUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let update = {
                    let mut objects = self.objects.borrow_mut();
                    let mut order = self.order.borrow_mut();

                    match body {
                        LocalUpdateBody::ObjectCreated { object } => {
                            let o = LocalObject::from_remote(&object);
                            order.insert(*o.group, o.sort().to_vec(), o.id);
                            objects.insert(o.id, o);
                            true
                        }
                        LocalUpdateBody::ObjectRemoved { object_id } => {
                            if let Some(o) = objects.remove(object_id) {
                                order.remove(*o.group, o.sort().to_vec(), o.id);
                            }

                            if self.updates.selected == object_id {
                                self.updates.select_object(
                                    ctx,
                                    Id::ZERO,
                                    &mut self.config,
                                    &objects,
                                );
                            }

                            self.object_requests.remove(&object_id);
                            true
                        }
                        LocalUpdateBody::Update {
                            object_id,
                            key,
                            value,
                        } => {
                            'done: {
                                let Some(o) = objects.get_mut(object_id) else {
                                    break 'done false;
                                };

                                let update = match key {
                                    // Don't support local updates of transform and
                                    // look at because they cause feedback loops
                                    // which are laggy.
                                    Key::TRANSFORM | Key::LOOK_AT => {
                                        break 'done false;
                                    }
                                    Key::SORT => {
                                        let Some((_, sort)) = o.sort_mut() else {
                                            break 'done false;
                                        };

                                        let new = value.as_bytes().unwrap_or_default().to_vec();

                                        let Some(old) = sort.replace(new) else {
                                            break 'done false;
                                        };

                                        order.remove(*o.group, old, o.id);
                                        order.insert(*o.group, o.sort().to_vec(), o.id);
                                        true
                                    }
                                    _ => false,
                                };

                                o.update(key, value) || update
                            }
                        }
                        LocalUpdateBody::ImageAdded { image_id, .. } => {
                            self.images.load(ctx, image_id);
                            false
                        }
                        LocalUpdateBody::ImageRemoved { image_id, .. } => {
                            self.images.remove(image_id);
                            false
                        }
                    }
                };

                self.needs_redraw = true;
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
                        peer_id,
                        objects,
                        images,
                    } => {
                        for object in objects {
                            let data = ObjectData::from_remote(&object);

                            self.peers
                                .insert(peer_id, data.id, PeerObject { peer_id, data });
                        }

                        for id in images {
                            self.images.load(ctx, id);
                        }
                    }
                    RemoteUpdateBody::Leave { peer_id } => {
                        self.peers.remove_peer(peer_id);
                    }
                    RemoteUpdateBody::Update {
                        object_id,
                        peer_id,
                        key,
                        value,
                    } => 'done: {
                        let Some(a) = self.peers.get_mut(peer_id, object_id) else {
                            break 'done;
                        };

                        a.update(key, value);
                    }
                    RemoteUpdateBody::ObjectAdded { peer_id, object } => {
                        let data = ObjectData::from_remote(&object);

                        self.peers
                            .insert(peer_id, data.id, PeerObject { peer_id, data });
                    }
                    RemoteUpdateBody::ObjectRemoved { peer_id, object_id } => {
                        self.peers.remove(peer_id, object_id);
                    }
                    RemoteUpdateBody::ImageAdded { image_id, .. } => {
                        self.images.load(ctx, image_id);
                    }
                    RemoteUpdateBody::ImageRemoved { image_id, .. } => {
                        self.images.remove(image_id);
                    }
                }

                self.needs_redraw = true;
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
                self.needs_redraw = true;
                Ok(false)
            }
            Msg::ImageMessage(msg) => {
                self.images.update(msg);
                self.needs_redraw = true;
                Ok(false)
            }
            Msg::CanvasDragOver(ev) => {
                ev.prevent_default();
                Ok(false)
            }
            Msg::DropImage(ev) => self.on_drop_image(ctx, ev),
            Msg::DropImageLoaded(width, height) => {
                let Some(drop_image) = &mut self.drop_image else {
                    return Ok(false);
                };

                drop_image.world_size = Some(DropImage::compute_world_image_size(width, height));
                drop_image.pixel_size = Some((width, height));
                self.try_create_dropped_object(ctx)
            }
            Msg::DropImageData(result) => match result {
                Ok(data) => {
                    let Some(drop_image) = &mut self.drop_image else {
                        return Ok(false);
                    };

                    drop_image.bytes = Some(data);
                    drop_image.file_reader = None;
                    self.try_create_dropped_object(ctx)
                }
                Err(err) => {
                    self.log.error(
                        "drop image",
                        Error::from(anyhow::anyhow!("file read error: {err}")),
                    );
                    Ok(false)
                }
            },
            Msg::DropImageUploaded(result) => {
                let body = result?;
                let body = body.decode()?;

                let Some(drop_image) = self.drop_image.take() else {
                    return Ok(false);
                };

                let world_pos = drop_image.world_pos;
                let (width, height) = drop_image.world_size.unwrap_or((2.0, 2.0));

                let transform = api::Transform::new(world_pos, api::Vec3::FORWARD);

                self._create_dropped_object = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: api::Type::STATIC,
                        props: api::Properties::from([
                            (Key::NAME, Value::from("Image")),
                            (Key::HIDDEN, Value::from(true)),
                            (Key::IMAGE_ID, Value::from(body.id)),
                            (Key::TRANSFORM, Value::from(transform)),
                            (Key::STATIC_WIDTH, Value::from(width)),
                            (Key::STATIC_HEIGHT, Value::from(height)),
                        ]),
                    })
                    .on_packet(ctx.link().callback(Msg::ObjectCreated))
                    .send();

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
                self.needs_redraw = true;
                Ok(false)
            }
            Msg::KeyDown(ev) => self.on_key_down(ctx, ev),
            Msg::KeyUp(ev) => self.on_key_up(ctx, ev),
            Msg::SelectObject(id) => {
                self.cancel_scaling();

                let objects = self.objects.borrow();
                self.updates
                    .select_object(ctx, id, &mut self.config, &objects);
                Ok(true)
            }
            Msg::ToggleFollowMumbleSelection => {
                *self.config.mumble_follow = !*self.config.mumble_follow;

                self._set_mumble_follow = update_config(
                    ctx,
                    vec![(Key::MUMBLE_FOLLOW, Value::from(*self.config.mumble_follow))],
                );
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
                    .on_packet(ctx.link().callback(Msg::ObjectCreated))
                    .send();

                Ok(false)
            }
            Msg::CreateGroup => {
                self._create_group = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::CreateObjectRequest {
                        ty: api::Type::GROUP,
                        props: api::Properties::from([
                            (Key::NAME, Value::from("Group")),
                            (Key::HIDDEN, Value::from(false)),
                            (Key::EXPANDED, Value::from(true)),
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
            Msg::ConfirmDelete(id) => {
                self.updates.context_menu = None;
                self.updates.delete = id;
                Ok(true)
            }
            Msg::CancelDelete => {
                self.updates.delete = Id::ZERO;
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

                self.updates.delete = Id::ZERO;
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
                self.updates.context_menu = None;
                Ok(true)
            }
        }
    }

    fn on_key_up(&mut self, _ctx: &Context<Self>, ev: KeyboardEvent) -> Result<bool, Error> {
        let key = ev.key();

        match key.as_str() {
            "Shift" => {
                let Some((_, true)) = self.start_press else {
                    return Ok(false);
                };

                let mut objects = self.objects.borrow_mut();

                let Some(o) = objects.get_mut(self.updates.selected) else {
                    return Ok(false);
                };

                o.arrow_target = None;
                self.start_press = None;

                let object_id = o.id;

                if let Some(look_at) = o.as_look_at_mut() {
                    **look_at = None;
                    self.updates.look_at.insert(object_id);
                }

                self.needs_redraw = true;
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    fn cancel_scaling(&mut self) -> bool {
        if self.scaling.take().is_some() {
            self.needs_redraw = true;
            return true;
        }

        false
    }

    fn update_scaling(&mut self, mouse: Vec3) -> bool {
        let Some(scale) = &mut self.scaling else {
            return false;
        };

        let distance = scale.position.dist(mouse).max(0.01);
        scale.scale = distance / scale.initial_distance;
        true
    }

    fn finalize_scaling(&mut self, ctx: &Context<Self>) -> bool {
        let Some(scale) = self.scaling.take() else {
            return false;
        };

        let mut objects = self.objects.borrow_mut();

        let Some(o) = objects.get_mut(scale.id) else {
            return false;
        };

        match &o.kind {
            ObjectKind::Static(s) => {
                let width = *s.width * scale.scale;
                let height = *s.height * scale.scale;

                if (width - *s.width).abs() > f32::EPSILON {
                    let requests = self.object_requests.entry(scale.id).or_default();
                    requests._scale_width = update(ctx, scale.id, Key::STATIC_WIDTH, width);
                }

                if (height - *s.height).abs() > f32::EPSILON {
                    let requests = self.object_requests.entry(scale.id).or_default();
                    requests._scale_height = update(ctx, scale.id, Key::STATIC_HEIGHT, height);
                }
            }
            ObjectKind::Token(t) => {
                let radius = *t.token_radius * scale.scale;

                if (radius - *t.token_radius).abs() > f32::EPSILON {
                    let requests = self.object_requests.entry(scale.id).or_default();
                    requests._scale_radius = update(ctx, scale.id, Key::TOKEN_RADIUS, radius);
                }
            }
            _ => {}
        }

        true
    }

    fn on_key_down(&mut self, ctx: &Context<Self>, ev: KeyboardEvent) -> Result<bool, Error> {
        let key = ev.key();

        match key.as_str() {
            "Delete" => {
                if self.updates.delete == self.updates.selected {
                    return Ok(false);
                }

                ev.prevent_default();
                self.updates.context_menu = None;
                self.updates.delete = self.updates.selected;
                Ok(true)
            }
            "Enter" => {
                if !self.updates.delete.is_zero() {
                    ev.prevent_default();
                    ctx.link()
                        .send_message(Msg::DeleteObject(self.updates.delete));
                }

                Ok(false)
            }
            "F1" | "?" => {
                ev.prevent_default();

                self.open_help = !self.open_help;
                Ok(true)
            }
            "s" | "S" => {
                ev.prevent_default();

                let Some(m) = self.mouse_position else {
                    return Ok(false);
                };

                let mut objects = self.objects.borrow_mut();

                if self.updates.selected.is_zero() {
                    return Ok(false);
                }

                if objects.is_locked(self.updates.selected) {
                    return Ok(false);
                };

                let Some(o) = objects.get_mut(self.updates.selected) else {
                    return Ok(false);
                };

                let Some(position) = o.as_transform().map(|t| t.position) else {
                    return Ok(false);
                };

                let distance = position.dist(m).max(0.01);

                o.move_target = None;

                self.scaling = Some(ScaleState {
                    id: self.updates.selected,
                    scale: 1.0,
                    position,
                    initial_distance: distance,
                });

                Ok(false)
            }
            "t" | "T" => {
                if self.updates.selected.is_zero() {
                    return Ok(false);
                }

                ev.prevent_default();
                self.toggle_locked(ctx, self.updates.selected)
            }
            "r" | "R" => self.start_rotation(ev),
            "Escape" => {
                ev.prevent_default();

                if self.cancel_scaling() {
                    return Ok(false);
                }

                self.open_settings = None;
                self.open_help = false;

                self.updates.delete = Id::ZERO;
                self.updates.selected = Id::ZERO;
                self.updates.context_menu = None;
                Ok(true)
            }
            "Shift" => self.start_rotation(ev),
            _ => Ok(false),
        }
    }

    fn start_rotation(&mut self, ev: KeyboardEvent) -> Result<bool, Error> {
        ev.prevent_default();

        let Some(m) = self.mouse_position else {
            return Ok(false);
        };

        let mut objects = self.objects.borrow_mut();

        if self.updates.selected.is_zero() {
            return Ok(false);
        }

        if objects.is_locked(self.updates.selected) {
            return Ok(false);
        }

        let Some(o) = objects.get_mut(self.updates.selected) else {
            return Ok(false);
        };

        let object_id = o.id;

        if let Some(look_at) = o.as_look_at_mut() {
            **look_at = Some(Vec3::new(m.x, 0.0, m.z));
            self.updates.look_at.insert(object_id);
        }

        if let Some(transform) = o.as_transform() {
            let p = transform.position;
            self.start_press = Some((p, true));

            self.rotation_offset = if o.is_static() {
                let cursor = m - p;
                transform.front.angle_xz() - cursor.angle_xz()
            } else {
                0.0
            };

            self.updates.look_at(&mut objects, p, m);
        }

        self.needs_redraw = true;
        Ok(false)
    }

    fn interpolate_movement(&mut self) {
        let mut objects = self.objects.borrow_mut();

        for o in objects.values_mut() {
            let id = o.id;

            let Some((transform, look_at, speed)) = o.data.as_interpolate_mut() else {
                continue;
            };

            let p = transform.position;

            'move_done: {
                let (Some(target), Some(speed)) = (&o.move_target, speed) else {
                    break 'move_done;
                };

                let dx = target.x - p.x;
                let dy = target.y - p.y;
                let dz = target.z - p.z;
                let distance = (dx * dx + dy * dy + dz * dz).sqrt();

                if distance < 0.01 {
                    transform.position = *target;
                    o.move_target = None;
                    self.updates.transforms.insert(id);
                    break 'move_done;
                }

                let step = speed / ANIMATION_FPS as f32;
                let move_distance = step.min(distance);
                let ratio = move_distance / distance;

                transform.position.x += dx * ratio;
                transform.position.y += dy * ratio;
                transform.position.z += dz * ratio;

                // Face the movement direction unless a look_at target is active.
                if look_at.is_none() {
                    transform.front = p.direction_to(*target);
                }

                self.updates.transforms.insert(id);
            };

            'look_done: {
                let Some(t) = look_at else {
                    break 'look_done;
                };

                o.arrow_target = Some(*t);
                transform.front = p.direction_to(*t);
                self.updates.transforms.insert(id);
            };
        }

        if objects.values().all(|o| o.move_target.is_none()) {
            self.animation_interval = None;
        }
    }

    fn send_transform_updates(&mut self, ctx: &Context<Self>) {
        if !matches!(self.state, ws::State::Open) {
            self.updates.transforms.clear();
            return;
        }

        let objects = self.objects.borrow();

        for id in self.updates.transforms.drain() {
            let Some(o) = objects.get(id) else {
                continue;
            };

            let Some(transform) = o.as_transform() else {
                continue;
            };

            let req = update(ctx, id, Key::TRANSFORM, *transform);
            self.transform_requests.insert(id, req);
        }
    }

    fn send_look_at_updates(&mut self, ctx: &Context<Self>) {
        if !matches!(self.state, ws::State::Open) {
            self.updates.look_at.clear();
            return;
        }

        let objects = self.objects.borrow();

        for id in self.updates.look_at.drain() {
            let Some(o) = objects.get(id) else {
                continue;
            };

            let req = update(ctx, id, Key::LOOK_AT, o.look_at().copied());
            self.look_at_requests.insert(id, req);
        }
    }

    fn on_context_menu(&mut self, _ctx: &Context<Self>, ev: MouseEvent) -> Result<(), Error> {
        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let t = ViewTransform::new(&canvas, &self.config);

        let w = Canvas2::new(ev.offset_x() as f64, ev.offset_y() as f64);
        let w = t.to_world(w);

        let objects = self.objects.borrow();

        let hit = objects
            .values()
            .find(|o| {
                let Some(geometry) = o.as_click_geometry() else {
                    return false;
                };

                geometry.intersects(w)
            })
            .map(|o| o.id);

        if let Some(object_id) = hit {
            self.updates.selected = object_id;
            self.updates.context_menu = Some(ContextMenu {
                object_id,
                x: ev.offset_x() as f64,
                y: ev.offset_y() as f64,
            });
        } else {
            self.updates.context_menu = None;
        }

        Ok(())
    }

    fn render_context_menu(&self, ctx: &Context<Self>, menu: &ContextMenu) -> Html {
        let object_id = menu.object_id;
        let style = format!("left: {}px; top: {}px;", menu.x, menu.y);

        let objects = self.objects.borrow();

        let Some(o) = objects.get(object_id) else {
            return html! {};
        };

        let is_hidden = o.is_hidden();
        let hidden_icon = if is_hidden { "link-slash" } else { "link" };
        let hidden_label = if is_hidden { "Show" } else { "Hide" };
        let local_hidden_icon = if is_hidden { "eye-slash" } else { "eye" };
        let local_hidden_label = if is_hidden {
            "Show locally"
        } else {
            "Hide locally"
        };
        let is_mumble = *self.config.mumble_object == object_id;
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
                        <Icon name={hidden_icon} invert={true} />
                        {hidden_label}
                    </button>
                    <button class="context-menu-item"
                        onclick={ctx.link().callback(move |_| Msg::ToggleLocalHidden(object_id))}>
                        <Icon name={local_hidden_icon} invert={true} />
                        {local_hidden_label}
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

        self.updates.context_menu = None;

        match ev.button() {
            LEFT_MOUSE_BUTTON if ev.shift_key() => {
                let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
                    return Ok(());
                };

                let mut objects = self.objects.borrow_mut();

                let Some(o) = objects.get_mut(self.updates.selected) else {
                    return Ok(());
                };

                let Some(t) = o.as_transform() else {
                    return Ok(());
                };

                let view = ViewTransform::new(&canvas, &self.config);
                let e = Canvas2::new(ev.offset_x() as f64, ev.offset_y() as f64);
                let e = view.to_world(e);
                let p = t.position;
                let id = o.id;

                self.start_press = Some((p, true));

                if o.is_static() {
                    let cursor = e - p;
                    self.rotation_offset = t.front.angle_xz() - cursor.angle_xz();
                } else {
                    self.rotation_offset = 0.0;
                }

                if let Some(look_at) = o.as_look_at_mut() {
                    **look_at = Some(e);
                    self.updates.look_at(&mut objects, p, e);
                    self.updates.look_at.insert(id);
                } else {
                    self.updates.look_at(&mut objects, p, e);
                }
            }
            LEFT_MOUSE_BUTTON => {
                if self.finalize_scaling(ctx) {
                    self.needs_redraw = true;
                    return Ok(());
                }

                let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
                    return Ok(());
                };

                let order = self.order.borrow();
                let mut objects = self.objects.borrow_mut();

                let view = ViewTransform::new(&canvas, &self.config);
                let e = Canvas2::new(ev.offset_x() as f64, ev.offset_y() as f64);
                let e = view.to_world(e);

                let hit = order.walk().flat_map(|id| objects.get(id)).find(|o| {
                    let Some(geometry) = o.as_click_geometry() else {
                        return false;
                    };

                    geometry.intersects(e)
                });

                let hit = hit.map(|o| o.id);

                self.needs_redraw = hit.is_some();

                let id = match (self.updates.selected, hit) {
                    (id, _) if !id.is_zero() => id,
                    (Id::ZERO, Some(hit)) if self.updates.selected != hit => {
                        self.updates
                            .select_object(ctx, hit, &mut self.config, &objects);

                        if objects.get(hit).is_some_and(|o| o.is_token()) {
                            return Ok(());
                        }

                        hit
                    }
                    _ => {
                        return Ok(());
                    }
                };

                if let Some(hit) = hit
                    && hit != id
                    && objects.get(hit).is_some_and(|o| o.is_token())
                {
                    // Forcibly update selection if the other click is a token.
                    self.updates
                        .select_object(ctx, hit, &mut self.config, &objects);
                    return Ok(());
                };

                if objects.is_locked(id) {
                    return Ok(());
                }

                let Some(object) = objects.get_mut(id) else {
                    return Ok(());
                };

                object.arrow_target = None;

                let id = object.id;
                let is_static = object.is_static();

                if is_static {
                    // Check that we hit the thing we are currently dragging.
                    if let Some(hit) = hit
                        && hit != self.updates.selected
                    {
                        self.updates
                            .select_object(ctx, Id::ZERO, &mut self.config, &objects);
                        return Ok(());
                    }

                    // Keep the cursor's offset relative to the object's origin while dragging.
                    // The stored vector is applied to the world cursor position on move.
                    let offset = object
                        .as_transform()
                        .map(|t| t.position - e)
                        .unwrap_or(Vec3::ZERO);

                    self.start_press = Some((offset, false));
                    self.updates.transforms.insert(id);
                } else {
                    self.start_press = Some((e, false));
                    object.move_target = Some(e);
                }
            }
            MIDDLE_MOUSE_BUTTON => {
                ev.prevent_default();
                self.pan_anchor = Some((ev.client_x() as f64, ev.client_y() as f64));
            }
            _ => {}
        }

        Ok(())
    }

    fn on_pointer_move(&mut self, ev: PointerEvent) -> Result<(), Error> {
        ev.prevent_default();

        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let v = ViewTransform::new(&canvas, &self.config);

        if let Some((ax, ay)) = self.pan_anchor {
            let dx = ev.client_x() as f64 - ax;
            let dy = ev.client_y() as f64 - ay;
            *self.config.pan = self.config.pan.add(dx, dy);
            self.pan_anchor = Some((ev.client_x() as f64, ev.client_y() as f64));
            self.update_world = true;
            self.needs_redraw = true;
        }

        let m = Canvas2::new(ev.offset_x() as f64, ev.offset_y() as f64);
        let m = v.to_world(m);

        self.mouse_position = Some(m);

        if self.update_scaling(m) {
            return Ok(());
        }

        let mut objects = self.objects.borrow_mut();

        'done: {
            let Some((p, shift_key)) = self.start_press else {
                break 'done;
            };

            let Some(o) = objects.get_mut(self.updates.selected) else {
                break 'done;
            };

            if shift_key {
                let dist = p.dist(m);

                if dist < ARROW_THRESHOLD {
                    break 'done;
                };

                if o.is_static() {
                    // Use the original cursor offset to rotate relative to the initial grab.
                    if let Some(transform) = o.as_transform_mut() {
                        let cursor = m - p;
                        let angle = cursor.angle_xz() + self.rotation_offset;
                        transform.front = Vec3::new(angle.cos(), transform.front.y, -angle.sin());
                        self.updates.transforms.insert(o.id);
                    }

                    o.arrow_target = Some(m);
                } else if let Some(look_at) = o.as_look_at_mut() {
                    **look_at = Some(Vec3::new(m.x, 0.0, m.z));
                    self.updates.look_at.insert(o.id);
                    self.updates.look_at(&mut objects, p, m);
                }

                self.needs_redraw = true;
                break 'done;
            }

            if o.is_static() {
                let Some(transform) = o.as_transform_mut() else {
                    break 'done;
                };

                transform.position = m + p;
                self.updates.transforms.insert(o.id);
                self.needs_redraw = true;
                break 'done;
            }

            o.move_target = Some(m);
            self.needs_redraw = true;
        }

        Ok(())
    }

    fn on_pointer_up(&mut self, ev: PointerEvent) -> Result<(), Error> {
        self.needs_redraw = {
            match ev.button() {
                LEFT_MOUSE_BUTTON => {
                    self.start_press = None;
                    self.rotation_offset = 0.0;

                    let mut objects = self.objects.borrow_mut();

                    if let Some(object) = objects.get_mut(self.updates.selected) {
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

        Ok(())
    }

    fn on_pointer_leave(&mut self) -> Result<(), Error> {
        let mut objects = self.objects.borrow_mut();

        let selected_arrow = objects
            .get(self.updates.selected)
            .and_then(|o| o.arrow_target);

        self.needs_redraw = selected_arrow.is_some() || self.start_press.is_some();

        self.pan_anchor = None;
        self.start_press = None;
        self.rotation_offset = 0.0;
        self.mouse_position = None;

        if let Some(object) = objects.get_mut(self.updates.selected) {
            object.arrow_target = None;
        }

        Ok(())
    }

    fn on_wheel(&mut self, ev: WheelEvent) -> Result<(), Error> {
        let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        ev.prevent_default();

        let delta = if ev.delta_y() < 0.0 {
            ZOOM_FACTOR
        } else {
            1.0 / ZOOM_FACTOR
        };

        let view_before = ViewTransform::new(&canvas, &self.config);
        *self.config.zoom = (*self.config.zoom * delta).clamp(0.1, 20.0);
        let view_after = ViewTransform::new(&canvas, &self.config);

        let c1 = Canvas2::new(ev.offset_x() as f64, ev.offset_y() as f64);
        let c2 = view_after.to_canvas(view_before.to_world(c1));

        self.config.pan.x += c1.x - c2.x;
        self.config.pan.y += c1.y - c2.y;

        self.update_world = true;
        self.needs_redraw = true;
        Ok(())
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

    fn drag_end(&mut self, ctx: &Context<Self>, object_id: Id) -> Result<bool, Error> {
        let Some(drag_over) = self.drag_over.take() else {
            return Ok(false);
        };

        let mut objects = self.objects.borrow_mut();
        let mut order = self.order.borrow_mut();

        let new_group = drag_over.target_group();

        // We have to refuse to drag a group into itself.
        if object_id == new_group {
            return Ok(true);
        }

        let Some(new_sort) = drag_over.new_sort(&objects, &order) else {
            return Ok(true);
        };

        let Some((o_group, o_sort)) = objects.get_mut(object_id).and_then(|o| o.sort_mut()) else {
            return Ok(true);
        };

        let old_group = **o_group;
        let old_sort = o_sort.to_vec();

        let group_changed = o_group.update(new_group);
        let sort_changed = o_sort.update(new_sort.clone());

        if group_changed {
            self._set_group = self::update(ctx, object_id, Key::GROUP, Value::from(new_group));
        }

        if sort_changed {
            self._set_sort = self::update(ctx, object_id, Key::SORT, Value::from(new_sort.clone()));
        }

        if sort_changed || group_changed {
            order.remove(old_group, old_sort, object_id);
            order.insert(new_group, new_sort, object_id);
        }

        Ok(true)
    }

    fn toggle_locked(&mut self, ctx: &Context<Self>, id: Id) -> Result<bool, Error> {
        let mut objects = self.objects.borrow_mut();

        let Some(object) = objects.get_mut(id) else {
            return Ok(false);
        };

        let Some(locked) = object.as_locked_mut() else {
            return Ok(false);
        };

        let new = !**locked;
        **locked = new;
        self._toggle_locked = update(ctx, id, Key::LOCKED, Value::from(new));
        self.needs_redraw = true;
        Ok(true)
    }

    fn redraw(&mut self) -> Result<(), Error> {
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

        let view = ViewTransform::new(&canvas, &self.config);

        render::draw_grid(&cx, &view, &self.config.extent, *self.config.zoom);

        let selected = self.updates.selected;

        let order = self.order.borrow();
        let objects = self.objects.borrow();

        // Draw remote static objects.
        for peer in self.peers.iter() {
            let Some(render) =
                RenderObject::from_data(peer, None, |id| self.peers.visibility(peer.peer_id, id))
            else {
                continue;
            };

            if render.base.visibility.is_hidden() {
                continue;
            }

            match &render.kind {
                RenderObjectKind::Static(this) => {
                    render::draw_static(&cx, &view, &render.base, this, |id| {
                        self.images.get(id).cloned()
                    })?;
                }
                RenderObjectKind::Token(this) => {
                    render::draw_token(&cx, &view, &render.base, this, |id| {
                        self.images.get(id).cloned()
                    })?;

                    if let Some(look_at) = this.look_at {
                        self.look_ats.push((*look_at, this.color));
                    }
                }
            }
        }

        for id in order.walk().rev() {
            let Some(data) = objects.get(id) else {
                continue;
            };

            let selected = selected == data.id;

            let arrow_target = selected.then_some(data.arrow_target.as_ref()).flatten();

            let Some(mut render) =
                RenderObject::from_data(data, arrow_target, |id| objects.visibility(id))
            else {
                continue;
            };

            if render.base.visibility.is_local_hidden() {
                continue;
            }

            if let Some(s) = &self.scaling
                && s.id == data.id
            {
                render.apply_scale(s.scale);
            }

            render.base.selected = selected;
            render.base.player = true;

            match &render.kind {
                RenderObjectKind::Static(this) => {
                    render::draw_static(&cx, &view, &render.base, this, |id| {
                        self.images.get(id).cloned()
                    })?;
                }
                RenderObjectKind::Token(this) => {
                    render::draw_token(&cx, &view, &render.base, this, |id| {
                        self.images.get(id).cloned()
                    })?;

                    if let Some(look_at) = this.look_at {
                        self.look_ats.push((*look_at, this.color));
                    }
                }
            }
        }

        for (look_at, color) in self.look_ats.drain(..) {
            render::draw_look_at(&cx, &view, look_at, color)?;
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

fn update_config(ctx: &Context<Map>, values: Vec<(Key, Value)>) -> ws::Request {
    ctx.props()
        .ws
        .request()
        .body(api::UpdateConfigRequest { values })
        .on_packet(ctx.link().callback(Msg::ConfigResult))
        .send()
}
