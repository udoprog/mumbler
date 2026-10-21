use auth::yew023::{AccountPanel, AdminUsers, AuthContext, UserMenu};
use musli_web::web03::prelude::*;
use musli_web::yew023::{HandleProvider, use_open};
use yew::prelude::*;
use yew_router::prelude::*;

use crate::error::Error;
use crate::l10n;
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

        // With sign-in, watchword's account and admin requests and its live
        // updates share the socket.
        if let Some((auth, _)) = ctx.link().context::<AuthContext>(Callback::noop()) {
            auth.set_socket(Some(ws.handle().clone()));
        }

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
                <HandleProvider context={self.ws.handle().clone()}>
                    <BrowserRouter>
                        <Switch<Route> render={switch} />
                    </BrowserRouter>
                </HandleProvider>
            </ContextProvider<log::Log>>
        }
    }
}

fn switch(route: Route) -> Html {
    html!(<Page {route} />)
}

#[derive(Properties, PartialEq)]
struct PageProps {
    route: Route,
}

#[component(Page)]
fn page(props: &PageProps) -> Html {
    let l10n = l10n::use_phrases();
    let signed_in = use_context::<AuthContext>().is_some();

    let component = match props.route {
        Route::Map => html!(<Map />),
        Route::Settings => html!(<Settings />),
        Route::Log => html!(<Log />),
        Route::Account if signed_in => html! {
            <div id="content" class="container"><AccountPanel /></div>
        },
        Route::Users if signed_in => html! {
            <div id="content" class="container"><AdminUsers /></div>
        },
        Route::Account | Route::Users | Route::NotFound => {
            html! {
                <div id="content" class="container">{l10n.text("app-not-found")}</div>
            }
        }
    };

    html! {
        <div class="container rows">
            <div class="status">
                <Navigation route={props.route.clone()} />
                <MumbleStatus />
                <RemoteStatus />
                <ConnectionStatus />
                <AccountMenu />
            </div>

            {component}
        </div>
    }
}

/// The signed-in user's menu, when the server requires sign-in.
#[component(AccountMenu)]
fn account_menu() -> Html {
    let auth = use_context::<AuthContext>();
    let navigator = use_navigator();

    let (Some(_), Some(navigator)) = (auth, navigator) else {
        return Html::default();
    };

    let on_account = {
        let navigator = navigator.clone();
        Callback::from(move |()| navigator.push(&Route::Account))
    };

    let on_admin = Callback::from(move |()| navigator.push(&Route::Users));

    html! {
        <UserMenu {on_account} {on_admin} />
    }
}

#[component(ConnectionStatus)]
fn connection_status() -> Html {
    let l10n = l10n::use_phrases();

    let (name, title) = if use_open().is_some() {
        ("signal", l10n.text("app-connected"))
    } else {
        ("signal-slash", l10n.text("app-disconnected"))
    };

    html! {
        <section class="connection control-group" {title}>
            <Icon {name} invert={true} small={true} />
        </section>
    }
}
