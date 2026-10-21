#[cfg(feature = "bundle")]
mod bundle;
mod imaging;
mod nonbundle;
mod origin;
mod ws;

use std::future::Future;

use anyhow::{Context as _, Result};
use api::{
    GetObjectSettingsRequest, GetObjectSettingsResponse, Id, Image, InitializeImageUploadResponse,
    InitializeMapResponse, InitializeRoomsResponse, PeerId, Properties, RemoteId, RemoteObject,
    RemotePeer, Type, UploadImageRequest,
};
use auth::Auth;
use auth::axum08::SignedIn;
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::{StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Extension, Router, middleware};
use tokio::net::TcpListener;
use tokio::task;

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
    auth: Option<Auth>,
) -> Result<impl Future<Output = Result<()>>> {
    let app = match dev {
        true => self::nonbundle::router,
        #[cfg(feature = "bundle")]
        _ => self::bundle::router,
        #[cfg(not(feature = "bundle"))]
        _ => self::nonbundle::router,
    };

    let app = app()
        .merge(guarded_routes(origin::Guard::new(dev), auth))
        .layer(Extension(backend));

    let service = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<origin::Connection>(),
    );

    Ok(async move {
        service.await?;
        Ok(())
    })
}

fn guarded_routes(guard: origin::Guard, auth: Option<Auth>) -> Router {
    let mut router = Router::new()
        .route("/ws", get(ws::entry))
        .route("/api/image/{peer_id}/{id}", get(image));

    let access = if let Some(auth) = auth {
        router = router
            .route_layer(middleware::from_fn_with_state(
                auth.clone(),
                require_sign_in,
            ))
            .merge(auth::axum08::routes().with_state(auth));
        "sign-in"
    } else {
        "open"
    };

    router
        .route("/api/access", get(move || async move { access }))
        .route_layer(middleware::from_fn_with_state(guard, origin::check))
}

/// Lets only signed-in requests through, leaving the [`SignedIn`] and the
/// [`Auth`] in the request's extensions for the WebSocket.
async fn require_sign_in(State(auth): State<Auth>, request: Request, next: Next) -> Response {
    let (mut parts, body) = request.into_parts();

    let signed_in = match SignedIn::from_request_parts(&mut parts, &auth).await {
        Ok(signed_in) => signed_in,
        Err(error) => return error.into_response(),
    };

    parts.extensions.insert(signed_in);
    parts.extensions.insert(auth);
    next.run(Request::from_parts(parts, body)).await
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
        let image = imaging::process(&request.data, request.crop, request.sizing, request.size)?;
        Ok::<_, anyhow::Error>(image)
    });

    let result = task.await;

    let (content_type, width, height, bytes) = result??;

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
