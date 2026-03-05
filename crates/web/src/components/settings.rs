use derive_more::From;
use gloo::file::callbacks::{FileReader, read_as_bytes};
use musli_web::web::Packet;
use musli_web::web03::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::HtmlInputElement;
use yew::prelude::*;

use crate::error::Error;

#[derive(From)]
pub(crate) enum Msg {
    StateChanged(ws::State),
    #[from(skip)]
    AvatarImageSelected(Event),
    #[from(skip)]
    AvatarImageData(String, Result<Vec<u8>, gloo::file::FileReadError>),
    #[from(skip)]
    AvatarUploaded(Result<Packet<api::UploadImage>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) ws: ws::Handle,
}

pub(crate) struct Settings {
    state: ws::State,
    _state_change: ws::StateListener,
    _file_reader: Option<FileReader>,
    _upload_avatar: ws::Request,
}

impl Settings {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::StateChanged(state) => {
                self.state = state;
                Ok(true)
            }
            Msg::AvatarImageSelected(e) => {
                let input = e
                    .target()
                    .ok_or("no target")?
                    .dyn_into::<HtmlInputElement>()
                    .map_err(|_| "target is not an input element")?;

                let files = input.files().ok_or("no file list")?;
                let file = files.get(0).ok_or("no file selected")?;

                let content_type = file.type_();
                let gloo_file = gloo::file::File::from(file);
                let link = ctx.link().clone();

                self._file_reader = Some(read_as_bytes(&gloo_file, move |res| {
                    link.send_message(Msg::AvatarImageData(content_type.clone(), res));
                }));

                Ok(false)
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
                result?;
                Ok(false)
            }
        }
    }
}

impl Component for Settings {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (state, _state_change) = ctx
            .props()
            .ws
            .on_state_change(ctx.link().callback(Msg::StateChanged));

        Self {
            state,
            _state_change,
            _file_reader: None,
            _upload_avatar: ws::Request::new(),
        }
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
        html! {
            <div class="settings">
                <h1>{"Settings" }</h1>

                <section class="user">
                    <label class="btn" title="Upload avatar image">
                        {"Upload avatar"}
                        <input
                            type="file"
                            accept="image/*"
                            style="display:none"
                            onchange={ctx.link().callback(Msg::AvatarImageSelected)}
                        />
                    </label>
                </section>
            </div>
        }
    }
}
