use core::pin::pin;

use std::collections::HashMap;
use std::net::SocketAddr;

use anyhow::{Context, Result};
use api::{Id, Key, Value};
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

use crate::backend::Backend;
use crate::backend::BackendEvent;
use crate::backend::LocalUpdateEvent;
use crate::backend::RemoteUpdateEvent;

struct Handler<'a> {
    sender_id: Id,
    backend: Backend,
    database_updates: HashMap<(Id, Key), Value>,
    database_updates_notify: &'a Notify,
}

impl<'a> Handler<'a> {
    fn new(sender_id: Id, backend: Backend, database_updates_notify: &'a Notify) -> Self {
        Self {
            sender_id,
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
                outgoing.write(super::initialize_map(&self.backend).await?);
            }
            api::Request::Update => {
                let request = incoming
                    .read::<api::UpdateRequest>()
                    .context("missing request")?;

                self.database_updates
                    .insert((request.object_id, request.key), request.value.clone());

                self.database_updates_notify.notify_one();

                if request.key == Key::TRANSFORM {
                    if let Some(transform) = request.value.as_transform() {
                        let mumble_id = self.backend.mumble_object();

                        if mumble_id.is_none() || mumble_id == Some(request.object_id) {
                            self.backend.set_mumblelink_transform(transform).await;
                        }
                    }
                }

                self.backend
                    .set_client(request.object_id, request.key, request.value.clone())
                    .await;

                self.backend
                    .broadcast(BackendEvent::LocalUpdate(LocalUpdateEvent {
                        sender_id: self.sender_id,
                        object_id: request.object_id,
                        key: request.key,
                        value: request.value,
                        broadcast_self: request.broadcast_self,
                    }));

                outgoing.write(api::Empty);
            }
            api::Request::UploadImage => {
                let request = incoming
                    .read::<api::UploadImageRequest>()
                    .context("missing request")?;

                outgoing.write(super::upload_image(&self.backend, request).await?);
            }
            api::Request::GetConfig => {
                _ = incoming
                    .read::<api::GetConfigRequest>()
                    .context("missing request")?;

                outgoing.write(super::get_config(&self.backend).await?);
            }
            api::Request::GetObjectSettings => {
                let request = incoming
                    .read::<api::GetObjectSettingsRequest>()
                    .context("missing request")?;

                let response = super::get_object_settings(&self.backend, request).await?;
                outgoing.write(response);
            }
            api::Request::CreateObject => {
                _ = incoming
                    .read::<api::CreateObjectRequest>()
                    .context("missing request")?;

                let response = super::create_object(&self.backend).await?;
                outgoing.write(response);
            }
            api::Request::DeleteObject => {
                let request = incoming
                    .read::<api::DeleteObjectRequest>()
                    .context("missing request")?;

                self.backend.delete_object(request.id).await?;
                outgoing.write(api::Empty);
            }
            api::Request::DeleteImage => {
                let request = incoming
                    .read::<api::DeleteImageRequest>()
                    .context("missing request")?;

                self.backend.db().delete_image(request.id).await?;
                outgoing.write(api::Empty);
            }
            api::Request::UpdateConfig => {
                let request = incoming
                    .read::<api::UpdateConfigRequest>()
                    .context("missing request")?;

                super::update_config(&self.backend, request.values).await?;
                outgoing.write(api::Empty);
            }
            api::Request::MumbleRestart => {
                _ = incoming
                    .read::<api::MumbleRestartRequest>()
                    .context("missing request")?;

                self.backend.restart_mumblelink();
                outgoing.write(api::Empty);
            }
            api::Request::GetMumbleStatus => {
                _ = incoming
                    .read::<api::GetMumbleStatusRequest>()
                    .context("missing request")?;

                let response = super::get_mumble_status(&self.backend).await?;
                outgoing.write(response);
            }
            api::Request::GetRemoteStatus => {
                _ = incoming
                    .read::<api::GetRemoteStatusRequest>()
                    .context("missing request")?;

                let response = super::get_remote_status(&self.backend).await?;
                outgoing.write(response);
            }
            api::Request::RemoteRestart => {
                _ = incoming
                    .read::<api::RemoteRestartRequest>()
                    .context("missing request")?;

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
            tracing::info!("Connected");

            let sender_id = Id::new(rand::random());

            let database_updates_notify = Notify::new();
            let mut server = axum08::server(socket, Handler::new(sender_id, backend.clone(), &database_updates_notify));
            let mut debounce_timer = pin!(Fuse::empty());
            let mut local_updates = HashMap::new();

            loop {
                let (result, done) = tokio::select! {
                    result = server.run() => {
                        (result.context("Error in server"), true)
                    }
                    () = database_updates_notify.notified() => {
                        let updates = &mut server.handler_mut().database_updates;

                        if updates.is_empty() {
                            continue;
                        }

                        let was_empty = local_updates.is_empty();

                        for (key, value) in updates.drain() {
                            local_updates.insert(key, value);
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
                            for ((id, key), value) in local_updates.drain() {
                                backend.db().set_property_value(id, key, value).await?;
                            }

                            Ok(())
                        };

                        (result.await, false)
                    }
                    event = events.recv() => {
                        let event = match event {
                            Ok(event) => event,
                            Err(error) => {
                                tracing::error!(%error, "Backend event");
                                break;
                            }
                        };

                        tracing::debug!(?event, "Backend event");

                        let result = match event {
                            BackendEvent::LocalUpdate(body) => {
                                if body.sender_id == sender_id && !body.broadcast_self {
                                    continue;
                                }

                                let body = api::LocalUpdateBody {
                                    object_id: body.object_id,
                                    key: body.key,
                                    value: body.value,
                                };

                                server.broadcast(body).context("send local update")
                            }
                            BackendEvent::RemoteUpdate(body) => {
                                let body = match body {
                                    RemoteUpdateEvent::RemoteLost => api::RemoteUpdateBody::RemoteLost,
                                    RemoteUpdateEvent::Join { peer_id, objects } => api::RemoteUpdateBody::Join { peer_id, objects },
                                    RemoteUpdateEvent::Leave { peer_id } => api::RemoteUpdateBody::Leave { peer_id },
                                    RemoteUpdateEvent::Update { peer_id, object_id, key, value } => api::RemoteUpdateBody::Update { peer_id, object_id, key, value },
                                    RemoteUpdateEvent::ObjectAdded { peer_id, object } => api::RemoteUpdateBody::ObjectAdded { peer_id, object },
                                    RemoteUpdateEvent::ObjectRemoved { peer_id, object_id } => api::RemoteUpdateBody::ObjectRemoved { peer_id, object_id },
                                };

                                server.broadcast(body).context("send broadcast")
                            }
                            BackendEvent::Notification { error, component, message } => {
                                let body = if error {
                                    api::ServerNotificationBody::Error { component, message }
                                } else {
                                    api::ServerNotificationBody::Info { component, message }
                                };
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
                    tracing::info!("Disconnected");
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
