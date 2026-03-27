#[cfg(feature = "bundle")]
mod bundle;
mod imaging;
mod nonbundle;
mod ws;

use std::future::Future;
use std::net::SocketAddr;

use anyhow::{Context as _, Result};
use api::{
    GetObjectSettingsRequest, GetObjectSettingsResponse, Id, Image, InitializeImageUploadResponse,
    InitializeMapResponse, InitializeRoomsResponse, Key, PeerId, Properties, RemoteId,
    RemoteObject, RemotePeer, Type, UploadImageRequest, Value,
};
use axum::extract::Path;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Extension, Router};
use tokio::net::TcpListener;
use tokio::task;
use tower_http::cors::{AllowMethods, AllowOrigin, CorsLayer};

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

const DEV_PORT: u16 = 44614;
const TRUNK_PORT: u16 = 8080;

pub(crate) fn default_bind(dev: bool, bind: &str) -> Result<(&str, u16, u16)> {
    let port: u16;
    let open_port: u16;

    let host = if let Some((host, port_s)) = bind.rsplit_once(':') {
        port = port_s.parse().context("port number")?;
        open_port = if dev { TRUNK_PORT } else { port };
        host
    } else {
        port = if dev { DEV_PORT } else { DEFAULT_PORT };
        open_port = if dev { TRUNK_PORT } else { port };
        bind
    };

    if dev {
        return Ok(("127.0.0.1", port, open_port));
    }

    Ok((host, port, open_port))
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
    let router = router.route("/api/image/{peer_id}/{id}", get(image));
    router
}

async fn image(
    Extension(backend): Extension<Backend>,
    Path((peer_id, id)): Path<(PeerId, Id)>,
) -> Result<impl IntoResponse, WebError> {
    const MIME: mime_guess::Mime = mime_guess::mime::IMAGE_PNG;

    let id = RemoteId::new(peer_id, id);

    let images = backend.read_images().await;

    let Some(data) = images.get(&id) else {
        return Err(WebError::not_found());
    };

    Ok(([(header::CONTENT_TYPE, MIME.as_ref())], data.to_vec()))
}

async fn initialize_map(b: &Backend) -> Result<InitializeMapResponse> {
    let mut objects = Vec::new();
    let mut images = Vec::new();
    let mut peers = Vec::new();
    let mut peer_objects = Vec::new();

    let state = b.client_state().await;

    for (id, object) in state.objects.iter() {
        objects.push(RemoteObject {
            ty: object.ty,
            id: *id,
            props: object.props.clone(),
        });
    }

    for id in state.images.keys() {
        images.push(RemoteId::local(*id));
    }

    for (peer_id, peer) in state.peers.iter() {
        peers.push(RemotePeer {
            id: *peer_id,
            public_key: peer.public_key,
            props: peer.props.clone(),
        });

        for object in peer.objects.values() {
            peer_objects.push((*peer_id, object.clone()));
        }

        for id in peer.images.iter() {
            images.push(RemoteId::new(*peer_id, *id));
        }
    }

    let res = InitializeMapResponse {
        public_key: state.keypair.public_key(),
        props: state.props.clone(),
        objects,
        images,
        peers,
        peer_objects,
    };

    Ok(res)
}

async fn initialize_rooms(b: &Backend) -> Result<InitializeRoomsResponse> {
    let mut local = Vec::new();
    let mut peers = Vec::new();
    let mut peer_objects = Vec::new();

    let state = b.client_state().await;

    for (id, object) in state.objects.iter() {
        if object.ty != Type::ROOM {
            continue;
        }

        local.push(RemoteObject {
            ty: object.ty,
            id: *id,
            props: object.props.clone(),
        });
    }

    for (peer_id, peer) in state.peers.iter() {
        peers.push(RemotePeer {
            id: *peer_id,
            public_key: peer.public_key,
            props: peer.props.clone(),
        });

        for object in peer.objects.values() {
            if object.ty != Type::ROOM {
                continue;
            }

            peer_objects.push((*peer_id, object.clone()));
        }
    }

    let res = InitializeRoomsResponse {
        public_key: state.keypair.public_key(),
        props: state.props.clone(),
        local,
        peers,
        peer_objects,
    };

    Ok(res)
}

async fn upload_image(backend: &Backend, request: UploadImageRequest) -> Result<Image> {
    tracing::info!(?request.content_type, size = request.data.len(), "received image upload request");

    let task = task::spawn_blocking(move || {
        imaging::process(&request.data, request.crop, request.sizing, request.size)
    });

    let (content_type, width, height, bytes) = task.await??;

    let id = backend
        .insert_image(content_type, request.role, width, height, bytes)
        .await?;

    Ok(Image {
        id: RemoteId::local(id),
        content_type,
        role: request.role,
        width,
        height,
    })
}

async fn remove_image(backend: &Backend, id: Id) -> Result<()> {
    backend.remove_image(id).await?;
    Ok(())
}

async fn get_config(backend: &Backend) -> Result<Properties> {
    let mut props = Properties::new();

    for (key, value) in backend.db().properties(Id::ZERO).await? {
        props.insert(key, value);
    }

    Ok(props)
}

async fn get_object_settings(
    backend: &Backend,
    request: GetObjectSettingsRequest,
) -> Result<GetObjectSettingsResponse> {
    let state = backend.client_state().await;

    let public_key = state.keypair.public_key();

    let object = state.objects.get(&request.id).context("object not found")?;

    let object = RemoteObject {
        ty: object.ty,
        id: object.id,
        props: object.props.clone(),
    };

    Ok(GetObjectSettingsResponse { object, public_key })
}

async fn initialize_image_upload(backend: &Backend) -> Result<InitializeImageUploadResponse> {
    let state = backend.client_state().await;

    let mut images = Vec::new();

    for image in state.images.values() {
        images.push(Image {
            id: RemoteId::local(image.id),
            content_type: image.content_type,
            role: image.role,
            width: image.width,
            height: image.height,
        });
    }

    Ok(InitializeImageUploadResponse { images })
}

async fn object_update(backend: &Backend, object_id: Id, key: Key, value: &Value) -> Result<()> {
    match key {
        Key::TRANSFORM => 'done: {
            let Some(transform) = value.as_transform() else {
                break 'done;
            };

            if backend.mumble_object() != object_id {
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
            let hidden = value.as_bool();

            backend.set_hidden(object_id, hidden);

            'out: {
                if backend.mumble_object() != object_id {
                    break 'out;
                }

                let state = backend.client_state().await;

                let Some(object) = state.objects.get(&object_id) else {
                    return Ok(());
                };

                let transform = if hidden {
                    None
                } else {
                    object.props.get(Key::TRANSFORM).as_transform()
                };

                backend.set_mumblelink_transform(transform).await;
            }
        }
        _ => {}
    }

    backend.object_update(object_id, key, value.clone()).await;
    Ok(())
}
