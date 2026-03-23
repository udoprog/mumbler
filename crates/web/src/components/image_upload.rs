use api::{Id, Image};
use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{HtmlInputElement, Url};
use yew::prelude::*;

use crate::error::Error;
use crate::log;

use super::{CropModal, Icon, ImageGalleryModal, into_target};

pub(crate) enum Msg {
    FileSelected(Event),
    FileRead(String, Result<Vec<u8>, gloo::file::FileReadError>),
    CropConfirmed(api::CropRegion),
    CropCancelled,
    Uploaded(Result<Packet<api::UploadImage>, ws::Error>),
    OpenGallery,
    CloseGallery,
    SelectImage(Id),
    DeleteImage(Id),
    DeleteResult(Result<Packet<api::DeleteImage>, ws::Error>),
    SetLog(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
    pub(crate) images: Vec<Image>,
    pub(crate) selected: Id,
    pub(crate) sizing: api::ImageSizing,
    pub(crate) size: u32,
    #[prop_or_default]
    pub(crate) crop_ratio: Option<f64>,
    pub(crate) input_id: AttrValue,
    pub(crate) onselect: Callback<Id>,
    pub(crate) onrefresh: Callback<()>,
    #[prop_or_default]
    pub(crate) onrescale: Option<Callback<Option<f64>>>,
}

pub(crate) struct ImageUpload {
    _delete_image: ws::Request,
    _file_reader: Option<FileReader>,
    _upload_image: ws::Request,
    crop_source_data: Option<(String, Vec<u8>)>,
    crop_source_url: Option<String>,
    gallery_open: bool,
    uploading: bool,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
}

impl Component for ImageUpload {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::SetLog))
            .expect("ErrorLog context not found");

        Self {
            _delete_image: ws::Request::new(),
            _file_reader: None,
            _upload_image: ws::Request::new(),
            crop_source_data: None,
            crop_source_url: None,
            gallery_open: false,
            uploading: false,
            log,
            _log_handle,
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

        html! {
            <>
            <section class="btn-group">
                <label for={input_id.clone()} class={classes!("btn", "primary", self.uploading.then_some("disabled"))}>
                    {"Upload"}
                    <Icon name="arrow-up-on-square" />
                </label>

                <button class="btn primary" onclick={ctx.link().callback(|_| Msg::OpenGallery)}>
                    {"Gallery"}
                    <Icon name="photo" />
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
                <ImageGalleryModal
                    images={ctx.props().images.clone()}
                    selected={ctx.props().selected}
                    onselect={ctx.link().callback(Msg::SelectImage)}
                    ondelete={ctx.link().callback(Msg::DeleteImage)}
                    onclose={ctx.link().callback(|_| Msg::CloseGallery)}
                />
            }

            if let Some(src) = &self.crop_source_url {
                <CropModal
                    source_url={src.clone()}
                    ratio={ctx.props().crop_ratio}
                    onconfirm={ctx.link().callback(Msg::CropConfirmed)}
                    oncancel={ctx.link().callback(|_| Msg::CropCancelled)}
                    rescale={ctx.props().onrescale.clone()}
                />
            }
            </>
        }
    }
}

impl ImageUpload {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::FileSelected(e) => {
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
                    link.send_message(Msg::FileRead(content_type.clone(), res));
                }));

                Ok(true)
            }
            Msg::FileRead(content_type, result) => {
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
                        sizing: ctx.props().sizing,
                        size: ctx.props().size,
                    })
                    .on_packet(ctx.link().callback(Msg::Uploaded))
                    .send();

                self.uploading = true;
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
            Msg::Uploaded(body) => {
                let body = body?;
                let body = body.decode()?;

                self.uploading = false;

                if let Some(url) = self.crop_source_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

                ctx.props().onselect.emit(body.id);
                ctx.props().onrefresh.emit(());
                Ok(true)
            }
            Msg::SelectImage(id) => {
                ctx.props().onselect.emit(id);
                self.gallery_open = false;
                Ok(true)
            }
            Msg::DeleteImage(id) => {
                self._delete_image = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::DeleteImageRequest { id })
                    .on_packet(ctx.link().callback(Msg::DeleteResult))
                    .send();
                Ok(false)
            }
            Msg::DeleteResult(body) => {
                let body = body?;
                _ = body.decode()?;
                ctx.props().onrefresh.emit(());
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
            Msg::SetLog(log) => {
                self.log = log;
                Ok(false)
            }
        }
    }
}
