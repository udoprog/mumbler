use api::{Extent, Id, Key, RemoteId, RemoteUpdateBody, Value};
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::{ImageUpload, SetupChannel, into_target};

pub(crate) enum Msg {
    Channel(Result<ws::Channel, Error>),
    ExtentXMinChanged(Event),
    ExtentXMaxChanged(Event),
    ExtentYMinChanged(Event),
    ExtentYMaxChanged(Event),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    ImageSelected(RemoteId),
    ImageClear,
    NameChanged(Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    ShowGridChanged(Event),
    UpdateResult(Result<Packet<api::ObjectUpdate>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) id: RemoteId,
}

pub(crate) struct RoomSettings {
    _list_settings: ws::Request,
    _remote_update_listener: ws::Listener,
    _select_background: ws::Request,
    _update_extent: ws::Request,
    _update_name: ws::Request,
    _update_show_grid: ws::Request,
    background: State<RemoteId>,
    extent: State<Extent>,
    log: log::Log,
    name: State<String>,
    show_grid: State<bool>,
    channel: ws::Channel,
    _setup_channel: SetupChannel,
}

impl Component for RoomSettings {
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
            _list_settings: ws::Request::new(),
            _remote_update_listener: ws::Listener::new(),
            _select_background: ws::Request::new(),
            _update_extent: ws::Request::new(),
            _update_name: ws::Request::new(),
            _update_show_grid: ws::Request::new(),
            background: State::new(RemoteId::ZERO),
            extent: State::new(Extent::arena()),
            name: State::default(),
            show_grid: State::new(true),
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
        let background_src =
            (!self.background.is_zero()).then(|| format!("/api/image/{}", *self.background));

        let extent = *self.extent;

        html! {
            <>
            <div id="content" class="rows">
                <section class="input-group">
                    <label for="name">{"Name:"}</label>

                    <input
                        id="name"
                        type="text"
                        placeholder="Enter name"
                        value={self.name.to_string()}
                        onchange={ctx.link().callback(Msg::NameChanged)}
                    />
                </section>

                <section class="input-group">
                    <label for="show-grid">{"Show Grid:"}</label>
                    <input
                        id="show-grid"
                        type="checkbox"
                        checked={*self.show_grid}
                        onchange={ctx.link().callback(Msg::ShowGridChanged)}
                    />
                </section>

                <section class="input-group">
                    <label for="extent-x-min">{"X Extents:"}</label>
                    <input
                        id="extent-x-min"
                        type="number"
                        step="1"
                        value={extent.x.start.to_string()}
                        onchange={ctx.link().callback(Msg::ExtentXMinChanged)}
                    />

                    {" - "}

                    <input
                        id="extent-x-max"
                        type="number"
                        step="1"
                        value={extent.x.end.to_string()}
                        onchange={ctx.link().callback(Msg::ExtentXMaxChanged)}
                    />
                </section>

                <section class="input-group">
                    <label for="extent-y-min">{"Y Extents:"}</label>
                    <input
                        id="extent-y-min"
                        type="number"
                        step="1"
                        value={extent.y.start.to_string()}
                        onchange={ctx.link().callback(Msg::ExtentYMinChanged)}
                    />

                    {" - "}

                    <input
                        id="extent-y-max"
                        type="number"
                        step="1"
                        value={extent.y.end.to_string()}
                        onchange={ctx.link().callback(Msg::ExtentYMaxChanged)}
                    />
                </section>

                <ImageUpload
                    selected={*self.background}
                    sizing={api::ImageSizing::Crop}
                    size={1024}
                    role={api::Role::BACKGROUND}
                    input_id="background"
                    onselect={ctx.link().callback(Msg::ImageSelected)}
                    onclear={ctx.link().callback(|_| Msg::ImageClear)}
                />

                if let Some(src) = background_src {
                    <section class="background-preview">
                        <img src={src} />
                    </section>
                }
            </div>
            </>
        }
    }
}

impl RoomSettings {
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
                    self.update_property(key, value);
                }

                Ok(true)
            }
            Msg::ImageSelected(id) => {
                *self.background = id;
                self._select_background =
                    object_update(&self.channel, ctx, Key::ROOM_BACKGROUND, id.id);
                Ok(true)
            }
            Msg::ImageClear => {
                *self.background = RemoteId::ZERO;
                self._select_background =
                    object_update(&self.channel, ctx, Key::ROOM_BACKGROUND, Id::ZERO);
                Ok(true)
            }
            Msg::ShowGridChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let value = input.checked();
                *self.show_grid = value;
                self._update_show_grid = object_update(&self.channel, ctx, Key::SHOW_GRID, value);
                Ok(true)
            }
            Msg::ExtentXMinChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let v = input.value().parse::<i32>()? as f32;
                self.extent.x.start = v.min(self.extent.x.end);
                self._update_extent =
                    object_update(&self.channel, ctx, Key::ROOM_EXTENT, *self.extent);
                Ok(true)
            }
            Msg::ExtentXMaxChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let v = input.value().parse::<i32>()? as f32;
                self.extent.x.end = v.max(self.extent.x.start);
                self._update_extent =
                    object_update(&self.channel, ctx, Key::ROOM_EXTENT, *self.extent);
                Ok(true)
            }
            Msg::ExtentYMinChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let v = input.value().parse::<i32>()? as f32;
                self.extent.y.start = v.min(self.extent.y.end);
                self._update_extent =
                    object_update(&self.channel, ctx, Key::ROOM_EXTENT, *self.extent);
                Ok(true)
            }
            Msg::ExtentYMaxChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let v = input.value().parse::<i32>()? as f32;
                self.extent.y.end = v.max(self.extent.y.start);
                self._update_extent =
                    object_update(&self.channel, ctx, Key::ROOM_EXTENT, *self.extent);
                Ok(true)
            }
            Msg::NameChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let name = input.value();

                *self.name = name.clone();
                self._update_name = object_update(&self.channel, ctx, Key::NAME, name);
                Ok(false)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    RemoteUpdateBody::ObjectUpdated { id, key, value, .. } => {
                        if id != ctx.props().id {
                            return Ok(false);
                        }

                        self.update_property(key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
            }
            Msg::UpdateResult(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
        }
    }

    fn update_property(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::NAME => self.name.update(value.as_str().to_owned()),
            Key::ROOM_BACKGROUND => self.background.update(RemoteId::local(value.as_id())),
            Key::ROOM_EXTENT => self
                .extent
                .update(value.as_extent().unwrap_or_else(Extent::arena)),
            Key::SHOW_GRID => self.show_grid.update(value.as_bool()),
            _ => false,
        }
    }
}

fn object_update(
    channel: &ws::Channel,
    ctx: &Context<RoomSettings>,
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
