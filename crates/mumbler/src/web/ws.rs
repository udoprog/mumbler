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
use crate::backend::RemoteAvatarEvent;

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
                outgoing.write(super::initialize_map(&self.backend).await?);
            }
            api::Request::Update => {
                let request = incoming
                    .read::<api::UpdateRequest>()
                    .context("missing request")?;

                self.database_updates
                    .insert((request.id, request.key), request.value.clone());

                self.database_updates_notify.notify_one();

                if request.key == Key::AVATAR_TRANSFORM {
                    if let Some(transform) = request.value.as_transform() {
                        self.backend.set_mumblelink_transform(transform).await;
                    }
                }

                self.backend
                    .set_client(request.id, request.key, request.value.clone())
                    .await;

                outgoing.write(api::Empty);
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

                let response = super::get_settings(&self.backend).await?;
                outgoing.write(response);
            }
            api::Request::DeleteImage => {
                let request = incoming
                    .read::<api::DeleteImageRequest>()
                    .context("missing request")?;

                let response = super::delete_image(&self.backend, request).await?;
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

            let database_updates_notify = Notify::new();
            let mut server = axum08::server(socket, Handler::new(backend.clone(), &database_updates_notify));
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
                            BackendEvent::Notification { error, component, message } => {
                                let body = if error {
                                    api::ServerNotificationBody::Error { component, message }
                                } else {
                                    api::ServerNotificationBody::Info { component, message }
                                };
                                server.broadcast(body).context("send notification")
                            }
                            BackendEvent::RemoteAvatar(body) => {
                                let body = match body {
                                    RemoteAvatarEvent::RemoteLost => api::RemoteAvatarUpdateBody::RemoteLost,
                                    RemoteAvatarEvent::Join { peer_id, objects } => api::RemoteAvatarUpdateBody::Join { peer_id, objects },
                                    RemoteAvatarEvent::Leave { peer_id } => api::RemoteAvatarUpdateBody::Leave { peer_id },
                                    RemoteAvatarEvent::Update { peer_id, object_id, key, value } => api::RemoteAvatarUpdateBody::Update { peer_id, object_id, key, value },
                                };

                                server.broadcast(body).context("send broadcast")
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
