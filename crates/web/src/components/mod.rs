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
pub(crate) use self::object_settings::ObjectSettings;

mod static_settings;
pub(crate) use self::static_settings::StaticSettings;

mod mumble_status;
pub(crate) use self::mumble_status::MumbleStatus;

mod remote_status;
pub(crate) use self::remote_status::RemoteStatus;

mod log;
pub(crate) use self::log::Log;

mod navigation;
pub(crate) use self::navigation::{Navigation, Route};

use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub(crate) struct IconProps {
    pub(crate) name: String,
    #[prop_or_default]
    pub(crate) title: Option<String>,
}

#[function_component(Icon)]
pub(crate) fn icon(props: &IconProps) -> Html {
    let title = props.title.clone();

    html! {
        <span class={classes!("icon", props.name.clone())} {title} />
    }
}
