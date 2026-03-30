mod macros;
use self::macros::into_target;

mod animation_frame;
use self::animation_frame::AnimationFrame;

mod app;
pub use self::app::App;

mod map;
use self::map::Map;

mod render;
pub(crate) use self::render::Visibility;
use self::render::{RenderObject, RenderObjectKind, ViewTransform};

mod crop;
use self::crop::{Crop, Extent};

mod drop_image;
use self::drop_image::{DropImage, DropImageResult};

mod image_gallery;
use self::image_gallery::ImageGallery;

mod settings;
use self::settings::Settings;

mod rooms;
use self::rooms::Rooms;

mod object_settings;
use self::object_settings::ObjectSettings;

mod token_settings;
use self::token_settings::TokenSettings;

mod static_settings;
use self::static_settings::StaticSettings;

mod image_upload;
use self::image_upload::ImageUpload;

mod group_settings;
use self::group_settings::GroupSettings;

mod mumble_status;
use self::mumble_status::MumbleStatus;

mod remote_status;
use self::remote_status::RemoteStatus;

mod log;
use self::log::Log;

mod navigation;
use self::navigation::{Navigation, Route};

mod object_list;
use self::object_list::ObjectList;

mod help;
use self::help::Help;

mod dynamic_canvas;
use self::dynamic_canvas::DynamicCanvas;

mod context_menu_dropdown;
use self::context_menu_dropdown::ContextMenuDropdown;

mod setup_channel;
use self::setup_channel::SetupChannel;

mod temporary_url;
use self::temporary_url::TemporaryUrl;

mod modal;
use self::modal::Modal;

mod icon;
use self::icon::Icon;

use musli_web::ChannelId;
use musli_web::web03::prelude::*;
use yew::prelude::*;

const UNKNOWN_ROOM: &str = "Unknown Room";
const COMMON_ROOM: &str = "Foyer";

trait ChannelExt {
    fn object_updates<T>(
        &self,
        ctx: &Context<T>,
        id: api::Id,
        values: impl IntoIterator<Item = (api::Key, api::Value), IntoIter: ExactSizeIterator>,
    ) -> ws::Request
    where
        T: Component<Message: From<Result<ws::Packet<api::ObjectUpdate>, ws::Error>>>;

    fn updates<T>(
        &self,
        ctx: &Context<T>,
        values: impl IntoIterator<Item = (api::Key, api::Value), IntoIter: ExactSizeIterator>,
    ) -> ws::Request
    where
        T: Component<Message: From<Result<ws::Packet<api::Updates>, ws::Error>>>;
}

impl ChannelExt for ws::Channel {
    fn object_updates<T>(
        &self,
        ctx: &Context<T>,
        id: api::Id,
        values: impl IntoIterator<Item = (api::Key, api::Value), IntoIter: ExactSizeIterator>,
    ) -> ws::Request
    where
        T: Component<Message: From<Result<ws::Packet<api::ObjectUpdate>, ws::Error>>>,
    {
        if self.id() == ChannelId::NONE {
            return ws::Request::default();
        }

        let mut iter = values.into_iter();

        if iter.len() > 1 {
            return self
                .request()
                .body(api::ObjectUpdateBody {
                    id,
                    values: iter.collect(),
                })
                .on_packet(ctx.link().callback(T::Message::from))
                .send();
        }

        let Some(value) = iter.next() else {
            return ws::Request::default();
        };

        self.request()
            .body(api::ObjectUpdateBodyRef {
                id,
                values: core::slice::from_ref(&value),
            })
            .on_packet(ctx.link().callback(T::Message::from))
            .send()
    }

    fn updates<T>(
        &self,
        ctx: &Context<T>,
        values: impl IntoIterator<Item = (api::Key, api::Value), IntoIter: ExactSizeIterator>,
    ) -> ws::Request
    where
        T: Component<Message: From<Result<ws::Packet<api::Updates>, ws::Error>>>,
    {
        if self.id() == ChannelId::NONE {
            return ws::Request::default();
        }

        let mut iter = values.into_iter();

        if iter.len() > 1 {
            return self
                .request()
                .body(api::UpdatesRequest {
                    values: iter.collect(),
                })
                .on_packet(ctx.link().callback(T::Message::from))
                .send();
        }

        let Some(value) = iter.next() else {
            return ws::Request::default();
        };

        self.request()
            .body(api::UpdatesRequestRef {
                values: core::slice::from_ref(&value),
            })
            .on_packet(ctx.link().callback(T::Message::from))
            .send()
    }
}
