use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{File, HtmlInputElement};
use yew::prelude::*;

use crate::error::Error;

pub(crate) enum Msg {
    StateChanged(ws::State),
    AvatarImageSelected(Event),
    AvatarImageUpload(MouseEvent),
    AvatarImageData(String, Result<Vec<u8>, gloo::file::FileReadError>),
    AvatarUploaded(Result<Packet<api::UploadImage>, ws::Error>),
    ListImages(Result<Packet<api::ListSettings>, ws::Error>),
    SelectImage(api::Id),
    SelectImageResult(Result<Packet<api::SelectImage>, ws::Error>),
    DeleteImage(api::Id),
    DeleteImageResult(Result<Packet<api::DeleteImage>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct Settings {
    state: ws::State,
    selected: Option<api::Id>,
    images: Vec<api::Image>,
    file: Option<File>,
    _state_change: ws::StateListener,
    _file_reader: Option<FileReader>,
    _upload_avatar: ws::Request,
    _list_images: ws::Request,
    _select_image: ws::Request,
    _delete_image: ws::Request,
}

impl Component for Settings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let mut this = Self {
            state,
            selected: None,
            images: Vec::new(),
            file: None,
            _state_change,
            _file_reader: None,
            _upload_avatar: ws::Request::new(),
            _list_images: ws::Request::new(),
            _select_image: ws::Request::new(),
            _delete_image: ws::Request::new(),
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                tracing::error!(%error, "Failed to update settings");
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        tracing::info!(?self.images);

        let images = self.images.iter().map(|image| {
            let id = image.id;
            let on_select = ctx.link().callback(move |_| Msg::SelectImage(id));
            let on_delete = ctx.link().callback(move |_: MouseEvent| Msg::DeleteImage(id));
            let classes = classes!("avatar", (self.selected == Some(id)).then_some("selected"), "clickable");

            html! {
                <div class="image-entry">
                    <img src={format!("/api/image/{}", image.id)} alt={format!("Image {}", image.id)} onclick={on_select} class={classes} />
                    <button class="btn danger floating icon" onclick={on_delete} title="Remove image">{"ⓧ"}</button>
                </div>
            }
        });

        let class = classes!("btn", self.file.is_none().then_some("disabled"));

        let upload = self
            .file
            .is_some()
            .then(|| ctx.link().callback(Msg::AvatarImageUpload));

        html! {
            <div class="settings rows">
                <h2>{"Select Avatar:"}</h2>

                <section>
                    <label for="avatar-file" class="btn">{"Choose Image"}</label>
                    <input
                        id="avatar-file"
                        class="hidden"
                        title="Upload avatar image"
                        type="file"
                        accept="image/*"
                        onchange={ctx.link().callback(Msg::AvatarImageSelected)}
                        />
                </section>

                <section>
                    <button {class} onclick={upload}>{"Upload"}</button>
                </section>

                <div class="gallery">
                    {for images}
                </div>
            </div>
        }
    }
}

impl Settings {
    fn refresh(&mut self, ctx: &Context<Self>) {
        if matches!(self.state, ws::State::Open) {
            self._list_images = ctx
                .props()
                .ws
                .request()
                .body(api::ListSettingsRequest)
                .on_packet(ctx.link().callback(Msg::ListImages))
                .send();
        } else {
            self._list_images = ws::Request::new();
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
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let files = input.files().ok_or("no file list")?;
                self.file = Some(files.get(0).ok_or("no file selected")?);
                Ok(true)
            }
            Msg::AvatarImageUpload(_e) => {
                let Some(file) = self.file.take() else {
                    return Err("no file selected".into());
                };

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

                self._upload_avatar = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UploadImageRequest { content_type, data })
                    .on_packet(ctx.link().callback(Msg::AvatarUploaded))
                    .send();

                Ok(false)
            }
            Msg::AvatarUploaded(result) => {
                let result = result?;
                let response = result.decode()?;
                tracing::info!(?response.id, "Avatar uploaded with ID");
                self.refresh(ctx);
                Ok(false)
            }
            Msg::ListImages(result) => {
                let result = result?;
                let response = result.decode()?;
                self.selected = response.selected;
                self.images = response.images;
                Ok(true)
            }
            Msg::SelectImage(id) => {
                self._select_image = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::SelectImageRequest { id })
                    .on_packet(ctx.link().callback(Msg::SelectImageResult))
                    .send();

                Ok(false)
            }
            Msg::SelectImageResult(result) => {
                let result = result?;
                let response = result.decode()?;
                self.selected = Some(response.id);
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
        }
    }
}
