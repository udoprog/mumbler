use core::error::Error as _;
use core::pin::pin;

use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use axum::Extension;
use axum::extract::ConnectInfo;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use musli_web::axum08;
use musli_web::ws;
use tracing::{Instrument, Level};

use crate::backend::Backend;

struct Handler {
    backend: Arc<Backend>,
}

impl Handler {
    fn new(service: Arc<Backend>) -> Self {
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
                outgoing.write(super::initialize(&self.backend).await?);
            }
            api::Request::UpdatePlayer => {
                let request = incoming
                    .read::<api::UpdatePlayerRequest>()
                    .context("missing request")?;

                tracing::info!(?request);
            }
            api::Request::UploadImage => {
                let request = incoming
                    .read::<api::UploadImageRequest>()
                    .context("missing request")?;

                outgoing.write(super::upload_image(&self.backend, request).await?);
            }
            api::Request::ListSettings => {
                _ = incoming
                    .read::<api::ListSettingsRequest>()
                    .context("missing request")?;

                let response = super::list_images(&self.backend).await?;
                outgoing.write(response);
            }
            api::Request::SelectImage => {
                let request = incoming
                    .read::<api::SelectImageRequest>()
                    .context("missing request")?;

                let response = super::select_image(&self.backend, request).await?;
                outgoing.write(response);
            }
        }

        Ok(())
    }
}

pub(super) async fn entry(
    ws: WebSocketUpgrade,
    Extension(backend): Extension<Arc<Backend>>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let future = async move {
            tracing::info!("Client connected");

            let server = pin!(axum08::server(socket, Handler::new(backend)));

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
