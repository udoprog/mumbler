use core::pin::{Pin, pin};
use core::time::Duration;

use anyhow::{Context as _, Result, anyhow, bail};
use api::{Key, RemoteObject, RemoteUpdateBody};
use async_fuse::Fuse;
use tokio::net::TcpStream;
use tokio::time::{self, Instant, Sleep};

use crate::Backend;
use crate::backend::BackendEvent;
use crate::remote::api::{
    Event, ImageAddedBody, ImageRemovedBody, JoinBody, LeaveBody, ObjectAddedBody,
    ObjectRemovedBody, PongBody, RemoteImage, UpdatedPeer,
};
use crate::remote::{Client, DEFAULT_PORT, DEFAULT_TLS_PORT, Peer};

const COMPONENT: &str = "remote-client";

async fn handle_peer(
    peer: &mut Peer,
    b: &Backend,
    last_ping: &mut Option<u64>,
    mut ping_timeout: Pin<&mut Sleep>,
) -> Result<()> {
    while let Some((id, body)) = peer.read::<Event>()? {
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

                if !body.images.is_empty() {
                    let mut images = b.images().await;

                    for image in body.images {
                        images.store(body.peer_id, image.id, image.bytes);
                        peer.images.insert(image.id);
                    }
                }

                for o in body.objects {
                    peer.objects.insert(o.id, o);
                }

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateBody::Join {
                    peer_id: body.peer_id,
                    objects: peer.objects.values().cloned().collect(),
                    images: peer.images.iter().cloned().collect(),
                }));
            }
            Event::Leave => {
                let body = body.decode::<LeaveBody>()?;
                tracing::debug!(?id, ?body.id);

                {
                    let mut remote = b.client_state().await;
                    remote.peers.remove(&body.id);
                }

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateBody::Leave {
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

                object.props.insert(body.key, body.value.clone());

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateBody::Update {
                    peer_id: body.peer_id,
                    object_id: body.object_id,
                    key: body.key,
                    value: body.value,
                }));
            }
            Event::ObjectAdded => {
                let body = body.decode::<ObjectAddedBody>()?;
                tracing::debug!(?id, ?body.peer_id, object_id = ?body.object.id, "ObjectAdded");

                let mut remote = b.client_state().await;

                let peer = remote.peers.entry(body.peer_id).or_default();

                peer.objects.insert(body.object.id, body.object.clone());

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateBody::ObjectAdded {
                    peer_id: body.peer_id,
                    object: body.object,
                }));
            }
            Event::ImageAdded => {
                let body = body.decode::<ImageAddedBody>()?;
                tracing::debug!(?id, ?body.peer_id, image_id = ?body.image.id, "ImageAdded");

                let mut images = b.images().await;
                images.store(body.peer_id, body.image.id, body.image.bytes.clone());

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateBody::ImageAdded {
                    peer_id: body.peer_id,
                    image_id: body.image.id,
                }));
            }
            Event::ObjectRemoved => {
                let body = body.decode::<ObjectRemovedBody>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.object_id, "ObjectRemoved");

                let removed = 'found: {
                    let mut remote = b.client_state().await;

                    if let Some(peer) = remote.peers.get_mut(&body.peer_id) {
                        break 'found peer.objects.remove(&body.object_id).is_some();
                    }

                    false
                };

                if removed {
                    b.broadcast(BackendEvent::RemoteUpdate(
                        RemoteUpdateBody::ObjectRemoved {
                            peer_id: body.peer_id,
                            object_id: body.object_id,
                        },
                    ));
                }
            }
            Event::ImageRemoved => {
                let body = body.decode::<ImageRemovedBody>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.image_id, "ImageRemoved");

                b.images().await.remove(body.peer_id, body.image_id);

                b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateBody::ImageRemoved {
                    peer_id: body.peer_id,
                    image_id: body.image_id,
                }));
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
        port = if tls { DEFAULT_TLS_PORT } else { DEFAULT_PORT };
        connect.as_str()
    };

    tracing::info!(?host, ?port, ?tls, "connecting to mumbler-server");

    let stream = TcpStream::connect((host, port))
        .await
        .with_context(|| anyhow!("connecting to {host}:{port}"))?;

    let client = if tls {
        Client::default_tls_connect(stream, host).await?
    } else {
        Client::plain(stream)
    };

    let addr = client.addr()?;

    tracing::info!(?addr, "connected");

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;
    let mut wait = pin!(b.client_wait());

    let mut objects = Vec::new();
    let mut images = Vec::new();

    {
        let state = b.client_state().await;

        for object in state.objects.values() {
            objects.push(RemoteObject {
                ty: object.ty,
                id: object.id,
                props: object.props.clone(),
            });
        }

        for image in state.images.values() {
            images.push(RemoteImage {
                id: image.id,
                content_type: image.content_type.clone(),
                bytes: Box::from(image.bytes.as_slice()),
                width: image.width,
                height: image.height,
            });
        }
    }

    let mut peer = Peer::new(client);
    peer.connect(b"default", &objects, &images)?;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;
                handle_peer(&mut peer, &b, &mut last_ping, ping_timeout.as_mut()).await?;
            }
            () = wait.as_mut() => {
                let mut client_state = b.client_state().await;
                let state = &mut *client_state;

                for id in state.objects_changed.drain() {
                    let Some(object) = state.objects.get_mut(&id) else {
                        continue;
                    };

                    for key in object.changed.drain() {
                        let value = object.props.get(key);
                        peer.update_peer(id, key, value)?;
                    }
                }

                for id in state.objects_added.drain() {
                    let Some(object) = state.objects.get(&id) else {
                        continue;
                    };

                    peer.add_object(RemoteObject { ty: object.ty, id, props: object.props.clone() })?;
                }

                for id in state.objects_deleted.drain() {
                    peer.remove_object(id)?;
                }

                for id in state.images_added.drain() {
                    let Some(image) = state.images.get(&id) else {
                        continue;
                    };

                    peer.add_image(RemoteImage {
                        id: image.id,
                        content_type: image.content_type,
                        bytes: Box::from(image.bytes.as_slice()),
                        width: image.width,
                        height: image.height,
                    })?;
                }

                for id in state.images_deleted.drain() {
                    peer.remove_image(id)?;
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

            b.broadcast(BackendEvent::RemoteUpdate(RemoteUpdateBody::RemoteLost));
        }

        if enabled {
            if let Some(connect) = &connect {
                tracing::info!("restarting");
                b.notify_info(COMPONENT, "restarting");
                Fuse::new(run(b.clone(), connect.to_string(), tls))
            } else {
                tracing::info!("enabled, but no server configured");
                b.notify_info(COMPONENT, "enabled, but no server configured");
                Fuse::empty()
            }
        } else {
            tracing::info!("disabling");
            b.notify_info(COMPONENT, "disabling");
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
                    tracing::info!("remote client stopped");
                    b.notify_info(COMPONENT, "remote client stopped");
                }

                tracing::info!("reconnecting in 5s");
                reconnect.set(Fuse::new(time::sleep(Duration::from_secs(5))));
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
