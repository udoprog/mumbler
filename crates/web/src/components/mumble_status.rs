use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use yew::prelude::*;

use super::Icon;
use crate::error::Error;

pub(crate) enum Msg {
    GetMumbleStatus(Result<Packet<api::GetMumbleStatus>, ws::Error>),
    Restart,
    RestartResponse(Result<Packet<api::MumbleRestart>, ws::Error>),
    Toggle,
    ToggleResponse(Result<Packet<api::MumbleToggle>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct MumbleStatus {
    enabled: bool,
    _get_status: ws::Request,
    _restart_request: ws::Request,
    _toggle_request: ws::Request,
}

impl Component for MumbleStatus {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let _get_status = ctx
            .props()
            .ws
            .request()
            .body(api::GetMumbleStatusRequest)
            .on_packet(ctx.link().callback(Msg::GetMumbleStatus))
            .send();

        Self {
            enabled: false,
            _get_status,
            _restart_request: ws::Request::new(),
            _toggle_request: ws::Request::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::error!(%error, "Failed to update mumble status");
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
            "Disable Mumble Link"
        } else {
            "Enable Mumble Link"
        };

        html! {
            <section class="mumble">
                <Icon name="mumble" />
                <button
                    class="btn square sm secondary"
                    onclick={on_restart}
                    disabled={!self.enabled}
                    title="Restart Mumble Link"
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

impl MumbleStatus {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::GetMumbleStatus(result) => {
                let packet = result?;
                let response = packet.decode()?;
                self.enabled = response.enabled;
                tracing::info!("Mumble initial state: enabled={}", self.enabled);
                Ok(true)
            }
            Msg::Restart => {
                let ws = ctx.props().ws.clone();
                let callback = ctx.link().callback(Msg::RestartResponse);

                self._restart_request = ws
                    .request()
                    .body(api::MumbleRestartRequest)
                    .on_packet(callback)
                    .send();

                Ok(false)
            }
            Msg::RestartResponse(result) => {
                let _ = result?;
                tracing::info!("Mumble restarted successfully");
                Ok(false)
            }
            Msg::Toggle => {
                let new_enabled = !self.enabled;
                let ws = ctx.props().ws.clone();
                let callback = ctx.link().callback(Msg::ToggleResponse);

                self._toggle_request = ws
                    .request()
                    .body(api::MumbleToggleRequest {
                        enabled: new_enabled,
                    })
                    .on_packet(callback)
                    .send();

                self.enabled = new_enabled;
                Ok(true)
            }
            Msg::ToggleResponse(result) => {
                let packet = result?;
                let response = packet.decode()?;
                self.enabled = response.enabled;
                tracing::info!("Mumble enabled: {}", self.enabled);
                Ok(true)
            }
        }
    }
}
