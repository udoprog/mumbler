use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{File, HtmlInputElement, Url};
use yew::prelude::*;

use crate::error::Error;

pub(crate) enum Msg {
    StateChanged(ws::State),
    AvatarImageSelected(Event),
    AvatarImageUpload(MouseEvent),
    AvatarImageClear(MouseEvent),
    AvatarImageData(String, Result<Vec<u8>, gloo::file::FileReadError>),
    AvatarUploaded(Result<Packet<api::UploadImage>, ws::Error>),
    ListImages(Result<Packet<api::ListSettings>, ws::Error>),
    SelectImage(api::Id),
    SelectImageResult(Result<Packet<api::SelectImage>, ws::Error>),
    DeleteImage(api::Id),
    DeleteImageResult(Result<Packet<api::DeleteImage>, ws::Error>),
    ColorChanged(Event),
    SelectColor(api::Color),
    SelectColorResult(Result<Packet<api::SelectColor>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct Settings {
    state: ws::State,
    selected: Option<api::Id>,
    color: api::Color,
    images: Vec<api::Image>,
    file: Option<File>,
    preview_url: Option<String>,
    _state_change: ws::StateListener,
    _file_reader: Option<FileReader>,
    _upload_avatar: ws::Request,
    _list_images: ws::Request,
    _select_image: ws::Request,
    _delete_image: ws::Request,
    _select_color: ws::Request,
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
            color: api::Color::neutral(),
            images: Vec::new(),
            file: None,
            preview_url: None,
            _state_change,
            _file_reader: None,
            _upload_avatar: ws::Request::new(),
            _list_images: ws::Request::new(),
            _select_image: ws::Request::new(),
            _delete_image: ws::Request::new(),
            _select_color: ws::Request::new(),
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

        let choose_classes = classes!("btn", self.file.is_some().then_some("hidden"));

        let ok = self
            .file
            .is_some()
            .then(|| ctx.link().callback(Msg::AvatarImageUpload));

        let ok_classes = classes!("btn", ok.is_none().then_some("hidden"));

        let cancel = self
            .file
            .is_some()
            .then(|| ctx.link().callback(Msg::AvatarImageClear));

        let cancel_classes = classes!("btn", "danger", cancel.is_none().then_some("hidden"));

        html! {
            <div class="settings rows">
                <h2>{"Select Avatar:"}</h2>

                if let Some(url) = &self.preview_url {
                    <section class="image-entry">
                        <img src={url.clone()} alt="Preview" class="avatar" />
                    </section>
                }

                <section>
                    <div class="btn-group">
                        <label for="avatar-file" class={choose_classes}>{"Upload image"}</label>
                        <button onclick={ok} class={ok_classes}>{"Ok"}</button>
                        <button onclick={cancel} class={cancel_classes}>{"Cancel"}</button>
                    </div>

                    <input
                        id="avatar-file"
                        class="hidden"
                        title="Upload avatar image"
                        type="file"
                        accept="image/*"
                        onchange={ctx.link().callback(Msg::AvatarImageSelected)}
                        />
                </section>

                <div class="gallery">
                    {for images}
                </div>

                <h2>{"Avatar Color:"}</h2>

                <section class="color-picker">
                    <label for="avatar-color">{"Select Color:"}</label>
                    <input
                        id="avatar-color"
                        type="color"
                        value={self.color.to_css_string()}
                        onchange={ctx.link().callback(Msg::ColorChanged)}
                        />
                </section>
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
                // Clean up old preview URL if any
                if let Some(url) = self.preview_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let files = input.files().ok_or("no file list")?;
                let file = files.get(0).ok_or("no file selected")?;

                // Create preview URL
                if let Ok(url) = Url::create_object_url_with_blob(&file) {
                    self.preview_url = Some(url);
                }

                self.file = Some(file);
                Ok(true)
            }
            Msg::AvatarImageUpload(_e) => {
                let Some(file) = self.file.take() else {
                    return Err("no file selected".into());
                };

                // Clean up preview URL when uploading
                if let Some(url) = self.preview_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

                let content_type = file.type_();
                let gloo_file = gloo::file::File::from(file);
                let link = ctx.link().clone();

                self._file_reader = Some(read_as_bytes(&gloo_file, move |res| {
                    link.send_message(Msg::AvatarImageData(content_type.clone(), res));
                }));

                Ok(true)
            }
            Msg::AvatarImageClear(_e) => {
                self.file = None;

                if let Some(url) = self.preview_url.take() {
                    let _ = Url::revoke_object_url(&url);
                }

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
                _ = result.decode()?;
                self.refresh(ctx);
                Ok(false)
            }
            Msg::ListImages(result) => {
                let result = result?;
                let response = result.decode()?;
                self.selected = response.selected;
                self.color = response.color;
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
            Msg::ColorChanged(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let hex_string = input.value();
                if let Some(color) = api::Color::from_hex(&hex_string) {
                    ctx.link().send_message(Msg::SelectColor(color));
                }
                Ok(false)
            }
            Msg::SelectColor(color) => {
                self._select_color = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::SelectColorRequest { color })
                    .on_packet(ctx.link().callback(Msg::SelectColorResult))
                    .send();
                Ok(false)
            }
            Msg::SelectColorResult(result) => {
                let result = result?;
                let response = result.decode()?;
                self.color = response.color;
                Ok(true)
            }
        }
    }
}
