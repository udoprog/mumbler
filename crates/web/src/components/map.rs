use std::collections::HashSet;

use api::{Avatar, AvatarId, World};
use derive_more::From;
use musli_web::web::Packet;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, MouseEvent, WheelEvent};
use yew::prelude::*;

use crate::error::Error;
use crate::ws;

const ZOOM_FACTOR: f64 = 0.1;
const ARROW_THRESHOLD: f64 = 10.0;
static COLORS: &[&str] = &["red", "green", "blue", "orange"];

/// Draws an arrow from `(x1, y1)` to `(x2, y2)` using the current stroke style.
fn draw_arrow(cx: &CanvasRenderingContext2d, x1: f64, y1: f64, x2: f64, y2: f64, head_len: f64) {
    let dx = x2 - x1;
    let dy = y2 - y1;

    if dx.hypot(dy) < 1.0 {
        return;
    }

    let angle = dy.atan2(dx);
    let head_angle = std::f64::consts::FRAC_PI_6;

    cx.begin_path();
    cx.move_to(x1, y1);
    cx.line_to(x2, y2);
    cx.stroke();

    cx.begin_path();
    cx.move_to(x2, y2);
    cx.line_to(
        x2 - head_len * (angle - head_angle).cos(),
        y2 - head_len * (angle - head_angle).sin(),
    );
    cx.move_to(x2, y2);
    cx.line_to(
        x2 - head_len * (angle + head_angle).cos(),
        y2 - head_len * (angle + head_angle).sin(),
    );
    cx.stroke();
}

/// Encapsulates the canvas ↔ world coordinate transform for a given frame.
struct ViewTransform {
    scale: f64,
    center_x: f64,
    center_y: f64,
}

impl ViewTransform {
    fn new(canvas: &HtmlCanvasElement, pan: (f64, f64), zoom: f64) -> Self {
        let canvas_min = canvas.width().min(canvas.height()) as f64;
        let scale = (canvas_min / 100.0) * zoom;

        let center_x = canvas.width() as f64 / 2.0 + pan.0;
        let center_y = canvas.height() as f64 / 2.0 + pan.1;

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
    _state_change: ws::StateListener,
    state: ws::State,
    initialize: Packet<api::Initialize>,
    canvas_sizer: NodeRef,
    canvas_ref: NodeRef,
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
                .body(api::Empty)
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

                    let t = ViewTransform::new(&canvas, self.pan, w.zoom as f64);
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

        let delta = (-e.delta_y().signum() * ZOOM_FACTOR) as f32;

        if let Some(w) = &mut self.world {
            w.zoom = (w.zoom + delta).clamp(0.1, 10.0);
        }

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

        let t = ViewTransform::new(&canvas, self.pan, w.zoom as f64);
        let token_radius = w.token_radius as f64 * t.scale;

        for (a, color) in self.avatars.iter().zip(COLORS.iter().cycle()) {
            let (x, y) = t.world_to_canvas(a.position.x, a.position.z);

            cx.set_fill_style_str(color);
            cx.begin_path();
            cx.arc(x, y, token_radius, 0.0, std::f64::consts::TAU)?;
            cx.fill();

            if a.id == w.player
                && let Some((mx, my)) = self.arrow_target
            {
                cx.set_stroke_style_str(color);
                cx.set_line_width(2.0 * w.zoom as f64);
                draw_arrow(&cx, x, y, mx, my, 6.0 * w.zoom as f64);
            } else {
                let arrow_len = token_radius + 8.0 * w.zoom as f64;
                let tip_x = x + a.front.x as f64 * arrow_len;
                let tip_y = y + a.front.z as f64 * arrow_len;
                cx.set_stroke_style_str(color);
                cx.set_line_width(2.0 * w.zoom as f64);
                draw_arrow(&cx, x, y, tip_x, tip_y, 6.0 * w.zoom as f64);
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
            _state_change,
            state,
            initialize: Packet::empty(),
            canvas_sizer: NodeRef::default(),
            canvas_ref: NodeRef::default(),
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
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::error!(%error, "Map error");
                false
            }
        }
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
                    <section class="user">{user}</section>
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
