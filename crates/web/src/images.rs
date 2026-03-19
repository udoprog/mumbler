use core::marker::PhantomData;
use std::collections::HashMap;

use api::Id;
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::HtmlImageElement;
use yew::{Component, Context};

use crate::error::Error;

pub(crate) enum ImageMessage {
    Loaded(Id),
    Errored(Id, Error),
}

struct ImageState {
    image: HtmlImageElement,
    load: Option<Closure<dyn FnMut()>>,
    error: Option<Closure<dyn FnMut()>>,
    users: usize,
}

/// Collection of images loaded for rendering.
pub(crate) struct Images<M> {
    inner: HashMap<Id, ImageState>,
    _marker: PhantomData<M>,
}

impl<M> Images<M>
where
    M: Component<Message: From<ImageMessage>>,
{
    /// Construct a new image.
    pub(crate) fn new() -> Self {
        Self {
            inner: HashMap::new(),
            _marker: PhantomData,
        }
    }

    pub(crate) fn update(&mut self, msg: ImageMessage) {
        match msg {
            ImageMessage::Loaded(id) => {
                if let Some(s) = self.inner.get_mut(&id) {
                    s.load = None;
                    s.error = None;
                }
            }
            ImageMessage::Errored(id, error) => {
                tracing::error!(?id, %error, "loading image");
                self.inner.remove(&id);
            }
        }
    }

    /// Remove a loaded image.
    pub(crate) fn remove(&mut self, id: Id) {
        if id.is_zero() {
            return;
        }

        if let Some(state) = self.inner.get_mut(&id) {
            state.users = state.users.saturating_sub(1);

            if state.users == 0 {
                self.inner.remove(&id);
            }
        }
    }

    /// Clear all loaded images.
    pub(crate) fn clear(&mut self) {
        self.inner.clear();
    }

    pub(crate) fn load(&mut self, ctx: &Context<M>, id: Id) {
        if id.is_zero() {
            return;
        }

        if let Some(state) = self.inner.get_mut(&id) {
            state.users = state.users.saturating_add(1);
            return;
        }

        match Self::load_image(ctx, id) {
            Ok(state) => {
                self.inner.insert(id, state);
            }
            Err(error) => {
                ctx.link()
                    .send_message(M::Message::from(ImageMessage::Errored(id, error)));
            }
        }
    }

    /// Get an image by id.
    pub(crate) fn get(&self, id: Id) -> Option<&HtmlImageElement> {
        let state = self.inner.get(&id)?;

        // Still loading.
        if state.load.is_some() {
            return None;
        }

        // Failed to load.
        if !state.image.complete() || state.image.natural_width() == 0 {
            return None;
        }

        Some(&state.image)
    }

    fn load_image(ctx: &Context<M>, id: Id) -> Result<ImageState, Error> {
        let img = HtmlImageElement::new()?;

        let load = Closure::<dyn FnMut()>::new({
            let link = ctx.link().clone();

            move || {
                link.send_message(M::Message::from(ImageMessage::Loaded(id)));
            }
        });

        let error = Closure::<dyn FnMut()>::new({
            let link = ctx.link().clone();

            move || {
                link.send_message(M::Message::from(ImageMessage::Errored(
                    id,
                    Error::from("loading image"),
                )));
            }
        });

        img.set_onload(Some(load.as_ref().unchecked_ref()));
        img.set_onerror(Some(error.as_ref().unchecked_ref()));
        img.set_src(&format!("/api/image/{id}"));

        Ok(ImageState {
            image: img,
            load: Some(load),
            error: Some(error),
            users: 1,
        })
    }
}
