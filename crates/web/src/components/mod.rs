mod macros;
use self::macros::into_target;

mod app;
pub use self::app::App;

mod map;
pub(crate) use self::map::Map;

pub(crate) mod render;

mod crop_modal;
pub(crate) use self::crop_modal::CropModal;

mod image_gallery_modal;
pub(crate) use self::image_gallery_modal::ImageGalleryModal;

mod settings;
pub(crate) use self::settings::Settings;

mod object_settings;
pub(crate) use self::object_settings::TokenSettings;

mod static_settings;
pub(crate) use self::static_settings::StaticSettings;

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

mod help_modal;
use self::help_modal::HelpModal;

use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub(crate) struct IconProps {
    pub(crate) name: String,
    #[prop_or_default]
    pub(crate) title: Option<String>,
    #[prop_or_default]
    pub(crate) invert: bool,
    #[prop_or_default]
    pub(crate) small: bool,
}

#[function_component(Icon)]
pub(crate) fn icon(props: &IconProps) -> Html {
    let title = props.title.clone();

    let class = match props.name.as_str() {
        "mumble" => "image-icon",
        _ => "icon",
    };

    let class = classes! {
        class,
        props.name.clone(),
        props.invert.then_some("invert"),
        props.small.then_some("sm"),
    };

    html! {
        <span {class} {title} />
    }
}
