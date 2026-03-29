use core::cell::Cell;
use std::rc::{Rc, Weak};

use wasm_bindgen::prelude::*;
use web_sys::Window;
use yew::Callback;

use crate::error::Error;

struct Inner {
    window: Window,
    render_id: Cell<Option<i32>>,
    _closure: Closure<dyn Fn(JsValue)>,
    callback: Callback<Result<f64, Error>>,
}

/// Handle for [`request_animation_frame`].
pub struct AnimationFrame {
    _inner: Rc<Inner>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(render_id) = self.render_id.take()
            && let Err(error) = self.window.cancel_animation_frame(render_id)
        {
            self.callback.emit(Err(Error::from(error)));
        }
    }
}

impl AnimationFrame {
    pub(crate) fn request(window: Window, callback: Callback<Result<f64, Error>>) -> Self {
        let inner = Rc::new_cyclic(move |inner: &Weak<Inner>| {
            let closure: Closure<dyn Fn(JsValue)> = {
                let inner = inner.clone();

                Closure::wrap(Box::new(move |v: JsValue| {
                    let time: f64 = v.as_f64().unwrap_or(0.0);

                    if let Some(inner) = inner.upgrade()
                        && inner.render_id.take().is_some()
                    {
                        inner.callback.emit(Ok(time));
                    }
                }))
            };

            let render_id = window.request_animation_frame(closure.as_ref().unchecked_ref());

            let render_id = match render_id {
                Ok(render_id) => Some(render_id),
                Err(error) => {
                    callback.emit(Err(Error::from(error)));
                    None
                }
            };

            Inner {
                window,
                render_id: Cell::new(render_id),
                callback,
                _closure: closure,
            }
        });

        Self { _inner: inner }
    }
}
