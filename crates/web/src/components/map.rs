use core::mem;
use std::f64::consts::{FRAC_PI_6, TAU};

use api::{Avatar, Extent2, Id, RemoteAvatar, Vec3, World};
use gloo::timers::callback::Interval;
use musli_web::web::Packet;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlElement, MouseEvent, ResizeObserver,
    WheelEvent,
};
use yew::prelude::*;

use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::ws;

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f32 = 0.1;
const MOVEMENT_SPEED: f32 = 5.0;
const ANIMATION_FPS: u32 = 60;
const HALF_SPAN: f64 = FRAC_PI_6;
const HELP: &str = "LMB to move / Shift + LBM to look / MMB to pan / Scroll to zoom";

/// Draws a 30° directional arc just outside the token circle to indicate
/// facing. `angle` is the canvas-space angle (radians) of the facing direction.
/// The arc is centred on that angle and spans ±15°.
fn draw_facing_arc(
    cx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    radius: f64,
    angle: f64,
    line_width: f64,
) -> Result<(), wasm_bindgen::JsValue> {
    cx.set_line_width(line_width);
    cx.begin_path();
    cx.arc(x, y, radius, angle - HALF_SPAN, angle + HALF_SPAN)?;
    cx.stroke();
    Ok(())
}

/// Draws a world-space grid with thicker lines at the origin axes.
/// Non-origin lines are drawn first, then origin lines on top.
fn draw_grid(cx: &CanvasRenderingContext2d, t: &ViewTransform, extent: &Extent2, zoom: f32) {
    const GRID_STEP: f32 = 2.0;
    const EPS: f32 = GRID_STEP * 0.01;

    cx.set_stroke_style_str("#2a2a2a");
    cx.set_line_width(zoom as f64 * 0.5);

    let mut x = (extent.x.start / GRID_STEP).ceil() * GRID_STEP;

    while x <= extent.x.end + EPS {
        if x.abs() >= EPS {
            let (px, py1) = t.world_to_canvas(x, extent.y.start);
            let (_, py2) = t.world_to_canvas(x, extent.y.end);
            cx.begin_path();
            cx.move_to(px, py1);
            cx.line_to(px, py2);
            cx.stroke();
        }

        x += GRID_STEP;
    }

    let mut z = (extent.y.start / GRID_STEP).ceil() * GRID_STEP;

    while z <= extent.y.end + EPS {
        if z.abs() >= EPS {
            let (px1, py) = t.world_to_canvas(extent.x.start, z);
            let (px2, _) = t.world_to_canvas(extent.x.end, z);
            cx.begin_path();
            cx.move_to(px1, py);
            cx.line_to(px2, py);
            cx.stroke();
        }

        z += GRID_STEP;
    }

    cx.set_stroke_style_str("#888888");
    cx.set_line_width(zoom as f64 * 1.5);

    if extent.x.contains(0.0) {
        let (px, py1) = t.world_to_canvas(0.0, extent.y.start);
        let (_, py2) = t.world_to_canvas(0.0, extent.y.end);
        cx.begin_path();
        cx.move_to(px, py1);
        cx.line_to(px, py2);
        cx.stroke();
    }

    if extent.y.contains(0.0) {
        let (px1, py) = t.world_to_canvas(extent.x.start, 0.0);
        let (px2, _) = t.world_to_canvas(extent.x.end, 0.0);
        cx.begin_path();
        cx.move_to(px1, py);
        cx.line_to(px2, py);
        cx.stroke();
    }
}

/// Encapsulates the canvas ↔ world coordinate transform for a given frame.
struct ViewTransform {
    scale: f64,
    center_x: f64,
    center_y: f64,
}

