use api::{Color, Key, RemoteId, Value};
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::{SetupChannel, into_target};

pub(crate) enum Msg {
    Channel(Result<ws::Channel, Error>),
    ColorChanged(Event),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    NameChanged(Event),
    SelectColor(api::Color),
    UpdateResult(Result<Packet<api::ObjectUpdate>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) id: RemoteId,
}

pub(crate) struct GroupSettings {
    log: log::Log,
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    _list_settings: ws::Request,
    _remote_update: ws::Listener,
    _select_color: ws::Request,
    _update_name: ws::Request,
    color: State<Option<api::Color>>,
    name: State<String>,
    _channel: ws::Request,
}

impl Component for GroupSettings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _) = ctx
            .link()
            .context::<log::Log>(Callback::noop())
            .expect("Log context not found");

        Self {
            log,
            channel: ws::Channel::default(),
            _setup_channel: SetupChannel::new(ctx, ctx.link().callback(Msg::Channel)),
            color: State::default(),
            name: State::default(),
            _list_settings: ws::Request::new(),
            _select_color: ws::Request::new(),
            _update_name: ws::Request::new(),
            _remote_update: ws::Listener::new(),
            _channel: ws::Request::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("static_settings::update", error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let color = self.color.unwrap_or_else(Color::neutral);

        html! {
            <>
            <div id="content" class="row">
                <div class="rows">
                    <section class="input-group">
                        <label for="group-name">{"Name:"}</label>

                        <input
                            id="group-name"
                            type="text"
                            placeholder="Enter name"
                            value={self.name.to_string()}
                            onchange={ctx.link().callback(Msg::NameChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="group-color">
                            {"Color:"}
                            <span class="color-preview" style={format!("--color: {}", color.to_css_string())} />
                        </label>

                        <input
                            id="group-color"
                            class="hidden"
                            type="color"
                            value={color.to_css_string()}
                            onchange={ctx.link().callback(Msg::ColorChanged)}
                            />
                    </section>
                </div>
            </div>
            </>
        }
    }
}

impl GroupSettings {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Channel(channel) => {
                self.channel = channel?;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(true);
                }

                self._list_settings = self
                    .channel
                    .request()
                    .body(api::GetObjectSettingsRequest {
                        id: ctx.props().id.id,
                    })
                    .on_packet(ctx.link().callback(Msg::GetObjectSettings))
                    .send();

                self._remote_update = self
                    .channel
                    .handle()
                    .on_broadcast::<api::RemoteUpdate>(ctx.link().callback(Msg::RemoteUpdate));

                Ok(true)
            }
            Msg::GetObjectSettings(result) => {
                let body = result?;
                let body = body.decode()?;

                for (key, value) in body.object.props {
                    self.update_property(ctx, key, value);
                }

                Ok(true)
            }
            Msg::ColorChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let hex_string = input.value();

                if let Some(color) = api::Color::from_hex(&hex_string) {
                    ctx.link().send_message(Msg::SelectColor(color));
                }

                Ok(false)
            }
            Msg::SelectColor(color) => {
                *self.color = Some(color);
                self._select_color = object_update(&self.channel, ctx, Key::COLOR, color);
                Ok(true)
            }
            Msg::NameChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let name = input.value();

                *self.name = name.clone();
                self._update_name = object_update(&self.channel, ctx, Key::NAME, name);
                Ok(false)
            }
            Msg::UpdateResult(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    api::RemoteUpdateBody::ObjectUpdated { id, key, value, .. } => {
                        if ctx.props().id != id {
                            return Ok(false);
                        }

                        self.update_property(ctx, key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
            }
        }
    }

    fn update_property(&mut self, _: &Context<Self>, key: Key, value: Value) -> bool {
        match key {
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.as_str().to_owned()),
            _ => false,
        }
    }
}

fn object_update(
    channel: &ws::Channel,
    ctx: &Context<GroupSettings>,
    key: Key,
    value: impl Into<Value>,
) -> ws::Request {
    channel
        .request()
        .body(api::ObjectUpdateBody {
            id: ctx.props().id.id,
            key,
            value: value.into(),
        })
        .on_packet(ctx.link().callback(Msg::UpdateResult))
        .send()
}
