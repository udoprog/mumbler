use yew::prelude::*;

use super::Icon;

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    #[prop_or_default]
    pub(crate) title: Html,
    pub(crate) onclose: Callback<MouseEvent>,
    #[prop_or_default]
    pub(crate) onclick: Callback<MouseEvent>,
    #[prop_or_default]
    pub(crate) class: Classes,
    pub(crate) children: Children,
}

#[component(Modal)]
pub(crate) fn modal(props: &Props) -> Html {
    let class = classes! {
        "modal-body",
        &props.class,
    };

    html! {
        <div class="modal-backdrop" onclick={&props.onclose}>
            <div class="modal" onclick={|ev: MouseEvent| ev.stop_propagation()}>
                <div class="modal-header">
                    <h2>{&props.title}</h2>
                    <button class="btn sm square danger" title="Close" onclick={&props.onclose}>
                        <Icon name="x-mark" />
                    </button>
                </div>
                <div {class} onclick={&props.onclick}>
                    { &props.children }
                </div>
            </div>
        </div>
    }
}
