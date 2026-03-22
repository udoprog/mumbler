use api::{Color, Id, Key, LocalUpdateBody, PeerId, PublicKey, RemoteId, UpdateBody, Value};
use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, HtmlInputElement, Url};
use yew::prelude::*;

use crate::components::Icon;
use crate::components::render::{ViewTransform, Visibility};
use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;
use crate::state::State;

use super::{CropModal, ImageGalleryModal, into_target, render};

pub(crate) enum Msg {
    CloseGallery,
    ColorChanged(Event),
    CropCancelled,
    CropConfirmed(api::CropRegion),
    DeleteImage(Id),
    DeleteImageResult(Result<Packet<api::DeleteImage>, ws::Error>),
    FixedRatioChanged(Event),
    Initialize(Result<Packet<api::GetObjectSettings>, ws::Error>),
    HeightChanged(Event),
    ImageData(String, Result<Vec<u8>, gloo::file::FileReadError>),
    ImageLoaded(ImageMessage),
    ImageSelected(Event),
    ImageUploaded(Result<Packet<api::UploadImage>, ws::Error>),
    LocalUpdate(Result<Packet<api::LocalUpdate>, ws::Error>),
    Update(Result<Packet<api::Update>, ws::Error>),
    NameChanged(Event),
    OpenGallery,
    Rescale(Option<f64>),
    SelectColor(api::Color),
    SelectImage(Id),
    SetLog(log::Log),
    StateChanged(ws::State),
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
    _delete_image: ws::Request,
    _file_reader: Option<FileReader>,
    _list_settings: ws::Request,
    _local_update_listener: ws::Listener,
    _update_listener: ws::Listener,
    _log_handle: ContextHandle<log::Log>,
    _select_color: ws::Request,
    _select_image: ws::Request,
    _state_change: ws::StateListener,
    _update_dimensions: ws::Request,
    _update_fixed_ratio: ws::Request,
    _update_name: ws::Request,
    color: State<Option<api::Color>>,
    crop_source_data: Option<(String, Vec<u8>)>,
    crop_source_url: Option<String>,
    gallery_open: bool,
    height: State<f32>,
    image_uploading: bool,
    image: State<Id>,
    images: Vec<api::Image>,
    public_key: PublicKey,
    log: log::Log,
    name: State<Option<String>>,
    preview_canvas: NodeRef,
    preview_images: Images<Self>,
    ratio: State<Option<f32>>,
    state: ws::State,
    upload_image: ws::Request,
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

        let _local_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::LocalUpdate>(ctx.link().callback(Msg::LocalUpdate));

        let _update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::Update>(ctx.link().callback(Msg::Update));

