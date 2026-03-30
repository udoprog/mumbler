use std::collections::HashMap;

use api::{Color, Extent, ImageSizing, Key, RemoteId, RemoteUpdateBody, Role, Value};
use musli_web::api::ChannelId;
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement};
use yew::prelude::*;

use crate::components::render::{self, ViewTransform};
use crate::error::Error;
use crate::images::Images;
use crate::log;

use super::{ChannelExt, DynamicCanvas, ImageUpload, SetupChannel, Visibility, into_target};

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
    Error(Error),
    Channel(Result<ws::Channel, Error>),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    RemoteIdChanged(Key, RemoteId),
    ImageChanged(Key, RemoteId),
    StringChanged(Key, Event),
    FloatChanged(Key, Event),
    BooleanChanged(Key, Event),
    ExtentChanged(Key, ExtentAxis, Event),
    ColorChanged(Key, Event),
    FixedRatioChanged(Event),
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    ObjectUpdate(Result<Packet<api::ObjectUpdate>, ws::Error>),
    ImageLoaded(Result<(), Error>),
    Ratio(f64),
    CanvasLoaded(HtmlCanvasElement),
    CanvasResized((u32, u32)),
}

impl From<Result<Packet<api::ObjectUpdate>, ws::Error>> for Msg {
    #[inline]
    fn from(value: Result<Packet<api::ObjectUpdate>, ws::Error>) -> Self {
        Msg::ObjectUpdate(value)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum ObjectRender {
    Static,
    Token,
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) id: RemoteId,
    pub(crate) keys: &'static [Key],
    #[prop_or_default]
    pub(crate) height_ratio: bool,
    #[prop_or_default]
    pub(crate) render: Option<ObjectRender>,
    #[prop_or_else(Color::neutral)]
    pub(crate) default_color: Color,
}

pub(crate) struct ObjectSettings {
    _get_object_settings: ws::Request,
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
    images: Images,
    canvas: Option<HtmlCanvasElement>,
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
            _get_object_settings: ws::Request::new(),
            _remote_update_listener: ws::Listener::new(),
            _select_background: ws::Request::new(),
            _update_extent: ws::Request::new(),
            _update_name: ws::Request::new(),
            _update_show_grid: ws::Request::new(),
            properties: api::Properties::default(),
            requests: HashMap::new(),
            images: Images::new(ctx.link().callback(Msg::ImageLoaded)),
            canvas: None,
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

    fn rendered(&mut self, ctx: &Context<Self>, _first_render: bool) {
        if let Some(render) = ctx.props().render
            && let Err(error) = self.redraw(ctx, render)
        {
            self.log.error("object_settings::redraw", error);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let has_render = ctx.props().render.is_some();
        let left_class = classes!(if has_render { "col-8" } else { "col-12" }, "rows");

        let preview = has_render.then(|| {
            html! {
                <div class="col-4 rows">
                    <section class="preview">
                        <DynamicCanvas
                            onload={ctx.link().callback(Msg::CanvasLoaded)}
                            onresize={ctx.link().callback(Msg::CanvasResized)}
                            onerror={ctx.link().callback(Msg::Error)}
                            />
                    </section>
                </div>
            }
        });

        html! {
            <div id="content" class="row">
                <div class={left_class}>
                    for key in ctx.props().keys {
                        {self.key_view(ctx, *key)}
                    }
                </div>

                {preview}
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
            (api::ValueType::Float, key) => match key {
                Key::RATIO if ctx.props().height_ratio => {
                    let set_ratio = self.properties.get(Key::RATIO).as_f64();

                    let ratio = if let Some(ratio) = set_ratio {
                        round(ratio)
                    } else {
                        let width = self.properties.get(Key::WIDTH).as_f64().unwrap_or(1.0);
                        let height = self.properties.get(Key::HEIGHT).as_f64().unwrap_or(1.0);
                        round(width / height)
                    };

                    let current_ratio =
                        html! { <span class="fixed-ratio"> {format!("{:}:1", ratio)} </span> };

                    html! {
                        <section class="input-group">
                            <label for={key.id()}>{format!("{}:", key.label())}</label>

                            <input
                                id={key.id()}
                                type="checkbox"
                                checked={set_ratio.is_some()}
                                onchange={ctx.link().callback(Msg::FixedRatioChanged)}
                                />

                            {current_ratio}
                        </section>
                    }
                }
                _ => {
                    let value = round(self.properties.get(key).as_f64().unwrap_or(0.0));

                    html! {
                        <section class="input-group">
                            <label for={key.id()}>{format!("{}:", key.label())}</label>

                            <input
                                id={key.id()}
                                type="number"
                                min="0.1"
                                max="50"
                                step="0.1"
                                placeholder={key.placeholder()}
                                value={value.to_string()}
                                onchange={ctx.link().callback(move |ev: Event| Msg::FloatChanged(key, ev))}
                            />
                        </section>
                    }
                }
            },
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
                            sizing={ImageSizing::Crop}
                            size={1024}
                            role={Role::BACKGROUND}
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
            (api::ValueType::Id, Key::IMAGE_ID) => {
                let value = RemoteId::local(self.properties.get(key).as_id());

                match ctx.props().render {
                    Some(ObjectRender::Static) => {
                        let ratio = self.properties.get(Key::RATIO).as_f64();

                        html! {
                            <ImageUpload
                                selected={value}
                                sizing={ImageSizing::Crop}
                                size={512}
                                {ratio}
                                role={Role::STATIC}
                                input_id="image"
                                onselect={ctx.link().callback(move |id| Msg::ImageChanged(key, id))}
                                onclear={ctx.link().callback(move |_| Msg::ImageChanged(key, RemoteId::ZERO))}
                                onratio={ctx.link().callback(Msg::Ratio)}
                            />
                        }
                    }
                    Some(ObjectRender::Token) => {
                        html! {
                            <ImageUpload
                                selected={value}
                                sizing={ImageSizing::Square}
                                size={128}
                                ratio={1.0}
                                role={Role::TOKEN}
                                input_id="image"
                                onselect={ctx.link().callback(move |id| Msg::ImageChanged(key, id))}
                                onclear={ctx.link().callback(move |_| Msg::ImageChanged(key, RemoteId::ZERO))}
                            />
                        }
                    }
                    None => {
                        html! {}
                    }
                }
            }
            (api::ValueType::Color, _) => {
                let value = self
                    .properties
                    .get(key)
                    .as_color()
                    .unwrap_or_else(|| ctx.props().default_color);

                html! {
                    <section class="input-group">
                        <label for={key.id()}>
                            {format!("{}:", key.label())}
                            <span class="color-preview" style={format!("--color: {}", value.to_css_string())} />
                        </label>

                        <input
                            id={key.id()}
                            class="hidden"
                            type="color"
                            value={value.to_css_string()}
                            onchange={ctx.link().callback(move |ev| Msg::ColorChanged(key, ev))}
                            />
                    </section>
                }
            }
            _ => return None,
        };

        Some(html)
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Error(error) => Err(error),
            Msg::Channel(channel) => {
                self.channel = channel?;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(true);
                }

                self._remote_update_listener = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::RemoteUpdate));

