mod map;
pub(crate) use self::map::Map;

pub(crate) mod render;

mod settings;
pub(crate) use self::settings::Settings;

mod mumble_status;
pub(crate) use self::mumble_status::MumbleStatus;

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
