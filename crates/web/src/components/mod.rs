mod map;
pub(crate) use self::map::Map;

mod settings;
pub(crate) use self::settings::Settings;

mod mumble_status;
pub(crate) use self::mumble_status::MumbleStatus;

use yew::prelude::*;

#[derive(Properties, PartialEq)]
pub struct IconProps {
    pub name: String,
}

#[function_component(Icon)]
fn icon(props: &IconProps) -> Html {
    html! {
        <span class={classes!("icon", props.name.clone())} />
    }
}
