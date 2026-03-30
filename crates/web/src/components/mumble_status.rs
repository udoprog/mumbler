use api::{Key, UpdateBody, Value};
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::{Icon, SetupChannel};

pub(crate) enum Msg {
    Channel(Result<ws::Channel, Error>),
    Restart,
    RestartResponse(Result<Packet<api::MumbleRestart>, ws::Error>),
    Toggle,
    ToggleResponse(Result<Packet<api::Updates>, ws::Error>),
    GetConfig(Result<Packet<api::GetConfig>, ws::Error>),
    ConfigUpdate(Result<Packet<api::Update>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props;

pub(crate) struct MumbleStatus {
    log: log::Log,
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    _get_status: ws::Request,
    _restart_request: ws::Request,
    _toggle_request: ws::Request,
    _config_update_listener: ws::Listener,
    enabled: State<bool>,
}

impl Component for MumbleStatus {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _) = ctx
            .link()
            .context::<log::Log>(Callback::noop())
            .expect("Log context not found");

        let (ws, _) = ctx
            .link()
            .context::<ws::Handle>(Callback::noop())
            .expect("WebSocket context not found");

        Self {
            log,
            channel: ws::Channel::default(),
            _setup_channel: SetupChannel::new(ws, ctx.link().callback(Msg::Channel)),
            _get_status: ws::Request::new(),
            _restart_request: ws::Request::new(),
            _toggle_request: ws::Request::new(),
            _config_update_listener: ws::Listener::new(),
            enabled: State::new(false),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("mumble::update", error);
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let on_restart = ctx.link().callback(|_| Msg::Restart);
        let on_toggle = ctx.link().callback(|_| Msg::Toggle);

        let toggle_text = if *self.enabled {
            html!(<Icon name="x-mark" />)
        } else {
            html!(<Icon name="check" />)
        };
        let toggle_title = if *self.enabled {
            "Disable Mumble Link"
        } else {
            "Enable Mumble Link"
        };

        html! {
            <section class="mumble control-group">
                <Icon name="mumble" title="Status of Mumble Link Connection" />
                <button
                    class="btn sm square"
                    onclick={on_restart}
                    disabled={!*self.enabled}
                    title="Restart Mumble Link"
                >
                    <Icon name="restart" />
                </button>
                <button
                    class="btn sm square"
                    onclick={on_toggle}
                    title={toggle_title}
                >
                    {toggle_text}
                </button>
            </section>
        }
    }
}

impl MumbleStatus {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Restart => {
                if self.channel.id() == ChannelId::NONE {
                    return Ok(false);
                }

                self._restart_request = self
                    .channel
                    .request()
                    .body(api::MumbleRestartRequest)
                    .on_packet(ctx.link().callback(Msg::RestartResponse))
                    .send();

                Ok(false)
            }
            Msg::RestartResponse(result) => {
                let _ = result?;
                Ok(false)
            }
            Msg::Toggle => {
                if self.channel.id() == ChannelId::NONE {
                    return Ok(false);
                }

                let new_enabled = !*self.enabled;
                *self.enabled = new_enabled;

                self._toggle_request = self
                    .channel
                    .request()
                    .body(api::UpdatesRequest {
                        values: Vec::from([(Key::MUMBLE_ENABLED, Value::from(new_enabled))]),
                    })
                    .on_packet(ctx.link().callback(Msg::ToggleResponse))
                    .send();

                Ok(true)
            }
            Msg::ToggleResponse(body) => {
                let body = body?;
                _ = body.decode()?;
                Ok(false)
            }
            Msg::Channel(channel) => {
                self.channel = channel?;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(true);
                }

                self._config_update_listener = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::ConfigUpdate));

                self._get_status = self
                    .channel
                    .request()
                    .body(api::GetConfigRequest)
                    .on_packet(ctx.link().callback(Msg::GetConfig))
                    .send();

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
                    UpdateBody::Config { key, value, .. } => {
                        let changed = self.update_config(key, value)?;
                        Ok(changed)
                    }
                    _ => Ok(false),
                }
            }
        }
    }

    fn update_config(&mut self, key: Key, value: Value) -> Result<bool, Error> {
        match key {
            Key::MUMBLE_ENABLED => Ok(self.enabled.update(value.as_bool())),
            _ => Ok(false),
        }
    }
}
