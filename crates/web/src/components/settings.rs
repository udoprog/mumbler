use api::{Key, UpdateBody, Value};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use musli_web::yew023::ContextExt as _;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::components::ChannelExt;
use crate::error::Error;
use crate::l10n;
use crate::log;
use crate::state::State;

use super::{LanguagePicker, into_target};

pub(crate) enum Msg {
    L10n(l10n::Phrases),
    Channel(ws::ChannelEvent),
    NameChanged(Event),
    PeerSecretChanged(Event),
    ServerChanged(Event),
    TlsToggled(Event),
    UpdateConfig(Result<Packet<api::Updates>, ws::Error>),
    GetConfig(Result<Packet<api::GetConfig>, ws::Error>),
    ConfigUpdate(Result<Packet<api::Update>, ws::Error>),
}

impl From<Result<Packet<api::Updates>, ws::Error>> for Msg {
    #[inline]
    fn from(result: Result<Packet<api::Updates>, ws::Error>) -> Self {
        Self::UpdateConfig(result)
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props;

pub(crate) struct Settings {
    l10n: l10n::Subscription,
    log: log::Log,
    channel: ws::Channel,
    name: State<String>,
    _name_request: ws::Request,
    peer_secret: State<String>,
    _peer_secret_request: ws::Request,
    remote_server: State<String>,
    _remote_server_request: ws::Request,
    remote_server_tls: State<bool>,
    _remote_server_tls_request: ws::Request,
    _get_config_request: ws::Request,
    _config_update: ws::Listener,
}

impl Component for Settings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _) = ctx
            .link()
            .context::<log::Log>(Callback::noop())
            .expect("Log context not found");

        let ws = ctx.handle();

        Self {
            l10n: l10n::Subscription::new(ctx, Msg::L10n),
            log,
            channel: ws
                .channel()
                .on_change(ctx.link().callback(Msg::Channel))
                .open(),
            name: State::new(String::new()),
            _name_request: ws::Request::new(),
            peer_secret: State::new(String::new()),
            _peer_secret_request: ws::Request::new(),
            remote_server: State::new(String::new()),
            _remote_server_request: ws::Request::new(),
            remote_server_tls: State::new(false),
            _remote_server_tls_request: ws::Request::new(),
            _get_config_request: ws::Request::new(),
            _config_update: ws::Listener::new(),
        }
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
                    <label for="name">{self.l10n.text("settings-name")}</label>

                    <input
                        id="name"
                        type="text"
                        placeholder={self.l10n.text("settings-name-placeholder")}
                        value={(*self.name).clone()}
                        onchange={ctx.link().callback(Msg::NameChanged)}
                        />
                </section>

                <section class="input-group">
                    <label for="peer-secret">{self.l10n.text("settings-peer-secret")}</label>

                    <input
                        id="peer-secret"
                        type="password"
                        placeholder={self.l10n.text("settings-peer-secret-placeholder")}
                        value={(*self.peer_secret).clone()}
                        onchange={ctx.link().callback(Msg::PeerSecretChanged)}
                        />
                </section>

                <section class="input-group">
                    <label for="remote-server">{self.l10n.text("settings-remote-server")}</label>

                    <input
                        id="remote-server"
                        type="text"
                        placeholder={self.l10n.text("settings-remote-server-placeholder")}
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
                        {" "}{self.l10n.text("settings-use-tls")}
                    </label>
                </section>

                <LanguagePicker />
            </div>
        }
    }
}

impl Settings {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::L10n(phrases) => Ok(self.l10n.update(phrases)),
            Msg::Channel(event) => {
                match event {
                    ws::ChannelEvent::Opened(..) => {}
                    ws::ChannelEvent::Failed(error) => return Err(error.into()),
                    _ => return Ok(true),
                }

                self._config_update = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::ConfigUpdate));

                self._get_config_request = self
                    .channel
                    .request()
                    .body(api::GetConfigRequest)
                    .on_packet(ctx.link().callback(Msg::GetConfig))
                    .send();

                Ok(true)
            }
            Msg::NameChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = input.value();

                if !self.name.update_str(value.trim()) {
                    return Ok(false);
                }

                if !self.channel.is_open() {
                    return Ok(false);
                }

                self._name_request = self
                    .channel
                    .updates(ctx, [(api::Key::PEER_NAME, self.name.as_str().into())]);

                Ok(false)
            }
            Msg::PeerSecretChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let value = input.value();

                if !self.peer_secret.update_str(value.trim()) {
                    return Ok(false);
                }

                if !self.channel.is_open() {
                    return Ok(false);
                }

                self._peer_secret_request = self
                    .channel
                    .updates(ctx, [(Key::PEER_SECRET, self.peer_secret.as_str().into())]);

                Ok(false)
            }
            Msg::ServerChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = input.value();

                if !self.remote_server.update_str(value.trim()) {
                    return Ok(false);
                }

                if !self.channel.is_open() {
                    return Ok(false);
                }

                self._remote_server_request = self.channel.updates(
                    ctx,
                    [(Key::REMOTE_SERVER, self.remote_server.deref_value())],
                );

                Ok(false)
            }
            Msg::TlsToggled(e) => {
                let input = into_target!(e, HtmlInputElement);

                let remote_server_tls = input.checked();
                *self.remote_server_tls = remote_server_tls;

                if !self.channel.is_open() {
                    return Ok(false);
                }

                self._remote_server_tls_request = self
                    .channel
                    .updates(ctx, [(Key::REMOTE_TLS, self.remote_server_tls.value())]);

                Ok(false)
            }
            Msg::UpdateConfig(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(true)
            }
            Msg::GetConfig(body) => {
                let body = body?;
                let body = body.decode()?;

                let mut changed = false;

                for (key, value) in body {
                    changed |= self.update_config(key, value)?;
                }

                Ok(changed)
            }
            Msg::ConfigUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                match body {
                    UpdateBody::Config { key, value, .. } => self.update_config(key, value),
                    _ => Ok(false),
                }
            }
        }
    }

    fn update_config(&mut self, key: Key, value: Value) -> Result<bool, Error> {
        match key {
            Key::PEER_NAME => Ok(self.name.update(value.as_str().to_owned())),
            Key::PEER_SECRET => Ok(self.peer_secret.update(value.as_str().to_owned())),
            Key::REMOTE_SERVER => Ok(self.remote_server.update(value.as_str().to_owned())),
            Key::REMOTE_TLS => Ok(self.remote_server_tls.update(value.as_bool())),
            _ => Ok(false),
        }
    }
}
