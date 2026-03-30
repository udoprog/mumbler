use std::collections::HashMap;

use api::{Extent, Key, RemoteId, RemoteUpdateBody, Value};
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;

use super::{ChannelExt, ImageUpload, SetupChannel, into_target};

#[derive(Debug, Clone, Copy)]
pub(crate) enum ExtentAxis {
    StartX,
    EndX,
    StartZ,
    EndZ,
}

impl ExtentAxis {
    fn axis_mut(self, extent: &mut Extent) -> &mut f32 {
        match self {
            ExtentAxis::StartX => &mut extent.x.start,
            ExtentAxis::EndX => &mut extent.x.end,
            ExtentAxis::StartZ => &mut extent.z.start,
            ExtentAxis::EndZ => &mut extent.z.end,
        }
    }
}

pub(crate) enum Msg {
    Channel(Result<ws::Channel, Error>),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    RemoteIdChanged(Key, RemoteId),
    StringChanged(Key, Event),
    BooleanChanged(Key, Event),
    ExtentChanged(Key, ExtentAxis, Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    ObjectUpdate(Result<Packet<api::ObjectUpdate>, ws::Error>),
}

impl From<Result<Packet<api::ObjectUpdate>, ws::Error>> for Msg {
    #[inline]
    fn from(value: Result<Packet<api::ObjectUpdate>, ws::Error>) -> Self {
        Msg::ObjectUpdate(value)
    }
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) id: RemoteId,
    pub(crate) keys: &'static [Key],
}

pub(crate) struct ObjectSettings {
    _list_settings: ws::Request,
    _remote_update_listener: ws::Listener,
    _select_background: ws::Request,
    _update_extent: ws::Request,
    _update_name: ws::Request,
    _update_show_grid: ws::Request,
    log: log::Log,
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    properties: api::Properties,
    requests: HashMap<Key, ws::Request>,
}

impl Component for ObjectSettings {
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
            _list_settings: ws::Request::new(),
            _remote_update_listener: ws::Listener::new(),
            _select_background: ws::Request::new(),
            _update_extent: ws::Request::new(),
            _update_name: ws::Request::new(),
            _update_show_grid: ws::Request::new(),
            properties: api::Properties::default(),
            requests: HashMap::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("room_settings::update", error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        html! {
            <div id="content" class="rows">
                for key in ctx.props().keys {
                    {self.key_view(ctx, *key)}
                }
            </div>
        }
    }
}

