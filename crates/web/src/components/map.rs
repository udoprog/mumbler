use std::collections::HashSet;

use api::{Avatar, AvatarId, Extent2, Vec3, World};
use derive_more::From;
use musli_web::web::Packet;
use wasm_bindgen::JsCast;
use web_sys::{
    CanvasRenderingContext2d, Event, HtmlCanvasElement, HtmlInputElement, MouseEvent, WheelEvent,
};
use yew::prelude::*;

use crate::error::Error;
use crate::ws;

const ZOOM_FACTOR: f64 = 1.2;
const ARROW_THRESHOLD: f64 = 5.0;
static COLORS: &[&str] = &["red", "green", "blue", "orange"];

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
    const HALF_SPAN: f64 = std::f64::consts::FRAC_PI_6;

    cx.set_line_width(line_width);
    cx.begin_path();
    cx.arc(x, y, radius, angle - HALF_SPAN, angle + HALF_SPAN)?;
    cx.stroke();
    Ok(())
}

/// Draws a world-space grid with thicker lines at the origin axes.
fn draw_grid(cx: &CanvasRenderingContext2d, t: &ViewTransform, extent: &Extent2) {
    const GRID_STEP: f32 = 2.0;
    const EPS: f32 = GRID_STEP * 0.01;

    let mut x = (extent.start_x / GRID_STEP).ceil() * GRID_STEP;
    while x <= extent.end_x + EPS {
        let (px, py1) = t.world_to_canvas(x, extent.start_y);
        let (_, py2) = t.world_to_canvas(x, extent.end_y);
        let is_origin = x.abs() < EPS;
        cx.set_stroke_style_str(if is_origin { "#888888" } else { "#2a2a2a" });
        cx.set_line_width(if is_origin { 1.5 } else { 0.5 });
        cx.begin_path();
        cx.move_to(px, py1);
        cx.line_to(px, py2);
        cx.stroke();
        x += GRID_STEP;
    }

    let mut z = (extent.start_y / GRID_STEP).ceil() * GRID_STEP;

    while z <= extent.end_y + EPS {
        let (px1, py) = t.world_to_canvas(extent.start_x, z);
        let (px2, _) = t.world_to_canvas(extent.end_x, z);
        let is_origin = z.abs() < EPS;
        cx.set_stroke_style_str(if is_origin { "#888888" } else { "#2a2a2a" });
        cx.set_line_width(if is_origin { 1.5 } else { 0.5 });
        cx.begin_path();
        cx.move_to(px1, py);
        cx.line_to(px2, py);
        cx.stroke();
        z += GRID_STEP;
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
        let world_w = (extent.end_x - extent.start_x) as f64;
        let world_h = (extent.end_y - extent.start_y) as f64;
        let scale = (canvas_min / world_w.max(world_h)) * zoom;

        let world_mid_x = ((extent.start_x + extent.end_x) / 2.0) as f64;
        let world_mid_y = ((extent.start_y + extent.end_y) / 2.0) as f64;
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
    _upload_avatar: ws::Request,
    /// Keeps the gloo FileReader alive until the read completes.
    _file_reader: Option<gloo::file::callbacks::FileReader>,
    _state_change: ws::StateListener,
    state: ws::State,
    initialize: Packet<api::Initialize>,
    canvas_sizer: NodeRef,
    canvas_ref: NodeRef,
    file_input_ref: NodeRef,
    /// World configuration.
    world: Option<World>,
    /// List of local avatars that need to be updated remotely.
    updates: HashSet<AvatarId>,
    /// Avatars in the world and their positions.
    avatars: Vec<Avatar>,
    /// Current pan offset in canvas pixels.
    pan: (f64, f64),
    /// Mouse position at the start of a middle-mouse drag.
    pan_anchor: Option<(f64, f64)>,
    /// Canvas-space position where the left button was pressed.
    start_press: Option<(f64, f64)>,
    /// Canvas-space mouse position once the drag exceeds the arrow threshold.
    arrow_target: Option<(f64, f64)>,
}

#[derive(From)]
pub(crate) enum Msg {
    Initialize(Result<Packet<api::Initialize>, ws::Error>),
    AvatarsUpdated(Result<Packet<api::UpdateAvatars>, ws::Error>),
    AvatarUploaded(Result<Packet<api::UploadAvatar>, ws::Error>),
    StateChanged(ws::State),
    #[from(skip)]
    MouseDown(MouseEvent),
    #[from(skip)]
    MouseMove(MouseEvent),
    #[from(skip)]
    MouseUp(MouseEvent),
    #[from(skip)]
    MouseLeave,
    #[from(skip)]
    Wheel(WheelEvent),
    #[from(skip)]
    AvatarImageSelected(Event),
    #[from(skip)]
    AvatarImageData(String, Result<Vec<u8>, gloo::file::FileReadError>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
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
                self.initialize = result?;

                if let Ok(initialize) = self.initialize.decode() {
                    self.world = Some(initialize.world);
                    self.avatars = initialize.avatars;
                }

                if let Err(error) = self.redraw() {
                    tracing::error!(%error, "Failed to redraw map");
                }

                Ok(true)
            }
            Msg::AvatarsUpdated(result) => {
                result?;
                self.updates.clear();
                Ok(false)
            }
            Msg::AvatarUploaded(result) => {
                result?;
                tracing::info!("Avatar uploaded successfully");
                Ok(false)
            }
            Msg::AvatarImageSelected(e) => {
                let input = e
                    .target()
                    .and_then(|t| t.dyn_into::<HtmlInputElement>().ok())
                    .ok_or("no input element")?;

                let files = input.files().ok_or("no file list")?;
                let file = files.get(0).ok_or("no file selected")?;

                let content_type = file.type_();
                let gloo_file = gloo::file::File::from(file);
                let link = ctx.link().clone();

                self._file_reader = Some(gloo::file::callbacks::read_as_bytes(
                    &gloo_file,
                    move |res| {
                        link.send_message(Msg::AvatarImageData(content_type.clone(), res));
                    },
                ));

                Ok(false)
            }
            Msg::AvatarImageData(content_type, result) => {
                self._file_reader = None;
                let data = result.map_err(|e| anyhow::anyhow!("file read error: {e}"))?;

                self._upload_avatar = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UploadAvatarRequest { content_type, data })
                    .on_packet(ctx.link().callback(Msg::AvatarUploaded))
                    .send();

                Ok(false)
            }
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::MouseDown(e) => {
                self.on_mouse_down(e)?;
                Ok(false)
            }
            Msg::MouseMove(e) => {
                self.on_mouse_move(e)?;
                Ok(false)
            }
            Msg::MouseUp(e) => {
                self.on_mouse_up(e)?;
                Ok(false)
            }
            Msg::MouseLeave => {
                self.on_mouse_leave()?;
                Ok(false)
            }
            Msg::Wheel(e) => {
                self.on_wheel(e)?;
                Ok(false)
            }
        }
    }

    fn send_updates(&mut self, ctx: &Context<Self>) {
        let updates = self
            .avatars
            .iter()
            .filter(|a| self.updates.contains(&a.id))
            .cloned()
            .collect();

        self._update_avatars = ctx
            .props()
            .ws
            .request()
            .body(api::UpdateAvatarsRequest { avatars: updates })
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

                    let Some(a) = self.avatars.iter_mut().find(|a| a.id == w.player) else {
                        break 'out false;
                    };

                    a.position.x = world_x as f32;
                    a.position.z = world_z as f32;
                    self.updates.insert(a.id);
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

                    let Some(w) = &self.world else {
                        break 'out false;
                    };

                    let Some(a) = self.avatars.iter_mut().find(|a| a.id == w.player) else {
                        break 'out false;
                    };

                    let angle_rad = (my - cy).atan2(mx - cx);
                    let dir_x = angle_rad.cos() as f32;
                    let dir_z = angle_rad.sin() as f32;

                    a.front = api::Vec3::new(dir_x, 0.0, dir_z);
                    self.updates.insert(a.id);
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
}

