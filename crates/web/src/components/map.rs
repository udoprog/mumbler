use api::{Avatar, Key, RemoteAvatar, Value, Vec3, World};
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

use super::render::{self, RenderAvatar, ViewTransform};

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const MOVEMENT_SPEED: f32 = 5.0;
const ANIMATION_FPS: u32 = 60;
const HELP: &str = "LMB to move (drag to update) / Shift to look / MMB to pan / Scroll to zoom";

pub(crate) struct Map {
    _initialize: ws::Request,
    update_transform_request: ws::Request,
    update_look_at_request: ws::Request,
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
    /// List of local avatars that need to be updated remotely.
    update_transform: bool,
    /// Update what the player is looking at.
    update_look_at: bool,
    /// Whether world settings need to be updated.
    update_world: bool,
    /// The player avatar.
    player: Avatar,
    /// Avatars in the world and their positions.
    remote_avatars: Vec<RemoteAvatar>,
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
    /// Target position for interpolated movement.
    move_target: Option<Vec3>,
    /// Animation interval for smooth movement.
    _animation_interval: Option<Interval>,
    /// Current world-space mouse position while cursor is over the canvas.
    mouse_world_pos: Option<(f32, f32)>,
    /// Keeps the document keydown listener alive.
    _keydown_listener: EventListener,
    /// Keeps the document keyup listener alive.
    _keyup_listener: EventListener,
}

pub(crate) enum Msg {
    LogUpdate(log::Log),
    KeyDown(KeyboardEvent),
    KeyUp(KeyboardEvent),
    InitializeMap(Result<Packet<api::InitializeMap>, ws::Error>),
    RemoteAvatarUpdate(Result<Packet<api::RemoteAvatarUpdate>, ws::Error>),
    TransformUpdated(Result<Packet<api::Update>, ws::Error>),
    LookAtUpdated(Result<Packet<api::Update>, ws::Error>),
    WorldUpdated(Result<Packet<api::UpdateWorld>, ws::Error>),
    StateChanged(ws::State),
    Resized,
    ImageLoaded(ImageMessage),
    MouseDown(MouseEvent),
    MouseMove(MouseEvent),
    MouseUp(MouseEvent),
    MouseLeave,
    Wheel(WheelEvent),
    AnimationFrame,
}

