use core::error::Error as _;
use core::pin::pin;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result, anyhow, bail};
use axum::Extension;
use axum::extract::ConnectInfo;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use musli_web::axum08;
use musli_web::ws;
use tokio::sync::RwLock;
use tracing::{Instrument, Level};

use crate::backend::Backend;

struct Handler {
    backend: Arc<RwLock<Backend>>,
}

impl Handler {
    fn new(service: Arc<RwLock<Backend>>) -> Self {
        Self { backend: service }
    }
}

impl ws::Handler for Handler {
    type Id = api::Request;
    type Response = Result<(), anyhow::Error>;

    async fn handle(
        &mut self,
        id: Self::Id,
        incoming: &mut ws::Incoming<'_>,
        outgoing: &mut ws::Outgoing<'_>,
    ) -> Self::Response {
        tracing::info!(?id);

        match id {
            api::Request::Initialize => {
                let backend = self.backend.read().await;
                outgoing.write(super::initialize(&backend));
            }
            api::Request::UpdateAvatars => {
                let request = incoming
                    .read::<api::UpdateAvatarsRequest>()
                    .context("missing request")?;
                tracing::info!(?request);
            }
            api::Request::UploadAvatar => {
                let request = incoming
                    .read::<api::UploadAvatarRequest>()
                    .context("missing request")?;
                tracing::info!(
                    bytes = request.data.len(),
                    content_type = %request.content_type,
                    "Avatar upload received"
                );
                outgoing.write(api::UploadAvatarResponse);
            }
        }

        Ok(())
    }
}

pub(super) async fn entry(
    ws: WebSocketUpgrade,
    Extension(service): Extension<Arc<RwLock<Backend>>>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let future = async move {
            tracing::info!("Client connected");

            let server = pin!(axum08::server(socket, Handler::new(service)));

            if let Err(error) = server.run().await {
                tracing::error!("{error}");

                let mut source = error.source();

                while let Some(cause) = source.take() {
                    tracing::error!("Caused by: {cause}");
                    source = cause.source();
                }
            }
        };

        let x_forwarded_host = headers
            .get("x-forwarded-host")
            .and_then(|v| v.to_str().ok());

        let host = headers.get("host").and_then(|v| v.to_str().ok());
        let host = x_forwarded_host.or(host);

        let span = tracing::span!(Level::INFO, "ws", ?remote, host);
        future.instrument(span)
    })
}