        let mut this = Self {
            _delete_image: ws::Request::new(),
            _file_reader: None,
            _list_settings: ws::Request::new(),
            _local_update_listener,
            _update_listener,
            _log_handle,
            _select_color: ws::Request::new(),
            _select_image: ws::Request::new(),
            _state_change,
            _update_dimensions: ws::Request::new(),
            _update_fixed_ratio: ws::Request::new(),
            _update_name: ws::Request::new(),
            color: State::new(None),
            crop_source_data: None,
            crop_source_url: None,
            gallery_open: false,
            height: State::new(1.0),
            image_uploading: false,
            image: State::new(Id::ZERO),
            images: Vec::new(),
            public_key: PublicKey::ZERO,
            log,
            name: State::new(None),
            preview_canvas: NodeRef::default(),
            preview_images: Images::new(),
            ratio: State::new(None),
            state,
            upload_image: ws::Request::new(),
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

                    <section class="btn-group">
                        <label for="static-file" class={classes!("btn", "primary", self.image_uploading.then_some("disabled"))}>
                            {"Upload"}
                            <Icon name="arrow-up-on-square" />
                        </label>

                        <button class="btn primary" onclick={ctx.link().callback(|_| Msg::OpenGallery)}>
                            {"Gallery"}
                            <Icon name="photo" />
                        </button>

                        <input
                            id="static-file"
                            class="hidden"
                            title="Upload image"
                            type="file"
                            accept="image/*"
                            onchange={ctx.link().callback(Msg::ImageSelected)}
                            />
                    </section>
                </div>

                <div class="col-4 rows">
                    <section class="token-preview">
                        <canvas ref={self.preview_canvas.clone()} width="200" height="200" />
                    </section>
                </div>
            </div>

            if self.gallery_open {
                <ImageGalleryModal
                    images={self.images.clone()}
                    selected={*self.image}
                    onselect={ctx.link().callback(Msg::SelectImage)}
                    ondelete={ctx.link().callback(Msg::DeleteImage)}
                    onclose={ctx.link().callback(|_| Msg::CloseGallery)}
                />
            }

            if let Some(src) = &self.crop_source_url {
                <CropModal
                    source_url={src.clone()}
                    ratio={(*self.width / *self.height) as f64}
                    on_confirm={ctx.link().callback(Msg::CropConfirmed)}
                    on_cancel={ctx.link().callback(|_| Msg::CropCancelled)}
                    rescale={ctx.link().callback(Msg::Rescale)}
                />
            }
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
            Msg::ImageSelected(e) => {
                if let Some(url) = self.crop_source_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

                self.crop_source_data = None;

                let input = into_target!(e, HtmlInputElement);

                let files = input.files().ok_or("no file list")?;
                let file = files.get(0).ok_or("no file selected")?;

                if let Ok(url) = Url::create_object_url_with_blob(&file) {
                    self.crop_source_url = Some(url);
                }

                let content_type = file.type_();
                let gloo_file = gloo::file::File::from(file);
                let link = ctx.link().clone();
                self._file_reader = Some(read_as_bytes(&gloo_file, move |res| {
                    link.send_message(Msg::ImageData(content_type.clone(), res));
                }));

                Ok(true)
            }
            Msg::ImageData(content_type, result) => {
                self._file_reader = None;
                let data = result.map_err(|e| anyhow::anyhow!("file read error: {e}"))?;
                self.crop_source_data = Some((content_type, data));
                Ok(false)
            }
            Msg::CropConfirmed(crop) => {
                let Some((content_type, data)) = self.crop_source_data.take() else {
                    return Err("image data not ready".into());
                };

                self.upload_image = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UploadImageRequest {
                        content_type,
                        data,
                        crop,
                        sizing: api::ImageSizing::Crop,
                        size: 512,
                    })
                    .on_packet(ctx.link().callback(Msg::ImageUploaded))
                    .send();

                self.image_uploading = true;
                Ok(true)
            }
            Msg::CropCancelled => {
                if let Some(url) = self.crop_source_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

                self.crop_source_data = None;
                self._file_reader = None;
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
            Msg::ImageUploaded(body) => {
                let body = body?;
                let body = body.decode()?;

                self.image_uploading = false;

                if let Some(url) = self.crop_source_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

                ctx.link().send_message(Msg::SelectImage(body.id));
                self.refresh(ctx);
                Ok(false)
            }
            Msg::SelectImage(id) => {
                *self.image = id;
                self.load_preview_image(ctx);
                self._select_image = object_update(ctx, Key::IMAGE_ID, id);
                Ok(true)
            }
            Msg::DeleteImage(id) => {
                self._delete_image = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::DeleteImageRequest { id })
                    .on_packet(ctx.link().callback(Msg::DeleteImageResult))
                    .send();
                Ok(false)
            }
            Msg::DeleteImageResult(result) => {
                result?;
                self.refresh(ctx);
                Ok(false)
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
            Msg::LocalUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    LocalUpdateBody::ObjectUpdated { id, key, value } => {
                        if ctx.props().id != RemoteId::local(id) {
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
            Msg::OpenGallery => {
                self.gallery_open = true;
                Ok(true)
            }
            Msg::CloseGallery => {
                self.gallery_open = false;
                Ok(true)
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
            let id = RemoteId::new(PeerId::ZERO, *self.image);
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
            image: RemoteId::new(PeerId::ZERO, *self.image),
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
