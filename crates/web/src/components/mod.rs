mod macros;
use self::macros::into_target;

mod animation_frame;
pub(crate) use self::animation_frame::AnimationFrame;

mod app;
pub use self::app::App;

mod map;
pub(crate) use self::map::Map;

pub(crate) mod render;

mod crop;
pub(crate) use self::crop::Crop;

mod image_gallery;
pub(crate) use self::image_gallery::ImageGallery;

mod settings;
pub(crate) use self::settings::Settings;

mod rooms;
pub(crate) use self::rooms::Rooms;

mod room_settings;
pub(crate) use self::room_settings::RoomSettings;

mod token_settings;
pub(crate) use self::token_settings::TokenSettings;

mod static_settings;
pub(crate) use self::static_settings::StaticSettings;

mod image_upload;
pub(crate) use self::image_upload::ImageUpload;

mod group_settings;
pub(crate) use self::group_settings::GroupSettings;

mod mumble_status;
pub(crate) use self::mumble_status::MumbleStatus;

mod remote_status;
pub(crate) use self::remote_status::RemoteStatus;

mod log;
pub(crate) use self::log::Log;

mod navigation;
pub(crate) use self::navigation::{Navigation, Route};

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
