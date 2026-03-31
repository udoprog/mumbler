use core::ops::Sub;

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

#[derive(Debug, Clone, Copy)]
struct Client2 {
    x: i32,
    y: i32,
}

impl Client2 {
    #[inline]
    fn client(ev: &PointerEvent) -> Self {
        Self {
            x: ev.client_x(),
            y: ev.client_y(),
        }
    }

    #[inline]
    fn style(&self) -> String {
        format!("left: {}px; top: {}px;", self.x, self.y)
    }
}

impl Sub for Client2 {
    type Output = Self;

    #[inline]
    fn sub(self, rhs: Self) -> Self {
        Self {
            x: self.x.saturating_sub(rhs.x),
            y: self.y.saturating_sub(rhs.y),
        }
    }
}

#[component(Modal)]
pub(crate) fn modal(props: &Props) -> Html {
    let class = classes! {
        "modal-body",
        &props.class,
    };

    let style = use_state(|| format!(""));
    let anchor = use_state(|| Client2 { x: 0, y: 0 });
    let dragging = use_state(|| None::<Client2>);

    let onpointerdown = {
        let anchor = anchor.clone();
        let dragging = dragging.clone();

        Callback::from(move |ev: PointerEvent| {
            dragging.set(Some(Client2::client(&ev) - (*anchor)));
        })
    };

    let onpointermove = (*dragging).map(|dragging| {
        let style = style.clone();

        Callback::from(move |ev: PointerEvent| {
            ev.prevent_default();

            let offset = Client2::client(&ev) - dragging;
            style.set(offset.style());
        })
    });

    let onpointerup = (*dragging).map(|d| {
        let anchor = anchor.clone();
        let dragging = dragging.clone();

        Callback::from(move |ev: PointerEvent| {
            anchor.set(Client2::client(&ev) - d);
            dragging.set(None);
        })
    });

    let onclose = props.onclose.reform(|ev: MouseEvent| {
        ev.stop_propagation();
        ev
    });

    let modal_class = classes! {
        "modal",
        dragging.is_some().then_some("moving"),
    };

    html! {
        <div class="modal-backdrop" onclick={onclose.clone()} {onpointermove} {onpointerup}>
            <div class={modal_class} onclick={|ev: MouseEvent| ev.stop_propagation()} style={(*style).clone()}>
                <div class="modal-header" {onpointerdown}>
                    <h2>{&props.title}</h2>

                    <button class="btn square danger" title="Close" onclick={onclose}>
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
