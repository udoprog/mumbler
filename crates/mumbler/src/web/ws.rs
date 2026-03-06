use core::error::Error as _;
use core::pin::pin;

use std::net::SocketAddr;

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
use crate::backend::BackendEvent;

struct Handler {
    backend: Backend,
}

impl Handler {
    fn new(backend: Backend) -> Self {
        Self { backend }
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

                self.backend
                    .set_position_front(request.avatar.position, request.avatar.front)
                    .await;
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
            api::Request::DeleteImage => {
                let request = incoming
                    .read::<api::DeleteImageRequest>()
                    .context("missing request")?;

                let response = super::delete_image(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::Unknown(id) => {
                anyhow::bail!("unknown request type: {id}");
            }
        }

        Ok(())
    }
}

pub(super) async fn entry(
    ws: WebSocketUpgrade,
    Extension(backend): Extension<Backend>,
    ConnectInfo(remote): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| {
        let mut events = backend.subscribe();

        let future = async move {
            tracing::info!("connected");

            let mut server = pin!(axum08::server(socket, Handler::new(backend)));

            loop {
                let (result, done) = tokio::select! {
                    result = server.as_mut().run() => {
                        (result, true)
                    }
                    event = events.recv() => {
                        let Ok(event) = event else {
                            tracing::error!("backend event stream error: {event:?}");
                            break;
                        };

                        tracing::info!(?event, "backend event");

                        let event = match event {
                            BackendEvent::RemoteLost => {
                                api::RemoteAvatarUpdateBody::RemoteLost
                            },
                            BackendEvent::Join { peer_id } => {
                                api::RemoteAvatarUpdateBody::Join { peer_id }
                            },
                            BackendEvent::Leave { peer_id } => {
                                api::RemoteAvatarUpdateBody::Leave { peer_id }
                            },
                            BackendEvent::Moved { peer_id, position, front } => {
                                api::RemoteAvatarUpdateBody::Move { peer_id, position, front }
                            },
                            BackendEvent::ImageUpdated { peer_id, image } => {
                                api::RemoteAvatarUpdateBody::ImageUpdated { peer_id, image }
                            }
                        };

                        (server.as_mut().broadcast(event), false)
                    }
                };

                if let Err(error) = result {
                    tracing::error!("{error}");

                    let mut source = error.source();

                    while let Some(cause) = source.take() {
                        tracing::error!("Caused by: {cause}");
                        source = cause.source();
                    }

                    break;
                }

                if done {
                    tracing::warn!("Websocket server future completed, shutting down connection");
                    break;
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