                self._get_object_settings = self
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

                let mut update = false;

                for (key, value) in body.object.props {
                    update |= self.update_property(ctx, key, value);
                }

                Ok(update)
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
            Msg::ImageChanged(key, id) => {
                let old = self.properties.insert(key, Value::from(id.id));

                if old.as_id() == id.id {
                    return Ok(false);
                }

                self.images.remove(&RemoteId::local(old.as_id()));
                self.images
                    .load_id(&id, ctx.link().callback(Msg::ImageLoaded));

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

                Ok(true)
            }
            Msg::FloatChanged(key, ev) => {
                let input = into_target!(ev, HtmlInputElement);
                let value = input.value();

                let Ok(value) = value.parse::<f64>() else {
                    return Ok(false);
                };

                let value = round(value);

                match key {
                    Key::HEIGHT if ctx.props().height_ratio => {
                        let ratio = self.properties.get(Key::RATIO).as_f64();

                        if let Some(ratio) = ratio {
                            let width = round(value * ratio);

                            let request = self.channel.object_updates(
                                ctx,
                                ctx.props().id.id,
                                [(Key::WIDTH, width.into())],
                            );

                            self.properties.insert(Key::WIDTH, width.into());
                            self.requests.insert(Key::WIDTH, request);
                        }
                    }
                    Key::WIDTH if ctx.props().height_ratio => {
                        let ratio = self.properties.get(Key::RATIO).as_f64();

                        if let Some(ratio) = ratio {
                            let width = round(value / ratio);

                            let request = self.channel.object_updates(
                                ctx,
                                ctx.props().id.id,
                                [(Key::HEIGHT, width.into())],
                            );

                            self.properties.insert(Key::HEIGHT, width.into());
                            self.requests.insert(Key::HEIGHT, request);
                        }
                    }
                    _ => {}
                }

                let old = self.properties.insert(key, Value::from(value));

                if old.as_f64() == Some(value) {
                    return Ok(false);
                }

                self.requests.insert(
                    key,
                    self.channel
                        .object_updates(ctx, ctx.props().id.id, [(key, value.into())]),
                );

                Ok(true)
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

                Ok(true)
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
            Msg::ColorChanged(key, ev) => {
                let input = into_target!(ev, HtmlInputElement);
                let value = input.value();

                let Some(value) = Color::from_hex(&value) else {
                    return Ok(false);
                };

                let old = self.properties.insert(key, Value::from(value));

                if old.as_color() == Some(value) {
                    return Ok(false);
                }

                self.requests.insert(
                    key,
                    self.channel
                        .object_updates(ctx, ctx.props().id.id, [(key, value.into())]),
                );

                Ok(true)
            }
            Msg::FixedRatioChanged(ev) => {
                let input = into_target!(ev, HtmlInputElement);
                let fixed_ratio = input.checked();

                let width = self.properties.get(Key::WIDTH).as_f64().unwrap_or(0.0);

                let height = self.properties.get(Key::HEIGHT).as_f64().unwrap_or(0.0);

                let ratio = if fixed_ratio {
                    let ratio = width / height;
                    Some(round(ratio))
                } else {
                    None
                };

                let old = self.properties.insert(Key::RATIO, ratio.into());

                if ratio == old.as_f64() {
                    return Ok(false);
                };

                let request = self.channel.object_updates(
                    ctx,
                    ctx.props().id.id,
                    [(Key::RATIO, ratio.into())],
                );

                self.requests.insert(Key::RATIO, request);
                Ok(true)
            }
            Msg::Ratio(ratio) => {
                let ratio = round(ratio);

                let height = self.properties.get(Key::HEIGHT).as_f64().unwrap_or(0.0);

                let new_width = round(height * ratio);

                let mut update = false;

                update |= self
                    .properties
                    .insert(Key::RATIO, Value::from(Some(ratio)))
                    .as_f64()
                    != Some(ratio);
                update |= self
                    .properties
                    .insert(Key::WIDTH, Value::from(new_width))
                    .as_f64()
                    != Some(new_width);

                if update {
                    let request = self.channel.object_updates(
                        ctx,
                        ctx.props().id.id,
                        [
                            (Key::RATIO, Value::from(Some(ratio))),
                            (Key::WIDTH, Value::from(new_width)),
                        ],
                    );

                    self.requests.insert(Key::RATIO, request);
                }

                Ok(update)
            }
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                match body {
                    RemoteUpdateBody::ObjectUpdated { id, key, value, .. } => {
                        if id != ctx.props().id {
                            return Ok(false);
                        }

                        Ok(self.update_property(ctx, key, value))
                    }
                    _ => Ok(false),
                }
            }
            Msg::ObjectUpdate(result) => {
                let result = result?;
                _ = result.decode()?;
                Ok(false)
            }
            Msg::ImageLoaded(result) => {
                result?;

                if let Some(render) = ctx.props().render {
                    self.redraw(ctx, render)?;
                }

                Ok(false)
            }
            Msg::CanvasLoaded(canvas) => {
                if let Some(render) = ctx.props().render {
                    self.canvas = Some(canvas);
                    self.redraw(ctx, render)?;
                }

                Ok(false)
            }
            Msg::CanvasResized((_, _)) => {
                if let Some(render) = ctx.props().render {
                    self.redraw(ctx, render)?;
                }

                Ok(false)
            }
        }
    }

    fn update_property(&mut self, ctx: &Context<Self>, key: Key, value: Value) -> bool {
        match key {
            Key::IMAGE_ID => {
                let new = RemoteId::local(value.as_id());
                let old = self.properties.insert(key, value);
                let old = RemoteId::local(old.as_id());

                if new != old {
                    self.images
                        .load_id(&new, ctx.link().callback(Msg::ImageLoaded));
                    self.images.remove(&old);
                    true
                } else {
                    false
                }
            }
            _ => {
                self.properties.insert(key, value);
                true
            }
        }
    }

    fn redraw(&self, ctx: &Context<Self>, render: ObjectRender) -> Result<(), Error> {
        let Some(canvas) = &self.canvas else {
            return Ok(());
        };

        let Some(cx) = canvas.get_context("2d")? else {
            return Ok(());
        };

        let Ok(cx) = cx.dyn_into::<CanvasRenderingContext2d>() else {
            return Ok(());
        };

        let name = self.properties.get(Key::NAME).as_str();

        let base = render::RenderBase {
            name,
            visibility: Visibility::Remote,
            selected: false,
        };

        let image = RemoteId::local(self.properties.get(Key::IMAGE_ID).as_id());

        let color = self
            .properties
            .get(Key::COLOR)
            .as_color()
            .unwrap_or(ctx.props().default_color);

        let width = canvas.width();
        let height = canvas.height();

        cx.clear_rect(0.0, 0.0, width as f64, height as f64);

        match render {
            ObjectRender::Static => {
                let render_width = self.properties.get(Key::WIDTH).as_f64().unwrap_or(1.0) as f32;

                let render_height = self.properties.get(Key::HEIGHT).as_f64().unwrap_or(1.0) as f32;

                let render = render::RenderStatic {
                    transform: &api::Transform::origin(),
                    image,
                    color,
                    width: render_width,
                    height: render_height,
                };

                let min = width.min(height) as f32;

                let scale = (min - min * 0.2) / render_width.max(render_height);
                let view = ViewTransform::simple(width, height, scale);

                render::draw_static(&cx, &view, &base, &render, &self.images)?;
            }
            ObjectRender::Token => {
                let render = render::RenderToken {
                    transform: &api::Transform::origin(),
                    look_at: None,
                    image,
                    color,
                    token_radius: 1.0,
                    arrow_target: None,
                };

                let view = ViewTransform::simple(width, height, 50.0);
                render::draw_token(&cx, &view, &base, &render, &self.images)?;
            }
        }

        Ok(())
    }
}

fn round(value: f64) -> f64 {
    (value * 100.0).round() / 100.0
}
