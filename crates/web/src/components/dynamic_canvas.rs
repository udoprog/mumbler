use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, HtmlElement, PointerEvent, ResizeObserver};
use yew::prelude::*;

use crate::error::Error;

pub(crate) enum Msg {
    Resized,
}

#[derive(Properties, PartialEq)]
pub(crate) struct Props {
    #[prop_or_default]
    pub(crate) id: AttrValue,
    pub(crate) onload: Callback<HtmlCanvasElement>,
    pub(crate) onerror: Callback<Error>,
    #[prop_or_default]
    pub(crate) onresize: Callback<(u32, u32)>,
    #[prop_or_default]
    pub(crate) onpointerdown: Callback<PointerEvent>,
    #[prop_or_default]
    pub(crate) onpointermove: Callback<PointerEvent>,
    #[prop_or_default]
    pub(crate) onpointerup: Callback<PointerEvent>,
    #[prop_or_default]
    pub(crate) onpointerleave: Callback<PointerEvent>,
    #[prop_or_default]
    pub(crate) onwheel: Callback<WheelEvent>,
    #[prop_or_default]
    pub(crate) oncontextmenu: Callback<MouseEvent>,
    #[prop_or_default]
    pub(crate) ondragover: Callback<DragEvent>,
    #[prop_or_default]
    pub(crate) ondrop: Callback<DragEvent>,
    #[prop_or_default]
    pub(crate) children: Children,
}

pub(crate) struct DynamicCanvas {
    canvas_container: NodeRef,
    canvas_ref: NodeRef,
    dimensions: Option<(u32, u32)>,
    _resize_observer: Option<(ResizeObserver, Closure<dyn FnMut()>)>,
}

impl Component for DynamicCanvas {
    type Message = Msg;
    type Properties = Props;

    fn create(_ctx: &Context<Self>) -> Self {
        Self {
            canvas_container: NodeRef::default(),
            canvas_ref: NodeRef::default(),
            dimensions: None,
            _resize_observer: None,
        }
    }

    fn update(&mut self, ctx: &Context<Self>, msg: Self::Message) -> bool {
        match msg {
            Msg::Resized => {
                self.refresh(ctx);
            }
        }

        false
    }

    fn rendered(&mut self, ctx: &Context<Self>, first_render: bool) {
        if first_render {
            if let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>() {
                ctx.props().onload.emit(canvas);
            }

            if let Err(error) = self.setup_resize_observer(ctx) {
                ctx.props().onerror.emit(error);
            }

            self.refresh(ctx);
        }
    }

    fn view(&self, ctx: &Context<Self>) -> Html {
        tracing::warn!("dynamic render");

        let Props {
            id,
            onpointerdown,
            onpointermove,
            onpointerup,
            onpointerleave,
            onwheel,
            oncontextmenu,
            ondragover,
            ondrop,
            children,
            ..
        } = ctx.props();

        html! {
            <div
                {id}
                class="canvas-container"
                ref={self.canvas_container.clone()}
                {onpointerdown}
                {onpointermove}
                {onpointerup}
                {onpointerleave}
                {onwheel}
                {oncontextmenu}
                {ondragover}
                {ondrop}
            >
                <canvas key="canvas" ref={self.canvas_ref.clone()} width="200" height="200" />

                {children}
            </div>
        }
    }
}

impl DynamicCanvas {
    fn setup_resize_observer(&mut self, ctx: &Context<Self>) -> Result<(), Error> {
        let Some(container) = self.canvas_container.cast::<HtmlElement>() else {
            return Ok(());
        };

        let link = ctx.link().clone();

        let closure = Closure::<dyn FnMut()>::new(move || {
            link.send_message(Msg::Resized);
        });

        let observer = ResizeObserver::new(closure.as_ref().unchecked_ref())?;

        observer.observe(&container);

        if let Some((o, _closure)) = self._resize_observer.replace((observer, closure)) {
            o.disconnect();
            drop(_closure);
        }

        Ok(())
    }

    fn refresh(&mut self, ctx: &Context<DynamicCanvas>) {
        let mut changed = false;

        if let Some(container) = self.canvas_container.cast::<HtmlElement>() {
            let width = u32::try_from(container.client_width()).unwrap_or_default();
            let height = u32::try_from(container.client_height()).unwrap_or_default();

            changed = match self.dimensions {
                Some((w, h)) => w != width || h != height,
                None => true,
            };

            self.dimensions = Some((width, height));

            if changed {
                ctx.props().onresize.emit((width, height));
            }
        }

        if changed
            && let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>()
            && let Some((width, height)) = self.dimensions
        {
            canvas.set_width(width as u32);
            canvas.set_height(height as u32);
        }
    }
}
