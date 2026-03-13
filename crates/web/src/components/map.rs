use std::collections::{BTreeSet, HashMap, HashSet};

use api::{Extent, Id, Key, LocalUpdateBody, Pan, PeerId, RemoteUpdateBody, Value, Vec3, VecXZ};
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
use yew::virtual_dom::{VList, VNode};

use crate::components::Icon;
use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::objects::{LocalObject, ObjectData, ObjectKind, Objects, PeerObject};
use crate::state::State;
use crate::ws;

use super::render::{self, RenderStatic, RenderToken, ViewTransform};
use super::{ObjectSettings, StaticSettings};

const LEFT_MOUSE_BUTTON: i16 = 0;
const MIDDLE_MOUSE_BUTTON: i16 = 1;

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const ANIMATION_FPS: u32 = 60;

pub(crate) struct Config {
    pub(crate) zoom: State<f32>,
    pub(crate) pan: State<Pan>,
    pub(crate) extent: State<Extent>,
    pub(crate) mumble_object: State<Option<Id>>,
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
            mumble_object: State::new(None),
            mumble_follow: State::new(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Drag {
    Above,
    Into,
    Below,
}

#[derive(Default)]
struct Hierarchy {
    inner: HashMap<Id, BTreeSet<(Vec<u8>, Id)>>,
}

impl Hierarchy {
    /// Get the children of the given group, sorted by their sort key.
    fn iter(&self, group: Id) -> impl DoubleEndedIterator<Item = Id> {
        self.inner
            .get(&group)
            .into_iter()
            .flatten()
            .map(|(_, id)| *id)
    }

    /// Remove the given id from all groups.
    fn remove(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        let key = (sort, id);

        if let Some(values) = self.inner.get_mut(&group) {
            values.remove(&key);
        }
    }

    /// Insert a child into the given group with the given sort key.
    fn insert(&mut self, group: Id, sort: Vec<u8>, id: Id) {
        self.inner.entry(group).or_default().insert((sort, id));
    }

    /// Extend the hierarchy with the given objects.
    fn extend<'a>(&mut self, objects: impl IntoIterator<Item = &'a LocalObject>) {
        for object in objects {
            self.inner
                .entry(*object.group)
                .or_default()
                .insert((object.sort().to_vec(), object.id));
        }
    }
}

pub(crate) struct Map {
    _create_object: ws::Request,
    _create_static: ws::Request,
    _create_group: ws::Request,
    _delete_object: ws::Request,
    _initialize: ws::Request,
    _keydown_listener: EventListener,
    _keyup_listener: EventListener,
    _local_update_listener: ws::Listener,
    _log_handle: ContextHandle<log::Log>,
    _remote_update_listener: ws::Listener,
    _resize_observer: Option<(ResizeObserver, Closure<dyn FnMut()>)>,
    _set_mumble_follow: ws::Request,
    _set_sort: ws::Request,
    _set_group: ws::Request,
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
    drag_over: Option<(Drag, Id, Id)>,
    hide_requests: HashMap<Id, ws::Request>,
    images: Images<Self>,
    log: log::Log,
    look_at_requests: HashMap<Id, ws::Request>,
    mouse_world_pos: Option<VecXZ>,
    objects: Objects,
    open_settings: Option<Id>,
    order: Hierarchy,
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
    CreateToken,
    CreateStatic,
    CreateGroup,
    DeleteObject(Id),
    DragEnd(Id),
    DragOver(Drag, Id, Id),
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
            _create_group: ws::Request::new(),
            _delete_object: ws::Request::new(),
            _initialize: ws::Request::new(),
            _keydown_listener,
            _keyup_listener,
            _local_update_listener,
            _log_handle,
            _remote_update_listener,
            _resize_observer: None,
            _set_mumble_follow: ws::Request::new(),
            _set_sort: ws::Request::new(),
            _set_group: ws::Request::new(),
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
            objects: Objects::default(),
            open_settings: None,
            order: Hierarchy::default(),
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

        if let Some(o) = self.selected.and_then(|id| self.objects.get(id))
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
            let o = self.selected.and_then(|id| self.objects.get(id));

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
            let o = self.selected.and_then(|id| self.objects.get(id));

            let is_hidden = o.map(|o| o.is_hidden()).unwrap_or_default();
            let is_locked = o.map(|o| o.is_locked()).unwrap_or_default();
            let is_mumble = o
                .map(|o| *self.config.mumble_object == Some(o.id))
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
                self.config.mumble_follow.then_some("success"),
            };