impl Map {
    fn size_canvas(&self) {
        if let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() {
            if let Some(sizer) = self.canvas_sizer.cast::<web_sys::HtmlElement>() {
                let width = sizer.client_width() as u32;
                let height = sizer.client_height() as u32;
                canvas.set_width(width);
                canvas.set_height(height);
                tracing::info!(width, height, "Canvas sized");
            }
        }
    }

    fn redraw(&self) -> Result<(), Error> {
        let w = self.world.as_ref().ok_or("Missing world configuration")?;

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

        draw_grid(&cx, &t, &w.extent);

        for (a, color) in self.avatars.iter().zip(COLORS.iter().cycle()) {
            let (x, y) = t.world_to_canvas(a.position.x, a.position.z);

            cx.set_fill_style_str(color);
            cx.begin_path();
            cx.arc(x, y, token_radius, 0.0, std::f64::consts::TAU)?;
            cx.fill();

            let front = if a.id == w.player
                && let Some((mx, my)) = self.arrow_target
            {
                let (x, y) = t.world_to_canvas(a.position.x, a.position.z);
                let angle_rad = (my - y).atan2(mx - x);
                let dir_x = angle_rad.cos() as f32;
                let dir_z = angle_rad.sin() as f32;
                Vec3::new(dir_x, 0.0, dir_z)
            } else {
                a.front
            };

            // Only draw the facing arc when the avatar has a non-zero facing direction.
            if front.x.hypot(front.z) > 0.01 {
                let angle = (front.z as f64).atan2(front.x as f64);
                let arc_radius = token_radius * 1.4;
                cx.set_stroke_style_str(color);
                draw_facing_arc(&cx, x, y, arc_radius, angle, token_radius * 0.25)?;
            }
        }

        Ok(())
    }
}

