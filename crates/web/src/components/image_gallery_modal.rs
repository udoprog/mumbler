use api::Id;
use web_sys::MouseEvent;
use yew::prelude::*;

use super::Icon;

pub(crate) enum Msg {
    Select(Id),
    Close,
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    /// All available images.
    pub(crate) images: Vec<api::Image>,
    /// Currently selected image, if any.
    #[prop_or_default]
    pub(crate) selected: Option<Id>,
    /// Callback fired when an image is selected.
    pub(crate) on_select: Callback<Id>,
    /// Callback fired when an image should be deleted.
    pub(crate) on_delete: Callback<Id>,
    /// Callback fired to close the modal.
    pub(crate) on_close: Callback<()>,
}

pub(crate) struct ImageGalleryModal;

impl Component for ImageGalleryModal {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Select(id) => {
                ctx.props().on_select.emit(id);
                ctx.props().on_close.emit(());
                false
            }
            Msg::Close => {
                ctx.props().on_close.emit(());
                false
            }
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        let images = ctx.props().images.iter().map(|image| {
            let id = image.id;
            let on_select = ctx.link().callback(move |_| Msg::Select(id));
            let on_delete = {
                let on_delete = ctx.props().on_delete.clone();
                Callback::from(move |e: MouseEvent| {
                    e.stop_propagation();
                    on_delete.emit(id);
                })
            };
            let classes = classes!(
                "token",
                (ctx.props().selected == Some(id)).then_some("selected"),
                "clickable"
            );

            html! {
                <div class="image-entry">
                    <img src={format!("/api/image/{}", image.id)} alt={format!("Image {}", image.id)} onclick={on_select} class={classes} />
                    <button class="btn danger floating icon" onclick={on_delete} title="Remove image">{"ⓧ"}</button>
                </div>
            }
        });

        html! {
            <div class="modal-backdrop" onclick={ctx.link().callback(|_| Msg::Close)}>
                <div class="modal" onclick={|e: MouseEvent| e.stop_propagation()}>
                    <div class="modal-header">
                        <h2>{"Select Image"}</h2>
                        <button class="btn sm square danger" title="Close"
                            onclick={ctx.link().callback(|_| Msg::Close)}>
                            <Icon name="x-mark" />
                        </button>
                    </div>
                    <div class="modal-body">
                        if ctx.props().images.is_empty() {
                            <p class="hint">{"No images uploaded yet."}</p>
                        } else {
                            <div class="gallery">
                                {for images}
                            </div>
                        }
                    </div>
                </div>
            </div>
        }
    }
}