            let follow_title = if *self.config.mumble_follow {
                "Disable MumbleLink selection following"
            } else {
                "Enable MumbleLink selection following"
            };

            let follow_click = ctx.link().callback(|_| Msg::ToggleFollowMumbleSelection);

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

            let hidden_click = o.map(|o| {
                let id = o.id;
                ctx.link().callback(move |_| Msg::ToggleHidden(id))
            });

            let locked_click = o.map(|o| {
                let id = o.id;
                ctx.link().callback(move |_| Msg::ToggleLocked(id))
            });

            let mumble_click = o.filter(|o| o.is_interactive()).map(|o| {
                let id = o.id;
                ctx.link().callback(move |_| Msg::ToggleMumbleObject(id))
            });

            let mumble_classes = classes! {
                "btn", "square",
                is_mumble.then_some("success"),
                mumble_click.is_none().then_some("disabled"),
            };

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
                    <button class={follow_classes} title={follow_title} onclick={follow_click}>
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

                    {self.object_list(ctx, Id::ZERO)}
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
                            <p>{format!("Remove \"{}\"?", self.objects.get(id).and_then(|o| o.name()).unwrap_or("unnamed"))}</p>
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
                            if self.objects.get(id).is_some_and(|o| o.is_static()) {
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
    fn object_list(&self, ctx: &Context<Self>, group: Id) -> Html {
        let mut objects = Vec::new();

        for (n, o) in self
            .order
            .iter(group)
            .flat_map(|id| self.objects.get(id))
            .enumerate()
        {
            let (icon_name, mumble_button, is_group) = match o.kind {
                ObjectKind::Token(..) => ("user", true, false),
                ObjectKind::Static(..) => ("squares-2x2", true, false),
                ObjectKind::Group(..) => ("folder", false, true),
                _ => ("question-mark-circle", false, false),
            };

            let id = o.id;
            let selected = self.selected == Some(id);

            let label = o.name().unwrap_or("");

            let onclick = ctx.link().callback(move |ev: MouseEvent| {
                ev.stop_propagation();
                Msg::SelectObject(Some(id))
            });

            let ondragend = ctx.link().callback(move |ev: DragEvent| {
                ev.stop_propagation();
                Msg::DragEnd(id)
            });

            let ondragstart = ctx.link().callback(move |ev: DragEvent| {
                ev.stop_propagation();
                Msg::DragOver(Drag::Below, group, id)
            });

            let drag_into = if is_group { Drag::Into } else { Drag::Below };

            let ondragover = ctx.link().callback(move |ev: DragEvent| {
                ev.stop_propagation();
                Msg::DragOver(drag_into, group, id)
            });

            if n == 0 {
                let class = classes! {
                    "object-drop",
                    (self.drag_over == Some((Drag::Above, group, id))).then_some("active"),
                };

                let ondragover = ctx.link().callback(move |ev: DragEvent| {
                    ev.stop_propagation();
                    Msg::DragOver(Drag::Above, group, id)
                });

                objects.push(html! {
                    <div key={format!("drop-above-{id}")} {class} {ondragover}>
                        <div class="dotted" />
                    </div>
                });
            }

            let is_hidden = o.is_hidden();
            let is_locked = o.is_locked();
            let hidden_icon = if is_hidden { "eye-slash" } else { "eye" };
            let hidden_title = if is_hidden {
                "Hidden from others"
            } else {
                "Visible to others"
            };

            let hidden_onclick = ctx.link().callback(move |ev: MouseEvent| {
                ev.stop_propagation();
                Msg::ToggleHidden(id)
            });

            let locked_icon = if is_locked {
                "lock-closed"
            } else {
                "lock-open"
            };
            let locked_title = if is_locked { "Locked" } else { "Unlocked" };

            let locked_onclick = ctx.link().callback(move |ev: MouseEvent| {
                ev.stop_propagation();
                Msg::ToggleLocked(id)
            });

            let is_mumble = *self.config.mumble_object == Some(id);

            let mumble_classes = classes! {
                "btn", "sm", "square", "object-action",
                is_mumble.then_some("success"),
                is_mumble.then_some("active"),
                (!mumble_button).then_some("disabled"),
            };

            let mumble_onclick = mumble_button.then(|| {
                ctx.link().callback(move |ev: MouseEvent| {
                    ev.stop_propagation();
                    Msg::ToggleMumbleObject(id)
                })
            });

            let hidden_classes = classes! {
                "btn", "sm", "square", "object-action",
                is_hidden.then_some("danger"),
                is_hidden.then_some("active"),
            };

            let locked_classes = classes! {
                "btn", "sm", "square", "object-action",
                is_locked.then_some("danger"),
                is_locked.then_some("active"),
            };

            let drop_into = (self.drag_over == Some((Drag::Into, group, o.id))).then(|| {
                html! {
                    <div key={format!("drop-into")} class="object-drop active">
                        <div class="dotted" />
                    </div>
                }
            });

            let children = match o.kind {
                ObjectKind::Group(..) => {
                    let children = self.object_list(ctx, o.id);

                    Some(html! {
                        <section key={format!("{id}-children")} class="object-children">
                            {children}
                            {drop_into}
                        </section>
                    })
                }
                _ => None,
            };

            let class = classes! {
                "object-button",
                selected.then_some("selected"),
            };

            let node = html! {
                <div key={format!("object-{id}")} class="object-item">
                    <section
                        key={format!("drag-{id}")}
                        class="object-drag"
                        draggable={true}
                        {onclick}
                        {ondragstart}
                        {ondragend}
                        {ondragover}
                    >
                        <section {class}>
                            <Icon name={icon_name} invert={true} small={true} />

                            <span class="object-label">{label}</span>

                            <button class={mumble_classes}
                                title="Toggle as MumbleLink Source"
                                onclick={mumble_onclick}>
                                <Icon name="mumble" />
                            </button>

                            <button class={hidden_classes}
                                title={hidden_title}
                                onclick={hidden_onclick}>
                                <Icon name={hidden_icon} />
                            </button>

                            <button class={locked_classes}
                                title={locked_title}
                                onclick={locked_onclick}>
                                <Icon name={locked_icon} />
                            </button>
                        </section>
                    </section>

                    {children}
                </div>
            };

            objects.push(node);

            let class = classes! {
                "object-drop",
                (self.drag_over == Some((Drag::Below, group, id))).then_some("active"),
            };

            let ondragover = ctx.link().callback(move |ev: DragEvent| {
                ev.stop_propagation();
                Msg::DragOver(Drag::Below, group, id)
            });

            objects.push(html! {
                <div key={format!("drag-below-{id}")} {class} {ondragover}>
                    <div class="dotted" />
                </div>
            });
        }

        let objects = VNode::from(VList::from(objects));

        html! {
            <div key={"objects-list"} class="object-list">{objects}</div>
        }
    }

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
            Msg::DragOver(drag, group, id) => {
                self.drag_over = Some((drag, group, id));
                Ok(true)
            }
            Msg::DragEnd(object_id) => {
                let Some((drag, group, target_id)) = self.drag_over.take() else {
                    return Ok(false);
                };

                if object_id == target_id {
                    return Ok(true);
                }

                let (new_sort, new_group) = {
                    let Some(target) = self.objects.get(target_id) else {
                        return Ok(true);
                    };

                    let next = match drag {
                        Drag::Into => {
                            // When inserting into, we insert after the last element in the group.
                            self.order.iter(target_id).last()
                        }
                        Drag::Above => self
                            .order
                            .iter(group)
                            .rev()
                            .skip_while(|id| *id != target_id)
                            .nth(1),
                        Drag::Below => self
                            .order
                            .iter(group)
                            .skip_while(|id| *id != target_id)
                            .nth(1),
                    };

                    let next = next.and_then(|id| Some(self.objects.get(id)?.sort()));

                    let sort = match (drag, next) {
                        (Drag::Above, Some(next)) => sorting::midpoint(next, target.sort()),
                        (Drag::Above, _) => sorting::before(target.sort()),
                        (Drag::Below, Some(next)) => sorting::midpoint(target.sort(), next),
                        (Drag::Below, _) => sorting::after(target.sort()),
                        (Drag::Into, Some(next)) => sorting::after(next),
                        (Drag::Into, _) => target.id.as_bytes().to_vec(),
                    };

                    let group = match drag {
                        Drag::Into => target_id,
                        _ => group,
                    };

                    (sort, group)
                };

                let Some((o_group, o_sort)) =
                    self.objects.get_mut(object_id).and_then(|o| o.sort_mut())
                else {
                    return Ok(true);
                };

                let old_group = **o_group;
                let old_sort = o_sort.to_vec();

                let group_changed = o_group.update(new_group);
                let sort_changed = o_sort.update(new_sort.clone());

                if group_changed {
                    self._set_group =
                        self::update(ctx, object_id, Key::GROUP, Value::from(new_group));
                }

                if sort_changed {
                    self._set_sort =
                        self::update(ctx, object_id, Key::SORT, Value::from(new_sort.clone()));
                }

                if sort_changed || group_changed {
                    self.order.remove(old_group, old_sort, object_id);
                    self.order.insert(new_group, new_sort, object_id);
                }

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
                let Some(object) = self.objects.get_mut(id) else {
                    return Ok(false);
                };

                let Some(locked) = object.as_locked_mut() else {
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

                let Some(object) = self.objects.get_mut(id) else {
                    return Ok(false);
                };

                let Some(hidden) = object.as_hidden_mut() else {
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
                    .map(|o| (o.id, (o)))
                    .collect();

                self.order.extend(self.objects.values());

                self.peers = body
                    .remote_objects
                    .iter()
                    .map(PeerObject::from_peer)
                    .map(|peer| ((peer.peer_id, peer.id), peer))
                    .collect();

                self.images.clear();

                for image_id in body.images {
                    self.images.load(ctx, image_id);
                }

                for image_id in body.remote_images {
                    self.images.load(ctx, image_id);
                }

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
                    LocalUpdateBody::ObjectCreated { object } => {
                        let o = LocalObject::from_remote(&object);
                        self.order.insert(*o.group, o.sort().to_vec(), o.id);
                        self.objects.insert(o.id, o);
                        true
                    }
                    LocalUpdateBody::ObjectRemoved { object_id } => {
                        if let Some(o) = self.objects.remove(object_id) {
                            self.order.remove(*o.group, o.sort().to_vec(), o.id);
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
                            let Some(o) = self.objects.get_mut(object_id) else {
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

                                    self.order.remove(*o.group, old, o.id);
                                    self.order.insert(*o.group, o.sort().to_vec(), o.id);
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
                        peer_id,
                        objects,
                        images,
                    } => {
                        for object in objects {
                            let data = ObjectData::from_remote(&object);

                            self.peers
                                .insert((peer_id, data.id), PeerObject { peer_id, data });
                        }

                        for id in images {
                            self.images.load(ctx, id);
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

                        a.update(key, value);
                    }
                    RemoteUpdateBody::ObjectAdded { peer_id, object } => {
                        let data = ObjectData::from_remote(&object);

                        self.peers
                            .insert((peer_id, data.id), PeerObject { peer_id, data });
                    }
                    RemoteUpdateBody::ObjectRemoved { peer_id, object_id } => {
                        self.peers.remove(&(peer_id, object_id));
                    }
                    RemoteUpdateBody::ImageAdded { image_id, .. } => {
                        self.images.load(ctx, image_id);
                    }
                    RemoteUpdateBody::ImageRemoved { image_id, .. } => {
                        self.images.remove(image_id);
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

                'out: {
                    if *self.config.mumble_follow && *self.config.mumble_object != id {
                        if let Some(id) = id
                            && !self.objects.is_interactive(id)
                        {
                            break 'out;
                        }

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
                };

                Ok(true)
            }
            Msg::ToggleFollowMumbleSelection => {
                *self.config.mumble_follow = !*self.config.mumble_follow;

                self._set_mumble_follow = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateConfigRequest {
                        values: vec![(Key::MUMBLE_FOLLOW, Value::from(*self.config.mumble_follow))],
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

        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(id)) else {
            return Ok(());
        };

        o.arrow_target = None;
        self.start_press = None;

        let object_id = o.id;

        if let Some(look_at) = o.as_look_at_mut() {
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

        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(id)) else {
            return Ok(());
        };

        let object_id = o.id;

        if let Some(look_at) = o.as_look_at_mut() {
            **look_at = Some(Vec3::new(m.x, 0.0, m.z));
            self.update_look_at_ids.insert(object_id);
        }

        if let Some(transform) = o.as_transform() {
            let p = transform.position.xz();
            self.start_press = Some((p, true));
            self.look_at(p, m);
        }

        self.redraw()?;
        Ok(())
    }

    fn interpolate_movement(&mut self) {
        for o in self.objects.values_mut() {
            let id = o.id;

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
            let Some(o) = self.objects.get(id) else {
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
            self.update_look_at_ids.clear();
            return;
        }

        for id in self.update_look_at_ids.drain() {
            let Some(o) = self.objects.get(id) else {
                continue;
            };

            let Some(look_at) = o.look_at() else {
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
                let Some((transform, click_radius)) = o.as_click_geometry() else {
                    return false;
                };

                transform.position.xz().dist(w) < click_radius && !o.is_locked()
            })
            .map(|o| o.id);

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

        let Some(o) = self.objects.get(object_id) else {
            return html! {};
        };

        let is_hidden = o.is_hidden();
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
                                let Some((transform, click_radius)) = o.as_click_geometry() else {
                                    return false;
                                };

                                transform.position.xz().dist(e) < click_radius && !o.is_locked()
                            })
                            .map(|o| o.id);

                        if let Some(hit_id) = hit
                            && self.selected != Some(hit_id)
                        {
                            ctx.link().send_message(Msg::SelectObject(Some(hit_id)));
                            self.delete = None;
                            break 'out true;
                        }

                        hit_something = hit.is_some();
                    }

                    let Some(object) = self.selected.and_then(|id| self.objects.get_mut(id)) else {
                        break 'out hit_something;
                    };

                    object.arrow_target = None;

                    let object_id = object.id;
                    let is_static = object.is_static();

                    let Some(transform) = object.as_transform_mut() else {
                        break 'out hit_something;
                    };

                    if ev.shift_key() {
                        let p = transform.position.xz();

                        self.start_press = Some((p, true));

                        if is_static {
                            // Shift-drag on a static object rotates it.
                            self.look_at(p, e);
                        } else if let Some(look_at) = object.as_look_at_mut() {
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

            let Some(o) = self.selected.and_then(|id| self.objects.get_mut(id)) else {
                break 'done;
            };

            if shift_key {
                let dist = p.dist(m);

                if dist < ARROW_THRESHOLD {
                    break 'done;
                };

                if o.is_static() {
                    self.look_at(p, m);
                } else if let Some(look_at) = o.as_look_at_mut() {
                    **look_at = Some(Vec3::new(m.x, 0.0, m.z));
                    self.update_look_at_ids.insert(o.id);
                    self.look_at(p, m);
                }

                needs_redraw = true;
                break 'done;
            }

            if o.is_static()
                && let Some(transform) = o.as_transform_mut()
            {
                // Static objects snap immediately while dragging.
                transform.position = m.xyz(0.0);
                self.update_transform_ids.insert(o.id);
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
        let Some(o) = self.selected.and_then(|id| self.objects.get_mut(id)) else {
            return;
        };

        let Some(transform) = o.data.as_transform_mut() else {
            return;
        };

        o.arrow_target = Some(m);
        transform.front = p.direction_to(m).xyz(0.0);
        self.update_transform_ids.insert(o.id);
    }

    fn on_pointer_up(&mut self, ev: PointerEvent) -> Result<(), Error> {
        let needs_redraw = {
            match ev.button() {
                LEFT_MOUSE_BUTTON => {
                    self.start_press = None;

                    if let Some(object) = self.selected.and_then(|id| self.objects.get_mut(id)) {
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
            .and_then(|id| self.objects.get(id))
            .and_then(|o| o.arrow_target);

        let needs_redraw = selected_arrow.is_some() || self.start_press.is_some();

        self.pan_anchor = None;
        self.start_press = None;
        self.mouse_world_pos = None;

        if let Some(object) = self.selected.and_then(|id| self.objects.get_mut(id)) {
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
            if let Some(mut s) = RenderStatic::from_data(o) {
                s.selected = selected == Some(o.id);
                render::draw_static_token(&cx, &t, &s, |id| self.images.get(id).cloned())?;
            }
        }
        // Draw remote static objects.
        for o in self.peers.values() {
            if let Some(s) = RenderStatic::from_data(o)
                && !s.hidden
            {
                render::draw_static_token(&cx, &t, &s, |id| self.images.get(id).cloned())?;
            }
        }

        let renders = || {
            let remotes = self
                .peers
                .values()
                .flat_map(|peer| RenderToken::from_data(peer))
                .filter(|render| !render.hidden);

            let locals = self.order.iter(Id::ZERO).flat_map(move |id| {
                let data = &self.objects.get(id)?;
                let mut token = RenderToken::from_data(data)?;
                token.player = true;
                token.selected = selected == Some(data.id);
                Some(token)
            });

            remotes.chain(locals)
        };

        let selected_arrow = self
            .selected
            .and_then(|id| self.objects.get(id))
            .and_then(|o| o.arrow_target.as_ref());

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
