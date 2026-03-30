use api::{Image, RemoteId, Role};
use musli_web::api::ChannelId;
use musli_web::web03::prelude::*;
use web_sys::MouseEvent;
use yew::prelude::*;

use crate::error::Error;
use crate::log::Log;

use super::SetupChannel;

static FILTER_BUTTONS: &[(&str, Role)] = &[
    ("All", Role::NONE),
    ("Token", Role::TOKEN),
    ("Static", Role::STATIC),
    ("Background", Role::BACKGROUND),
];

pub(crate) enum Msg {
    Channel(Result<ws::Channel, Error>),
    Select(RemoteId),
    Role(Role),
    Log(Log),
    Initialize(Result<ws::Packet<api::InitializeImageUpload>, ws::Error>),
    RemoteUpdate(Result<ws::Packet<api::RemoteUpdate>, ws::Error>),
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    /// Currently selected image, if any.
    #[prop_or_default]
    pub(crate) selected: RemoteId,
    /// Callback fired when an image is selected.
    pub(crate) onselect: Callback<RemoteId>,
    /// Callback fired when an image should be deleted.
    pub(crate) ondelete: Callback<RemoteId>,
    /// The role to pre-select in the filter. Defaults to Role::NONE (show all).
    #[prop_or_default]
    pub(crate) default_role: Role,
}

pub(crate) struct ImageGallery {
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    log: Log,
    _initialize: ws::Request,
    _listener: ws::Listener,
    filter: Role,
    images: Vec<Image>,
}

impl Component for ImageGallery {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _) = ctx
            .link()
            .context::<Log>(ctx.link().callback(Msg::Log))
            .expect("Log context not found");

        let (ws, _) = ctx
            .link()
            .context::<ws::Handle>(Callback::noop())
            .expect("WebSocket context not found");

        Self {
            log,
            channel: ws::Channel::default(),
            _setup_channel: SetupChannel::new(ws, ctx.link().callback(Msg::Channel)),
            _initialize: ws::Request::new(),
            _listener: ws::Listener::new(),
            filter: ctx.props().default_role,
            images: Vec::new(),
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match self.try_update(ctx, msg) {
            Ok(changed) => changed,
            Err(error) => {
                self.log.error("ImageGalleryModal::update", error);
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let filter_buttons = FILTER_BUTTONS.iter().map(|&(label, role)| {
            let inactive = self.filter != role;
            let onclick = ctx.link().callback(move |_| Msg::Role(role));

            html! {
                <button class={classes!("btn", "sm", inactive.then_some("inactive"))} {onclick}>
                    {label}
                </button>
            }
        });

        let images = self
            .images
            .iter()
            .map(|image| {
                let id = image.id;

                let on_select = ctx.link().callback(move |_: MouseEvent| {
                    Msg::Select(id)
                });

                let on_delete = ctx.props().ondelete.reform(move |ev: MouseEvent| {
                    ev.stop_propagation();
                    id
                });

                let classes = classes!(
                    "image",
                    (ctx.props().selected == image.id).then_some("selected"),
                    "clickable"
                );

                html! {
                    <div class="image-entry">
                        <img src={format!("/api/image/{}", image.id)} alt={format!("Image {}", image.id)} onclick={on_select} class={classes} />
                        <button class="btn danger floating icon" onclick={on_delete} title="Remove Image">{"ⓧ"}</button>
                    </div>
                }
            });

        html! {
            <>
                <div class="control-group btn-group">
                    {for filter_buttons}
                </div>

                if self.images.is_empty() {
                    <p class="hint">{"No images."}</p>
                } else {
                    <div class="gallery">
                        {for images}
                    </div>
                }
            </>
        }
    }
}

impl ImageGallery {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Select(id) => {
                ctx.props().onselect.emit(id);
                Ok(false)
            }
            Msg::Role(role) => {
                self.filter = role;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(true);
                }

                self._initialize = self
                    .channel
                    .request()
                    .body(api::InitializeImageUploadRequest)
                    .on_packet(ctx.link().callback(Msg::Initialize))
                    .send();

                Ok(true)
            }
            Msg::Log(log) => {
                self.log = log;
                Ok(true)
            }
            Msg::Channel(channel) => {
                self.channel = channel?;

                if self.channel.id() == ChannelId::NONE {
                    return Ok(false);
                }

                self._initialize = self
                    .channel
                    .request()
                    .body(api::InitializeImageUploadRequest)
                    .on_packet(ctx.link().callback(Msg::Initialize))
                    .send();

                self._listener = self
                    .channel
                    .handle()
                    .on_broadcast(ctx.link().callback(Msg::RemoteUpdate));

                Ok(false)
            }
            Msg::Initialize(response) => {
                let response = response?;
                let response = response.decode()?;
                self.initialize(response);
                Ok(true)
            }
            Msg::RemoteUpdate(response) => {
                let response = response?;
                let response = response.decode()?;
                Ok(self.remote_update(response))
            }
        }
    }

    fn initialize(&mut self, response: api::InitializeImageUploadResponse) {
        self.images = response.images;

        self.images
            .retain(|image| self.filter == Role::NONE || image.role == self.filter);
    }

    fn remote_update(&mut self, response: api::RemoteUpdateBody) -> bool {
        match response {
            api::RemoteUpdateBody::ImageCreated { image } => {
                self.images.push(image);
            }
            api::RemoteUpdateBody::ImageRemoved { id } => {
                self.images.retain(|image| image.id != id);
            }
            _ => return false,
        }

        true
    }
}
