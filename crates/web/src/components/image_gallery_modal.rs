use api::{Image, RemoteId, Role};
use musli_web::web03::prelude::*;
use web_sys::MouseEvent;
use yew::prelude::*;

use crate::error::Error;
use crate::log::Log;

use super::{Icon, SetupChannel};

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
    Close,
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
    /// Callback fired to close the modal.
    pub(crate) onclose: Callback<()>,
    /// The role to pre-select in the filter. Defaults to Role::NONE (show all).
    #[prop_or_default]
    pub(crate) default_role: Role,
}

pub(crate) struct ImageGalleryModal {
    channel: ws::Channel,
    _setup_channel: SetupChannel,
    log: Log,
    _initialize: ws::Request,
    _listener: ws::Listener,
    filter: Role,
    images: Vec<Image>,
}

impl Component for ImageGalleryModal {
    type Message = Msg;
    type Properties = Props;

    fn create(ctx: &Context<Self>) -> Self {
        let (log, _) = ctx
            .link()
            .context::<Log>(ctx.link().callback(Msg::Log))
            .expect("Log context not found");

        Self {
            log,
            channel: ws::Channel::default(),
            _setup_channel: SetupChannel::new(ctx, ctx.link().callback(Msg::Channel)),
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
                let on_select = ctx.link().callback(move |_| Msg::Select(id));

                let on_delete = ctx.props().ondelete.reform(move |e: MouseEvent| {
                    e.stop_propagation();
                    id
                });

                let classes = classes!(
                    "token",
                    (ctx.props().selected == image.id).then_some("selected"),
                    "clickable"
                );

                html! {
                    <div class="image-entry">
                        <img src={format!("/api/image/{}", image.id)} alt={format!("Image {}", image.id)} onclick={on_select} class={classes} />
                        <button class="btn danger floating icon" onclick={on_delete} title="Remove image">{"ⓧ"}</button>
                    </div>
                }
            });

        html! {
            <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::Close)}>
                <div class="modal" onclick={|e: MouseEvent| e.stop_propagation()}>
                    <div class="modal-header">
                        <h2>{"Select Image"}</h2>
                        <button class="btn sm square danger" title="Close"
                            onclick={ctx.link().callback(|_| Msg::Close)}>
                            <Icon name="x-mark" />
                        </button>
                    </div>

                    <div class="modal-body rows">
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
                    </div>
                </div>
            </div>
        }
    }
}

impl ImageGalleryModal {
    fn try_update(&mut self, ctx: &Context<Self>, msg: Msg) -> Result<bool, Error> {
        match msg {
            Msg::Select(id) => {
                ctx.props().onselect.emit(id);
                ctx.props().onclose.emit(());
                Ok(false)
            }
            Msg::Role(role) => {
                self.filter = role;
                Ok(true)
            }
            Msg::Log(log) => {
                self.log = log;
                Ok(true)
            }
            Msg::Close => {
                ctx.props().onclose.emit(());
                Ok(false)
            }
            Msg::Channel(channel) => {
                self.channel = channel?;

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
                if let Err(error) = self.initialize(response) {
                    self.log.error("ImageGalleryModal::initialize", error);
                }

                Ok(true)
            }
            Msg::RemoteUpdate(response) => {
                if let Err(error) = self.remote_update(response) {
                    self.log.error("ImageGalleryModal::remote_update", error);
                }

                Ok(true)
            }
        }
    }

    fn initialize(
        &mut self,
        response: Result<ws::Packet<api::InitializeImageUpload>, ws::Error>,
    ) -> Result<(), ws::Error> {
        let response = response?;
        let response = response.decode()?;

        self.images = response.images;

        self.images
            .retain(|image| self.filter == Role::NONE || image.role == self.filter);

        Ok(())
    }

    fn remote_update(
        &mut self,
        response: Result<ws::Packet<api::RemoteUpdate>, ws::Error>,
    ) -> Result<bool, ws::Error> {
        let response = response?;
        let response = response.decode()?;

        match response {
            api::RemoteUpdateBody::ImageCreated { image } => {
                self.images.push(image);
            }
            api::RemoteUpdateBody::ImageRemoved { id } => {
                self.images.retain(|image| image.id != id);
            }
            _ => return Ok(false),
        }

        Ok(true)
    }
}
