use api::{Canvas2, RemoteId};
use yew::prelude::*;

use super::Icon;

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    pub(crate) position: Canvas2,
    pub(crate) object_id: RemoteId,
    pub(crate) is_hidden: bool,
    pub(crate) mumble_object: Option<RemoteId>,
    pub(crate) onclose: Callback<()>,
    pub(crate) onsettings: Callback<()>,
    pub(crate) onhidden: Callback<()>,
    pub(crate) onlocalhidden: Callback<()>,
    pub(crate) onmumbleobject: Callback<()>,
    pub(crate) onremove: Callback<()>,
}

#[function_component(ContextMenuDropdown)]
pub(crate) fn context_menu_dropdown(props: &Props) -> Html {
    let object_id = props.object_id;

    let style = format!("left: {}px; top: {}px;", props.position.x, props.position.y);

    let hidden_icon = if props.is_hidden { "eye" } else { "eye-slash" };
    let hidden_label = if props.is_hidden { "Show" } else { "Hide" };
    let local_hidden_label = if props.is_hidden {
        "Show locally"
    } else {
        "Hide locally"
    };

    let is_mumble = props.mumble_object == Some(object_id);

    let mumble_label = if is_mumble {
        "Unset MumbleLink"
    } else {
        "Set as MumbleLink"
    };

    let onsettings = props.onsettings.reform(move |ev: MouseEvent| {
        ev.prevent_default();
    });

    let onhidden = props.onhidden.reform(move |ev: MouseEvent| {
        ev.prevent_default();
    });

    let onlocalhidden = props.onlocalhidden.reform(move |ev: MouseEvent| {
        ev.prevent_default();
    });

    let onmumbleobject = props.onmumbleobject.reform(move |ev: MouseEvent| {
        ev.prevent_default();
    });

    let onremove = props.onremove.reform(move |ev: MouseEvent| {
        ev.prevent_default();
    });

    let onclick = |ev: MouseEvent| ev.stop_propagation();

    html! {
        <div key="context-menu" class="context-menu-backdrop" onclick={props.onclose.reform(|_| ())}>
            <div class="context-menu" {style} {onclick}>
                <button class="context-menu-item" onclick={onsettings}>
                    <Icon name="cog" invert={true} />
                    {"Settings"}
                </button>
                <button class="context-menu-item" onclick={onhidden}>
                    <Icon name={hidden_icon} invert={true} />
                    {hidden_label}
                </button>
                <button class="context-menu-item" onclick={onlocalhidden}>
                    <Icon name="no-symbol" invert={true} />
                    {local_hidden_label}
                </button>
                <button class="context-menu-item" onclick={onmumbleobject}>
                    <Icon name="mumble" invert={true} />
                    {mumble_label}
                </button>
                <div class="context-menu-separator" />
                <button class="context-menu-item danger" onclick={onremove}>
                    <Icon name="x-mark" invert={true} />
                    {"Remove"}
                </button>
            </div>
        </div>
    }
}
