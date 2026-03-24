use api::{Key, UpdateBody, Value};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use yew::prelude::*;

use super::Icon;
use crate::error::Error;
use crate::log;
use crate::state::State;

pub(crate) enum Msg {
    Restart,
    RestartResponse(Result<Packet<api::RemoteRestart>, ws::Error>),
    Toggle,
    ToggleResponse(Result<Packet<api::Updates>, ws::Error>),
    StateChanged(ws::State),
    Channel(Result<ws::Channel, ws::Error>),
    LogUpdate(log::Log),
    GetConfig(Result<Packet<api::GetConfig>, ws::Error>),
    ConfigUpdate(Result<Packet<api::Update>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct RemoteStatus {
    enabled: State<bool>,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
    state: ws::State,
    _get_status: ws::Request,
    _restart_request: ws::Request,
    _toggle_request: ws::Request,
    _config_update_listener: ws::Listener,
    _channel: ws::Request,
    channel: ws::Channel,
}

impl Component for RemoteStatus {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::LogUpdate))
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
            enabled: State::new(true),
            log,
            _log_handle,
            _state_change,
            state,
            _get_status: ws::Request::new(),
            _restart_request: ws::Request::new(),
            _toggle_request: ws::Request::new(),
            _config_update_listener,
            _channel: ws::Request::new(),
            channel: ws::Channel::default(),
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("remote::update", error);
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
            "Disable Remote Server"
        } else {
            "Enable Remote Server"
        };

        html! {
            <section class="remote control-group">
                <Icon name="remote" title="Status of Remote Server connection" invert={true} small={true} />

                <button
                    class="btn sm square"
                    onclick={on_restart}
                    disabled={!*self.enabled}
                    title="Restart remote connection"
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

impl RemoteStatus {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Restart => {
                self._restart_request = self
                    .channel
                    .request()
                    .body(api::RemoteRestartRequest)
                    .on_packet(ctx.link().callback(Msg::RestartResponse))
                    .send();

                Ok(false)
            }
            Msg::RestartResponse(result) => {
                let _ = result?;
                Ok(false)
            }
            Msg::Toggle => {
                let new_enabled = !*self.enabled;
                *self.enabled = new_enabled;

                self._toggle_request = self
                    .channel
                    .request()
                    .body(api::UpdatesRequest {
                        values: Vec::from([(Key::REMOTE_ENABLED, Value::from(new_enabled))]),
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
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::LogUpdate(log) => {
                self.log = log;
                Ok(false)
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
            Msg::Channel(channel) => {
                self.channel = channel?;

                self._get_status = self
                    .channel
                    .request()
                    .body(api::GetConfigRequest)
                    .on_packet(ctx.link().callback(Msg::GetConfig))
                    .send();

                Ok(true)
            }
        }
    }

    fn update_config(&mut self, key: Key, value: Value) -> Result<bool, Error> {
        match key {
            Key::REMOTE_ENABLED => Ok(self.enabled.update(value.as_bool().unwrap_or_default())),
            _ => Ok(false),
        }
    }

    fn refresh(&mut self, ctx: &Context<Self>) {
        if self.state.is_open() {
            self._channel = ctx
                .props()
                .ws
                .channel()
                .on_open(ctx.link().callback(Msg::Channel))
                .send();
        } else {
            self.channel = ws::Channel::default();
        }
    }
}
