use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use yew::prelude::*;

use super::Icon;
use crate::error::Error;
use crate::log;

pub(crate) enum Msg {
    GetRemoteStatus(Result<Packet<api::GetRemoteStatus>, ws::Error>),
    Restart,
    RestartResponse(Result<Packet<api::RemoteRestart>, ws::Error>),
    Toggle,
    ToggleResponse(Result<Packet<api::RemoteToggle>, ws::Error>),
    StateChanged(ws::State),
    LogUpdate(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct RemoteStatus {
    enabled: bool,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
    state: ws::State,
    _get_status: ws::Request,
    _restart_request: ws::Request,
    _toggle_request: ws::Request,
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

        let mut this = Self {
            enabled: true,
            log,
            _log_handle,
            _state_change,
            state,
            _get_status: ws::Request::new(),
            _restart_request: ws::Request::new(),
            _toggle_request: ws::Request::new(),
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("remote::update", &error);
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let on_restart = ctx.link().callback(|_| Msg::Restart);
        let on_toggle = ctx.link().callback(|_| Msg::Toggle);

        let toggle_text = if self.enabled {
            html!(<Icon name="x-mark" />)
        } else {
            html!(<Icon name="check" />)
        };

        let toggle_title = if self.enabled {
            "Disable Remote Server"
        } else {
            "Enable Remote Server"
        };

        html! {
            <section class="remote control-group">
                <Icon name="remote" title="Status of Remote Server connection" />
                <button
                    class="btn square sm secondary"
                    onclick={on_restart}
                    disabled={!self.enabled}
                    title="Restart remote connection"
                >
                    <Icon name="restart" />
                </button>
                <button
                    class="btn square sm secondary"
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
            Msg::GetRemoteStatus(result) => {
                let packet = result?;
                let response = packet.decode()?;
                self.enabled = response.enabled;
                Ok(true)
            }
            Msg::Restart => {
                if !matches!(self.state, ws::State::Open) {
                    return Ok(false);
                }

                self._restart_request = ctx
                    .props()
                    .ws
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
                if !matches!(self.state, ws::State::Open) {
                    return Ok(false);
                }

                let new_enabled = !self.enabled;

                self._toggle_request = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::RemoteToggleRequest {
                        enabled: new_enabled,
                    })
                    .on_packet(ctx.link().callback(Msg::ToggleResponse))
                    .send();

                self.enabled = new_enabled;
                Ok(true)
            }
            Msg::ToggleResponse(result) => {
                let packet = result?;
                let response = packet.decode()?;
                self.enabled = response.enabled;
                Ok(true)
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
        }
    }

    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._get_status = ctx
                .props()
                .ws
                .request()
                .body(api::GetRemoteStatusRequest)
                .on_packet(ctx.link().callback(Msg::GetRemoteStatus))
                .send();
        }
    }
}
