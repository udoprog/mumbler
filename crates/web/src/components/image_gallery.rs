use api::{Image, RemoteId, Role};
use musli_web::web03::prelude::*;
use musli_web::yew023::ContextExt as _;
use web_sys::MouseEvent;
use yew::prelude::*;

use crate::error::Error;
use crate::l10n;
use crate::log::Log;

static FILTER_BUTTONS: &[(&str, Role)] = &[
    ("image-filter-all", Role::NONE),
    ("image-filter-token", Role::TOKEN),
    ("image-filter-static", Role::STATIC),
    ("image-filter-background", Role::BACKGROUND),
];

pub(crate) enum Msg {
    L10n(l10n::Phrases),
    Channel(ws::ChannelEvent),
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
    l10n: l10n::Subscription,
    channel: ws::Channel,
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

        let ws = ctx.handle();

        Self {
            l10n: l10n::Subscription::new(ctx, Msg::L10n),
            log,
            channel: ws
                .channel()
                .on_change(ctx.link().callback(Msg::Channel))
                .open(),
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
                    {self.l10n.text(label)}
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

                let id_text = image.id.to_string();
                let alt = self.l10n.msg("image-alt", &l10n::args([("id", id_text.as_str())]));

                html! {
                    <div class="image-entry">
                        <img src={format!("/api/image/{}", image.id)} {alt} onclick={on_select} class={classes} />
                        <button class="btn danger floating icon" onclick={on_delete} title={self.l10n.text("image-remove")}>{"ⓧ"}</button>
                    </div>
                }
            });

        html! {
            <>
                <div class="control-group btn-group">
                    {for filter_buttons}
                </div>

                if self.images.is_empty() {
                    <p class="hint">{self.l10n.text("image-none")}</p>
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
            Msg::L10n(phrases) => Ok(self.l10n.update(phrases)),
            Msg::Select(id) => {
                ctx.props().onselect.emit(id);
                Ok(false)
            }
            Msg::Role(role) => {
                self.filter = role;

                if !self.channel.is_open() {
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
            Msg::Channel(event) => {
                match event {
                    ws::ChannelEvent::Opened(..) => {}
                    ws::ChannelEvent::Failed(error) => return Err(error.into()),
                    _ => return Ok(false),
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
