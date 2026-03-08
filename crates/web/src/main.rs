#![allow(clippy::type_complexity)]

mod components;
mod error;
mod images;
mod log;

use components::{Icon, Route};
use musli_web::web03::prelude::*;
use tracing::Level;
use tracing_wasm::WASMLayerConfigBuilder;
use yew::prelude::*;
use yew_router::prelude::*;

const COMPONENT: &str = "app::update";

struct App {
    ws: ws::Service,
    state: ws::State,
    log: log::Log,
    _state_listener: ws::StateListener,
    _notification_listener: ws::Listener,
}

enum Msg {
    Error(error::Error),
    StateChanged(ws::State),
    Notification(Result<ws::Packet<api::ServerNotification>, ws::Error>),
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

        let _notification_listener = ws
            .handle()
            .on_broadcast::<api::ServerNotification>(ctx.link().callback(Msg::Notification));

        ws.connect();
        Self {
            ws,
            state,
            log: log::Log::new(),
            _state_listener,
            _notification_listener,
        }
    }

    fn update(&mut self, _: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Error(error) => {
                self.log
                    .error(COMPONENT, format_args!("Websocket error: {error:#}"));
                false
            }
            Msg::StateChanged(state) => {
                self.state = state;
                true
            }
            Msg::Notification(result) => {
                match result.and_then(|p| p.decode()) {
                    Ok(api::ServerNotificationBody::Info { component, message }) => {
                        self.log.info(component, message);
                    }
                    Ok(api::ServerNotificationBody::Error { component, message }) => {
                        self.log.error(component, message);
                    }
                    Err(error) => {
                        self.log
                            .error(COMPONENT, format_args!("Notification error: {error:#}"));
                    }
                }

                false
            }
        }
    }

    fn view(&self, _: &Context<Self>) -> Html {
        let ws = self.ws.handle().clone();
        let state = self.state;

        html! {
            <ContextProvider<log::Log> context={self.log.clone()}>
                <BrowserRouter>
                    <Switch<Route> render={move |route| switch(route, &ws, state)} />
                </BrowserRouter>
            </ContextProvider<log::Log>>
        }
    }
}

fn switch(route: Route, ws: &ws::Handle, state: ws::State) -> Html {
    let component = match route {
        Route::Map => html!(<components::Map ws={ws.clone()} />),
        Route::Settings => html!(<components::Settings ws={ws.clone()} />),
        Route::ObjectSettings { id } => {
            html!(<components::ObjectSettings ws={ws.clone()} {id} />)
        }
        Route::Log => html!(<components::Log />),
        Route::NotFound => {
            html! {
                <div id="content" class="container">{"There is nothing here"}</div>
            }
        }
    };

    let connected;
    let icon;
    let title;

    match state {
        ws::State::Open => {
            connected = "signal";
            icon = "check";
            title = "Connected";
        }
        _ => {
            connected = "signal-slash";
            icon = "x-mark";
            title = "Not connected, will reconnect automatically";
        }
    }

    html! {
        <div class="container">
            <div class="status">
                <components::Navigation route={route} />
                <components::MumbleStatus ws={ws.clone()} />
                <components::RemoteStatus ws={ws.clone()} />
                <section class="connection control-group" {title}>
                    <Icon name={connected} title="Application connection" />
                    <Icon name={icon} />
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