impl From<ImageMessage> for Msg {
    #[inline]
    fn from(message: ImageMessage) -> Self {
        Msg::ImageLoaded(message)
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
            .context::<log::Log>(ctx.link().callback(Msg::LogUpdate))
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
            update_transform_request: ws::Request::new(),
            update_look_at_request: ws::Request::new(),
            _update_world: ws::Request::new(),
            _remote_avatar_listener: remote_avatar_listener,
            _state_change,
            state,
            log,
            _log_handle,
            canvas_sizer: NodeRef::default(),
            canvas_ref: NodeRef::default(),
            world: api::World::zero(),
            player: api::Avatar::default(),
            remote_avatars: Vec::new(),
            update_transform: false,
            update_look_at: false,
            update_world: false,
            pan_anchor: None,
            start_press: None,
            arrow_target: None,
            _resize_observer: None,
            images: Images::new(),
            move_target: None,
            _animation_interval: None,
            mouse_world_pos: None,
            _keydown_listener,
            _keyup_listener,
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

        if self.update_transform {
            self.send_transform_update(ctx);
            self.update_transform = false;
        }

        if self.update_look_at {
            self.send_look_at_update(ctx);
            self.update_look_at = false;
        }

        if self.update_world {
            self.send_world_updates(ctx);
            self.update_world = false;
        }

        if self._animation_interval.is_none() && self.move_target.is_some() {
            let link = ctx.link().clone();

            let interval = Interval::new(1000 / ANIMATION_FPS, move || {
                link.send_message(Msg::AnimationFrame);
            });

            self._animation_interval = Some(interval);
        }

        changed
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let p = self.player.transform().position;
        let f = self.player.transform().front;

        let pos = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", p.x, p.y, p.z);
        let front = format!("X:{:.02}, Y:{:.02}, Z:{:.02}", f.x, f.y, f.z);

        html! {
            <div class="rows">
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

                <div class="pre">{pos}{" / "}{front}</div>
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
            Msg::LogUpdate(log) => {
                self.log = log;
                Ok(false)
            }
            Msg::InitializeMap(result) => {
                let body = result?;
                let body = body.decode()?;

                tracing::debug!(?body, "Initialize");

                self.world = body.world;
                self.player = body.player;
                self.remote_avatars = body.remote_avatars;

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
                    api::RemoteAvatarUpdateBody::Join { peer_id, values } => {
                        let a = RemoteAvatar {
                            id: peer_id,
                            values: values.clone(),
                        };

                        if let Some(id) = values.get(&Key::AVATAR_IMAGE_ID).and_then(|v| v.as_id())
                        {
                            self.images.load(ctx, id);
                        }

                        self.remote_avatars.push(a);
                    }
                    api::RemoteAvatarUpdateBody::Leave { peer_id } => {
                        self.remote_avatars.retain(|a| a.id != peer_id);
                    }
                    api::RemoteAvatarUpdateBody::Update {
                        peer_id,
                        key,
                        value,
                    } => {
                        if let Some(a) = self.remote_avatars.iter_mut().find(|a| a.id == peer_id) {
                            if key == Key::AVATAR_IMAGE_ID {
                                let old = a
                                    .values
                                    .insert(Key::AVATAR_IMAGE_ID, value.clone())
                                    .unwrap_or_default();

                                if let Some(old) = old.as_id() {
                                    self.images.remove(old);
                                }

                                if let Some(new) = value.as_id() {
                                    self.images.load(ctx, new);
                                }
                            } else {
                                a.values.insert(key, value.clone());
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
            Msg::ImageLoaded(msg) => {
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
        }
    }

    fn on_key_up(&mut self, e: KeyboardEvent) -> Result<(), Error> {
        if e.key() != "Shift" {
            return Ok(());
        }

        let Some((_, _, true)) = self.start_press else {
            return Ok(());
        };

        self.start_press = None;
        self.arrow_target = None;
        self.player.clear_look_at();
        self.update_look_at = true;
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

        let (px, py) = (
            self.player.transform().position.x,
            self.player.transform().position.z,
        );

        self.start_press = Some((px, py, true));
        self.look_at(px, py, mx, my);
        *self.player.look_at_mut() = Vec3::new(mx, 0.0, my);
        self.update_look_at = true;
        self.redraw()?;
        Ok(())
    }

    fn interpolate_movement(&mut self) {
        let p = self.player.transform().position;

        'done: {
            let Some(target) = self.move_target else {
                break 'done;
            };

            let dx = target.x - p.x;
            let dz = target.z - p.z;
            let distance = (dx * dx + dz * dz).sqrt();

            if distance < 0.01 {
                self.player.transform_mut().position = target;
                self.move_target = None;
                self._animation_interval = None;
                self.update_transform = true;
                break 'done;
            }

            let step = MOVEMENT_SPEED / ANIMATION_FPS as f32;
            let move_distance = step.min(distance);
            let ratio = move_distance / distance;

            self.player.transform_mut().position.x += dx * ratio;
            self.player.transform_mut().position.z += dz * ratio;

            // Face the movement direction unless a look_at target is active.
            if self.player.look_at().is_none() {
                let angle_rad = dz.atan2(dx);
                self.player.transform_mut().front =
                    api::Vec3::new(angle_rad.cos(), 0.0, angle_rad.sin());
            }

            self.update_transform = true;
        };

        'done: {
            let Some(target) = self.player.look_at() else {
                break 'done;
            };

            self.look_at(p.x, p.z, target.x, target.z);
        };
    }

    fn send_transform_update(&mut self, ctx: &Context<Self>) {
        if !matches!(self.state, ws::State::Open) {
            return;
        }

        self.update_transform_request = ctx
            .props()
            .ws
            .request()
            .body(api::UpdateRequest {
                key: Key::AVATAR_TRANSFORM,
                value: Value::from(self.player.transform()),
            })
            .on_packet(ctx.link().callback(Msg::TransformUpdated))
            .send();
    }

    fn send_look_at_update(&mut self, ctx: &Context<Self>) {
        if !matches!(self.state, ws::State::Open) {
            return;
        }

        self.update_look_at_request = ctx
            .props()
            .ws
            .request()
            .body(api::UpdateRequest {
                key: Key::AVATAR_LOOK_AT,
                value: Value::from(self.player.look_at()),
            })
            .on_packet(ctx.link().callback(Msg::LookAtUpdated))
            .send();
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

                    self.arrow_target = None;

                    let t = ViewTransform::new(&canvas, &self.world);

                    let (ex, ey) = (e.offset_x() as f64, e.offset_y() as f64);
                    let (ex, ey) = t.canvas_to_world(ex, ey);

                    if e.shift_key() {
                        let (px, py) = (
                            self.player.transform().position.x,
                            self.player.transform().position.z,
                        );
                        self.start_press = Some((px, py, true));

                        self.look_at(px, py, ex, ey);
                        *self.player.look_at_mut() = Vec3::new(ex, 0.0, ey);
                        self.update_look_at = true;
                    } else {
                        self.start_press = Some((ex, ey, false));
                        self.move_target = Some(Vec3::new(ex, 0.0, ey));
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
            if shift_key {
                let dist = (mx - px).hypot(my - py);
                if dist >= ARROW_THRESHOLD {
                    *self.player.look_at_mut() = Vec3::new(mx, 0.0, my);
                    self.update_look_at = true;
                    self.look_at(px, py, mx, my);
                    needs_redraw = true;
                }
            } else {
                // LMB drag: continuously update move target to cursor position.
                self.move_target = Some(Vec3::new(mx, 0.0, my));
                needs_redraw = true;
            }
        }

        if needs_redraw {
            self.redraw()?;
        }

        Ok(())
    }

    fn look_at(&mut self, px: f32, py: f32, mx: f32, my: f32) {
        self.arrow_target = Some((mx, my));
        let angle_rad = (my - py).atan2(mx - px);
        let dir_x = angle_rad.cos();
        let dir_z = angle_rad.sin();
        self.player.transform_mut().front = api::Vec3::new(dir_x, 0.0, dir_z);
        self.update_transform = true;
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

        let ids = self.player.image().into_iter();
        let ids = ids.chain(self.remote_avatars.iter().filter_map(|a| a.image()));

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

        let avatars = || {
            let avatars = self.remote_avatars.iter().map(RenderAvatar::from_remote);
            avatars.chain([RenderAvatar::from_player(&self.player)])
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
