use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;

pub(crate) enum Msg {
    StateChanged(ws::State),
    ServerChanged(Event),
    TlsToggled(Event),
    SetRemoteServer(String),
    SetRemoteServerResult(Result<Packet<api::SetRemoteServer>, ws::Error>),
    ListSettings(Result<Packet<api::ListSettings>, ws::Error>),
    ContextUpdate(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct Settings {
    state: ws::State,
    remote_server: String,
    remote_server_tls: bool,
    set_remote_server: ws::Request,
    _list_settings: ws::Request,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
}

impl Component for Settings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::ContextUpdate))
            .expect("ErrorLog context not found");

        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let mut this = Self {
            state,
            remote_server: String::new(),
            remote_server_tls: false,
            set_remote_server: ws::Request::new(),
            _list_settings: ws::Request::new(),
            log,
            _log_handle,
            _state_change,
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("settings::update", &error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let is_remote_pending = self.set_remote_server.is_pending();

        html! {
            <div id="content" class="rows">
                <h2>{"Remote Server"}</h2>

                <div class="hint">
                    {"If a remote server is configured and enabled, it can be used to synchronize state between many Mumbler Clients."}
                </div>

                <section>
                    <input
                        id="remote-server"
                        type="text"
                        placeholder="host[:port]"
                        value={self.remote_server.clone()}
                        onchange={ctx.link().callback(Msg::ServerChanged)}
                        disabled={is_remote_pending}
                        />

                    <label class="checkbox-label">
                        <input
                            id="remote-server-tls"
                            type="checkbox"
                            checked={self.remote_server_tls}
                            onchange={ctx.link().callback(Msg::TlsToggled)}
                            disabled={is_remote_pending}
                            />
                        {" Use TLS"}
                    </label>

                    if is_remote_pending {
                        <div class="loading">{"Saving ..."}</div>
                    }
                </section>
            </div>
        }
    }
}

impl Settings {
    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._list_settings = ctx
                .props()
                .ws
                .request()
                .body(api::ListSettingsRequest)
                .on_packet(ctx.link().callback(Msg::ListSettings))
                .send();
        } else {
            self._list_settings = ws::Request::new();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::ListSettings(result) => {
                let result = result?;
                let response = result.decode()?;
                self.remote_server = response.remote_server.unwrap_or_default();
                self.remote_server_tls = response.remote_server_tls;
                Ok(true)
            }
            Msg::ServerChanged(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let value = input.value();
                ctx.link().send_message(Msg::SetRemoteServer(value));
                Ok(false)
            }
            Msg::TlsToggled(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                self.remote_server_tls = input.checked();
                ctx.link()
                    .send_message(Msg::SetRemoteServer(self.remote_server.clone()));
                Ok(false)
            }
            Msg::SetRemoteServer(server) => {
                let server = server.trim();
                let server = (!server.is_empty()).then_some(server.to_owned());

                self.set_remote_server = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::SetRemoteServerRequest {
                        server,
                        tls: self.remote_server_tls,
                    })
                    .on_packet(ctx.link().callback(Msg::SetRemoteServerResult))
                    .send();

                Ok(false)
            }
            Msg::SetRemoteServerResult(result) => {
                let result = result?;
                let response = result.decode()?;
                self.remote_server = response.server.unwrap_or_default();
                self.remote_server_tls = response.tls;
                Ok(true)
            }
            Msg::ContextUpdate(log) => {
                self.log = log;
                Ok(false)
            }
        }
    }
}
