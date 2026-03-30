use api::{Color, Key, PublicKey, RemoteId, RemoteUpdateBody, UpdateBody, Value};
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement};
use yew::prelude::*;

use crate::error::Error;
use crate::images::Images;
use crate::log;
use crate::state::State;

use super::{
    ChannelExt, DynamicCanvas, ImageUpload, SetupChannel, ViewTransform, Visibility, into_target,
    render,
};

pub(crate) enum Msg {
    Channel(Result<ws::Channel, Error>),
    Error(Error),
    ColorChanged(Event),
    FixedRatioChanged(Event),
    HeightChanged(Event),
    ImageSelected(RemoteId),
    Initialize(Result<Packet<api::GetObjectSettings>, ws::Error>),
    NameChanged(Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    Ratio(f64),
    SelectColor(api::Color),
    Update(Result<Packet<api::Update>, ws::Error>),
    ObjectUpdate(Result<Packet<api::ObjectUpdate>, ws::Error>),
    WidthChanged(Event),
    ImageLoaded(Result<(), Error>),
    CanvasLoaded(HtmlCanvasElement),
    CanvasResized((u32, u32)),
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
}

pub(crate) struct StaticSettings {
    log: log::Log,
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    _list_settings: ws::Request,
    _remote_update_listener: ws::Listener,
    _update_listener: ws::Listener,
    _select_color: ws::Request,
    _select_image: ws::Request,
    _update_dimensions: ws::Request,
    _update_name: ws::Request,
    color: State<Option<api::Color>>,
    height: State<f32>,
    image: State<RemoteId>,
    public_key: PublicKey,
    name: State<String>,
    canvas: Option<HtmlCanvasElement>,
    preview_images: Images,
    ratio: State<Option<f32>>,
    width: State<f32>,
}

impl Component for StaticSettings {
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
            _update_listener: ws::Listener::new(),
            _select_color: ws::Request::new(),
            _select_image: ws::Request::new(),
            _update_dimensions: ws::Request::new(),
            _update_name: ws::Request::new(),
            color: State::default(),
            height: State::new(1.0),
            image: State::new(RemoteId::ZERO),
            public_key: PublicKey::ZERO,
            name: State::default(),
            canvas: None,
            preview_images: Images::new(ctx.link().callback(Msg::ImageLoaded)),
            ratio: State::default(),
            width: State::new(1.0),
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

    fn rendered(&mut self, _ctx: &Context<Self>, _first_render: bool) {
        if let Err(error) = self.redraw_preview() {
            self.log.error("static_settings::redraw_preview", error);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let color = self.color.unwrap_or_else(Color::neutral);

        let current_ratio = if let Some(ratio) = *self.ratio {
            html! { <span class="fixed-ratio"> {format!("{:.2}:1", ratio)} </span> }
        } else {
            html! {}
        };

        html! {
            <>
            <div id="content" class="row">
                <div class="col-8 rows">
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
                        <label for="color">
                            {"Color:"}
                            <span class="color-preview" style={format!("--color: {}", color.to_css_string())} />
                        </label>

                        <input
                            id="color"
                            class="hidden"
                            type="color"
                            value={color.to_css_string()}
                            onchange={ctx.link().callback(Msg::ColorChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="width">{"Width:"}</label>

                        <input
                            id="width"
                            type="number"
                            min="0.05"
                            max="50"
                            step="0.05"
                            value={format!("{}", *self.width)}
                            onchange={ctx.link().callback(Msg::WidthChanged)}
                            />
                    </section>

                    if self.ratio.is_none() {
                        <section class="input-group">
                            <label for="height">{"Height:"}</label>

                            <input
                                id="height"
                                type="number"
                                min="0.05"
                                max="50"
                                step="0.05"
                                value={format!("{}", *self.height)}
                                onchange={ctx.link().callback(Msg::HeightChanged)}
                                />
                        </section>
                    }

                    <section class="input-group">
                        <label for="fixed-ratio">{"Fixed Ratio:"}</label>

                        <input
                            id="fixed-ratio"
                            type="checkbox"
                            checked={self.ratio.is_some()}
                            onchange={ctx.link().callback(Msg::FixedRatioChanged)}
                            />

                        {current_ratio}
                    </section>

                    <ImageUpload
                        selected={*self.image}
                        sizing={api::ImageSizing::Crop}
                        size={512}
                        ratio={Some((*self.width / *self.height) as f64)}
                        role={api::Role::STATIC}
                        input_id="image"
                        onselect={ctx.link().callback(Msg::ImageSelected)}
                        onclear={ctx.link().callback(|_| Msg::ImageSelected(RemoteId::ZERO))}
                        onratio={ctx.link().callback(Msg::Ratio)}
                    />
                </div>

                <div class="col-4 rows">
                    <section class="preview">
                        <DynamicCanvas
                            onload={ctx.link().callback(Msg::CanvasLoaded)}
                            onerror={ctx.link().callback(Msg::Error)}
                            onresize={ctx.link().callback(Msg::CanvasResized)}
                            />
                    </section>
                </div>
            </div>
            </>
        }
    }
}

impl StaticSettings {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Error(error) => {
                self.log.error("static_settings", error);
                Ok(false)
            }
            Msg::Initialize(result) => {
                let body = result?;
                let body = body.decode()?;

                for (key, value) in body.object.props {
                    self.update_property(ctx, key, value);
                }

                self.public_key = body.public_key;
                Ok(true)
            }
            Msg::Channel(channel) => {
                self.channel = channel?;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(false);
                }

                self._remote_update_listener = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::RemoteUpdate));

                self._update_listener = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::Update));

