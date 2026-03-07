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
use tokio::time::{self, Duration, Instant};
use tracing::{Instrument, Level};

use crate::backend::Backend;
use crate::backend::BackendEvent;

struct Handler {
    backend: Backend,
    update_transform: Option<api::Transform>,
    update_look_at: Option<Option<api::Vec3>>,
}

impl Handler {
    fn new(backend: Backend) -> Self {
        Self {
            backend,
            update_transform: None,
            update_look_at: None,
        }
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
        match id {
            api::Request::InitializeMap => {
                outgoing.write(super::initialize_map(&self.backend).await?);
            }
            api::Request::UpdateTransform => {
                let request = incoming
                    .read::<api::UpdateTransformRequest>()
                    .context("missing request")?;

                let fut1 = self.backend.set_client_transform(request.transform);

                let fut2 = self.backend.set_mumblelink_transform(request.transform);

                let ((), ()) = tokio::join!(fut1, fut2);
                self.update_transform = Some(request.transform);
            }
            api::Request::UpdateLookAt => {
                let request = incoming
                    .read::<api::UpdateLookAtRequest>()
                    .context("missing request")?;

                self.backend.set_client_look_at(request.look_at).await;

                self.update_look_at = Some(request.look_at);
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
            api::Request::SelectColor => {
                let request = incoming
                    .read::<api::SelectColorRequest>()
                    .context("missing request")?;

                let response = super::select_color(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::UpdateName => {
                let request = incoming
                    .read::<api::UpdateNameRequest>()
                    .context("missing request")?;

                let response = super::update_name(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::UpdateWorld => {
                let request = incoming
                    .read::<api::UpdateWorldRequest>()
                    .context("missing request")?;

                let response = super::update_world(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::MumbleRestart => {
                let request = incoming
                    .read::<api::MumbleRestartRequest>()
                    .context("missing request")?;

                let response = super::mumble_restart(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::MumbleToggle => {
                let request = incoming
                    .read::<api::MumbleToggleRequest>()
                    .context("missing request")?;

                let response = super::mumble_toggle(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::GetMumbleStatus => {
                let request = incoming
                    .read::<api::GetMumbleStatusRequest>()
                    .context("missing request")?;

                let response = super::get_mumble_status(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::GetRemoteStatus => {
                let request = incoming
                    .read::<api::GetRemoteStatusRequest>()
                    .context("missing request")?;

                let response = super::get_remote_status(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::RemoteRestart => {
                let request = incoming
                    .read::<api::RemoteRestartRequest>()
                    .context("missing request")?;

                let response = super::remote_restart(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::RemoteToggle => {
                let request = incoming
                    .read::<api::RemoteToggleRequest>()
                    .context("missing request")?;

                let response = super::remote_toggle(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::SetRemoteServer => {
                let request = incoming
                    .read::<api::SetRemoteServerRequest>()
                    .context("missing request")?;

                let response = super::set_remote_server(&self.backend, request).await?;
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
            tracing::info!("Connected");

            let mut server = axum08::server(socket, Handler::new(backend.clone()));
            let mut debounce_timer = pin!(time::sleep(Duration::from_secs(0)));
            let mut update_transform = None;
            let mut update_look_at = None;

            loop {
                if let Some(update) = server.handler_mut().update_transform.take() {
                    update_transform = Some(update);

                    debounce_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(1));
                }

                if let Some(update) = server.handler_mut().update_look_at.take() {
                    update_look_at = Some(update);

                    debounce_timer
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(1));
                }

                let (result, done) = tokio::select! {
                    result = server.run() => {
                        (result.context("Error in server"), true)
                    }
                    () = debounce_timer.as_mut(), if update_transform.is_some() || update_look_at.is_some() => {
                        tracing::debug!("Saving transform");

                        let mut result = async || {
                            if let Some(transform) = update_transform.take() {
                                backend.db().set_config("avatar/transform", transform).await.context("saving avatar transform")?;
                            }

                            if let Some(look_at) = update_look_at.take() {
                                backend.db().set_optional_config("avatar/look_at", look_at).await.context("saving avatar look_at")?;
                            }

                            Ok(())
                        };

                        (result().await, false)
                    }
                    event = events.recv() => {
                        let Ok(event) = event else {
                            tracing::error!("backend event stream error: {event:?}");
                            break;
                        };

                        tracing::debug!(?event, "backend event");

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
                            BackendEvent::Moved { peer_id, transform } => {
                                api::RemoteAvatarUpdateBody::Move { peer_id, transform }
                            },
                            BackendEvent::LookAt { peer_id, look_at } => {
                                api::RemoteAvatarUpdateBody::LookAt { peer_id, look_at }
                            },
                            BackendEvent::ImageUpdated { peer_id, image } => {
                                api::RemoteAvatarUpdateBody::ImageUpdated { peer_id, image }
                            },
                            BackendEvent::ColorUpdated { peer_id, color } => {
                                api::RemoteAvatarUpdateBody::ColorUpdated { peer_id, color }
                            },
                            BackendEvent::NameUpdated { peer_id, name } => {
                                api::RemoteAvatarUpdateBody::NameUpdated { peer_id, name }
                            }
                        };

                        (server.broadcast(event).context("send broadcast"), false)
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
