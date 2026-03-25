use core::cell::RefCell;
use core::fmt;

use std::collections::HashMap;
use std::rc::Rc;

use api::{PeerId, RemoteId};
use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::HtmlImageElement;
use yew::Callback;

use crate::error::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum Icon {
    EyeSlashDanger,
}

impl fmt::Display for Icon {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Icon::EyeSlashDanger => write!(f, "eye-slash-danger"),
        }
    }
}

/// Collection of images loaded for rendering.
pub(crate) struct Images {
    inner: Rc<RefCell<Inner>>,
}

impl Images {
    /// Construct a new image.
    pub(crate) fn new(initial: Callback<Result<(), Error>>) -> Self {
        let this = Self {
            inner: Rc::new(RefCell::new(Inner::default())),
        };

        this.load_icon(Icon::EyeSlashDanger, initial.clone());
        this
    }

    /// Retain only images matching the given predicate.
    pub(crate) fn retain(&self, mut f: impl FnMut(PeerId) -> bool) {
        let mut inner = self.inner.borrow_mut();
        inner.ids.retain(|id, _| f(id.peer_id));
    }

    /// Remove a loaded image.
    pub(crate) fn remove(&self, id: &RemoteId) {
        if id.is_zero() {
            return;
        }

        let mut inner = self.inner.borrow_mut();
        inner.ids.remove(id);
    }

    /// Clear all loaded images.
    pub(crate) fn clear(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.ids.clear();
    }

    pub(crate) fn load_id(&self, id: &RemoteId, load: Callback<Result<(), Error>>) {
        if id.is_zero() {
            return;
        }

        match self.load_id_image(id) {
            Ok(mut state) => {
                state.load = load;
                let mut inner = self.inner.borrow_mut();
                inner.ids.insert(*id, state);
            }
            Err(error) => {
                load.emit(Err(error));
            }
        }
    }

    pub(crate) fn load_icon(&self, icon: Icon, load: Callback<Result<(), Error>>) {
        match self.load_icon_image(icon) {
            Ok(mut state) => {
                state.load = load;
                let mut inner = self.inner.borrow_mut();
                inner.icons.insert(icon, state);
            }
            Err(error) => {
                load.emit(Err(error));
            }
        }
    }

    /// Get an image by id.
    pub(crate) fn get_id(&self, id: &RemoteId) -> Option<HtmlImageElement> {
        let inner = self.inner.borrow();
        let state = inner.ids.get(id)?;

        if state._load.is_some() {
            return None;
        }

        let image = state.image.as_ref()?;

        if !image.complete() || image.natural_width() == 0 {
            return None;
        }

        Some(image.clone())
    }

    /// Get an icon by id.
    pub(crate) fn get_icon(&self, icon: Icon) -> Option<HtmlImageElement> {
        let inner = self.inner.borrow();
        let state = inner.icons.get(&icon)?;

        if state._load.is_some() {
            return None;
        }

        let image = state.image.as_ref()?;

        if !image.complete() || image.natural_width() == 0 {
            return None;
        }

        Some(image.clone())
    }

    fn load_id_image(&self, id: &RemoteId) -> Result<ImageState, Error> {
        let img = HtmlImageElement::new()?;

        let load = Closure::<dyn FnMut()>::new({
            let inner = Rc::downgrade(&self.inner);
            let id = *id;

            move || {
                if let Some(inner) = inner.upgrade() {
                    let mut inner = inner.borrow_mut();
                    inner.loaded_id(id);
                }
            }
        });

        let error = Closure::<dyn FnMut()>::new({
            let inner = Rc::downgrade(&self.inner);
            let id = *id;

            move || {
                if let Some(inner) = inner.upgrade() {
                    let mut inner = inner.borrow_mut();
                    inner.errored_id(id, Error::from("error loading image"));
                }
            }
        });

        img.set_onload(Some(load.as_ref().unchecked_ref()));
        img.set_onerror(Some(error.as_ref().unchecked_ref()));
        img.set_src(&format!("/api/image/{}/{}", id.peer_id, id.id));

        Ok(ImageState {
            image: Some(img),
            _load: Some(load),
            _error: Some(error),
            load: Callback::noop(),
        })
    }

    fn load_icon_image(&self, icon: Icon) -> Result<ImageState, Error> {
        let img = HtmlImageElement::new()?;

        let load = Closure::<dyn FnMut()>::new({
            let inner = Rc::downgrade(&self.inner);

            move || {
                if let Some(inner) = inner.upgrade() {
                    let mut inner = inner.borrow_mut();
                    inner.loaded_icon(icon);
                }
            }
        });

        let error = Closure::<dyn FnMut()>::new({
            let inner = Rc::downgrade(&self.inner);

            move || {
                if let Some(inner) = inner.upgrade() {
                    let mut inner = inner.borrow_mut();
                    inner.errored_icon(icon, Error::from("error loading image"));
                }
            }
        });

        img.set_onload(Some(load.as_ref().unchecked_ref()));
        img.set_onerror(Some(error.as_ref().unchecked_ref()));
        img.set_src(&format!("/static/icons/{icon}.svg"));

        Ok(ImageState {
            image: Some(img),
            _load: Some(load),
            _error: Some(error),
            load: Callback::noop(),
        })
    }
}

struct ImageState {
    image: Option<HtmlImageElement>,
    _load: Option<Closure<dyn FnMut()>>,
    _error: Option<Closure<dyn FnMut()>>,
    load: Callback<Result<(), Error>>,
}

#[derive(Default)]
struct Inner {
    ids: HashMap<RemoteId, ImageState>,
    icons: HashMap<Icon, ImageState>,
}

impl Inner {
    fn loaded_id(&mut self, id: RemoteId) {
        tracing::debug!(?id, "loaded");

        let Some(s) = self.ids.get_mut(&id) else {
            return;
        };

        s._load = None;
        s._error = None;
        s.load.emit(Ok(()));
    }

    fn errored_id(&mut self, id: RemoteId, error: Error) {
        tracing::error!(?id, %error);
        self.ids.remove(&id);
    }

    fn loaded_icon(&mut self, icon: Icon) {
        tracing::debug!(?icon, "loaded");

        if let Some(s) = self.icons.get_mut(&icon) {
            s._load = None;
            s._error = None;
        }
    }

    fn errored_icon(&mut self, icon: Icon, error: Error) {
        tracing::error!(?icon, %error);
        self.icons.remove(&icon);
    }
}