impl Component for Map {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let mut this = Self {
            _initialize: ws::Request::new(),
            _update_avatars: ws::Request::new(),
            _upload_avatar: ws::Request::new(),
            _file_reader: None,
            _state_change,
            state,
            initialize: Packet::empty(),
            canvas_sizer: NodeRef::default(),
            canvas_ref: NodeRef::default(),
            file_input_ref: NodeRef::default(),
            world: None,
            avatars: Vec::new(),
            updates: HashSet::new(),
            pan: (0.0, 0.0),
            pan_anchor: None,
            start_press: None,
            arrow_target: None,
        };

        this.refresh(&ctx);
        this
    }

    fn rendered(&mut self, _ctx: &Context<Self>, first_render: bool) {
        if first_render {
            self.size_canvas();
        }

        if let Err(error) = self.redraw() {
            tracing::error!(%error, "Failed to redraw map");
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

        if !self.updates.is_empty() {
            self.send_updates(ctx);
        }

        changed
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let user;

        if let Ok(initialize) = self.initialize.decode() {
            user = initialize.name.as_deref().unwrap_or("Unknown").to_owned();
        } else {
            user = "Unknown".to_owned();
        };

        let connected;
        let indicator;
        let indicator_class;

        match self.state {
            ws::State::Open => {
                connected = "Connected";
                indicator = "✔";
                indicator_class = "success";
            }
            _ => {
                connected = "Disconnected";
                indicator = "⚠";
                indicator_class = "warning";
            }
        }

        html! {
            <div class="container">
                <div class="status">
                    <section class="connection">
                        {connected}
                        <span class={classes!("connection-indicator", indicator_class)}>{indicator}</span>
                    </section>
                    <section class="user">
                        <label class="btn" title="Upload avatar image">
                            {"\u{1F464}"}
                            <input
                                ref={self.file_input_ref.clone()}
                                type="file"
                                accept="image/*"
                                style="display:none"
                                onchange={ctx.link().callback(Msg::AvatarImageSelected)}
                            />
                        </label>
                        {user}
                    </section>
                </div>

                <div class="map-sizer" ref={self.canvas_sizer.clone()}>
                    <canvas id="grid" ref={self.canvas_ref.clone()}
                        onmousedown={ctx.link().callback(Msg::MouseDown)}
                        onmousemove={ctx.link().callback(Msg::MouseMove)}
                        onmouseup={ctx.link().callback(Msg::MouseUp)}
                        onmouseleave={ctx.link().callback(|_| Msg::MouseLeave)}
                        onwheel={ctx.link().callback(Msg::Wheel)}
                    ></canvas>
                </div>
            </div>
        }
    }
}
