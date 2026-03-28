use core::fmt;
use core::num::ParseIntError;
use core::str::Utf8Error;

/// Errors raised in this application.
pub struct Error {
    error: anyhow::Error,
}

impl Error {
    #[inline]
    pub(crate) fn message(message: impl fmt::Display + fmt::Debug + Send + Sync + 'static) -> Self {
        Self {
            error: anyhow::Error::msg(message),
        }
    }

    #[inline]
    pub(crate) fn into_inner(self) -> anyhow::Error {
        self.error
    }
}

impl fmt::Display for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl fmt::Debug for Error {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl From<anyhow::Error> for Error {
    #[inline]
    fn from(error: anyhow::Error) -> Self {
        Self { error }
    }
}

impl From<ParseIntError> for Error {
    #[inline]
    fn from(error: ParseIntError) -> Self {
        Self {
            error: anyhow::Error::from(error),
        }
    }
}

impl From<&'static str> for Error {
    #[inline]
    fn from(value: &'static str) -> Self {
        Self {
            error: anyhow::Error::msg(value),
        }
    }
}

impl From<wasm_bindgen::JsValue> for Error {
    #[inline]
    fn from(value: wasm_bindgen::JsValue) -> Self {
        Self {
            error: anyhow::Error::msg(format!("{:?}", value)),
        }
    }
}

impl From<url::ParseError> for Error {
    #[inline]
    fn from(error: url::ParseError) -> Self {
        Self {
            error: anyhow::Error::from(error),
        }
    }
}

impl From<musli_web::web::Error> for Error {
    #[inline]
    fn from(error: musli_web::web::Error) -> Self {
        Self {
            error: anyhow::Error::from(error),
        }
    }
}

impl From<Utf8Error> for Error {
    #[inline]
    fn from(error: Utf8Error) -> Self {
        Self {
            error: anyhow::Error::from(error),
        }
    }
}
