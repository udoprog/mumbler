use api::{Key, Value};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::into_target;

pub(crate) enum Msg {
    StateChanged(ws::State),
    NameChanged(Event),
    ServerChanged(Event),
    TlsToggled(Event),
    UpdateConfig(Result<Packet<api::Updates>, ws::Error>),
    ContextUpdate(log::Log),
    GetConfig(Result<Packet<api::GetConfig>, ws::Error>),
    ConfigUpdate(Result<Packet<api::Update>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct Settings {
    state: ws::State,
    name: State<String>,
    _name_request: ws::Request,
    remote_server: State<String>,
    _remote_server_request: ws::Request,
    remote_server_tls: State<bool>,
    _remote_server_tls_request: ws::Request,
    _get_config_request: ws::Request,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
    _config_update_listener: ws::Listener,
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

        let _config_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::Update>(ctx.link().callback(Msg::ConfigUpdate));

        let mut this = Self {
            state,
            name: State::new(String::new()),
            _name_request: ws::Request::new(),
            remote_server: State::new(String::new()),
            _remote_server_request: ws::Request::new(),
            remote_server_tls: State::new(false),
            _remote_server_tls_request: ws::Request::new(),
            _get_config_request: ws::Request::new(),
            log,
            _log_handle,
            _state_change,
            _config_update_listener,
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("settings::update", error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div id="content" class="rows">
                <section class="input-group">
                    <label for="name">{"Name:"}</label>

                    <input
                        id="name"
                        type="text"
                        placeholder="Name"
                        value={(*self.name).clone()}
                        onchange={ctx.link().callback(Msg::NameChanged)}
                        />
                </section>

                <section class="input-group">
                    <label for="remote-server">{"Remote Server:"}</label>

                    <input
                        id="remote-server"
                        type="text"
                        placeholder="host[:port]"
                        value={(*self.remote_server).clone()}
                        onchange={ctx.link().callback(Msg::ServerChanged)}
                        />

                    <label class="checkbox-label">
                        <input
                            id="remote-server-tls"
                            type="checkbox"
                            checked={*self.remote_server_tls}
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
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::NameChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = input.value();
                let value = value.trim();

                let value = if value.is_empty() {
                    *self.name = String::new();
                    api::Value::empty()
                } else {
                    *self.name = value.to_owned();
                    api::Value::from((*self.name).clone())
                };

                self._name_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdatesRequest {
                        values: vec![(api::Key::PEER_NAME, value)],
                    })
                    .on_packet(ctx.link().callback(Msg::UpdateConfig))
                    .send();

                Ok(false)
            }
            Msg::ServerChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = input.value();
                let value = value.trim();

                let value = if value.is_empty() {
                    *self.remote_server = String::new();
                    api::Value::empty()
                } else {
                    *self.remote_server = value.to_owned();
                    api::Value::from((*self.remote_server).clone())
                };

                self._remote_server_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdatesRequest {
                        values: vec![(api::Key::REMOTE_SERVER, value)],
                    })
                    .on_packet(ctx.link().callback(Msg::UpdateConfig))
                    .send();

                Ok(false)
            }
            Msg::TlsToggled(e) => {
                let input = into_target!(e, HtmlInputElement);

                let remote_server_tls = input.checked();
                *self.remote_server_tls = remote_server_tls;

                self._remote_server_tls_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdatesRequest {
                        values: vec![(api::Key::REMOTE_TLS, remote_server_tls.into())],
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
            Msg::GetConfig(result) => {
                let packet = result?;
                let response = packet.decode()?;

                let mut changed = false;
                for (key, value) in response.iter() {
                    changed |= self.update_config(key, value)?;
                }

                Ok(changed)
            }
            Msg::ConfigUpdate(body) => {
                let body = body?;
                let body = body.decode()?;
                let changed = self.update_config(body.key, &body.value)?;
                Ok(changed)
            }
        }
    }

    fn update_config(&mut self, key: Key, value: &Value) -> Result<bool, Error> {
        match key {
            Key::PEER_NAME => Ok(self
                .name
                .update(value.as_str().unwrap_or_default().to_string())),
            Key::REMOTE_SERVER => Ok(self
                .remote_server
                .update(value.as_str().unwrap_or_default().to_string())),
            Key::REMOTE_TLS => Ok(self
                .remote_server_tls
                .update(value.as_bool().unwrap_or_default())),
            _ => Ok(false),
        }
    }
}
