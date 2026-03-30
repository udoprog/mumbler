use anyhow::Context as _;
use api::{RemoteId, Role};
use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;
use crate::log;

use super::{Crop, Extent, Icon, ImageGallery, Modal, SetupChannel, TemporaryUrl, into_target};

pub(crate) enum Msg {
    Error(Error),
    Channel(Result<ws::Channel, Error>),
    FileSelected(Event),
    FileRead(String, Result<Vec<u8>, gloo::file::FileReadError>),
    CropDrag(Option<Extent>),
    CropConfirmed(api::CropRegion),
    CropCancelled,
    Uploaded(Result<Packet<api::UploadImage>, ws::Error>),
    OpenGallery,
    CloseGallery,
    SelectImage(RemoteId),
    RemoveImage(RemoteId),
    RemoveResult(RemoteId, Result<Packet<api::RemoveImage>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) selected: RemoteId,
    pub(crate) sizing: api::ImageSizing,
    pub(crate) size: u32,
    #[prop_or_default]
    pub(crate) ratio: Option<f64>,
    pub(crate) input_id: AttrValue,
    pub(crate) role: Role,
    pub(crate) onselect: Callback<RemoteId>,
    #[prop_or_default]
    pub(crate) onratio: Option<Callback<f64>>,
    #[prop_or_default]
    pub(crate) onclear: Callback<()>,
}

pub(crate) struct ImageUpload {
    log: log::Log,
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    _remove_image: ws::Request,
    _file_reader: Option<FileReader>,
    _upload_image: ws::Request,
    crop_source_data: Option<(String, Vec<u8>)>,
    crop_source_url: Option<TemporaryUrl>,
    drag: Option<Extent>,
    gallery_open: bool,
    uploading: bool,
}

impl Component for ImageUpload {
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
            _remove_image: ws::Request::new(),
            _file_reader: None,
            _upload_image: ws::Request::new(),
            crop_source_data: None,
            crop_source_url: None,
            drag: None,
            gallery_open: false,
            uploading: false,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("image_upload::update", error);
                true
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let input_id = ctx.props().input_id.clone();

        let can_clear = !ctx.props().selected.is_zero();
        let onclear = can_clear.then(|| ctx.props().onclear.reform(|_| ()));

        html! {
            <>
            <section class="btn-group">
                <label for={input_id.clone()} class={classes!("btn", "primary", self.uploading.then_some("disabled"))}>
                    if self.uploading {
                        {"Uploading"}
                        <span class="loader" />
                    } else {
                        {"Upload"}
                        <Icon name="arrow-up-on-square" />
                    }
                </label>

                <button class="btn primary" onclick={ctx.link().callback(|_| Msg::OpenGallery)}>
                    {"Gallery"}
                    <Icon name="photo" />
                </button>

                <button class={classes!("btn", "danger", (!can_clear).then_some("disabled"))} onclick={onclear}>
                    {"Clear"}
                    <Icon name="x-circle" />
                </button>

                <input
                    id={input_id}
                    class="hidden"
                    type="file"
                    accept="image/*"
                    onchange={ctx.link().callback(Msg::FileSelected)}
                />
            </section>

            if self.gallery_open {
                <Modal title="Images" class="rows" onclose={ctx.link().callback(|_| Msg::CloseGallery)}>
                    <ImageGallery
                        selected={ctx.props().selected}
                        default_role={ctx.props().role}
                        onselect={ctx.link().callback(Msg::SelectImage)}
                        ondelete={ctx.link().callback(Msg::RemoveImage)}
                    />
                </Modal>
            }

            if let Some(source_url) = &self.crop_source_url {
                <Modal title="Crop Image" class="rows" onclose={ctx.link().callback(|_| Msg::CropCancelled)}>
                    <Crop
                        drag={self.drag}
                        ondrag={ctx.link().callback(Msg::CropDrag)}
                        source_url={(*source_url).to_string()}
                        ratio={ctx.props().ratio}
                        onconfirm={ctx.link().callback(Msg::CropConfirmed)}
                        onratio={ctx.props().onratio.clone()}
                    />
                </Modal>
            }
            </>
        }
    }
}

impl ImageUpload {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Error(error) => Err(error),
            Msg::Channel(channel) => {
                self.channel = channel?;
                Ok(false)
            }
            Msg::FileSelected(e) => {
                let input = into_target!(e, HtmlInputElement);

                self.crop_source_url = None;
                self.crop_source_data = None;

                let files = input.files().ok_or("no file list")?;
                let file = files.get(0).ok_or("no file selected")?;

                self.crop_source_url = Some(TemporaryUrl::create(
                    &file,
                    ctx.link().callback(Msg::Error),
                )?);

                let content_type = file.type_();
                let gloo_file = gloo::file::File::from(file);
                let link = ctx.link().clone();
                self._file_reader = Some(read_as_bytes(&gloo_file, move |res| {
                    link.send_message(Msg::FileRead(content_type.clone(), res));
                }));

                Ok(true)
            }
            Msg::FileRead(content_type, result) => {
                self._file_reader = None;
                let data = result
                    .map_err(anyhow::Error::from)
                    .context("reading image")?;
                self.crop_source_data = Some((content_type, data));
                Ok(false)
            }
            Msg::CropDrag(drag) => {
                self.drag = drag;
                Ok(true)
            }
            Msg::CropConfirmed(crop) => {
                let Some((content_type, data)) = self.crop_source_data.take() else {
                    return Err("image data not ready".into());
                };

                self.crop_source_url = None;
                self.uploading = true;

                self._upload_image = self
                    .channel
                    .request()
                    .body(api::UploadImageRequestRef {
                        content_type: &content_type,
                        role: ctx.props().role,
                        crop,
                        sizing: ctx.props().sizing,
                        size: ctx.props().size,
                        data: &data,
                    })
                    .on_packet(ctx.link().callback(Msg::Uploaded))
                    .send();

                Ok(true)
            }
            Msg::CropCancelled => {
                self.crop_source_url = None;
                self.crop_source_data = None;
                self._file_reader = None;
                Ok(true)
            }
            Msg::Uploaded(body) => {
                let body = body?;
                let body = body.decode()?;

                self.crop_source_url = None;
                self.uploading = false;

                ctx.props().onselect.emit(body.image.id);
                Ok(true)
            }
            Msg::SelectImage(id) => {
                ctx.props().onselect.emit(id);
                Ok(true)
            }
            Msg::RemoveImage(id) => {
                self._remove_image = self
                    .channel
                    .request()
                    .body(api::RemoveImageRequest { id: id.id })
                    .on_packet(
                        ctx.link()
                            .callback(move |packet| Msg::RemoveResult(id, packet)),
                    )
                    .send();

                Ok(false)
            }
            Msg::RemoveResult(id, body) => {
                let body = body?;
                _ = body.decode()?;

                if id == ctx.props().selected {
                    ctx.props().onselect.emit(RemoteId::ZERO);
                }

                Ok(false)
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
}
