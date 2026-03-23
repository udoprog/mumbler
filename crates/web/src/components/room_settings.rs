use api::{Extent, Id, Image, Key, PeerId, RemoteUpdateBody, Value};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::{ImageUpload, into_target};

pub(crate) enum Msg {
    StateChanged(ws::State),
    Channel(Result<ws::Channel, ws::Error>),
    ExtentXMinChanged(Event),
    ExtentXMaxChanged(Event),
    ExtentYMinChanged(Event),
    ExtentYMaxChanged(Event),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    ImageSelected(Id),
    ImagesRefresh,
    NameChanged(Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    SetLog(log::Log),
    ShowGridChanged(Event),
    UpdateName(Option<String>),
    UpdateResult(Result<Packet<api::ObjectUpdate>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) id: Id,
}

pub(crate) struct RoomSettings {
    _list_settings: ws::Request,
    _log_handle: ContextHandle<log::Log>,
    _remote_update_listener: ws::Listener,
    _select_background: ws::Request,
    _state_change: ws::StateListener,
    _update_extent: ws::Request,
    _update_name: ws::Request,
    _update_show_grid: ws::Request,
    background: State<Id>,
    extent: State<Extent>,
    images: Vec<Image>,
    log: log::Log,
    name: State<Option<String>>,
    show_grid: State<bool>,
    state: ws::State,
    _channel: ws::Request,
    channel: ws::Channel,
}

impl Component for RoomSettings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::SetLog))
            .expect("ErrorLog context not found");

        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let _remote_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::RemoteUpdate>(ctx.link().callback(Msg::RemoteUpdate));

        let mut this = Self {
            _list_settings: ws::Request::new(),
            _log_handle,
            _remote_update_listener,
            _select_background: ws::Request::new(),
            _state_change,
            _update_extent: ws::Request::new(),
            _update_name: ws::Request::new(),
            _update_show_grid: ws::Request::new(),
            background: State::new(Id::ZERO),
            extent: State::new(Extent::arena()),
            images: Vec::new(),
            log,
            name: State::new(None),
            show_grid: State::new(true),
            state,
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
                self.log.error("room_settings::update", error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let background_src = (!self.background.is_zero())
            .then(|| format!("/api/image/{}/{}", PeerId::ZERO, *self.background));

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
                        value={(*self.name).clone().unwrap_or_default()}
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
                    ws={&ctx.props().ws}
                    images={self.images.clone()}
                    selected={*self.background}
                    sizing={api::ImageSizing::Crop}
                    size={1024}
                    role={api::Role::BACKGROUND}
                    input_id="background"
                    onselect={ctx.link().callback(Msg::ImageSelected)}
                    onrefresh={ctx.link().callback(|_| Msg::ImagesRefresh)}
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

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::Channel(channel) => {
                self.channel = channel?;

                self._list_settings = self
                    .channel
                    .request()
                    .body(api::GetObjectSettingsRequest { id: ctx.props().id })
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

                self.images = body.images;
                Ok(true)
            }
            Msg::ImagesRefresh => {
                self.refresh(ctx);
                Ok(false)
            }
            Msg::ImageSelected(id) => {
                *self.background = id;
                self._select_background =
                    object_update(&self.channel, ctx, Key::ROOM_BACKGROUND, id);
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
                let value = input.value();
                let name = if value.is_empty() { None } else { Some(value) };
                ctx.link().send_message(Msg::UpdateName(name));
                Ok(false)
            }
            Msg::UpdateName(name) => {
                *self.name = name.clone();
                self._update_name = object_update(&self.channel, ctx, Key::OBJECT_NAME, name);
                Ok(true)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    RemoteUpdateBody::ObjectUpdated { id, key, value, .. } => {
                        if id.id != ctx.props().id {
                            return Ok(false);
                        }

                        self.update_property(key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
            }
            Msg::SetLog(log) => {
                self.log = log;
                Ok(false)
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
            Key::OBJECT_NAME => self.name.update(value.as_str().map(str::to_owned)),
            Key::ROOM_BACKGROUND => self.background.update(value.as_id()),
            Key::ROOM_EXTENT => self
                .extent
                .update(value.as_extent().unwrap_or_else(Extent::arena)),
            Key::SHOW_GRID => self.show_grid.update(value.as_bool().unwrap_or(true)),
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
            id: ctx.props().id,
            key,
            value: value.into(),
        })
        .on_packet(ctx.link().callback(Msg::UpdateResult))
        .send()
}
