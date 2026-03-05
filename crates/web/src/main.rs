mod components;
mod error;

use musli_web::web03::prelude::*;
use tracing::Level;
use tracing_wasm::WASMLayerConfigBuilder;
use yew::prelude::*;
use yew_router::prelude::*;

#[derive(Debug, Clone, Copy, PartialEq, Routable)]
enum Route {
    #[at("/")]
    Map,
    #[at("/settings")]
    Settings,
    #[not_found]
    #[at("/404")]
    NotFound,
}

struct App {
    ws: ws::Service,
    state: ws::State,
    _state_listener: ws::StateListener,
}

enum Msg {
    Error(error::Error),
    StateChanged(ws::State),
}

impl From<error::Error> for Msg {
    #[inline]
    fn from(error: error::Error) -> Self {
        Self::Error(error)
    }
}

impl From<musli_web::web::Error> for Msg {
    #[inline]
    fn from(error: musli_web::web::Error) -> Self {
        Self::Error(error::Error::from(error))
    }
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let ws = ws::connect(ws::Connect::location("/ws"))
            .on_error(ctx.link().callback(Msg::Error).reform(Into::into))
            .build();

        let (state, _state_listener) = ws
            .handle()
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        ws.connect();
        Self {
            ws,
            state,
            _state_listener,
        }
    }

    fn update(&mut self, _: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Error(error) => {
                tracing::error!("Failed to fetch: {error}");
                false
            }
            Msg::StateChanged(state) => {
                self.state = state;
                true
            }
        }
    }

    fn view(&self, _: &Context<Self>) -> Html {
        let ws = self.ws.handle().clone();
        let state = self.state;

        html! {
            <BrowserRouter>
                <Switch<Route> render={move |route| switch(route, &ws, state)} />
            </BrowserRouter>
        }
    }
}

fn switch(route: Route, ws: &ws::Handle, state: ws::State) -> Html {
    let component = match route {
        Route::Map => html!(<components::Map {ws} />),
        Route::Settings => html!(<components::Settings {ws} />),
        Route::NotFound => {
            html! {
                <div id="content" class="container">{"There is nothing here"}</div>
            }
        }
    };

    let connected;
    let indicator;
    let indicator_class;

    match state {
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
                <section class="navigation">
                    <Link<Route> to={Route::Map} classes={classes!((route == Route::Map).then_some("active"))}>{"Map"}</Link<Route>>
                    <Link<Route> to={Route::Settings} classes={classes!((route == Route::Settings).then_some("active"))}>{"Settings"}</Link<Route>>
                </section>
                <section class="connection">
                    {connected}
                    <span class={classes!("connection-indicator", indicator_class)}>{indicator}</span>
                </section>
            </div>

            {component}
        </div>
    }
}

fn main() -> anyhow::Result<()> {
    let config = WASMLayerConfigBuilder::new()
        .set_max_level(Level::INFO)
        .build();

    tracing_wasm::set_as_global_default_with_config(config);
    tracing::trace!("Started up");
    yew::Renderer::<App>::new().render();
    Ok(())
}
