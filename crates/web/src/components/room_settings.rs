use api::{Id, Image, Key, PeerId, RemoteUpdateBody, Value};
use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, Url};
use yew::prelude::*;

use crate::error::Error;
use crate::log;
use crate::state::State;

use super::{CropModal, Icon, ImageGalleryModal, into_target};

pub(crate) enum Msg {
    BackgroundFileRead(String, Result<Vec<u8>, gloo::file::FileReadError>),
    BackgroundFileSelected(Event),
    CloseGallery,
    CropCancelled,
    CropConfirmed(api::CropRegion),
    DeleteImage(Id),
    DeleteImageResult(Result<Packet<api::DeleteImage>, ws::Error>),
    GetObjectSettings(Result<Packet<api::GetObjectSettings>, ws::Error>),
    ImageUploaded(Result<Packet<api::UploadImage>, ws::Error>),
    NameChanged(Event),
    OpenGallery,
    RemoteUpdate(Result<Packet<api::RemoteUpdate>, ws::Error>),
    SelectBackground(Id),
    SetLog(log::Log),
    StateChanged(ws::State),
    UpdateName(Option<String>),
    UpdateResult(Result<Packet<api::ObjectUpdate>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) id: Id,
}

pub(crate) struct RoomSettings {
    _delete_image: ws::Request,
    _file_reader: Option<FileReader>,
    _list_settings: ws::Request,
    _log_handle: ContextHandle<log::Log>,
    _remote_update_listener: ws::Listener,
    _select_background: ws::Request,
    _state_change: ws::StateListener,
    _update_name: ws::Request,
    _upload_image: ws::Request,
    background: State<Id>,
    crop_source_data: Option<(String, Vec<u8>)>,
    crop_source_url: Option<String>,
    gallery_open: bool,
    image_uploading: bool,
    images: Vec<Image>,
    log: log::Log,
    name: State<Option<String>>,
    state: ws::State,
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
            _delete_image: ws::Request::new(),
            _file_reader: None,
            _list_settings: ws::Request::new(),
            _log_handle,
            _remote_update_listener,
            _select_background: ws::Request::new(),
            _state_change,
            _update_name: ws::Request::new(),
            _upload_image: ws::Request::new(),
            background: State::new(Id::ZERO),
            crop_source_data: None,
            crop_source_url: None,
            gallery_open: false,
            image_uploading: false,
            images: Vec::new(),
            log,
            name: State::new(None),
            state,
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

        html! {
            <>
            <div id="content" class="rows">
                <section class="input-group">
                    <label for="room-name">{"Name:"}</label>

                    <input
                        id="room-name"
                        type="text"
                        placeholder="Enter name"
                        value={(*self.name).clone().unwrap_or_default()}
                        onchange={ctx.link().callback(Msg::NameChanged)}
                    />
                </section>

                <section class="input-group">
                    <label for="room-background-file" class={classes!("btn", "primary", self.image_uploading.then_some("disabled"))}>
                        {"Upload"}
                        <Icon name="arrow-up-on-square" />
                    </label>

                    <button class="btn primary" onclick={ctx.link().callback(|_| Msg::OpenGallery)}>
                        {"Gallery"}
                        <Icon name="photo" />
                    </button>

                    <input
                        id="room-background-file"
                        class="hidden"
                        title="Upload background image"
                        type="file"
                        accept="image/*"
                        onchange={ctx.link().callback(Msg::BackgroundFileSelected)}
                    />
                </section>

                if let Some(src) = background_src {
                    <section class="background-preview">
                        <img src={src} />
                    </section>
                }
            </div>

            if self.gallery_open {
                <ImageGalleryModal
                    images={self.images.clone()}
                    selected={*self.background}
                    onselect={ctx.link().callback(Msg::SelectBackground)}
                    ondelete={ctx.link().callback(Msg::DeleteImage)}
                    onclose={ctx.link().callback(|_| Msg::CloseGallery)}
                />
            }

            if let Some(src) = &self.crop_source_url {
                <CropModal
                    source_url={src.clone()}
                    onconfirm={ctx.link().callback(Msg::CropConfirmed)}
                    oncancel={ctx.link().callback(|_| Msg::CropCancelled)}
                />
            }
            </>
        }
    }
}

impl RoomSettings {
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
            Msg::GetObjectSettings(result) => {
                let body = result?;
                let body = body.decode()?;

                for (key, value) in body.object.props {
                    self.update_property(key, value);
                }

                self.images = body.images;
                Ok(true)
            }
            Msg::BackgroundFileSelected(e) => {
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
                    link.send_message(Msg::BackgroundFileRead(content_type.clone(), res));
                }));

                Ok(true)
            }
            Msg::BackgroundFileRead(content_type, result) => {
                self._file_reader = None;
                let data = result.map_err(|e| anyhow::anyhow!("file read error: {e}"))?;
                self.crop_source_data = Some((content_type, data));
                Ok(false)
            }
            Msg::CropConfirmed(crop) => {
                let Some((content_type, data)) = self.crop_source_data.take() else {
                    return Err("image data not ready".into());
                };

                self._upload_image = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UploadImageRequest {
                        content_type,
                        data,
                        crop,
                        sizing: api::ImageSizing::Crop,
                        size: 1024,
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

                ctx.link().send_message(Msg::SelectBackground(body.id));
                self.refresh(ctx);
                Ok(false)
            }
            Msg::SelectBackground(id) => {
                *self.background = id;
                self._select_background = object_update(ctx, Key::ROOM_BACKGROUND, id);
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
            Msg::RemoteUpdate(body) => {
                let body = body?;
                let body = body.decode()?;

                let changed = match body {
                    RemoteUpdateBody::ObjectUpdated { id, key, value } => {
                        if id.id != ctx.props().id {
                            return Ok(false);
                        }

                        self.update_property(key, value)
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
            _ => false,
        }
    }
}

fn object_update(ctx: &Context<RoomSettings>, key: Key, value: impl Into<Value>) -> ws::Request {
    ctx.props()
        .ws
        .request()
        .body(api::ObjectUpdateBody {
            id: ctx.props().id,
            key,
            value: value.into(),
        })
        .on_packet(ctx.link().callback(Msg::UpdateResult))
        .send()
}
