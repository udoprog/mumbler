use core::mem;
use std::collections::HashMap;
use std::f64::consts::{FRAC_PI_6, TAU};

use api::{Avatar, Extent2, Id, RemoteAvatar, Vec3, World};
use musli_web::web::Packet;
use wasm_bindgen::JsCast;
use wasm_bindgen::closure::Closure;
use web_sys::{
    CanvasRenderingContext2d, HtmlCanvasElement, HtmlElement, HtmlImageElement, MouseEvent,
    ResizeObserver, WheelEvent,
};
use yew::prelude::*;

use crate::error::Error;
use crate::ws;

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f64 = 5.0;

/// Draws a 30° directional arc just outside the token circle to indicate facing.
/// `angle` is the canvas-space angle (radians) of the facing direction.
/// The arc is centred on that angle and spans ±15°.
fn draw_facing_arc(
    cx: &CanvasRenderingContext2d,
    x: f64,
    y: f64,
    radius: f64,
    angle: f64,
    line_width: f64,
) -> Result<(), wasm_bindgen::JsValue> {
    const HALF_SPAN: f64 = FRAC_PI_6;

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
    fn new(canvas: &HtmlCanvasElement, extent: &Extent2, pan: (f64, f64), zoom: f64) -> Self {
        let canvas_min = canvas.width().min(canvas.height()) as f64;
        let world_w = (extent.x.end - extent.x.start) as f64;
        let world_h = (extent.y.end - extent.y.start) as f64;
        let scale = (canvas_min / world_w.max(world_h)) * zoom;

        let world_mid_x = ((extent.x.start + extent.x.end) / 2.0) as f64;
        let world_mid_y = ((extent.y.start + extent.y.end) / 2.0) as f64;
        let center_x = canvas.width() as f64 / 2.0 + pan.0 - world_mid_x * scale;
        let center_y = canvas.height() as f64 / 2.0 + pan.1 - world_mid_y * scale;

        Self {
            scale,
            center_x,
            center_y,
        }
    }

    fn world_to_canvas(&self, world_x: f32, world_z: f32) -> (f64, f64) {
        let x = self.center_x + world_x as f64 * self.scale;
        let y = self.center_y + world_z as f64 * self.scale;
        (x, y)
    }

    fn canvas_to_world(&self, canvas_x: f64, canvas_y: f64) -> (f64, f64) {
        let world_x = (canvas_x - self.center_x) / self.scale;
        let world_z = (canvas_y - self.center_y) / self.scale;
        (world_x, world_z)
    }
}

pub(crate) struct Map {
    _initialize: ws::Request,
    _update_avatars: ws::Request,
    _state_change: ws::StateListener,
    _remote_avatar_listener: ws::Listener,
    state: ws::State,
    canvas_sizer: NodeRef,
    canvas_ref: NodeRef,
    /// World configuration.
    world: Option<World>,
    /// List of local avatars that need to be updated remotely.
    update: bool,
    /// The player avatar.
    player: Option<Avatar>,
    /// Avatars in the world and their positions.
    remote_avatars: Vec<RemoteAvatar>,
    /// Current pan offset in canvas pixels.
    pan: (f64, f64),
    /// Mouse position at the start of a middle-mouse drag.
    pan_anchor: Option<(f64, f64)>,
    /// Canvas-space position where the left button was pressed.
    start_press: Option<(f64, f64)>,
    /// Canvas-space mouse position once the drag exceeds the arrow threshold.
    arrow_target: Option<(f64, f64)>,
    /// Keeps the ResizeObserver and its closure alive for the component's lifetime.
    _resize_observer: Option<(ResizeObserver, Closure<dyn FnMut()>)>,
    /// Loaded avatar images, keyed by image id.
    /// The closure must be kept alive alongside the element for the onload callback to fire.
    images: HashMap<Id, (HtmlImageElement, Option<Closure<dyn FnMut()>>)>,
}

pub(crate) enum Msg {
    Initialize(Result<Packet<api::Initialize>, ws::Error>),
    AvatarsUpdated(Result<Packet<api::UpdatePlayer>, ws::Error>),
    RemoteAvatarUpdate(Result<Packet<api::RemoteAvatarUpdate>, ws::Error>),
    StateChanged(ws::State),
    Resized,
    ImageLoaded(Id),
    MouseDown(MouseEvent),
    MouseMove(MouseEvent),
    MouseUp(MouseEvent),
    MouseLeave,
    Wheel(WheelEvent),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

impl Component for Map {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
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
            _update_avatars: ws::Request::new(),
            _remote_avatar_listener: remote_avatar_listener,
            _state_change,
            state,
            canvas_sizer: NodeRef::default(),
            canvas_ref: NodeRef::default(),
            world: None,
            player: None,
            remote_avatars: Vec::new(),
            update: false,
            pan: (0.0, 0.0),
            pan_anchor: None,
            start_press: None,
            arrow_target: None,
            _resize_observer: None,
            images: HashMap::new(),
        };

