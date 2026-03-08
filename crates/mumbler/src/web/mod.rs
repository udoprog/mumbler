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

use anyhow::Result;
use api::{Id, Key};
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Extension, Router};
use tokio::net::TcpListener;
use tokio::task;
use tower_http::cors::{AllowMethods, AllowOrigin, CorsLayer};

pub(crate) fn default_bind(bundle: bool, bind: &str) -> &str {
    #[cfg(feature = "bundle")]
    if bundle {
        return bind;
    }

    #[cfg(not(feature = "bundle"))]
    {
        _ = bundle;
        _ = bind;
    }

    "127.0.0.1:44614"
}

pub(crate) fn setup(
    listener: TcpListener,
    backend: Backend,
    bundle: bool,
) -> Result<impl Future<Output = Result<()>>> {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::any())
        .allow_methods(AllowMethods::any());

    let app = match bundle {
        #[cfg(feature = "bundle")]
        true => self::bundle::router,
        #[cfg(not(feature = "bundle"))]
        true => {
            anyhow::bail!("cannot setup, bundle feature not enabled and `--dev` is not specified")
        }
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
    Extension(backend): Extension<Backend>,
    Path(id): Path<Id>,
) -> Result<impl IntoResponse, WebError> {
    const MIME: mime_guess::Mime = mime_guess::mime::IMAGE_PNG;

    {
        let images = backend.images_read().await;

        if let Some(data) = images.get(&id) {
            return Ok(([(header::CONTENT_TYPE, MIME.as_ref())], data.to_vec()));
        }
    }

    let data = backend.db().get_image_data(id).await?;

    let Some(data) = data else {
        return Err(WebError::not_found());
    };

    Ok(([(header::CONTENT_TYPE, MIME.as_ref())], data))
}

async fn initialize_map(b: &Backend) -> Result<api::InitializeMapEvent> {
    let player;
    let mut remote_avatars = Vec::new();

    {
        let state = b.client_state().await;

        player = api::Avatar {
            values: state.player.values.clone(),
        };

        for (id, peer) in state.peers.iter() {
            remote_avatars.push(api::RemoteAvatar {
                id: *id,
                values: peer.values.clone(),
            });
        }
    }

    let zoom = b
        .db()
        .get::<f32>(Id::GLOBAL, Key::WORLD_ZOOM)
        .await?
        .unwrap_or(10.0);

    let pan = b
        .db()
        .get::<api::Pan>(Id::GLOBAL, Key::WORLD_PAN)
        .await?
        .unwrap_or_else(api::Pan::zero);

    let ev = api::InitializeMapEvent {
        player,
        remote_avatars,
        world: api::World {
            zoom,
            pan,
            extent: api::Extent2::zero(),
            token_radius: 0.5,
        },
    };

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
    let images = backend.db().list_images().await?;
    let state = backend.client_state().await;
    let remote_server = backend
        .db()
        .get::<String>(Id::GLOBAL, Key::REMOTE_SERVER)
        .await?;
    let remote_server_tls = backend
        .db()
        .get::<bool>(Id::GLOBAL, Key::REMOTE_TLS)
        .await?
        .unwrap_or(false);

    Ok(api::ListSettingsResponse {
        images,
        image: state.player.image(),
        color: state.player.color(),
        name: state.player.name().map(str::to_owned),
        remote_server,
        remote_server_tls,
    })
}

async fn delete_image(
    backend: &Backend,
    request: api::DeleteImageRequest,
) -> Result<api::DeleteImageResponse> {
    backend.db().delete_image(request.id).await?;
    Ok(api::DeleteImageResponse)
}

async fn update_world(backend: &Backend, request: api::UpdateWorldRequest) -> Result<api::Empty> {
    backend
        .db()
        .set(Id::GLOBAL, Key::WORLD_PAN, request.pan)
        .await?;
    backend
        .db()
        .set(Id::GLOBAL, Key::WORLD_ZOOM, request.zoom)
        .await?;
    Ok(api::Empty)
}

async fn mumble_restart(
    backend: &Backend,
    _request: api::MumbleRestartRequest,
) -> Result<api::MumbleRestartResponse> {
    backend.restart_mumblelink();
    Ok(api::MumbleRestartResponse)
}

async fn mumble_toggle(
    backend: &Backend,
    request: api::MumbleToggleRequest,
) -> Result<api::MumbleToggleResponse> {
    backend
        .db()
        .set(Id::GLOBAL, Key::MUMBLE_ENABLED, request.enabled)
        .await?;
    backend.restart_mumblelink();
    Ok(api::MumbleToggleResponse {
        enabled: request.enabled,
    })
}

async fn get_mumble_status(
    backend: &Backend,
    _request: api::GetMumbleStatusRequest,
) -> Result<api::GetMumbleStatusResponse> {
    let enabled = backend
        .db()
        .get::<bool>(Id::GLOBAL, Key::MUMBLE_ENABLED)
        .await?
        .unwrap_or(false);

    Ok(api::GetMumbleStatusResponse { enabled })
}

async fn get_remote_status(
    backend: &Backend,
    _request: api::GetRemoteStatusRequest,
) -> Result<api::GetRemoteStatusResponse> {
    let enabled = backend
        .db()
        .get::<bool>(Id::GLOBAL, Key::REMOTE_ENABLED)
        .await?
        .unwrap_or(true);

    let server = backend
        .db()
        .get::<String>(Id::GLOBAL, Key::REMOTE_SERVER)
        .await?;

    Ok(api::GetRemoteStatusResponse { enabled, server })
}

async fn remote_restart(
    backend: &Backend,
    _request: api::RemoteRestartRequest,
) -> Result<api::RemoteRestartResponse> {
    backend.restart_client();
    Ok(api::RemoteRestartResponse)
}

async fn remote_toggle(
    backend: &Backend,
    request: api::RemoteToggleRequest,
) -> Result<api::RemoteToggleResponse> {
    backend
        .db()
        .set(Id::GLOBAL, Key::REMOTE_ENABLED, request.enabled)
        .await?;

    backend.restart_client();

    Ok(api::RemoteToggleResponse {
        enabled: request.enabled,
    })
}

async fn set_remote_server(
    backend: &Backend,
    request: api::SetRemoteServerRequest,
) -> Result<api::SetRemoteServerResponse> {
    backend
        .db()
        .set_optional(Id::GLOBAL, Key::REMOTE_SERVER, request.server.clone())
        .await?;
    backend
        .db()
        .set(Id::GLOBAL, Key::REMOTE_TLS, request.tls)
        .await?;

    backend.restart_client();

    Ok(api::SetRemoteServerResponse {
        server: request.server.clone(),
        tls: request.tls,
    })
}
