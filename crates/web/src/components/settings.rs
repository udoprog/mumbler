use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, File, HtmlCanvasElement, HtmlInputElement, Url};
use yew::prelude::*;

use crate::error::Error;
use crate::images::{ImageMessage, Images};
use crate::log;

use super::render;

pub(crate) enum Msg {
    StateChanged(ws::State),
    AvatarImageSelected(Event),
    AvatarImageUpload(MouseEvent),
    AvatarImageClear(MouseEvent),
    AvatarImageData(String, Result<Vec<u8>, gloo::file::FileReadError>),
    ImageUploaded(Result<Packet<api::UploadImage>, ws::Error>),
    ListImages(Result<Packet<api::ListSettings>, ws::Error>),
    SelectImage(api::Id),
    SelectImageResult(Result<Packet<api::SelectImage>, ws::Error>),
    DeleteImage(api::Id),
    DeleteImageResult(Result<Packet<api::DeleteImage>, ws::Error>),
    ColorChanged(Event),
    SelectColor(api::Color),
    SelectColorResult(Result<Packet<api::SelectColor>, ws::Error>),
    NameChanged(Event),
    UpdateName(Option<String>),
    UpdateNameResult(Result<Packet<api::UpdateName>, ws::Error>),
    ServerChanged(Event),
    SetRemoteServer(String),
    SetRemoteServerResult(Result<Packet<api::SetRemoteServer>, ws::Error>),
    ImageLoaded(ImageMessage),
    ContextUpdate(log::Log),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct Settings {
    state: ws::State,
    selected: Option<api::Id>,
    color: api::Color,
    name: Option<String>,
    images: Vec<api::Image>,
    file: Option<File>,
    preview_url: Option<String>,
    preview_canvas: NodeRef,
    preview_images: Images<Self>,
    log: log::Log,
    _log_handle: ContextHandle<log::Log>,
    _state_change: ws::StateListener,
    _file_reader: Option<FileReader>,
    upload_image: ws::Request,
    image_uploading: bool,
    _list_images: ws::Request,
    _select_image: ws::Request,
    _delete_image: ws::Request,
    _select_color: ws::Request,
    _update_name: ws::Request,
    remote_server: String,
    _set_remote_server: ws::Request,
}

impl From<ImageMessage> for Msg {
    #[inline]
    fn from(message: ImageMessage) -> Self {
        Msg::ImageLoaded(message)
    }
}

impl Component for Settings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _log_handle) = ctx
            .link()
            .context::<log::Log>(ctx.link().callback(Msg::ContextUpdate))
            .expect("ErrorLog context not found");

        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        let mut this = Self {
            state,
            selected: None,
            color: api::Color::neutral(),
            name: None,
            images: Vec::new(),
            file: None,
            preview_url: None,
            preview_canvas: NodeRef::default(),
            preview_images: Images::new(),
            log,
            _log_handle,
            _state_change,
            _file_reader: None,
            upload_image: ws::Request::new(),
            image_uploading: false,
            _list_images: ws::Request::new(),
            _select_image: ws::Request::new(),
            _delete_image: ws::Request::new(),
            _select_color: ws::Request::new(),
            _update_name: ws::Request::new(),
            remote_server: String::new(),
            _set_remote_server: ws::Request::new(),
        };

        this.refresh(ctx);
        this
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("settings::update", &error);
                true
            }
        }
    }

    fn rendered(&mut self, _ctx: &Context<Self>, _first_render: bool) {
        if let Err(error) = self.redraw_preview() {
            self.log.error("settings::redraw_preview", error);
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

        let choose_classes = classes!(
            "btn",
            self.file.is_some().then_some("hidden"),
            self.image_uploading.then_some("disabled")
        );
        let choose_disabled = self.file.is_some() || self.image_uploading;

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
            <div class="row">
                <div class="col-8 rows">
                    <h2>{"Avatar Name"}</h2>

                    <section>
                        <input
                            id="avatar-name"
                            type="text"
                            placeholder="Enter avatar name"
                            value={self.name.clone().unwrap_or_default()}
                            onchange={ctx.link().callback(Msg::NameChanged)}
                            />
                    </section>

                    <h2>{"Remote Server"}</h2>

                    <div class="hint">
                        {"If a remote server is configured and enabled, it can be used to synchronize state between many Mumbler Clients."}
                    </div>

                    <section>
                        <input
                            id="remote-server"
                            type="text"
                            placeholder="host[:port]"
                            value={self.remote_server.clone()}
                            onchange={ctx.link().callback(Msg::ServerChanged)}
                            />
                    </section>

                    <h2>{"Select Avatar"}</h2>

                    if let Some(url) = &self.preview_url {
                        <section class="image-entry">
                            <img src={url.clone()} alt="Preview" class="avatar" />
                        </section>
                    }

                    <section>
                        <div class="btn-group">
                            <label for="avatar-file" class={choose_classes} disabled={choose_disabled}>{"Upload image"}</label>
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

                    <h2>{"Avatar Color"}</h2>

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

                <div class="col-4 rows">
                    <h2>{"Avatar Preview"}</h2>

                    <section class="avatar-preview">
                        <canvas ref={self.preview_canvas.clone()} width="200" height="200" />
                    </section>
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

                self.upload_image = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UploadImageRequest { content_type, data })
                    .on_packet(ctx.link().callback(Msg::ImageUploaded))
                    .send();

                self.image_uploading = true;
                Ok(false)
            }
            Msg::ImageUploaded(result) => {
                self.image_uploading = false;
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
                self.name = response.name;
                self.remote_server = response.remote_server.unwrap_or_default();
                self.load_preview_image(ctx);
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
                self.load_preview_image(ctx);
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
            Msg::NameChanged(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let value = input.value();
                let name = if value.is_empty() { None } else { Some(value) };
                ctx.link().send_message(Msg::UpdateName(name));
                Ok(false)
            }
            Msg::UpdateName(name) => {
                self._update_name = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::UpdateNameRequest { name })
                    .on_packet(ctx.link().callback(Msg::UpdateNameResult))
                    .send();
                Ok(false)
            }
            Msg::UpdateNameResult(result) => {
                let result = result?;
                let response = result.decode()?;
                self.name = response.name;
                Ok(true)
            }
            Msg::ServerChanged(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let value = input.value();
                ctx.link().send_message(Msg::SetRemoteServer(value));
                Ok(false)
            }
            Msg::SetRemoteServer(server) => {
                self._set_remote_server = ctx
                    .props()
                    .ws
                    .request()
                    .body(api::SetRemoteServerRequest { server })
                    .on_packet(ctx.link().callback(Msg::SetRemoteServerResult))
                    .send();
                Ok(false)
            }
            Msg::SetRemoteServerResult(result) => {
                let result = result?;
                let response = result.decode()?;
                self.remote_server = response.server;
                Ok(true)
            }
            Msg::ImageLoaded(msg) => {
                self.preview_images.update(msg);
                Ok(true)
            }
            Msg::ContextUpdate(log) => {
                self.log = log;
                Ok(false)
            }
        }
    }

    fn load_preview_image(&mut self, ctx: &Context<Self>) {
        self.preview_images.clear();

        if let Some(id) = self.selected {
            self.preview_images.load(ctx, id);
        }
    }

    fn redraw_preview(&self) -> Result<(), Error> {
        let Some(canvas) = self.preview_canvas.cast::<HtmlCanvasElement>() else {
            return Ok(());
        };

        let cx = canvas.get_context("2d")?.ok_or("missing canvas context")?;

        let cx = cx
            .dyn_into::<CanvasRenderingContext2d>()
            .map_err(|_| "invalid canvas context")?;

        let avatar = render::RenderAvatar {
            transform: api::Transform::origin(),
            look_at: None,
            image: self.selected,
            color: self.color,
            name: self.name.clone(),
            player: true,
        };

        render::draw_avatar_preview(&cx, &canvas, &avatar, |id| {
            self.preview_images.get(id).cloned()
        })?;

        Ok(())
    }
}
