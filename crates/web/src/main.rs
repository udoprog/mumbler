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
    Dashboard,
    #[not_found]
    #[at("/404")]
    NotFound,
}

struct App {
    ws: ws::Service,
}

enum Msg {
    Error(error::Error),
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
        ws.connect();
        Self { ws }
    }

    fn update(&mut self, _: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Error(error) => {
                tracing::error!("Failed to fetch: {error}");
                false
            }
        }
    }

    fn view(&self, _: &Context<Self>) -> Html {
        let ws = self.ws.handle().clone();

        html! {
            <BrowserRouter>
                <Switch<Route> render={move |route| switch(route, &ws)} />
            </BrowserRouter>
        }
    }
}

fn switch(routes: Route, ws: &ws::Handle) -> Html {
    match routes {
        Route::Dashboard => html!(<components::Map ws={ws} />),
        Route::NotFound => {
            html! {
                <div id="content" class="container">{"There is nothing here"}</div>
            }
        }
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
