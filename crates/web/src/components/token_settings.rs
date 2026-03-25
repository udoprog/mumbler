use api::{Color, Id, Image, Key, PublicKey, RemoteId, RemoteUpdateBody, Value};
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

use super::{ImageUpload, into_target, render};

pub(crate) enum Msg {
    Channel(Result<ws::Channel, ws::Error>),
    ColorChanged(Event),
    ImageSelected(Id),
    ImagesRefresh,
    Initialize(Result<Packet<api::GetObjectSettings>, ws::Error>),
    NameChanged(Event),
    RadiusChanged(Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    SelectColor(api::Color),
    SetLog(log::Log),
    SpeedChanged(Event),
    StateChanged(ws::State),
    UpdateResult(Result<Packet<api::ObjectUpdate>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) id: RemoteId,
}

pub(crate) struct TokenSettings {
    _list_settings: ws::Request,
    _remote_update_listener: ws::Listener,
    _log_handle: ContextHandle<log::Log>,
    _select_color: ws::Request,
    _select_image: ws::Request,
    _state_change: ws::StateListener,
    _update_name: ws::Request,
    _update_radius: ws::Request,
    color: State<Option<api::Color>>,
    image: State<Id>,
    images: Vec<Image>,
    public_key: PublicKey,
    log: log::Log,
    name: State<String>,
    preview_canvas: NodeRef,
    preview_images: Images,
    speed: State<f32>,
    state: ws::State,
    token_radius: State<f32>,
    _channel: ws::Request,
    channel: ws::Channel,
}

impl Component for TokenSettings {
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
            _remote_update_listener,
            _log_handle,
            _select_color: ws::Request::new(),
            _select_image: ws::Request::new(),
            _state_change,
            _update_name: ws::Request::new(),
            _update_radius: ws::Request::new(),
            color: State::default(),
            image: State::new(Id::ZERO),
            images: Vec::new(),
            public_key: PublicKey::ZERO,
            log,
            name: State::default(),
            preview_canvas: NodeRef::default(),
            preview_images: Images::new(),
            speed: State::new(5.0),
            state,
            _channel: ws::Request::new(),
            channel: ws::Channel::default(),
            token_radius: State::new(0.25),
        };

        this.refresh(ctx);
        this
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
                        ws={ctx.props().ws.clone()}
                        images={self.images.clone()}
                        selected={*self.image}
                        sizing={api::ImageSizing::Square}
                        size={128}
                        ratio={1.0}
                        role={api::Role::TOKEN}
                        input_id="image"
                        onselect={ctx.link().callback(Msg::ImageSelected)}
                        onrefresh={ctx.link().callback(|_| Msg::ImagesRefresh)}
                    />
                </div>

                <div class="col-4 rows">
                    <section class="preview">
                        <canvas ref={self.preview_canvas.clone()} width="200" height="200" />
                    </section>
                </div>
            </div>
            </>
        }
    }
}

impl TokenSettings {
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
                    self.update_property(key, value);
                }

                self.images = body.images;
                self.public_key = body.public_key;
                Ok(true)
            }
            Msg::ImagesRefresh => {
                self.refresh(ctx);
                Ok(false)
            }
            Msg::ImageSelected(id) => {
                *self.image = id;
                self.load_preview_image();
                self._select_image = object_update(&self.channel, ctx, Key::IMAGE_ID, id);
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
                self._update_name = object_update(&self.channel, ctx, Key::OBJECT_NAME, name);
                Ok(false)
            }
            Msg::RadiusChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = 'done: {
                    let Ok(radius) = input.value().parse::<f32>() else {
                        break 'done false;
                    };

                    let radius = radius.clamp(0.05, 10.0);
                    *self.token_radius = radius;
                    self._update_radius =
                        object_update(&self.channel, ctx, Key::TOKEN_RADIUS, radius);
                    true
                };

                Ok(value)
            }
            Msg::SpeedChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = 'done: {
                    let Ok(speed) = input.value().parse::<f32>() else {
                        break 'done false;
                    };

                    let speed = speed.clamp(0.5, 100.0);
                    *self.speed = speed;
                    self._update_radius = object_update(&self.channel, ctx, Key::SPEED, speed);
                    true
                };

                Ok(value)
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
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    RemoteUpdateBody::ObjectUpdated { id, key, value, .. } => {
                        if ctx.props().id != id {
                            return Ok(false);
                        }

                        self.update_property(key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
            }
        }
    }

    fn update_property(&mut self, key: Key, value: Value) -> bool {
        match key {
            Key::IMAGE_ID => {
                if self.image.update(value.as_id()) {
                    self.load_preview_image();
                    true
                } else {
                    false
                }
            }
            Key::COLOR => self.color.update(value.as_color()),
            Key::OBJECT_NAME => self.name.update(value.as_str().to_owned()),
            Key::TOKEN_RADIUS => self.token_radius.update(value.as_f32().unwrap_or(0.25)),
            Key::SPEED => self.speed.update(value.as_f32().unwrap_or(5.0)),
            _ => false,
        }
    }

    fn load_preview_image(&mut self) {
        self.preview_images.clear();
        let id = RemoteId::local(*self.image);
        self.preview_images.load_id(&id);
    }

    fn redraw_preview(&self) -> Result<(), Error> {
        let Some(canvas) = self.preview_canvas.cast::<HtmlCanvasElement>() else {
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
            image: RemoteId::local(*self.image),
            color: self.color.unwrap_or_else(Color::neutral),
            token_radius: 1.0,
            arrow_target: None,
        };

        let view = ViewTransform::preview(&canvas);

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        render::draw_token(&cx, &view, &base, &render, &self.preview_images)?;
        Ok(())
    }
}

fn object_update(
    channel: &ws::Channel,
    ctx: &Context<TokenSettings>,
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
