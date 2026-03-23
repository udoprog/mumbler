use api::{Color, Id, Key, PublicKey, RemoteId, RemoteUpdateBody, UpdateBody, Value};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement};
use yew::prelude::*;

use crate::components::render::{ViewTransform, Visibility};
use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::state::State;

use super::{ImageUpload, into_target, render};

pub(crate) enum Msg {
    ColorChanged(Event),
    FixedRatioChanged(Event),
    HeightChanged(Event),
    ImageLoaded(ImageMessage),
    ImageSelected(Id),
    ImagesRefresh,
    Initialize(Result<Packet<api::GetObjectSettings>, ws::Error>),
    NameChanged(Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    Rescale(Option<f64>),
    SelectColor(api::Color),
    SetLog(log::Log),
    StateChanged(ws::State),
    Update(Result<Packet<api::Update>, ws::Error>),
    UpdateName(Option<String>),
    UpdateResult(Result<Packet<api::ObjectUpdate>, ws::Error>),
    WidthChanged(Event),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) id: RemoteId,
}

pub(crate) struct StaticSettings {
    _list_settings: ws::Request,
    _remote_update_listener: ws::Listener,
    _update_listener: ws::Listener,
    _log_handle: ContextHandle<log::Log>,
    _select_color: ws::Request,
    _select_image: ws::Request,
    _state_change: ws::StateListener,
    _update_dimensions: ws::Request,
    _update_fixed_ratio: ws::Request,
    _update_name: ws::Request,
    color: State<Option<api::Color>>,
    height: State<f32>,
    image: State<Id>,
    images: Vec<api::Image>,
    public_key: PublicKey,
    log: log::Log,
    name: State<Option<String>>,
    preview_canvas: NodeRef,
    preview_images: Images<Self>,
    ratio: State<Option<f32>>,
    state: ws::State,
    width: State<f32>,
}

impl From<ImageMessage> for Msg {
    #[inline]
    fn from(message: ImageMessage) -> Self {
        Msg::ImageLoaded(message)
    }
}

impl Component for StaticSettings {
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

        let _update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::Update>(ctx.link().callback(Msg::Update));

        let mut this = Self {
            _list_settings: ws::Request::new(),
            _remote_update_listener,
            _update_listener,
            _log_handle,
            _select_color: ws::Request::new(),
            _select_image: ws::Request::new(),
            _state_change,
            _update_dimensions: ws::Request::new(),
            _update_fixed_ratio: ws::Request::new(),
            _update_name: ws::Request::new(),
            color: State::new(None),
            height: State::new(1.0),
            image: State::new(Id::ZERO),
            images: Vec::new(),
            public_key: PublicKey::ZERO,
            log,
            name: State::new(None),
            preview_canvas: NodeRef::default(),
            preview_images: Images::new(),
            ratio: State::new(None),
            state,
            width: State::new(1.0),
        };

        this.refresh(ctx);
        this
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
                        <label for="static-name">{"Name:"}</label>

