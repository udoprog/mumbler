#[cfg(feature = "bundle")]
mod bundle;
mod imaging;
mod nonbundle;

mod ws;

use crate::backend::Backend;
use crate::remote::DEFAULT_PORT;

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

use anyhow::{Context as _, Result};
use api::{Id, Key, Properties, Value};
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Extension, Router};
use tokio::net::TcpListener;
use tokio::task;
use tower_http::cors::{AllowMethods, AllowOrigin, CorsLayer};

pub(crate) fn default_bind(dev: bool, bind: &str) -> Result<(&str, u16)> {
    let port: u16;

    let host = if let Some((host, port_s)) = bind.rsplit_once(':') {
        port = port_s.parse().context("port number")?;
        host
    } else {
        port = if dev { 8080 } else { DEFAULT_PORT };
        bind
    };

    if dev {
        return Ok(("127.0.0.1", port));
    }

    Ok((host, port))
}

pub(crate) fn setup(
    listener: TcpListener,
    backend: Backend,
    dev: bool,
) -> Result<impl Future<Output = Result<()>>> {
    let cors = CorsLayer::new()
        .allow_origin(AllowOrigin::any())
        .allow_methods(AllowMethods::any());

    let app = match dev {
        true => self::nonbundle::router,
        #[cfg(feature = "bundle")]
        _ => self::bundle::router,
        #[cfg(not(feature = "bundle"))]
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
    let mut objects = Vec::new();
    let mut remote_avatars = Vec::new();

    {
        let state = b.client_state().await;

        for (id, object) in state.objects.iter() {
            objects.push(api::RemoteObject {
                id: *id,
                properties: object.properties.clone(),
            });
        }

        for (peer_id, peer) in state.peers.iter() {
            for object in peer.objects.values() {
                remote_avatars.push(api::RemotePeerObject {
                    peer_id: *peer_id,
                    object: api::RemoteObject {
                        id: object.id,
                        properties: object.properties.clone(),
                    },
                });
            }
        }
    }

    let mut config = Properties::new();

    for (key, value) in b.db().configs().await? {
        config.insert(key, value);
    }

    let ev = api::InitializeMapEvent {
        objects,
        remote_avatars,
        config,
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

async fn get_config(backend: &Backend) -> Result<Properties> {
    let mut properties = Properties::new();

    for (key, value) in backend.db().configs().await? {
        properties.insert(key, value);
    }

    Ok(properties)
}

async fn get_object_settings(
    backend: &Backend,
    request: api::GetObjectSettingsRequest,
) -> Result<api::GetObjectSettingsResponse> {
    let object = {
        let state = backend.client_state().await;

        state.objects.get(&request.id).map(|o| api::RemoteObject {
            id: request.id,
            properties: o.properties.clone(),
        })
    };

    let images = backend.db().list_images().await?;
    Ok(api::GetObjectSettingsResponse { object, images })
}

async fn update(backend: &Backend, object_id: Id, key: Key, value: &Value) -> Result<()> {
    match key {
        Key::TRANSFORM => 'done: {
            let Some(transform) = value.as_transform() else {
                break 'done;
            };

            if backend.mumble_object() != Some(object_id) {
                break 'done;
            };

            let transform = if backend.is_hidden(object_id) {
                None
            } else {
                Some(transform)
            };

            backend.set_mumblelink_transform(transform).await;
        }
        Key::HIDDEN => {
            let hidden = value.as_bool().unwrap_or_default();
            backend.set_hidden(object_id, hidden);

            'out: {
                if backend.mumble_object() != Some(object_id) {
                    break 'out;
                }

                let state = backend.client_state().await;

                let Some(object) = state.objects.get(&object_id) else {
                    return Ok(());
                };

                let transform = if hidden {
                    None
                } else {
                    object.properties.get(Key::TRANSFORM).as_transform()
                };

                backend.set_mumblelink_transform(transform).await;
            }
        }
        _ => {}
    }

    backend.set_client(object_id, key, value.clone()).await;
    Ok(())
}

async fn update_config(
    backend: &Backend,
    values: impl IntoIterator<Item = (Key, Value)>,
) -> Result<()> {
    let mut restart_mumblelink = false;
    let mut restart_client = false;

    for (key, value) in values {
        match key {
            Key::MUMBLE_ENABLED => {
                restart_mumblelink = true;
            }
            Key::REMOTE_ENABLED | Key::REMOTE_SERVER | Key::REMOTE_TLS => {
                restart_client = true;
            }
            Key::MUMBLE_OBJECT => {
                let mumble_object = value.as_id();
                backend.store_mumble_object(mumble_object);

                let transform = 'transform: {
                    let Some(object_id) = mumble_object else {
                        break 'transform None;
                    };

                    let state = backend.client_state().await;

                    let Some(object) = state.objects.get(&object_id) else {
                        break 'transform None;
                    };

                    if object
                        .properties
                        .get(Key::HIDDEN)
                        .as_bool()
                        .unwrap_or_default()
                    {
                        None
                    } else {
                        object.properties.get(Key::TRANSFORM).as_transform()
                    }
                };

                backend.set_mumblelink_transform(transform).await;
            }
            _ => {}
        }

        backend.db().set_config_value(key, value).await?;
    }

    if restart_mumblelink {
        backend.restart_mumblelink();
    }

    if restart_client {
        backend.restart_client();
    }

    Ok(())
}

async fn get_mumble_status(backend: &Backend) -> Result<api::GetMumbleStatusResponse> {
    let enabled = backend
        .db()
        .config::<bool>(Key::MUMBLE_ENABLED)
        .await?
        .unwrap_or(false);

    Ok(api::GetMumbleStatusResponse { enabled })
}

async fn get_remote_status(backend: &Backend) -> Result<api::GetRemoteStatusResponse> {
    let enabled = backend
        .db()
        .config::<bool>(Key::REMOTE_ENABLED)
        .await?
        .unwrap_or(true);

    let server = backend.db().config::<String>(Key::REMOTE_SERVER).await?;
    Ok(api::GetRemoteStatusResponse { enabled, server })
}
