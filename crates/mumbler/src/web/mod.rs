#[cfg(feature = "bundle")]
mod bundle;
mod imaging;
mod nonbundle;

mod ws;

use crate::backend::Backend;

/// Error type for web module.
pub struct WebError {
    kind: WebErrorKind,
}

impl WebError {
    fn not_found() -> Self {
        Self {
            kind: WebErrorKind::NotFound,
        }
    }
}

impl IntoResponse for WebError {
    fn into_response(self) -> axum::response::Response {
        match self.kind {
            WebErrorKind::Error(err) => {
                let body = format!("Internal server error: {err}");
                (StatusCode::INTERNAL_SERVER_ERROR, body).into_response()
            }
            WebErrorKind::NotFound => (StatusCode::NOT_FOUND, "Not Found").into_response(),
        }
    }
}

enum WebErrorKind {
    Error(anyhow::Error),
    NotFound,
}

impl From<anyhow::Error> for WebError {
    #[inline]
    fn from(err: anyhow::Error) -> Self {
        Self {
            kind: WebErrorKind::Error(err),
        }
    }
}

use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Result, bail};
use api::{Id, Vec3};
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Extension, Router};
use tokio::net::TcpListener;
use tokio::task;
use tower_http::cors::{AllowMethods, AllowOrigin, CorsLayer};

pub(crate) fn default_bind(_bundle: bool) -> &'static str {
    #[cfg(feature = "bundle")]
    if _bundle {
        return "127.0.0.1:8080";
    }

    "127.0.0.1:44614"
}

pub(crate) fn setup(
    listener: TcpListener,
    backend: Arc<Backend>,
    bundle: bool,
) -> Result<impl Future<Output = Result<()>>> {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::any())
        .allow_methods(AllowMethods::any());

    let app = match bundle {
        #[cfg(feature = "bundle")]
        true => self::bundle::router,
        #[cfg(not(feature = "bundle"))]
        true => bail!("cannot setup, bundle feature not enabled and `--dev` is not specified"),
        _ => self::nonbundle::router,
    };

    let app = app().layer(Extension(backend)).layer(cors);

    let service = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    );

    Ok(async move {
        service.await?;
        Ok(())
    })
}

#[allow(clippy::let_and_return)]
fn common_routes(router: Router) -> Router {
    let router = router.route("/ws", get(ws::entry));
    let router = router.route("/api/image/{id}", get(image));
    router
}

async fn image(
    Extension(backend): Extension<Arc<Backend>>,
    Path(id): Path<Id>,
) -> Result<impl IntoResponse, WebError> {
    const MIME: mime_guess::Mime = mime_guess::mime::IMAGE_PNG;

    let data = backend.db().get_image_data(id).await?;

    let Some(data) = data else {
        return Err(WebError::not_found());
    };

    Ok(([(header::CONTENT_TYPE, MIME.as_ref())], data))
}

async fn initialize(backend: &Backend) -> Result<api::InitializeEvent> {
    let image = backend.db().get_config::<Id>("avatar/image").await?;

    let mut ev = api::InitializeEvent {
        player: api::Avatar {
            id: Id::new(0),
            position: Vec3::ZERO,
            front: Vec3::FORWARD,
            image,
        },
        name: Some("Gilbert".to_owned()),
        avatars: Vec::new(),
        world: api::World {
            zoom: 10.0,
            extent: api::Extent2 {
                x: api::Span {
                    start: -50.0,
                    end: 50.0,
                },
                y: api::Span {
                    start: -50.0,
                    end: 50.0,
                },
            },
            token_radius: 0.5,
        },
        images: Vec::new(),
    };

    if let Some(id) = image {
        ev.images.extend(backend.db().get_image(id).await?);
    }

    Ok(ev)
}

async fn upload_image(
    backend: &Backend,
    request: api::UploadImageRequest,
) -> Result<api::UploadImageResponse> {
    tracing::info!(?request.content_type, size = request.data.len(), "Received image upload request");

    let task = task::spawn_blocking(move || imaging::process(&request.data, 128));

    let bytes = task.await??;
    let id = backend.db().save_image(128, 128, bytes).await?;
    Ok(api::UploadImageResponse { id })
}

async fn list_images(backend: &Backend) -> Result<api::ListSettingsResponse> {
    let selected = backend.db().get_config::<Id>("avatar/image").await?;
    let images = backend.db().list_images().await?;
    Ok(api::ListSettingsResponse { selected, images })
}

async fn select_image(
    backend: &Backend,
    request: api::SelectImageRequest,
) -> Result<api::SelectImageResponse> {
    backend.db().set_config("avatar/image", request.id).await?;
    Ok(api::SelectImageResponse { id: request.id })
}

async fn delete_image(
    backend: &Backend,
    request: api::DeleteImageRequest,
) -> Result<api::DeleteImageResponse> {
    backend.db().delete_image(request.id).await?;
    Ok(api::DeleteImageResponse)
}