impl ViewTransform {
    fn new(canvas: &HtmlCanvasElement, w: &World) -> Self {
        let canvas_min = canvas.width().min(canvas.height()) as f64;
        let world_w = (w.extent.x.end - w.extent.x.start) as f64;
        let world_h = (w.extent.y.end - w.extent.y.start) as f64;
        let scale = (canvas_min / world_w.max(world_h)) * w.zoom as f64;

        let world_mid_x = ((w.extent.x.start + w.extent.x.end) / 2.0) as f64;
        let world_mid_y = ((w.extent.y.start + w.extent.y.end) / 2.0) as f64;
        let center_x = canvas.width() as f64 / 2.0 + w.pan.x - world_mid_x * scale;
        let center_y = canvas.height() as f64 / 2.0 + w.pan.y - world_mid_y * scale;

        Self {
            scale,
            center_x,
            center_y,
        }
    }

    fn world_to_canvas(&self, world_x: f32, world_z: f32) -> (f64, f64) {
        let x = self.center_x + world_x as f64 * self.scale;
        let y = self.center_y - world_z as f64 * self.scale;
        (x, y)
    }

    fn canvas_to_world(&self, canvas_x: f64, canvas_y: f64) -> (f32, f32) {
        let world_x = ((canvas_x - self.center_x) / self.scale) as f32;
        let world_z = ((self.center_y - canvas_y) / self.scale) as f32;
        (world_x, world_z)
    }
}

pub(crate) struct Map {
    _initialize: ws::Request,
    _update_transform: ws::Request,
    _update_look_at: ws::Request,
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
}

pub(crate) enum Msg {
    InitializeMap(Result<Packet<api::InitializeMap>, ws::Error>),
    TransformUpdated(Result<Packet<api::UpdateTransform>, ws::Error>),
    LookAtUpdated(Result<Packet<api::UpdateLookAt>, ws::Error>),
    WorldUpdated(Result<Packet<api::UpdateWorld>, ws::Error>),
    RemoteAvatarUpdate(Result<Packet<api::RemoteAvatarUpdate>, ws::Error>),
    StateChanged(ws::State),
    Resized,
    ImageLoaded(ImageMessage),
    MouseDown(MouseEvent),
    MouseMove(MouseEvent),
    MouseUp(MouseEvent),
    MouseLeave,
    Wheel(WheelEvent),
    AnimationFrame,
    LogUpdate(log::Log),
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

        let mut this = Self {
            _initialize: ws::Request::new(),
            _update_transform: ws::Request::new(),
            _update_look_at: ws::Request::new(),
            _update_world: ws::Request::new(),
            _remote_avatar_listener: remote_avatar_listener,
            _state_change,
            state,
            log,
            _log_handle,
            canvas_sizer: NodeRef::default(),
            canvas_ref: NodeRef::default(),
            world: api::World::zero(),
            player: api::Avatar::zero(),
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

            let interval = Interval::new((1000 / ANIMATION_FPS) as u32, move || {
                link.send_message(Msg::AnimationFrame);
            });

            self._animation_interval = Some(interval);
        }

