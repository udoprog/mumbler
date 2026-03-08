use core::pin::{Pin, pin};
use core::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use api::{Key, Properties, RemoteObject, Value};
use async_fuse::Fuse;
use tokio::net::TcpStream;
use tokio::time::{self, Instant, Sleep};

use crate::Backend;
use crate::backend::RemoteUpdateEvent;
use crate::backend::{BackendEvent, ClientState};
use crate::remote::api::{
    Event, JoinBody, LeaveBody, ObjectAddedBody, ObjectRemovedBody, PongBody, UpdatedPeer,
};
use crate::remote::{Client, Peer, REMOTE_PORT, REMOTE_TLS_PORT};

const COMPONENT: &str = "remote-client";

async fn handle_peer(
    peer: &mut Peer,
    b: &Backend,
    last_ping: &mut Option<u64>,
    mut ping_timeout: Pin<&mut Sleep>,
) -> Result<()> {
    while let Some((id, body)) = peer.handle::<Event>()? {
        match id {
            Event::Pong => {
                let body = body.decode::<PongBody>()?;
                tracing::debug!(?id, body.payload);

                if Some(body.payload) == *last_ping {
                    *last_ping = None;
                    ping_timeout
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(1));
                }
            }
            Event::Join => {
                let body = body.decode::<JoinBody>()?;
                tracing::debug!(?id, ?body.peer_id, objects = body.objects.len());

                let mut remote = b.client_state().await;
                let peer = remote.peers.entry(body.peer_id).or_default();

                for mut o in body.objects {
                    if let Some(image) = o.properties.remove(Key::IMAGE_BYTES).into_bytes() {
                        let mut images = b.images().await;
                        let image = images.store(image);
                        o.properties.insert(Key::IMAGE_ID, Value::from(image));
                    }

                    peer.objects.insert(o.id, o);
                }

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateEvent::Join {
                    peer_id: body.peer_id,
                    objects: peer.objects.values().cloned().collect(),
                }));
            }
            Event::Leave => {
                let body = body.decode::<LeaveBody>()?;
                tracing::debug!(?id, ?body.id);

                {
                    let mut remote = b.client_state().await;
                    remote.peers.remove(&body.id);
                }

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateEvent::Leave {
                    peer_id: body.id,
                }));
            }
            Event::Updated => {
                let body = body.decode::<UpdatedPeer>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.key, ?body.value);

                let mut remote = b.client_state().await;

                let Some(peer) = remote.peers.get_mut(&body.peer_id) else {
                    continue;
                };

                let Some(object) = peer.objects.get_mut(&body.object_id) else {
                    continue;
                };

                let (key, value) = match body.key {
                    Key::IMAGE_BYTES => {
                        let Some(bytes) = body.value.into_bytes() else {
                            continue;
                        };

                        let mut images = b.images().await;

                        let image = images.store(bytes);

                        if let Some(id) = object
                            .properties
                            .insert(Key::IMAGE_ID, Value::from(image))
                            .as_id()
                        {
                            images.remove(id);
                        }

                        (Key::IMAGE_ID, Value::from(image))
                    }
                    key => {
                        object.properties.insert(key, body.value.clone());
                        (key, body.value)
                    }
                };

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateEvent::Update {
                    peer_id: body.peer_id,
                    object_id: body.object_id,
                    key,
                    value,
                }));
            }
            Event::ObjectAdded => {
                let body = body.decode::<ObjectAddedBody>()?;
                tracing::debug!(?id, ?body.peer_id, object_id = ?body.object.id, "ObjectAdded");

                let mut remote = b.client_state().await;

                let peer = remote.peers.entry(body.peer_id).or_default();

                let mut object = body.object.clone();

                if let Some(image) = object.properties.remove(Key::IMAGE_BYTES).into_bytes() {
                    let mut images = b.images().await;
                    let image = images.store(image);
                    object.properties.insert(Key::IMAGE_ID, Value::from(image));
                }

                peer.objects.insert(object.id, object.clone());

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateEvent::ObjectAdded {
                    peer_id: body.peer_id,
                    object,
                }));
            }
            Event::ObjectRemoved => {
                let body = body.decode::<ObjectRemovedBody>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.object_id, "ObjectRemoved");

                {
                    let mut remote = b.client_state().await;

                    if let Some(peer) = remote.peers.get_mut(&body.peer_id) {
                        if let Some(old) = peer.objects.remove(&body.object_id)
                            && let Some(image_id) = old.properties.get(Key::IMAGE_ID).as_id()
                        {
                            b.images().await.remove(image_id);
                        }
                    }
                }

                b.broadcast(BackendEvent::RemoteUpdate(
                    RemoteUpdateEvent::ObjectRemoved {
                        peer_id: body.peer_id,
                        object_id: body.object_id,
                    },
                ));
            }
            id => {
                tracing::debug!(?id, body = body.len(), "Unknown event");
            }
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub(crate) async fn run(b: Backend, connect: String, tls: bool) -> Result<()> {
    let port;

    let host = if let Some((host, port_s)) = connect.rsplit_once(':') {
        port = port_s.parse::<u16>().context("invalid port number")?;
        host
    } else {
        port = if tls { REMOTE_TLS_PORT } else { REMOTE_PORT };
        connect.as_str()
    };

    let players = {
        let remote = b.client_state().await;
        remote.objects.clone()
    };

    tracing::info!(?host, ?port, ?tls, "Connecting to mumbler-server");

    let stream = TcpStream::connect((host, port))
        .await
        .with_context(|| anyhow!("Connecting to {host}:{port}"))?;

    let client = if tls {
        Client::default_tls_connect(stream, host).await?
    } else {
        Client::plain(stream)
    };

    let addr = client.addr()?;

    tracing::info!(?addr, "Connected");

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;
    let mut wait = pin!(b.client_wait());

    let mut objects = Vec::new();

    for (id, object) in players {
        let mut properties = Properties::new();

        for (key, value) in object.properties.iter() {
            let (key, value) = match key {
                Key::IMAGE_ID => {
                    let Some(image) = value.as_id() else {
                        continue;
                    };

                    let Some(bytes) = b.db().get_image_data(image).await? else {
                        continue;
                    };

                    (Key::IMAGE_BYTES, Value::from(bytes))
                }
                key => (key, value.clone()),
            };

            properties.insert(key, value);
        }

        objects.push(RemoteObject { id, properties });
    }

    let mut peer = Peer::new(addr, client);
    peer.connect(b"default", objects)?;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;
                handle_peer(&mut peer, &b, &mut last_ping, ping_timeout.as_mut()).await?;
            }
            () = wait.as_mut() => {
                let ClientState { objects, objects_changed, object_added, object_deleted, .. } = &mut *b.client_state().await;

                for id in objects_changed.drain() {
                    let Some(object) = objects.get_mut(&id) else {
                        continue;
                    };

                    for key in object.changed.drain() {
                        let value = object.properties.get(key);

                        let owned;

                        let (key, value) = match key {
                            Key::IMAGE_ID => {
                                let Some(image) = value.as_id() else {
                                    continue;
                                };

                                let bytes = b.db().get_image_data(image).await?;
                                owned = Value::from(bytes);
                                (Key::IMAGE_BYTES, &owned)
                            }
                            _ => (key, value),
                        };

                        peer.update_peer(id, key, value)?;
                    }
                }

                for id in object_added.drain() {
                    let Some(object) = objects.get(&id) else {
                        continue;
                    };

                    let mut properties = Properties::new();

                    for (key, value) in object.properties.iter() {
                        let (key, value) = match key {
                            Key::IMAGE_ID => {
                                let Some(image) = value.as_id() else {
                                    continue;
                                };

                                let Some(bytes) = b.db().get_image_data(image).await? else {
                                    continue;
                                };

                                (Key::IMAGE_BYTES, Value::from(bytes))
                            }
                            key => (key, value.clone()),
                        };

                        properties.insert(key, value);
                    }

                    peer.add_object(RemoteObject { id, properties })?;
                }

                for id in object_deleted.drain() {
                    peer.remove_object(id)?;
                }

                wait.set(b.client_wait());
            }
            _ = ping_timeout.as_mut(), if last_ping.is_none() => {
                let payload = rand::random();
                last_ping = Some(payload);
                peer.ping(payload)?;
                pong_timeout.as_mut().reset(Instant::now() + Duration::from_secs(5));
            }
            _ = pong_timeout.as_mut(), if last_ping.is_some() => {
                bail!("pong timeout");
            }
        }
    }
}