        this.refresh(ctx);
        this
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            self.resize_canvas();

            if let Some(sizer) = self.canvas_sizer.cast::<HtmlElement>() {
                let link = ctx.link().clone();
                let closure = Closure::<dyn FnMut()>::new(move || {
                    link.send_message(Msg::Resized);
                });
                let observer = ResizeObserver::new(closure.as_ref().unchecked_ref()).unwrap();
                observer.observe(&sizer);

                if let Some((o, _closure)) = self._resize_observer.replace((observer, closure)) {
                    o.disconnect();
                    drop(_closure);
                }
            }
        }

        if let Err(error) = self.redraw() {
            tracing::error!(%error, "Failed to redraw map");
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
                tracing::error!(%error, "Map error");
                false
            }
        };

        if self.update {
            self.send_updates(ctx);
            self.update = false;
        }

        changed
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <>
                {self.player.as_ref().map(|p| format!("{:?}", p.transform))}
                <div class="map-sizer" ref={self.canvas_sizer.clone()}>
                    <canvas id="map" ref={self.canvas_ref.clone()}
                        onmousedown={ctx.link().callback(Msg::MouseDown)}
                        onmousemove={ctx.link().callback(Msg::MouseMove)}
                        onmouseup={ctx.link().callback(Msg::MouseUp)}
                        onmouseleave={ctx.link().callback(|_| Msg::MouseLeave)}
                        onwheel={ctx.link().callback(Msg::Wheel)}
                    ></canvas>
                </div>
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
                .body(api::InitializeRequest)
                .on_packet(ctx.link().callback(Msg::Initialize))
                .send();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Initialize(result) => {
                let initialize = result?;
                let initialize = initialize.decode()?;

                tracing::debug!(?initialize, "initialize");

                self.world = Some(initialize.world);
                self.player = Some(initialize.player);
                self.remote_avatars = initialize.remote_avatars;

                self.load_initialize_images(ctx);

                if let Err(error) = self.redraw() {
                    tracing::error!(%error, "Failed to redraw map");
                }

                Ok(true)
            }
            Msg::AvatarsUpdated(result) => {
                result?;
                self.update = false;
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
                            color: api::Color::neutral_gray(),
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
                    api::RemoteAvatarUpdateBody::ImageUpdated { peer_id, image } => {
                        let old = if let Some(a) =
                            self.remote_avatars.iter_mut().find(|a| a.id == peer_id)
                        {
                            mem::replace(&mut a.image, image)
                        } else {
                            None
                        };

                        if let Some(id) = old {
                            self.images.remove(&id);
                        }

                        if let Some(id) = image {
                            let image = Self::load_image(ctx, id);
                            self.images.insert(id, image);
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
            Msg::ImageLoaded(id) => {
                if let Some((_, closure)) = self.images.get_mut(&id) {
                    *closure = None;
                }

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
        }
    }

    fn send_updates(&mut self, ctx: &Context<Self>) {
        let Some(avatar) = &self.player else {
            return;
        };

        self._update_avatars = ctx
            .props()
            .ws
            .request()
            .body(api::UpdatePlayerRequest {
                avatar: avatar.clone(),
            })
            .on_packet(ctx.link().callback(Msg::AvatarsUpdated))
            .send();
    }

    fn on_mouse_down(&mut self, e: MouseEvent) -> Result<(), Error> {
        let needs_redraw = 'out: {
            match e.button() {
                0 => {
                    let Some(w) = &self.world else {
                        break 'out false;
                    };

                    let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() else {
                        break 'out false;
                    };

                    let pos = (e.offset_x() as f64, e.offset_y() as f64);
                    self.start_press = Some(pos);
                    self.arrow_target = None;

                    let t = ViewTransform::new(&canvas, &w.extent, self.pan, w.zoom as f64);
                    let (world_x, world_z) = t.canvas_to_world(pos.0, pos.1);

                    let Some(a) = &mut self.player else {
                        break 'out false;
                    };

                    a.transform.position.x = world_x as f32;
                    a.transform.position.z = world_z as f32;
                    self.update = true;
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
        let mut needs_redraw = false;

        if let Some((ax, ay)) = self.pan_anchor {
            let dx = e.client_x() as f64 - ax;
            let dy = e.client_y() as f64 - ay;
            self.pan = (self.pan.0 + dx, self.pan.1 + dy);
            self.pan_anchor = Some((e.client_x() as f64, e.client_y() as f64));
            needs_redraw = true;
        }

        if let Some((px, py)) = self.start_press {
            let mx = e.offset_x() as f64;
            let my = e.offset_y() as f64;
            let dist = (mx - px).hypot(my - py);

            if dist >= ARROW_THRESHOLD {
                self.arrow_target = Some((mx, my));
                needs_redraw = true;

                // Update front direction in real-time as the user drags
                if let Some(a) = &mut self.player {
                    let angle_rad = (my - py).atan2(mx - px);
                    let dir_x = angle_rad.cos() as f32;
                    let dir_z = angle_rad.sin() as f32;
                    a.transform.front = api::Vec3::new(dir_x, 0.0, dir_z);
                    self.update = true;
                }
            }
        }

        if needs_redraw {
            self.redraw()?;
        }

        Ok(())
    }

    fn on_mouse_up(&mut self, e: MouseEvent) -> Result<(), Error> {
        let needs_redraw = 'out: {
            match e.button() {
                0 => {
                    let Some((cx, cy)) = self.start_press.take() else {
                        break 'out false;
                    };

                    let Some((mx, my)) = self.arrow_target.take() else {
                        break 'out false;
                    };

                    let Some(a) = &mut self.player else {
                        break 'out false;
                    };

                    let angle_rad = (my - cy).atan2(mx - cx);
                    let dir_x = angle_rad.cos() as f32;
                    let dir_z = angle_rad.sin() as f32;

                    a.transform.front = api::Vec3::new(dir_x, 0.0, dir_z);
                    self.update = true;
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

        let Some(w) = &mut self.world else {
            return Ok(());
        };

        let t_before = ViewTransform::new(&canvas, &w.extent, self.pan, w.zoom as f64);
        let (wx, wz) = t_before.canvas_to_world(mx, my);

        w.zoom = (w.zoom * delta).clamp(0.1, 20.0);

        let t_after = ViewTransform::new(&canvas, &w.extent, self.pan, w.zoom as f64);
        let (cx2, cy2) = t_after.world_to_canvas(wx as f32, wz as f32);
        self.pan.0 += mx - cx2;
        self.pan.1 += my - cy2;

        self.redraw()?;
        Ok(())
    }

    /// Resize the canvas according to its parent sizer element.
    fn load_initialize_images(&mut self, ctx: &Context<Self>) {
        self.images.clear();

        let ids = self.player.iter().flat_map(|a| a.image);
        let ids = ids.chain(self.remote_avatars.iter().filter_map(|a| a.image));

        for id in ids {
            let image = Self::load_image(ctx, id);
            self.images.insert(id, image);
        }
    }

    fn load_image(ctx: &Context<Map>, id: Id) -> (HtmlImageElement, Option<Closure<dyn FnMut()>>) {
        let link = ctx.link().clone();
        let img = HtmlImageElement::new().unwrap();
        let closure = Closure::<dyn FnMut()>::new(move || {
            link.send_message(Msg::ImageLoaded(id));
        });
        img.set_onload(Some(closure.as_ref().unchecked_ref()));
        img.set_src(&format!("/api/image/{id}"));
        (img, Some(closure))
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

    fn redraw(&self) -> Result<(), Error> {
        struct RenderAvatar {
            transform: api::Transform,
            image: Option<Id>,
            color: api::Color,
            player: bool,
        }

        let Some(w) = &self.world else {
            return Ok(());
        };

        let canvas = self
            .canvas_ref
            .cast::<HtmlCanvasElement>()
            .ok_or("missing canvas")?;

        let cx = canvas.get_context("2d")?.ok_or("missing canvas context")?;

        let cx = cx
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "invalid canvas context")?;

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        let t = ViewTransform::new(&canvas, &w.extent, self.pan, w.zoom as f64);
        let token_radius = w.token_radius as f64 * t.scale;

        draw_grid(&cx, &t, &w.extent, w.zoom);

        let player_avatar = |a: &Avatar| RenderAvatar {
            transform: a.transform,
            image: a.image,
            color: a.color,
            player: true,
        };

        let remote_avatar = |a: &RemoteAvatar| RenderAvatar {
            transform: a.transform,
            image: a.image,
            color: a.color,
            player: false,
        };

        let avatars = self.remote_avatars.iter().map(remote_avatar);
        let avatars = avatars.chain(self.player.iter().map(player_avatar));

        for a in avatars {
            let (x, y) = t.world_to_canvas(a.transform.position.x, a.transform.position.z);

            // Draw avatar token: circular image if available, otherwise a filled circle.
            let image_drawn = 'draw: {
                let Some(id) = a.image else {
                    break 'draw false;
                };

                let Some((img, _)) = self.images.get(&id) else {
                    break 'draw false;
                };

                if !img.complete() || img.natural_width() == 0 {
                    break 'draw false;
                }

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
                let (x, y) = t.world_to_canvas(a.transform.position.x, a.transform.position.z);
                let angle_rad = (my - y).atan2(mx - x);
                let dir_x = angle_rad.cos() as f32;
                let dir_z = angle_rad.sin() as f32;
                Vec3::new(dir_x, 0.0, dir_z)
            } else {
                a.transform.front
            };

            // Only draw the facing arc when the avatar has a non-zero facing direction.
            if front.x.hypot(front.z) > 0.01 {
                let angle = (front.z as f64).atan2(front.x as f64);
                let arc_radius = token_radius * 1.4;
                cx.set_stroke_style_str(&a.color.to_css_string());
                draw_facing_arc(&cx, x, y, arc_radius, angle, token_radius * 0.25)?;
            }
        }

        Ok(())
    }
}
