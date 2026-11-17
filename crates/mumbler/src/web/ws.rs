use core::future;
use core::pin::pin;

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::{Context, Result};
use api::{Id, Key, Value};
use api::{RemoteId, RemoteUpdateBody};
use async_fuse::Fuse;
use auth::Auth;
use auth::axum08::SignedIn;
use auth::musliweb06::{AuthUpdate, Connection, Updates};
use axum::Extension;
use axum::extract::ConnectInfo;
use axum::extract::ws::WebSocketUpgrade;
use axum::http::HeaderMap;
use axum::response::IntoResponse;
use musli_web::api::ChannelId;
use musli_web::axum08;
use musli_web::ws::{self, Handler as _};
use tokio::sync::{Mutex, Notify, broadcast};
use tokio::time::{self, Duration};
use tracing::{Instrument, Level};

use crate::backend::{Backend, Broadcast};

struct HandlerInner {
    database_updates: Mutex<HashMap<(Id, Key), Value>>,
    database_updates_notify: Arc<Notify>,
}

#[derive(Clone)]
struct Handler {
    backend: Backend,
    inner: Arc<HandlerInner>,
}

impl Handler {
    fn new(backend: Backend, database_updates_notify: Arc<Notify>) -> Self {
        Self {
            backend,
            inner: Arc::new(HandlerInner {
                database_updates: Mutex::new(HashMap::new()),
                database_updates_notify,
            }),
        }
    }
}

impl ws::Handler for Handler {
    type Id = api::Request;
    type Response = Result<(), anyhow::Error>;

    async fn handle(
        &self,
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

                    let mut database_updates = self.inner.database_updates.lock().await;

                    for (key, value) in request.values {
                        database_updates.insert((Id::ZERO, key), value);
                    }

                    self.inner.database_updates_notify.notify_one();
                }

                outgoing.write(api::Empty);
            }
            api::Request::ObjectUpdate => {
                let request = incoming
                    .read::<api::ObjectUpdateBody>()
                    .context("missing request")?;

                if request.values.is_empty() {
                    outgoing.write(api::Empty);
                    return Ok(());
                }

                tracing::debug!(?id, ?request);

                self.backend
                    .object_update(request.id, &request.values)
                    .await;

                for (key, value) in request.values {
                    self.inner
                        .database_updates
                        .lock()
                        .await
                        .insert((request.id, key), value.clone());

                    self.backend.broadcast_from(
                        RemoteUpdateBody::ObjectUpdated {
                            id: RemoteId::local(request.id),
                            key,
                            value,
                        },
                        incoming.channel(),
                    );
                }

                self.inner.database_updates_notify.notify_one();
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

                self.backend.broadcast_from(
                    RemoteUpdateBody::ObjectCreated {
                        id: RemoteId::local(object.id),
                        object: object.clone(),
                    },
                    incoming.channel(),
                );

                outgoing.write(api::CreateObjectResponse { object });
            }
            api::Request::RemoveObject => {
                let request = incoming
                    .read::<api::RemoveObjectRequest>()
                    .context("missing request")?;

                tracing::debug!(?id, ?request);

                self.backend.remove_object(request.id).await?;

                self.backend.broadcast_from(
                    RemoteUpdateBody::ObjectRemoved {
                        id: RemoteId::local(request.id),
                    },
                    incoming.channel(),
                );

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
    auth: Option<Extension<Auth>>,
    signed_in: Option<Extension<SignedIn>>,
    headers: HeaderMap,
) -> impl IntoResponse {
    let conn = match (auth, signed_in) {
        (Some(Extension(auth)), Some(Extension(signed_in))) => {
            Some(Connection::new(auth, &signed_in))
        }
        _ => None,
    };

    ws.on_upgrade(move |socket| {
        let events = backend.subscribe();

        let future = async move {
            tracing::info!("connected");

            let database_updates_notify = Arc::new(Notify::new());
            let handler = Handler::new(backend.clone(), database_updates_notify.clone());
            let inner = handler.inner.clone();

            match conn {
                Some(conn) => {
                    // Ends the socket with its session: signed out, revoked,
                    // deleted or expired.
                    let updates = conn.updates();
                    let connect = axum08::server(socket, handler.or(conn));

                    let server = match updates.unless_ended(connect.connect()).await {
                        Some(Ok(server)) => server,
                        Some(Err(error)) => {
                            tracing::error!(%error, "failed to negotiate");
                            return;
                        }
                        None => return,
                    };

                    serve(
                        server,
                        backend,
                        events,
                        inner,
                        database_updates_notify,
                        Some(updates),
                    )
                    .await;
                }
                None => {
                    let server = match axum08::server(socket, handler).connect().await {
                        Ok(server) => server,
                        Err(error) => {
                            tracing::error!(%error, "failed to negotiate");
                            return;
                        }
                    };

                    serve(
                        server,
                        backend,
                        events,
                        inner,
                        database_updates_notify,
                        None,
                    )
                    .await;
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

async fn serve<H>(
    mut server: ws::Server<axum08::AxumServer, H>,
    backend: Backend,
    mut events: broadcast::Receiver<(ChannelId, Broadcast)>,
    inner: Arc<HandlerInner>,
    database_updates_notify: Arc<Notify>,
    mut updates: Option<Updates>,
) where
    H: ws::Handler,
{
    let mut debounce_timer = pin!(Fuse::empty());
    let mut local_updates = HashMap::new();

    loop {
        let (result, done) = tokio::select! {
            result = server.run() => {
                (result.context("Error in server"), true)
            }
            () = database_updates_notify.notified() => {
                let was_empty = local_updates.is_empty();

                let mut database_updates = inner.database_updates.lock().await;

                if !database_updates.is_empty() {
                    for (key, value) in database_updates.drain() {
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
            update = next_update(&mut updates) => {
                let Some(update) = update else {
                    tracing::info!("session ended");
                    break;
                };

                (server.broadcast(update).context("send auth update"), false)
            }
            event = events.recv() => {
                let (origin, event) = match event {
                    Ok(event) => event,
                    Err(error) => {
                        tracing::error!(%error, "event");
                        break;
                    }
                };

                tracing::debug!(?origin, ?event, "event");

                let result = match event {
                    Broadcast::Update(body) => server
                        .broadcast_from(body, origin)
                        .context("send config update"),
                    Broadcast::RemoteUpdate(body) => server
                        .broadcast_from(body, origin)
                        .context("send broadcast"),
                    Broadcast::Notification(body) => server
                        .broadcast_from(body, origin)
                        .context("send notification"),
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
}

/// The next auth update, or never without sign-in.
async fn next_update(updates: &mut Option<Updates>) -> Option<AuthUpdate> {
    match updates {
        Some(updates) => updates.next().await,
        None => future::pending().await,
    }
}
