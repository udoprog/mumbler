#[cfg(feature = "bundle")]
mod bundle;
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
use api::{Avatar, AvatarId, Vec3};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::{Extension, Router};
use tokio::net::TcpListener;
use tokio::sync::RwLock;
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
    service: Arc<RwLock<Backend>>,
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

    let app = app().layer(Extension(service)).layer(cors);

    let service = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    );

    Ok(async move {
        service.await?;
        Ok(())
    })
}

fn common_routes(router: Router) -> Router {
    let router = router.route("/ws", get(ws::entry));
    router
}

fn initialize(_: &Backend) -> api::InitializeEvent {
    api::InitializeEvent {
        name: Some("Gilbert".to_owned()),
        avatars: vec![
            Avatar {
                id: AvatarId::new(0),
                position: Vec3::new(0.0, 0.0, -1.0),
                front: Vec3::FORWARD,
            },
            Avatar {
                id: AvatarId::new(1),
                position: Vec3::new(0.0, 0.0, 1.0),
                front: Vec3::FORWARD,
            },
        ],
        world: api::World {
            zoom: 1.0,
            width: 100.0,
            height: 100.0,
            token_radius: 1.0,
            player: AvatarId::new(0),
        },
    }
}
