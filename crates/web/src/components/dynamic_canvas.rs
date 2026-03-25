use wasm_bindgen::prelude::*;
use web_sys::{HtmlCanvasElement, HtmlElement, ResizeObserver};
use yew::prelude::*;

use crate::error::Error;

pub(crate) enum Msg {
    Resized,
}

#[derive(Properties, PartialEq)]
pub(crate) struct DynamicCanvasProps {
    pub(crate) onload: Callback<HtmlCanvasElement>,
    #[prop_or_default]
    pub(crate) onerror: Callback<Error>,
    #[prop_or_default]
    pub(crate) onresize: Callback<(i32, i32)>,
}

pub(crate) struct DynamicCanvas {
    canvas_container: NodeRef,
    canvas_ref: NodeRef,
    dimensions: Option<(i32, i32)>,
    _resize_observer: Option<(ResizeObserver, Closure<dyn FnMut()>)>,
}

impl Component for DynamicCanvas {
    type Message = Msg;
    type Properties = DynamicCanvasProps;

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
                let mut changed = false;

                if let Some(container) = self.canvas_container.cast::<HtmlElement>() {
                    let width = container.client_width();
                    let height = container.client_height();

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

        false
    }

    fn changed(&mut self, ctx: &Context<Self>, old_props: &Self::Properties) -> bool {
        if old_props.onload != ctx.props().onload
            && let Some(canvas) = self.canvas_ref.cast::<HtmlCanvasElement>()
        {
            ctx.props().onload.emit(canvas);
        }

        if old_props.onresize != ctx.props().onresize
            && let Some(dimensions) = self.dimensions
        {
            ctx.props().onresize.emit(dimensions);
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
        }
    }

    fn view(&self, _ctx: &Context<Self>) -> Html {
        html! {
            <div class="canvas-container" ref={self.canvas_container.clone()} key="canvas-container">
                <canvas ref={self.canvas_ref.clone()} width="200" height="200" />
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
}
