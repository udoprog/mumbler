use api::Key;
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
    UpdateConfig(Result<Packet<api::UpdateConfig>, ws::Error>),
    GetConfig(Result<Packet<api::GetConfig>, ws::Error>),
    ContextUpdate(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct Settings {
    state: ws::State,
    remote_server: String,
    _remote_server_request: ws::Request,
    remote_server_tls: bool,
    _remote_server_tls_request: ws::Request,
    _get_config_request: ws::Request,
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
            _remote_server_request: ws::Request::new(),
            remote_server_tls: false,
            _remote_server_tls_request: ws::Request::new(),
            _get_config_request: ws::Request::new(),
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
                        />

                    <label class="checkbox-label">
                        <input
                            id="remote-server-tls"
                            type="checkbox"
                            checked={self.remote_server_tls}
                            onchange={ctx.link().callback(Msg::TlsToggled)}
                            />
                        {" Use TLS"}
                    </label>
                </section>
            </div>
        }
    }
}

impl Settings {
    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._get_config_request = ctx
                .props()
                .ws
                .request()
                .body(api::GetConfigRequest)
                .on_packet(ctx.link().callback(Msg::GetConfig))
                .send();
        } else {
            self._get_config_request = ws::Request::new();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::GetConfig(result) => {
                let body = result?;
                let body = body.decode()?;
                self.remote_server = body
                    .get(Key::REMOTE_SERVER)
                    .as_string()
                    .unwrap_or_default()
                    .to_string();
                self.remote_server_tls = body.get(Key::REMOTE_TLS).as_bool().unwrap_or_default();
                Ok(true)
            }
            Msg::ServerChanged(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let value = input.value();
                let value = value.trim();

                let value = if value.is_empty() {
                    self.remote_server = String::new();
                    api::Value::empty()
                } else {
                    self.remote_server = value.to_owned();
                    api::Value::from(self.remote_server.clone())
                };

                self._remote_server_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateConfigRequest {
                        values: vec![(api::Key::REMOTE_SERVER, value)],
                    })
                    .on_packet(ctx.link().callback(Msg::UpdateConfig))
                    .send();

                Ok(false)
            }
            Msg::TlsToggled(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                self.remote_server_tls = input.checked();

                self._remote_server_tls_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateConfigRequest {
                        values: vec![(api::Key::REMOTE_TLS, self.remote_server_tls.into())],
                    })
                    .on_packet(ctx.link().callback(Msg::UpdateConfig))
                    .send();

                Ok(false)
            }
            Msg::UpdateConfig(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(true)
            }
            Msg::ContextUpdate(log) => {
                self.log = log;
                Ok(false)
            }
        }
    }
}