                self._list_settings = self
                    .channel
                    .request()
                    .body(api::GetObjectSettingsRequest {
                        id: ctx.props().id.id,
                    })
                    .on_packet(ctx.link().callback(Msg::Initialize))
                    .send();

                Ok(true)
            }
            Msg::ImageSelected(id) => {
                if !self.image.update(id) {
                    return Ok(false);
                }

                self.load_preview_image(ctx);

                self._select_image = self.channel.object_updates(
                    ctx,
                    ctx.props().id.id,
                    [(Key::IMAGE_ID, self.image.id.into())],
                );
                Ok(true)
            }
            Msg::Ratio(ratio) => {
                let mut update = false;

                update |= self.ratio.update(Some(ratio as f32));
                update |= self.width.update_epsilon(*self.height * ratio as f32);

                if update {
                    self._update_dimensions = self.channel.object_updates(
                        ctx,
                        ctx.props().id.id,
                        [
                            (Key::RATIO, self.ratio.value()),
                            (Key::STATIC_WIDTH, self.width.value()),
                        ],
                    );
                }

                Ok(update)
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
                if !self.color.update(Some(color)) {
                    return Ok(false);
                }

                self._select_color = self.channel.object_updates(
                    ctx,
                    ctx.props().id.id,
                    [(Key::COLOR, self.color.value())],
                );

                Ok(true)
            }
            Msg::NameChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                if !self.name.update(input.value()) {
                    return Ok(false);
                }

                self._update_name = self.channel.object_updates(
                    ctx,
                    ctx.props().id.id,
                    [(Key::NAME, self.name.deref_value())],
                );

                Ok(true)
            }
            Msg::WidthChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let changed = 'done: {
                    let Ok(width) = input.value().parse::<f32>() else {
                        break 'done false;
                    };

                    if !self.width.update(width.clamp(0.05, 50.0)) {
                        break 'done false;
                    }

                    if let Some(ratio) = *self.ratio
                        && self.height.update((*self.width / ratio).clamp(0.05, 50.0))
                    {
                        self._update_dimensions = self.channel.object_updates(
                            ctx,
                            ctx.props().id.id,
                            [
                                (Key::STATIC_WIDTH, self.width.value()),
                                (Key::STATIC_HEIGHT, self.height.value()),
                            ],
                        );
                    } else {
                        self._update_dimensions = self.channel.object_updates(
                            ctx,
                            ctx.props().id.id,
                            [(Key::STATIC_WIDTH, self.width.value())],
                        );
                    }

                    true
                };

                Ok(changed)
            }
            Msg::HeightChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let changed = 'done: {
                    let Ok(height) = input.value().parse::<f32>() else {
                        break 'done false;
                    };

                    let height = height.clamp(0.05, 50.0);
                    *self.height = height;

                    if let Some(ratio) = *self.ratio {
                        *self.width = (*self.height * ratio).clamp(0.05, 50.0);

                        self._update_dimensions = self.channel.object_updates(
                            ctx,
                            ctx.props().id.id,
                            [
                                (Key::STATIC_HEIGHT, height.into()),
                                (Key::STATIC_WIDTH, (*self.width).into()),
                            ],
                        );
                    } else {
                        self._update_dimensions = self.channel.object_updates(
                            ctx,
                            ctx.props().id.id,
                            [(Key::STATIC_HEIGHT, height.into())],
                        );
                    }

                    true
                };

                Ok(changed)
            }
            Msg::FixedRatioChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let fixed_ratio = input.checked();

                let ratio = if fixed_ratio {
                    let ratio = *self.width / *self.height;
                    Some((ratio * 100.0).round() / 100.0)
                } else {
                    None
                };

                if !self.ratio.update(ratio) {
                    return Ok(false);
                };

                self._update_dimensions = self.channel.object_updates(
                    ctx,
                    ctx.props().id.id,
                    [(Key::RATIO, (*self.ratio).into())],
                );

                Ok(true)
            }
            Msg::ObjectUpdate(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    RemoteUpdateBody::ObjectUpdated {
                        channel,
                        id,
                        key,
                        value,
                    } => {
                        if self.channel.id() != channel {
                            return Ok(false);
                        }

                        if ctx.props().id != id {
                            return Ok(false);
                        }

                        self.update_property(ctx, key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
            }
            Msg::Update(body) => {
                let body = body?;
                let body = body.decode()?;

                match body {
                    UpdateBody::PublicKey { public_key } => {
                        self.public_key = public_key;
                        Ok(true)
                    }
                    _ => Ok(false),
                }
            }
            Msg::ImageLoaded(result) => {
                result?;
                self.redraw_preview()?;
                Ok(false)
            }
            Msg::CanvasLoaded(canvas) => {
                self.canvas = Some(canvas);
                self.redraw_preview()?;
                Ok(false)
            }
            Msg::CanvasResized((_, _)) => {
                self.redraw_preview()?;
                Ok(false)
            }
        }
    }

    fn update_property(&mut self, ctx: &Context<Self>, key: Key, value: Value) -> bool {
        match key {
            Key::IMAGE_ID => {
                if self.image.update(RemoteId::local(value.as_id())) {
                    self.load_preview_image(ctx);
                    true
                } else {
                    false
                }
            }
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.as_str().to_owned()),
            Key::STATIC_WIDTH => self.width.update(value.as_f32().unwrap_or(1.0)),
            Key::STATIC_HEIGHT => self.height.update(value.as_f32().unwrap_or(1.0)),
            Key::RATIO => self.ratio.update(value.as_f32()),
            _ => false,
        }
    }

    fn load_preview_image(&mut self, ctx: &Context<Self>) {
        self.preview_images.clear();

        if !self.image.is_zero() {
            self.preview_images
                .load_id(&self.image, ctx.link().callback(Msg::ImageLoaded));
        }
    }

    fn redraw_preview(&self) -> Result<(), Error> {
        let Some(canvas) = &self.canvas else {
            return Ok(());
        };

        let Some(cx) = canvas.get_context("2d")? else {
            return Ok(());
        };

        let Ok(cx) = cx.dyn_into::<CanvasRenderingContext2d>() else {
            return Ok(());
        };

        let base = render::RenderBase {
            name: self.name.as_str(),
            visibility: Visibility::Remote,
            selected: false,
        };

        let width = canvas.width();
        let height = canvas.height();

        let render = render::RenderStatic {
            transform: &api::Transform::origin(),
            image: *self.image,
            color: self.color.unwrap_or_else(Color::neutral),
            width: *self.width,
            height: *self.height,
        };

        let min = width.min(height) as f32;

        let scale = (min - min * 0.2) / self.width.max(*self.height);
        let view = ViewTransform::simple(width, height, scale);

        cx.clear_rect(0.0, 0.0, view.width as f64, view.height as f64);
        render::draw_static(&cx, &view, &base, &render, &self.preview_images)?;
        Ok(())
    }
}