/// Runs and automatically reconnects the remote client.
///
/// Reads `remote/server` and `remote/enabled` from the database via
/// [`Backend::remote_config`] and re-reads them whenever a restart is
/// signalled by [`Backend::restart_client`].  The inner [`run`] loop is
/// restarted on error after a 5-second back-off, unless the connection is
/// disabled.
pub async fn managed(b: Backend, default_connect: Option<&str>) -> Result<()> {
    let settings = async || -> Result<(Option<String>, bool, bool)> {
        let connect = b
            .db()
            .config::<String>(Key::REMOTE_SERVER)
            .await?
            .as_deref()
            .or(default_connect)
            .map(str::to_owned);

        let enabled = b
            .db()
            .config::<bool>(Key::REMOTE_ENABLED)
            .await?
            .unwrap_or(true);

        let tls = b
            .db()
            .config::<bool>(Key::REMOTE_TLS)
            .await?
            .unwrap_or(false);

        Ok((connect, enabled, tls))
    };

    let (mut connect, mut enabled, mut tls) = settings().await?;

    let build = async |connect: Option<&str>, enabled: bool, tls: bool| {
        {
            let mut remote = b.client_state().await;
            remote.peers.clear();

            b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateEvent::RemoteLost));
        }

        if enabled {
            if let Some(connect) = &connect {
                tracing::info!("Restarting");
                b.notify_info(COMPONENT, "Restarting");
                Fuse::new(run(b.clone(), connect.to_string(), tls))
            } else {
                tracing::info!("Enabled, but no server configured");
                b.notify_info(COMPONENT, "Enabled, but no server configured");
                Fuse::empty()
            }
        } else {
            tracing::info!("Disabling");
            b.notify_info(COMPONENT, "Disabling");
            Fuse::empty()
        }
    };

    let mut future = pin!(build(connect.as_deref(), enabled, tls).await);
    let mut reconnect = pin!(Fuse::empty());

    loop {
        tokio::select! {
            result = future.as_mut() => {
                if let Err(error) = result {
                    tracing::error!(%error);

                    for cause in error.chain().skip(1) {
                        tracing::error!(%cause);
                    }

                    b.notify_error(COMPONENT, format_args!("{error:#}"));
                } else {
                    tracing::info!("Remote Client Stopped");
                    b.notify_info(COMPONENT, "Remote Client Stopped");
                }

                reconnect.set(Fuse::new(time::sleep(Duration::from_secs(5))));
                tracing::info!("Reconnecting in 5s");
            }
            _ = reconnect.as_mut() => {
                future.set(build(connect.as_deref(), enabled, tls).await);
            }
            () = b.client_restart_wait() => {
                (connect, enabled, tls) = settings().await?;
                reconnect.set(Fuse::empty());
                future.set(build(connect.as_deref(), enabled, tls).await);
            }
        }
    }
}
