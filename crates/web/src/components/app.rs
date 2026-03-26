use musli_web::web03::prelude::*;
use yew::prelude::*;
use yew_router::prelude::*;

use crate::error::Error;
use crate::log;

use super::{Icon, Log, Map, MumbleStatus, Navigation, RemoteStatus, Route, Settings};

const COMPONENT: &str = "app::update";

pub struct App {
    ws: ws::Service,
    log: log::Log,
    _notification_listener: ws::Listener,
}

pub enum Msg {
    Error(Error),
    Notification(Result<ws::Packet<api::Notification>, ws::Error>),
}

impl From<Error> for Msg {
    #[inline]
    fn from(error: Error) -> Self {
        Self::Error(error)
    }
}

impl From<musli_web::web::Error> for Msg {
    #[inline]
    fn from(error: musli_web::web::Error) -> Self {
        Self::Error(Error::from(error))
    }
}

impl Component for App {
    type Message = Msg;
    type Properties = ();

    fn create(ctx: &Context<Self>) -> Self {
        let ws = ws::connect(ws::Connect::location("/ws"))
            .close_before_unload()
            .on_error(ctx.link().callback(Msg::Error).reform(Into::into))
            .build();

        let _notification_listener = ws
            .handle()
            .on_broadcast::<api::Notification>(ctx.link().callback(Msg::Notification));

        Self {
            ws,
            log: log::Log::new(),
            _notification_listener,
        }
    }

    fn update(&mut self, _: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Error(error) => {
                self.log
                    .error_message(COMPONENT, format_args!("Websocket error: {error:#}"));
                false
            }
            Msg::Notification(result) => {
                match result.and_then(|p| p.decode()) {
                    Ok(api::NotificationBody::Info { component, message }) => {
                        self.log.info(component, message);
                    }
                    Ok(api::NotificationBody::Error { component, message }) => {
                        self.log.error_message(component, message);
                    }
                    Err(error) => {
                        self.log.error_message(
                            COMPONENT,
                            format_args!("Notification error: {error:#}"),
                        );
                    }
                }

                false
            }
        }
    }

    fn view(&self, _: &Context<Self>) -> Html {
        html! {
            <ContextProvider<log::Log> context={self.log.clone()}>
                <ContextProvider<ws::Handle> context={self.ws.handle()}>
                    <BrowserRouter>
                        <Switch<Route> render={switch} />
                    </BrowserRouter>
                </ContextProvider<ws::Handle>>
            </ContextProvider<log::Log>>
        }
    }
}

fn switch(route: Route) -> Html {
    let component = match route {
        Route::Map => html!(<Map />),
        Route::Settings => html!(<Settings />),
        Route::Log => html!(<Log />),
        Route::NotFound => {
            html! {
                <div id="content" class="container">{"There is nothing here"}</div>
            }
        }
    };

    html! {
        <div class="container rows">
            <div class="status">
                <Navigation route={route} />
                <MumbleStatus />
                <RemoteStatus />
                <ConnectionStatus />
            </div>

            {component}
        </div>
    }
}

#[component(ConnectionStatus)]
fn connection_status() -> Html {
    let ws = use_context::<ws::Handle>().expect("WebSocket context not found");

    let state = use_state(|| ws::State::Closed);

    {
        let state = state.clone();

        use_memo((), move |_| {
            let (s, listener) = ws.on_state_change({
                let state = state.clone();

                Callback::from(move |new_state| {
                    state.set(new_state);
                })
            });

            state.set(s);
            listener
        });
    }

    let name;
    let title;

    match *state {
        ws::State::Open => {
            name = "signal";
            title = "Connected to application";
        }
        _ => {
            name = "signal-slash";
            title = "Not connected, will reconnect automatically";
        }
    }

    html! {
        <section class="connection control-group" {title}>
            <Icon {name} invert={true} small={true} />
        </section>
    }
}
