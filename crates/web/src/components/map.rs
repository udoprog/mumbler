use api::{Avatar, World};
use derive_more::From;
use musli_web::web::Packet;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement};
use yew::prelude::*;

use crate::error::Error;
use crate::ws;

pub(crate) struct Map {
    _initialize: ws::Request,
    _state_change: ws::StateListener,
    state: ws::State,
    initialize: Packet<api::Initialize>,
    canvas_sizer: NodeRef,
    canvas_ref: NodeRef,
    /// World configuration.
    world: Option<World>,
    /// Avatars in the world and their positions.
    avatars: Vec<Avatar>,
}

#[derive(From)]
pub(crate) enum Msg {
    Initialize(Result<Packet<api::Initialize>, ws::Error>),
    StateChanged(ws::State),
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
        }
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
        let world = self.world.ok_or("Missing world configuration")?;
        let canvas = self
            .canvas_ref
            .cast::<HtmlCanvasElement>()
            .ok_or("missing canvas")?;

        let cx = canvas.get_context("2d")?.ok_or("missing canvas context")?;

        let cx = cx
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "invalid canvas context")?;

        let scale_w = canvas.width() as f32 / world.width;
        let scale_h = canvas.height() as f32 / world.height;

        let center_x = canvas.width() as f32 / 2.0;
        let center_y = canvas.height() as f32 / 2.0;

        let pos = |avatar: &Avatar| {
            let x = center_x + avatar.position.x * scale_w;
            let y = center_y + avatar.position.z * scale_h;
            (x, y)
        };

        const COLORS: [&str; 4] = ["red", "green", "blue", "orange"];

        for (avatar, color) in self.avatars.iter().zip(COLORS) {
            tracing::info!(?avatar, "Drawing avatar");

            cx.set_fill_style_str(color);
            let (x, y) = pos(avatar);
            cx.begin_path();
            cx.arc(x as f64, y as f64, 5.0, 0.0, std::f64::consts::TAU)?;
            cx.fill();
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

    fn view(&self, _: &Context<Self>) -> Html {
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
                    <canvas id="grid" ref={self.canvas_ref.clone()}></canvas>
                </div>
            </div>
        }
    }
}