impl ObjectSettings {
    fn key_view(&self, ctx: &Context<Self>, key: Key) -> Option<Html> {
        let html = match (key.ty()?, key) {
            (api::ValueType::String, _) => {
                let value = self.properties.get(key).as_str();

                html! {
                    <section class="input-group">
                        <label for={key.id()}>{format!("{}:", key.label())}</label>

                        <input
                            id={key.id()}
                            type="text"
                            placeholder={key.placeholder()}
                            value={value.to_owned()}
                            onchange={ctx.link().callback(move |ev: Event| Msg::StringChanged(key, ev))}
                        />
                    </section>
                }
            }
            (api::ValueType::Boolean, _) => {
                let value = self.properties.get(key).as_bool();

                html! {
                    <section class="input-group">
                        <label for={key.id()}>{format!("{}:", key.label())}</label>
                        <input
                            id={key.id()}
                            type="checkbox"
                            checked={value}
                            onchange={ctx.link().callback(move |ev: Event| Msg::BooleanChanged(key, ev))}
                        />
                    </section>
                }
            }
            (api::ValueType::Extent, _) => {
                let extent = self
                    .properties
                    .get(key)
                    .as_extent()
                    .unwrap_or_else(Extent::arena);

                html! {
                    <>
                        <section class="input-group">
                            <label for="extent-x-min">{"X Extents:"}</label>
                            <input
                                id="extent-x-min"
                                type="number"
                                step="1"
                                value={extent.x.start.to_string()}
                                onchange={ctx.link().callback(move |ev| Msg::ExtentChanged(key, ExtentAxis::StartX, ev))}
                            />

                            {" - "}

                            <input
                                id="extent-x-max"
                                type="number"
                                step="1"
                                value={extent.x.end.to_string()}
                                onchange={ctx.link().callback(move |ev| Msg::ExtentChanged(key, ExtentAxis::EndX, ev))}
                            />
                        </section>

                        <section class="input-group">
                            <label for="extent-z-min">{"Z Extents:"}</label>
                            <input
                                id="extent-z-min"
                                type="number"
                                step="1"
                                value={extent.z.start.to_string()}
                                onchange={ctx.link().callback(move |ev| Msg::ExtentChanged(key, ExtentAxis::StartZ, ev))}
                            />

                            {" - "}

                            <input
                                id="extent-z-max"
                                type="number"
                                step="1"
                                value={extent.z.end.to_string()}
                                onchange={ctx.link().callback(move |ev| Msg::ExtentChanged(key, ExtentAxis::EndZ, ev))}
                            />
                        </section>
                    </>
                }
            }
            (api::ValueType::Id, Key::ROOM_BACKGROUND) => {
                let value = RemoteId::local(self.properties.get(key).as_id());

                let src = (!value.is_zero()).then(|| format!("/api/image/{value}"));

                html! {
                    <>
                        <ImageUpload
                            selected={value}
                            sizing={api::ImageSizing::Crop}
                            size={1024}
                            role={api::Role::BACKGROUND}
                            input_id={key.id()}
                            onselect={ctx.link().callback(move |id| Msg::RemoteIdChanged(key, id))}
                            onclear={ctx.link().callback(move |_| Msg::RemoteIdChanged(key, RemoteId::ZERO))}
                        />

                        if let Some(src) = src {
                            <section class="input-image-preview">
                                <img src={src} />
                            </section>
                        }
                    </>
                }
            }
            _ => return None,
        };

        Some(html)
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Channel(channel) => {
                self.channel = channel?;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(true);
                }

                self._remote_update_listener = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::RemoteUpdate));

                self._list_settings = self
                    .channel
                    .request()
                    .body(api::GetObjectSettingsRequest {
                        id: ctx.props().id.id,
                    })
                    .on_packet(ctx.link().callback(Msg::GetObjectSettings))
                    .send();

                Ok(true)
            }
            Msg::GetObjectSettings(result) => {
                let body = result?;
                let body = body.decode()?;

                for (key, value) in body.object.props {
                    self.properties.insert(key, value);
                }

                Ok(true)
            }
            Msg::RemoteIdChanged(key, id) => {
                let old = self.properties.insert(key, Value::from(id.id));

                if old.as_id() == id.id {
                    return Ok(false);
                }

                self.requests.insert(
                    key,
                    self.channel.object_updates(
                        ctx,
                        ctx.props().id.id,
                        [(key, Value::from(id.id))],
                    ),
                );

                Ok(true)
            }
            Msg::StringChanged(key, ev) => {
                let input = into_target!(ev, HtmlInputElement);

                let value = input.value();

                let old = self.properties.insert(key, Value::from(value.as_str()));

                if old.as_str() == value.as_str() {
                    return Ok(false);
                }

                self.requests.insert(
                    key,
                    self.channel
                        .object_updates(ctx, ctx.props().id.id, [(key, value.into())]),
                );

                Ok(false)
            }
            Msg::BooleanChanged(key, ev) => {
                let input = into_target!(ev, HtmlInputElement);

                let value = input.checked();

                let old = self.properties.insert(key, Value::from(value));

                if old.as_bool() == value {
                    return Ok(false);
                }

                self.requests.insert(
                    key,
                    self.channel
                        .object_updates(ctx, ctx.props().id.id, [(key, value.into())]),
                );

                Ok(false)
            }
            Msg::ExtentChanged(key, axis, ev) => {
                let input = into_target!(ev, HtmlInputElement);
                let v = input.value().parse::<i32>()? as f32;

                let value = self.properties.into_mut(key);
                let extent = value.into_extent_mut();

                let a = axis.axis_mut(extent);

                if *a == v {
                    return Ok(false);
                }

                *a = v;

                self.requests.insert(
                    key,
                    self.channel
                        .object_updates(ctx, ctx.props().id.id, [(key, (*extent).into())]),
                );

                Ok(true)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                match body {
                    RemoteUpdateBody::ObjectUpdated { id, key, value, .. } => {
                        if id != ctx.props().id {
                            return Ok(false);
                        }

                        self.properties.insert(key, value);
                    }
                    _ => return Ok(false),
                }

                Ok(true)
            }
            Msg::ObjectUpdate(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
        }
    }
}
