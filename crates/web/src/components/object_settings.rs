use api::{Color, Id, Key, Value};
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

use super::CropModal;
use super::ImageGalleryModal;
use super::{into_target, render};

pub(crate) enum Msg {
    AvatarImageData(String, Result<Vec<u8>, gloo::file::FileReadError>),
    AvatarImageSelected(Event),
    CloseGallery,
    ColorChanged(Event),
    CropCancelled,
    CropConfirmed(api::CropRegion),
    DeleteImage(api::Id),
    DeleteImageResult(Result<Packet<api::DeleteImage>, ws::Error>),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    ImageLoaded(ImageMessage),
    ImageUploaded(Result<Packet<api::UploadImage>, ws::Error>),
    LocalUpdate(Result<Packet<api::LocalUpdate>, ws::Error>),
    NameChanged(Event),
    OpenGallery,
    RadiusChanged(Event),
    SelectColor(api::Color),
    SelectImage(api::Id),
    SetLog(log::Log),
    SpeedChanged(Event),
    StateChanged(ws::State),
    UpdateName(Option<String>),
    UpdateResult(Result<Packet<api::Update>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) id: Id,
}

pub(crate) struct TokenSettings {
    _delete_image: ws::Request,
    _file_reader: Option<FileReader>,
    _list_settings: ws::Request,
    _local_update_listener: ws::Listener,
    _log_handle: ContextHandle<log::Log>,
    _select_color: ws::Request,
    _select_image: ws::Request,
    _state_change: ws::StateListener,
    _update_name: ws::Request,
    _update_radius: ws::Request,
    color: State<Option<api::Color>>,
    crop_source_data: Option<(String, Vec<u8>)>,
    crop_source_url: Option<String>,
    gallery_open: bool,
    image_uploading: bool,
    image: State<Option<api::Id>>,
    images: Vec<api::Image>,
    log: log::Log,
    name: State<Option<String>>,
    preview_canvas: NodeRef,
    preview_images: Images<Self>,
    speed: State<f32>,
    state: ws::State,
    token_radius: State<f32>,
    upload_image: ws::Request,
}

impl From<ImageMessage> for Msg {
    #[inline]
    fn from(message: ImageMessage) -> Self {
        Msg::ImageLoaded(message)
    }
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

        let _local_update_listener = ctx
            .props()
            .ws
            .on_broadcast::<api::LocalUpdate>(ctx.link().callback(Msg::LocalUpdate));

