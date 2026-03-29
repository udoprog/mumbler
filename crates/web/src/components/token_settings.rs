use anyhow::Context as _;
use api::{Color, Key, PublicKey, RemoteId, RemoteUpdateBody, Value};
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement};
use yew::prelude::*;

use crate::components::render::{ViewTransform, Visibility};
use crate::error::Error;
use crate::images::Images;
use crate::log;
use crate::state::State;

use super::{ChannelExt, DynamicCanvas, ImageUpload, SetupChannel, into_target, render};

pub(crate) enum Msg {
    Error(Error),
    Channel(Result<ws::Channel, Error>),
    ColorChanged(Event),
    ImageSelected(RemoteId),
    Initialize(Result<Packet<api::GetObjectSettings>, ws::Error>),
    NameChanged(Event),
    RadiusChanged(Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    SpeedChanged(Event),
    ObjectUpdate(Result<Packet<api::ObjectUpdate>, ws::Error>),
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

pub(crate) struct TokenSettings {
    log: log::Log,
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    _list_settings: ws::Request,
    _remote_update: ws::Listener,
    _select_color: ws::Request,
    _select_image: ws::Request,
    _update_name: ws::Request,
    _update_radius: ws::Request,
    color: State<Option<Color>>,
    image: State<RemoteId>,
    public_key: PublicKey,
    name: State<String>,
    canvas: Option<HtmlCanvasElement>,
    preview_images: Images,
    speed: State<f32>,
    token_radius: State<f32>,
}

impl Component for TokenSettings {
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
            _remote_update: ws::Listener::new(),
            _select_color: ws::Request::new(),
            _select_image: ws::Request::new(),
            _update_name: ws::Request::new(),
            _update_radius: ws::Request::new(),
            color: State::default(),
            image: State::new(RemoteId::ZERO),
            public_key: PublicKey::ZERO,
            name: State::default(),
            canvas: None,
            preview_images: Images::new(ctx.link().callback(Msg::ImageLoaded)),
            speed: State::new(5.0),
            token_radius: State::new(0.25),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("object_settings::update", error);
                true
            }
        }
    }

    fn rendered(&mut self, _ctx: &Context<Self>, _first_render: bool) {
        if let Err(error) = self.redraw_preview() {
            self.log.error("object_settings::redraw_preview", error);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let color = self.color.unwrap_or_else(Color::neutral);

        html! {
            <>
            <div id="content" class="row">
                <div class="col-6 rows">
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
                        <label for="radius">{"Radius:"}</label>

                        <input
                            id="radius"
                            type="number"
                            min="0.05"
                            max="10"
                            step="0.05"
                            value={format!("{}", *self.token_radius)}
                            onchange={ctx.link().callback(Msg::RadiusChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="speed">{"Speed:"}</label>

                        <input
                            id="speed"
                            type="number"
                            min="0.5"
                            max="100"
                            step="0.5"
                            value={format!("{}", *self.speed)}
                            onchange={ctx.link().callback(Msg::SpeedChanged)}
                            />
                    </section>

                    <ImageUpload
                        selected={*self.image}
                        sizing={api::ImageSizing::Square}
                        size={128}
                        ratio={1.0}
                        role={api::Role::TOKEN}
                        input_id="image"
                        onselect={ctx.link().callback(Msg::ImageSelected)}
                        onclear={ctx.link().callback(|_| Msg::ImageSelected(RemoteId::ZERO))}
                    />
                </div>

                <div class="col-6 rows">
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

impl TokenSettings {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Error(error) => {
                self.log.error("token_settings", error);
                Ok(false)
            }
            Msg::Channel(channel) => {
                self.channel = channel?;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(true);
                }

                self._remote_update = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::RemoteUpdate));

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
            Msg::Initialize(body) => {
                let body = body?;
                let body = body.decode()?;

                for (key, value) in body.object.props {
                    self.update_property(ctx, key, value);
                }

                self.public_key = body.public_key;
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
            Msg::ColorChanged(e) => {
                let input = into_target!(e, HtmlInputElement);
                let color = input.value();
                let color = Color::from_hex(&color).context("invalid color hex string")?;

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
            Msg::RadiusChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let Ok(radius) = input.value().parse::<f32>() else {
                    return Ok(false);
                };

                if !self.token_radius.update(radius.clamp(0.05, 10.0)) {
                    return Ok(false);
                }

                self._update_radius = self.channel.object_updates(
                    ctx,
                    ctx.props().id.id,
                    [(Key::TOKEN_RADIUS, self.token_radius.value())],
                );

                Ok(true)
            }
            Msg::SpeedChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = 'done: {
                    let Ok(speed) = input.value().parse::<f32>() else {
                        break 'done false;
                    };

                    *self.speed = speed.clamp(0.5, 100.0);
                    self._update_radius = self.channel.object_updates(
                        ctx,
                        ctx.props().id.id,
                        [(Key::SPEED, self.speed.value())],
                    );
                    true
                };

                Ok(value)
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
                    RemoteUpdateBody::ObjectUpdated { id, key, value, .. } => {
                        if ctx.props().id != id {
                            return Ok(false);
                        }

                        self.update_property(ctx, key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
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
                if !self.image.update(RemoteId::local(value.as_id())) {
                    return false;
                }

                self.load_preview_image(ctx);
                true
            }
            Key::COLOR => self.color.update(value.as_color()),
            Key::NAME => self.name.update(value.as_str().to_owned()),
            Key::TOKEN_RADIUS => self.token_radius.update(value.as_f32().unwrap_or(0.25)),
            Key::SPEED => self.speed.update(value.as_f32().unwrap_or(5.0)),
            _ => false,
        }
    }

    fn load_preview_image(&mut self, ctx: &Context<Self>) {
        self.preview_images.clear();
        self.preview_images
            .load_id(&self.image, ctx.link().callback(Msg::ImageLoaded));
    }

    fn redraw_preview(&self) -> Result<(), Error> {
        let Some(canvas) = self.canvas.as_ref() else {
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

        let render = render::RenderToken {
            transform: &api::Transform::origin(),
            look_at: None,
            image: *self.image,
            color: self.color.unwrap_or_else(Color::neutral),
            token_radius: 1.0,
            arrow_target: None,
        };

        let view = ViewTransform::simple(canvas.width(), canvas.height(), 50.0);

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        render::draw_token(&cx, &view, &base, &render, &self.preview_images)?;
        Ok(())
    }
}
