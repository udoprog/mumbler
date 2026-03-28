use core::pin::pin;

use std::collections::HashMap;
use std::net::SocketAddr;

use anyhow::{Context, Result};
use api::{Id, Key, Value};
use api::{RemoteId, RemoteUpdateBody};
use async_fuse::Fuse;
use axum::Extension;
use axum::extract::ConnectInfo;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use musli_web::axum08;
use musli_web::ws;
use tokio::sync::Notify;
use tokio::time::{self, Duration};
use tracing::{Instrument, Level};

use crate::backend::{Backend, Broadcast};

struct Handler<'a> {
    backend: Backend,
    database_updates: HashMap<(Id, Key), Value>,
    database_updates_notify: &'a Notify,
}

impl<'a> Handler<'a> {
    fn new(backend: Backend, database_updates_notify: &'a Notify) -> Self {
        Self {
            backend,
            database_updates: HashMap::new(),
            database_updates_notify,
        }
    }
}

impl ws::Handler for Handler<'_> {
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
                let request = incoming
                    .read::<api::InitializeMapRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                outgoing.write(super::initialize_map(&self.backend).await?);
            }
            api::Request::InitializeRooms => {
                let request = incoming
                    .read::<api::InitializeRoomsRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                outgoing.write(super::initialize_rooms(&self.backend).await?);
            }
            api::Request::Updates => {
                let request = incoming
                    .read::<api::UpdatesRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                if !request.values.is_empty() {
                    self.backend
                        .updates(incoming.channel(), &request.values)
                        .await?;

                    for (key, value) in request.values {
                        self.database_updates.insert((Id::ZERO, key), value);
                    }

                    self.database_updates_notify.notify_one();
                }

                outgoing.write(api::Empty);
            }
            api::Request::ObjectUpdate => {
                let request = incoming
                    .read::<api::ObjectUpdateBody>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                super::object_update(&self.backend, request.id, request.key, &request.value)
                    .await?;

                self.database_updates
                    .insert((request.id, request.key), request.value.clone());

                self.database_updates_notify.notify_one();

                self.backend.broadcast(RemoteUpdateBody::ObjectUpdated {
                    channel: incoming.channel(),
                    id: RemoteId::local(request.id),
                    key: request.key,
                    value: request.value,
                });

                outgoing.write(api::Empty);
            }
            api::Request::GetConfig => {
                let request = incoming
                    .read::<api::GetConfigRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                outgoing.write(super::get_config(&self.backend).await?);
            }
            api::Request::GetObjectSettings => {
                let request = incoming
                    .read::<api::GetObjectSettingsRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                let response = super::get_object_settings(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::InitializeImageUpload => {
                let request = incoming
                    .read::<api::InitializeImageUploadRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                let response = super::initialize_image_upload(&self.backend).await?;
                outgoing.write(response);
            }
            api::Request::CreateObject => {
                let request = incoming
                    .read::<api::CreateObjectRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                let object = self
                    .backend
                    .create_object(request.ty, request.props)
                    .await?;

                self.backend.broadcast(RemoteUpdateBody::ObjectCreated {
                    channel: incoming.channel(),
                    id: RemoteId::local(object.id),
                    object,
                });

                outgoing.write(api::Empty);
            }
            api::Request::RemoveObject => {
                let request = incoming
                    .read::<api::RemoveObjectRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                self.backend.remove_object(request.id).await?;

                self.backend.broadcast(RemoteUpdateBody::ObjectRemoved {
                    channel: incoming.channel(),
                    id: RemoteId::local(request.id),
                });

                outgoing.write(api::Empty);
            }
            api::Request::UploadImage => {
                let request = incoming
                    .read::<api::UploadImageRequest>()
                    .context("missing request")?;

                tracing::debug! {
                    ?id,
                    ?request.content_type,
                    ?request.role,
                    ?request.crop,
                    ?request.sizing,
                    request.size,
                    request.data = request.data.len(),
                };

                let image = super::upload_image(&self.backend, request).await?;

                outgoing.write(api::UploadImageResponse {
                    image: image.clone(),
                });

                self.backend
                    .broadcast(RemoteUpdateBody::ImageCreated { image });
            }
            api::Request::RemoveImage => {
                let request = incoming
                    .read::<api::RemoveImageRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                super::remove_image(&self.backend, request.id).await?;

                outgoing.write(api::Empty);

                self.backend.broadcast(RemoteUpdateBody::ImageRemoved {
                    id: RemoteId::local(request.id),
                });
            }
            api::Request::MumbleRestart => {
                let request = incoming
                    .read::<api::MumbleRestartRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                self.backend.restart_mumblelink();
                outgoing.write(api::Empty);
            }
            api::Request::RemoteRestart => {
                let request = incoming
                    .read::<api::RemoteRestartRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                self.backend.restart_client();
                outgoing.write(api::Empty);
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

            let database_updates_notify = Notify::new();
            let mut server = axum08::server(
                socket,
                Handler::new(backend.clone(), &database_updates_notify),
            );
            let mut debounce_timer = pin!(Fuse::empty());
            let mut local_updates = HashMap::new();

            loop {
                let (result, done) = tokio::select! {
                    result = server.run() => {
                        (result.context("Error in server"), true)
                    }
                    () = database_updates_notify.notified() => {
                        let handler = &mut server.handler_mut();

                        let was_empty = local_updates.is_empty();

                        if !handler.database_updates.is_empty() {
                            for (key, value) in handler.database_updates.drain() {
                                local_updates.insert(key, value);
                            }
                        }

                        if was_empty {
                            debounce_timer
                                .set(Fuse::new(time::sleep(Duration::from_secs(1))));
                        }

                        continue;
                    }
                    () = debounce_timer.as_mut() => {
                        tracing::debug!("Saving updates");

                        let result = async {
                            let values = local_updates.drain().map(|((id, key), value)| (id, key, value)).collect::<Vec<_>>();
                            backend.db().set_properties(values).await?;
                            Ok(())
                        };

                        (result.await, false)
                    }
                    event = events.recv() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(error) => {
                                tracing::error!(%error, "event");
                                break;
                            }
                        };

                        tracing::debug!(?event, "event");

                        let result = match event {
                            Broadcast::Update(body) => {
                                server.broadcast(body).context("send config update")
                            }
                            Broadcast::RemoteUpdate(body) => {
                                server.broadcast(body).context("send broadcast")
                            }
                            Broadcast::Notification(body) => {
                                server.broadcast(body).context("send notification")
                            }
                        };

                        (result, false)
                    }
                };

                if let Err(error) = result {
                    tracing::error!(%error);

                    for cause in error.chain().skip(1) {
                        tracing::error!(%cause);
                    }

                    break;
                }

                if done {
                    tracing::info!("disconnected");
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