        let mut this = Self {
            _delete_image: ws::Request::new(),
            _file_reader: None,
            _list_settings: ws::Request::new(),
            _local_update_listener,
            _log_handle,
            _select_color: ws::Request::new(),
            _select_image: ws::Request::new(),
            _state_change,
            _update_name: ws::Request::new(),
            _update_radius: ws::Request::new(),
            color: State::new(None),
            crop_source_data: None,
            crop_source_url: None,
            gallery_open: false,
            image_uploading: false,
            image: State::new(None),
            images: Vec::new(),
            log,
            name: State::new(None),
            preview_canvas: NodeRef::default(),
            preview_images: Images::new(),
            speed: State::new(5.0),
            state,
            token_radius: State::new(0.25),
            upload_image: ws::Request::new(),
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
                        <label for="token-name">{"Name:"}</label>

                        <input
                            id="token-name"
                            type="text"
                            placeholder="Enter name"
                            value={(*self.name).clone().unwrap_or_default()}
                            onchange={ctx.link().callback(Msg::NameChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="token-color">
                            {"Color:"}
                            <span class="color-preview" style={format!("--color: {}", color.to_css_string())} />
                        </label>

                        <input
                            id="token-color"
                            class="hidden"
                            type="color"
                            value={color.to_css_string()}
                            onchange={ctx.link().callback(Msg::ColorChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="token-radius">{"Radius:"}</label>

                        <input
                            id="token-radius"
                            type="number"
                            min="0.05"
                            max="10"
                            step="0.05"
                            value={format!("{}", *self.token_radius)}
                            onchange={ctx.link().callback(Msg::RadiusChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="token-speed">{"Speed:"}</label>

                        <input
                            id="token-speed"
                            type="number"
                            min="0.5"
                            max="100"
                            step="0.5"
                            value={format!("{}", *self.speed)}
                            onchange={ctx.link().callback(Msg::SpeedChanged)}
                            />
                    </section>

                    <section class="input-group">
                        <label for="token-file" class={classes!("btn", "primary", self.image_uploading.then_some("disabled"))}>
                            {"Upload"}
                            <Icon name="arrow-up-on-square" />
                        </label>

                        <button class="btn primary" onclick={ctx.link().callback(|_| Msg::OpenGallery)}>
                            {"Gallery"}
                            <Icon name="photo" />
                        </button>

                        <input
                            id="token-file"
                            class="hidden"
                            title="Upload image"
                            type="file"
                            accept="image/*"
                            onchange={ctx.link().callback(Msg::AvatarImageSelected)}
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
                    on_select={ctx.link().callback(Msg::SelectImage)}
                    on_delete={ctx.link().callback(Msg::DeleteImage)}
                    on_close={ctx.link().callback(|_| Msg::CloseGallery)}
                />
            }

            if let Some(src) = &self.crop_source_url {
                <CropModal
                    source_url={src.clone()}
                    ratio={1.0}
                    on_confirm={ctx.link().callback(Msg::CropConfirmed)}
                    on_cancel={ctx.link().callback(|_| Msg::CropCancelled)}
                />
            }
            </>
        }
    }
}

impl TokenSettings {
    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._list_settings = ctx
                .props()
                .ws
                .request()
                .body(api::GetObjectSettingsRequest { id: ctx.props().id })
                .on_packet(ctx.link().callback(Msg::GetObjectSettings))
                .send();
        }
    }

    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                self.refresh(ctx);
                Ok(true)
            }
            Msg::AvatarImageSelected(e) => {
                let input = into_target!(e, HtmlInputElement);

                if let Some(url) = self.crop_source_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

                self.crop_source_data = None;

                let files = input.files().ok_or("no file list")?;
                let file = files.get(0).ok_or("no file selected")?;

                if let Ok(url) = Url::create_object_url_with_blob(&file) {
                    self.crop_source_url = Some(url);
                }

                let content_type = file.type_();
                let gloo_file = gloo::file::File::from(file);
                let link = ctx.link().clone();
                self._file_reader = Some(read_as_bytes(&gloo_file, move |res| {
                    link.send_message(Msg::AvatarImageData(content_type.clone(), res));
                }));

                Ok(true)
            }
            Msg::AvatarImageData(content_type, result) => {
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
                        sizing: api::ImageSizing::Square,
                        size: 128,
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
            Msg::GetObjectSettings(result) => {
                let result = result?;
                let response = result.decode()?;

                for (key, value) in response.object.props {
                    self.update_property(ctx, key, value);
                }

                self.images = response.images;
                Ok(true)
            }
            Msg::SelectImage(id) => {
                *self.image = Some(id);
                self.load_preview_image(ctx);
                self._select_image = send_update(ctx, Key::IMAGE_ID, id);
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
                self._select_color = send_update(ctx, Key::COLOR, color);
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
                self._update_name = send_update(ctx, Key::NAME, name);
                Ok(true)
            }
            Msg::RadiusChanged(e) => {
                let input = into_target!(e, HtmlInputElement);

                let value = 'done: {
                    let Ok(radius) = input.value().parse::<f32>() else {
                        break 'done false;
                    };

                    let radius = radius.clamp(0.05, 10.0);
                    *self.token_radius = radius;
                    self._update_radius = send_update(ctx, Key::TOKEN_RADIUS, radius);
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
                    self._update_radius = send_update(ctx, Key::SPEED, speed);
                    true
                };

                Ok(value)
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
                    api::LocalUpdateBody::Update {
                        object_id,
                        key,
                        value,
                    } => {
                        if object_id != ctx.props().id {
                            return Ok(false);
                        }

                        self.update_property(ctx, key, value)
                    }
                    _ => return Ok(false),
                };

                Ok(changed)
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
            Key::NAME => self.name.update(value.as_str().map(str::to_owned)),
            Key::TOKEN_RADIUS => self.token_radius.update(value.as_f32().unwrap_or(0.25)),
            Key::SPEED => self.speed.update(value.as_f32().unwrap_or(5.0)),
            _ => false,
        }
    }

    fn load_preview_image(&mut self, ctx: &Context<Self>) {
        self.preview_images.clear();

        if let Some(id) = *self.image {
            self.preview_images.load(ctx, id);
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
            player: true,
        };

        let render = render::RenderToken {
            transform: &api::Transform::origin(),
            look_at: None,
            image: *self.image,
            color: self.color.unwrap_or_else(Color::neutral),
            token_radius: 1.0,
            arrow_target: None,
        };

        let view = ViewTransform::preview(&canvas);

        cx.clear_rect(0.0, 0.0, canvas.width() as f64, canvas.height() as f64);

        render::draw_token(&cx, &view, &base, &render, |id| {
            self.preview_images.get(id).cloned()
        })?;

        Ok(())
    }
}

fn send_update(ctx: &Context<TokenSettings>, key: Key, value: impl Into<Value>) -> ws::Request {
    ctx.props()
        .ws
        .request()
        .body(api::UpdateRequest {
            object_id: ctx.props().id,
            key,
            value: value.into(),
        })
        .on_packet(ctx.link().callback(Msg::UpdateResult))
        .send()
}
