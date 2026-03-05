use derive_more::From;
use musli_web::web::Packet;
use web_sys::HtmlCanvasElement;
use yew::prelude::*;

use crate::error::Error;
use crate::ws;

pub(crate) struct Dashboard {
    _initialize: ws::Request,
    _state_change: ws::StateListener,
    state: ws::State,
    initialize: Packet<api::Initialize>,
    canvas_ref: NodeRef,
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

impl Dashboard {
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

    fn update_fallible(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Initialize(result) => {
                log::info!("Dashboard initialized");
                self.initialize = result?;
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

impl Component for Dashboard {
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
            canvas_ref: NodeRef::default(),
        };

        this.refresh(&ctx);
        this
    }

    fn rendered(&mut self, _ctx: &Context<Self>, first_render: bool) {
        if first_render {
            if let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() {
                log::info!("Canvas ready: {}x{}", canvas.width(), canvas.height());
            }
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.update_fallible(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                log::error!("Dashboard error: {error}");
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
                <h1>{"Dashboard"}</h1>
                <canvas id="grid" ref={self.canvas_ref.clone()}></canvas>
            </div>
        }
    }
}