                        <input
                            id="static-name"
                            type="text"
                            placeholder="Enter name"
                            value={(*self.name).clone().unwrap_or_default()}
                            onchange={ctx.link().callback(Msg::NameChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="static-color">
                            {"Color:"}
                            <span class="color-preview" style={format!("--color: {}", color.to_css_string())} />
                        </label>

                        <input
                            id="static-color"
                            class="hidden"
                            type="color"
                            value={color.to_css_string()}
                            onchange={ctx.link().callback(Msg::ColorChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="static-width">{"Width:"}</label>

                        <input
                            id="static-width"
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
                            <label for="static-height">{"Height:"}</label>

                            <input
                                id="static-height"
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
                        <label for="static-fixed-ratio">{"Fixed Ratio:"}</label>

                        <input
                            id="static-fixed-ratio"
                            type="checkbox"
                            checked={self.ratio.is_some()}
                            onchange={ctx.link().callback(Msg::FixedRatioChanged)}
                            />

                        {current_ratio}
                    </section>

                    <ImageUpload
                        ws={ctx.props().ws.clone()}
                        images={self.images.clone()}
                        selected={*self.image}
                        sizing={api::ImageSizing::Crop}
                        size={512}
                        crop_ratio={Some((*self.width / *self.height) as f64)}
                        input_id="static-file"
                        onselect={ctx.link().callback(Msg::ImageSelected)}
                        onrefresh={ctx.link().callback(|_| Msg::ImagesRefresh)}
                        onrescale={ctx.link().callback(Msg::Rescale)}
                    />
                </div>

                <div class="col-4 rows">
                    <section class="token-preview">
                        <canvas ref={self.preview_canvas.clone()} width="200" height="200" />
                    </section>
                </div>
            </div>
            </>
        }
    }
}

impl StaticSettings {
    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._list_settings = ctx
                .props()
                .ws
                .request()
                .body(api::GetObjectSettingsRequest {
                    id: ctx.props().id.id,
                })
                .on_packet(ctx.link().callback(Msg::Initialize))
                .send();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Initialize(result) => {
                let body = result?;
                let body = body.decode()?;

                for (key, value) in body.object.props {
                    self.update_property(ctx, key, value);
                }

                self.images = body.images;
                self.public_key = body.public_key;
                Ok(true)
            }
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::ImagesRefresh => {
                self.refresh(ctx);
                Ok(false)
            }
            Msg::ImageSelected(id) => {
                *self.image = id;
                self.load_preview_image(ctx);
                self._select_image = object_update(ctx, Key::IMAGE_ID, id);
                Ok(true)
            }
            Msg::Rescale(ratio) => {
                self._update_fixed_ratio = object_update(ctx, Key::RATIO, ratio);

                let Some(ratio) = ratio else {
                    return Ok(false);
                };

                *self.width = *self.height * ratio as f32;
                self._update_dimensions = object_update(ctx, Key::STATIC_WIDTH, *self.width);

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
                self._select_color = object_update(ctx, Key::COLOR, color);
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
                self._update_name = object_update(ctx, Key::OBJECT_NAME, name);
                Ok(true)
            }
            Msg::WidthChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let changed = 'done: {
                    let Ok(width) = input.value().parse::<f32>() else {
                        break 'done false;
                    };

                    let width = width.clamp(0.05, 50.0);
                    *self.width = width;
                    self._update_dimensions = object_update(ctx, Key::STATIC_WIDTH, width);

                    if let Some(ratio) = *self.ratio {
                        *self.height = (*self.width / ratio).clamp(0.05, 50.0);
                        self._update_dimensions =
                            object_update(ctx, Key::STATIC_HEIGHT, *self.height);
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
                    self._update_dimensions = object_update(ctx, Key::STATIC_HEIGHT, height);

                    if let Some(ratio) = *self.ratio {
                        *self.width = (*self.height * ratio).clamp(0.05, 50.0);
                        self._update_dimensions =
                            object_update(ctx, Key::STATIC_WIDTH, *self.width);
                    }

                    true
                };

                Ok(changed)
            }
            Msg::FixedRatioChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let fixed_ratio = input.checked();

                if fixed_ratio {
                    let ratio = *self.width / *self.height;
                    *self.ratio = Some((ratio * 100.0).round() / 100.0);
                } else {
                    *self.ratio = None;
                };

                self._update_fixed_ratio = object_update(ctx, Key::RATIO, *self.ratio);
                Ok(true)
            }
            Msg::ImageLoaded(msg) => {
                self.preview_images.update(msg);
                Ok(true)
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
                    RemoteUpdateBody::ObjectUpdated { id, key, value } => {
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
        }
    }

    fn update_property(&mut self, ctx: &Context<Self>, key: Key, value: Value) -> bool {
        match key {
            Key::IMAGE_ID => {
                if self.image.update(value.as_id()) {
                    self.load_preview_image(ctx);
                    true
                } else {
                    false
                }
            }
            Key::COLOR => self.color.update(value.as_color()),
            Key::OBJECT_NAME => self.name.update(value.as_str().map(str::to_owned)),
            Key::STATIC_WIDTH => self.width.update(value.as_f32().unwrap_or(1.0)),
            Key::STATIC_HEIGHT => self.height.update(value.as_f32().unwrap_or(1.0)),
            Key::RATIO => self.ratio.update(value.as_f32()),
            _ => false,
        }
    }

    fn load_preview_image(&mut self, ctx: &Context<Self>) {
        self.preview_images.clear();

        if !self.image.is_zero() {
            let id = RemoteId::local(*self.image);
            self.preview_images.load(ctx, &id);
        }
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
            name: self.name.as_deref(),
            visibility: Visibility::Remote,
            selected: false,
            player: false,
        };

        let render = render::RenderStatic {
            transform: &api::Transform::origin(),
            image: RemoteId::local(*self.image),
            color: self.color.unwrap_or_else(Color::neutral),
            width: (*self.width).min(*self.height * 3.0),
            height: (*self.height).min(*self.width * 3.0),
        };

        let view = ViewTransform::preview(&canvas);

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        render::draw_static(&cx, &view, &base, &render, |id| {
            self.preview_images.get(id).cloned()
        })?;
        Ok(())
    }
}

fn object_update(ctx: &Context<StaticSettings>, key: Key, value: impl Into<Value>) -> ws::Request {
    ctx.props()
        .ws
        .request()
        .body(api::ObjectUpdateBody {
            id: ctx.props().id.id,
            key,
            value: value.into(),
        })
        .on_packet(ctx.link().callback(Msg::UpdateResult))
        .send()
}