        changed
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let p = self.player.transform.position;
        let f = self.player.transform.front;

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
            Msg::InitializeMap(result) => {
                let initialize = result?;
                let initialize = initialize.decode()?;

                tracing::debug!(?initialize, "initialize");

                self.world = initialize.world;
                self.player = initialize.player;
                self.remote_avatars = initialize.remote_avatars;

                self.load_initialize_images(ctx);
                self.redraw()?;
                Ok(true)
            }
            Msg::TransformUpdated(result) => {
                result?;
                Ok(false)
            }
            Msg::LookAtUpdated(result) => {
                result?;
                Ok(false)
            }
            Msg::WorldUpdated(result) => {
                result?;
                Ok(false)
            }
            Msg::RemoteAvatarUpdate(update) => {
                let update = update?;
                let update = update.decode()?;

                tracing::debug!(?update, "remote avatar update");

                match update {
                    api::RemoteAvatarUpdateBody::RemoteLost => {
                        self.remote_avatars.clear();
                    }
                    api::RemoteAvatarUpdateBody::Join { peer_id } => {
                        self.remote_avatars.push(RemoteAvatar {
                            id: peer_id,
                            transform: api::Transform::origin(),
                            image: None,
                            color: api::Color::neutral(),
                            look_at: None,
                        });
                    }
                    api::RemoteAvatarUpdateBody::Leave { peer_id } => {
                        self.remote_avatars.retain(|a| a.id != peer_id);
                    }
                    api::RemoteAvatarUpdateBody::Move { peer_id, transform } => {
                        if let Some(a) = self.remote_avatars.iter_mut().find(|a| a.id == peer_id) {
                            a.transform = transform;
                        }
                    }
                    api::RemoteAvatarUpdateBody::LookAt { peer_id, look_at } => {
                        if let Some(a) = self.remote_avatars.iter_mut().find(|a| a.id == peer_id) {
                            a.look_at = look_at;
                        }
                    }
                    api::RemoteAvatarUpdateBody::ImageUpdated { peer_id, image } => {
                        let old = if let Some(a) =
                            self.remote_avatars.iter_mut().find(|a| a.id == peer_id)
                        {
                            mem::replace(&mut a.image, image)
                        } else {
                            None
                        };

                        if let Some(id) = old {
                            self.images.remove(id);
                        }

                        if let Some(id) = image {
                            self.images.load(ctx, id);
                        }
                    }
                    api::RemoteAvatarUpdateBody::ColorUpdated { peer_id, color } => {
                        if let Some(a) = self.remote_avatars.iter_mut().find(|a| a.id == peer_id) {
                            a.color = color;
                        }
                    }
                }

                self.redraw()?;
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
            Msg::LogUpdate(log) => {
                self.log = log;
                Ok(false)
            }
        }
    }

    fn interpolate_movement(&mut self) {
        let p = self.player.transform.position;

        'done: {
            let Some(target) = self.move_target else {
                break 'done;
            };

            let dx = target.x - p.x;
            let dz = target.z - p.z;
            let distance = (dx * dx + dz * dz).sqrt();

            if distance < 0.01 {
                self.player.transform.position = target;
                self.move_target = None;
                self._animation_interval = None;
                self.update_transform = true;
                break 'done;
            }

            let step = MOVEMENT_SPEED / ANIMATION_FPS as f32;
            let move_distance = step.min(distance);
            let ratio = move_distance / distance;

            self.player.transform.position.x += dx * ratio;
            self.player.transform.position.z += dz * ratio;
            self.update_transform = true;
        };

        'done: {
            let Some(target) = self.player.look_at else {
                break 'done;
            };

            self.look_at(p.x, p.z, target.x, target.z);
        };
    }

    fn send_transform_update(&mut self, ctx: &Context<Self>) {
        self._update_transform = ctx
            .props()
            .ws
            .request()
            .body(api::UpdateTransformRequest {
                transform: self.player.transform,
            })
            .on_packet(ctx.link().callback(Msg::TransformUpdated))
            .send();
    }

    fn send_look_at_update(&mut self, ctx: &Context<Self>) {
        self._update_look_at = ctx
            .props()
            .ws
            .request()
            .body(api::UpdateLookAtRequest {
                look_at: self.player.look_at,
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
                            self.player.transform.position.x,
                            self.player.transform.position.z,
                        );
                        self.start_press = Some((px, py, true));

                        self.look_at(px, py, ex, ey);
                        self.player.look_at = Some(Vec3::new(ex, 0.0, ey));
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

        if let Some((px, py, shift_key)) = self.start_press {
            let mx = e.offset_x() as f64;
            let my = e.offset_y() as f64;

            let (mx, my) = v.canvas_to_world(mx, my);

            let dist = (mx - px).hypot(my - py);

            if dist >= ARROW_THRESHOLD {
                if shift_key {
                    self.player.look_at = Some(Vec3::new(mx, 0.0, my));
                } else {
                    self.player.look_at = None;
                }

                self.update_look_at = true;
                self.look_at(px, py, mx, my);
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
        self.player.transform.front = api::Vec3::new(dir_x, 0.0, dir_z);
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

        let ids = self.player.image.into_iter();
        let ids = ids.chain(self.remote_avatars.iter().filter_map(|a| a.image));

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
        struct RenderAvatar {
            transform: api::Transform,
            look_at: Option<Vec3>,
            image: Option<Id>,
            color: api::Color,
            player: bool,
        }

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

        draw_grid(&cx, &t, &self.world.extent, self.world.zoom);

        let player_avatar = |a: &Avatar| RenderAvatar {
            transform: a.transform,
            look_at: a.look_at,
            image: a.image,
            color: a.color,
            player: true,
        };

        let remote_avatar = |a: &RemoteAvatar| RenderAvatar {
            transform: a.transform,
            look_at: a.look_at,
            image: a.image,
            color: a.color,
            player: false,
        };

        let avatars = || {
            let avatars = self.remote_avatars.iter().map(remote_avatar);
            avatars.chain([player_avatar(&self.player)])
        };

        for a in avatars() {
            let (x, y) = t.world_to_canvas(a.transform.position.x, a.transform.position.z);

            // Draw avatar token: circular image if available, otherwise a filled circle.
            let image_drawn = 'draw: {
                let Some(id) = a.image else {
                    break 'draw false;
                };

                let Some(img) = self.images.get(id) else {
                    break 'draw false;
                };

                let iw = img.natural_width() as f64;
                let ih = img.natural_height() as f64;

                let scale = (token_radius * 2.0) / iw.min(ih);
                let dw = iw * scale;
                let dh = ih * scale;

                cx.save();
                cx.begin_path();
                cx.arc(x, y, token_radius, 0.0, TAU)?;
                cx.clip();

                cx.draw_image_with_html_image_element_and_dw_and_dh(
                    img,
                    x - dw / 2.0,
                    y - dh / 2.0,
                    dw,
                    dh,
                )?;

                cx.restore();
                true
            };

            if !image_drawn {
                cx.set_fill_style_str(&a.color.to_css_string());
                cx.begin_path();
                cx.arc(x, y, token_radius, 0.0, TAU)?;
                cx.fill();
            }

            let front = if a.player
                && let Some((mx, my)) = self.arrow_target
            {
                let (x, y) = (a.transform.position.x, a.transform.position.z);
                let angle_rad = (my - y).atan2(mx - x);
                let dir_x = angle_rad.cos() as f32;
                let dir_z = angle_rad.sin() as f32;
                Vec3::new(dir_x, 0.0, dir_z)
            } else {
                a.transform.front
            };

            // Only draw the facing arc when the avatar has a non-zero facing direction.
            if front.x.hypot(front.z) > 0.01 {
                let angle = (-front.z as f64).atan2(front.x as f64);
                let arc_radius = token_radius * 1.5;
                cx.set_stroke_style_str(&a.color.to_css_string());
                draw_facing_arc(&cx, x, y, arc_radius, angle, token_radius * 0.25)?;
            }
        }

        for a in avatars() {
            // Draw eye icon at look_at position
            if let Some(target) = a.look_at {
                let zoom = self.world.zoom as f64;

                let eye_width = 24.0 * zoom;
                let eye_height = 12.0 * zoom;
                let radius = 6.0 * zoom;

                let color = a.color.to_css_string();

                let (ex, ey) = t.world_to_canvas(target.x, target.z);

                cx.save();
                cx.set_stroke_style_str(&color);
                cx.set_line_width(2.0 * zoom);
                cx.begin_path();
                cx.ellipse(ex, ey, eye_width / 2.0, eye_height / 2.0, 0.0, 0.0, TAU)?;
                cx.stroke();

                cx.set_fill_style_str(&color);
                cx.begin_path();
                cx.arc(ex, ey, radius, 0.0, TAU)?;
                cx.fill();

                cx.restore();
            }
        }

        Ok(())
    }
}
