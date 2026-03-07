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
use api::Id;
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
            transform: state.player.transform,
            look_at: state.player.look_at,
            image: state.player.image,
            color: state.player.color.clone(),
            name: state.player.name.clone(),
        };

        for (id, peer) in state.peers.iter() {
            remote_avatars.push(api::RemoteAvatar {
                id: Id::new(id.get()),
                transform: peer.transform,
                image: peer.image,
                color: peer.color.clone(),
                look_at: peer.look_at,
                name: peer.name.clone(),
            });
        }
    }

    let zoom = b
        .db()
        .get_config::<f32>("world/zoom")
        .await?
        .unwrap_or(10.0);

    let pan = b
        .db()
        .get_config::<api::Pan>("world/pan")
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
    let remote_server = backend.db().get_config::<String>("remote/server").await?;

    Ok(api::ListSettingsResponse {
        selected: state.player.image,
        images,
        color: state.player.color,
        name: state.player.name.clone(),
        remote_server,
    })
}

async fn select_image(
    backend: &Backend,
    request: api::SelectImageRequest,
) -> Result<api::SelectImageResponse> {
    backend.db().set_config("avatar/image", request.id).await?;
    backend.set_client_image(Some(request.id)).await;
    Ok(api::SelectImageResponse { id: request.id })
}

async fn delete_image(
    backend: &Backend,
    request: api::DeleteImageRequest,
) -> Result<api::DeleteImageResponse> {
    backend.db().delete_image(request.id).await?;
    Ok(api::DeleteImageResponse)
}

async fn select_color(
    backend: &Backend,
    request: api::SelectColorRequest,
) -> Result<api::SelectColorResponse> {
    let color = request.color;
    backend.db().set_config("avatar/color", color).await?;
    backend.set_client_color(color).await;
    Ok(api::SelectColorResponse { color })
}

async fn update_name(
    backend: &Backend,
    request: api::UpdateNameRequest,
) -> Result<api::UpdateNameResponse> {
    let name = request.name;
    backend
        .db()
        .set_optional_config("avatar/name", name.clone())
        .await?;
    backend.set_client_name(name.clone()).await;
    Ok(api::UpdateNameResponse { name })
}

async fn update_world(
    backend: &Backend,
    request: api::UpdateWorldRequest,
) -> Result<api::UpdateWorldResponse> {
    backend.db().set_config("world/pan", request.pan).await?;
    backend.db().set_config("world/zoom", request.zoom).await?;
    Ok(api::UpdateWorldResponse)
}

async fn mumble_restart(
    backend: &Backend,
    _request: api::MumbleRestartRequest,
) -> Result<api::MumbleRestartResponse> {
    backend.restart_mumblelink().await;
    Ok(api::MumbleRestartResponse)
}

async fn mumble_toggle(
    backend: &Backend,
    request: api::MumbleToggleRequest,
) -> Result<api::MumbleToggleResponse> {
    backend
        .db()
        .set_config("mumble/enabled", request.enabled)
        .await?;
    backend.set_mumblelink_enabled(request.enabled).await;
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
        .get_config::<bool>("mumble/enabled")
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
        .get_config::<bool>("remote/enabled")
        .await?
        .unwrap_or(true);
    let server = backend
        .db()
        .get_config::<String>("remote/server")
        .await?
        .unwrap_or_else(|| "127.0.0.1:44114".to_string());
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
        .set_config("remote/enabled", request.enabled)
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
    let server = request.server;
    backend
        .db()
        .set_config("remote/server", server.clone())
        .await?;
    backend.restart_client();
    Ok(api::SetRemoteServerResponse { server })
}
