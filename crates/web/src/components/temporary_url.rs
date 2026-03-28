use core::ops::Deref;

use std::rc::Rc;

use web_sys::{File, Url};
use yew::Callback;

use crate::error::Error;

struct Inner {
    url: String,
    onerror: Callback<Error>,
}

pub(crate) struct TemporaryUrl {
    inner: Rc<Inner>,
}

impl TemporaryUrl {
    #[inline]
    pub(crate) fn create(file: &File, onerror: Callback<Error>) -> Result<Self, Error> {
        let Ok(url) = Url::create_object_url_with_blob(file) else {
            return Err(Error::message("failed to create object url"));
        };

        Ok(Self {
            inner: Rc::new(Inner { url, onerror }),
        })
    }
}

impl Deref for TemporaryUrl {
    type Target = str;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.inner.url
    }
}

impl Drop for Inner {
    #[inline]
    fn drop(&mut self) {
        if let Err(error) = Url::revoke_object_url(&self.url) {
            self.onerror.emit(error.into());
        }
    }
}
