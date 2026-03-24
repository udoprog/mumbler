use core::pin::{Pin, pin};
use core::time::Duration;

use std::collections::HashSet;

use anyhow::{Context as _, Result, anyhow, bail};
use api::{Key, Properties, RemoteId, RemoteObject, RemotePeer, RemoteUpdateBody, Value};
use async_fuse::Fuse;
use musli_web::api::ChannelId;
use tokio::net::TcpStream;
use tokio::time::{self, Instant, Sleep};

use crate::Backend;
use crate::backend::PeerInfo;
use crate::remote::api::{
    ChallengeBody, Event, ImageCreatedBody, ImageRemovedBody, ObjectCreatedBody, ObjectRemovedBody,
    ObjectUpdatedBody, PeerConnectedBody, PeerDisconnectBody, PeerJoinBody, PeerLeaveBody,
    PeerUpdatedBody, PongBody, RemoteImage,
};
use crate::remote::{Client, DEFAULT_PORT, DEFAULT_TLS_PORT, Peer};

const COMPONENT: &str = "remote-client";

async fn handle_peer(
    peer: &mut Peer,
    b: &Backend,
    last_ping: &mut Option<u64>,
    mut ping_timeout: Pin<&mut Sleep>,
    authenticated: &mut bool,
) -> Result<()> {
    while let Some((id, body)) = peer.read::<Event>()? {
        match id {
            Event::Challenge => {
                let body = body.decode::<ChallengeBody>()?;

                let public_key;
                let signature;

                let mut objects = Vec::new();
                let mut images = Vec::new();
                let mut props = Properties::new();

                {
                    let state = b.client_state().await;

                    public_key = state.keypair.public_key();
                    signature = state.keypair.sign(&body.nonce);

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
                            content_type: image.content_type,
                            bytes: Box::from(image.bytes.as_slice()),
                            width: image.width,
                            height: image.height,
                        });
                    }

                    for (key, value) in state.props.iter() {
                        if key.is_remote() {
                            props.insert(key, value.clone());
                        }
                    }
                }

                peer.connect(public_key, signature, &objects, &images, &props)?;
                *authenticated = true;
            }
            Event::Pong => {
                let body = body.decode::<PongBody>()?;
                tracing::trace!(?id, body.payload);

                if Some(body.payload) == *last_ping {
                    *last_ping = None;
                    ping_timeout
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(1));
                }
            }
            Event::PeerConnected => {
                let body = body.decode::<PeerConnectedBody>()?;
                tracing::debug!(?id, ?body.peer_id, objects = body.objects.len(), "PeerConnected");

                let mut state = b.client_state().await;

                let peer = PeerInfo {
                    public_key: body.public_key,
                    objects: body.objects.iter().map(|o| (o.id, o.clone())).collect(),
                    images: HashSet::new(),
                    props: body.props.clone(),
                };

                state.peers.insert(body.peer_id, peer);

                b.broadcast(RemoteUpdateBody::PeerConnected {
                    peer: RemotePeer {
                        peer_id: body.peer_id,
                        public_key: body.public_key,
                        objects: body.objects,
                        props: body.props.clone(),
                    },
                });
            }
            Event::PeerJoin => {
                let body = body.decode::<PeerJoinBody>()?;
                tracing::debug!(?id, ?body.peer_id, objects = body.objects.len());

                let mut state = b.client_state().await;

                let Some(peer) = state.peers.get_mut(&body.peer_id) else {
                    continue;
                };

                peer.images.clear();

                if !body.images.is_empty() {
                    let mut images = b.write_images().await;

                    for image in body.images {
                        let id = RemoteId::new(body.peer_id, image.id);
                        images.store(id, image.bytes);
                        peer.images.insert(image.id);
                    }
                }

                for o in body.objects {
                    peer.objects.insert(o.id, o);
                }

                b.broadcast(RemoteUpdateBody::PeerJoin {
                    peer_id: body.peer_id,
                    objects: peer.objects.values().cloned().collect(),
                    images: peer.images.iter().cloned().collect(),
                });
            }
            Event::PeerDisconnect => {
                let body = body.decode::<PeerDisconnectBody>()?;
                tracing::debug!(?id, ?body.id);

                let mut state = b.client_state().await;
                state.peers.remove(&body.id);

                b.broadcast(RemoteUpdateBody::PeerDisconnect { peer_id: body.id });
            }
            Event::PeerLeave => {
                let body = body.decode::<PeerLeaveBody>()?;
                tracing::debug!(?id, ?body.id);

                let mut state = b.client_state().await;

                let Some(peer) = state.peers.get_mut(&body.id) else {
                    continue;
                };

                peer.images.clear();
                peer.objects.retain(|_, o| o.ty.is_global());

                b.broadcast(RemoteUpdateBody::PeerLeave { peer_id: body.id });
            }
            Event::PeerUpdated => {
                let body = body.decode::<PeerUpdatedBody>()?;
                tracing::debug!(?id, ?body.peer_id, key = ?body.key, value = ?body.value);

                let mut state = b.client_state().await;

                let Some(peer) = state.peers.get_mut(&body.peer_id) else {
                    continue;
                };

                peer.props.insert(body.key, body.value.clone());

                b.broadcast(RemoteUpdateBody::PeerUpdate {
                    peer_id: body.peer_id,
                    key: body.key,
                    value: body.value,
                });
            }
            Event::ObjectUpdated => {
                let body = body.decode::<ObjectUpdatedBody>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.key, ?body.value);

                let mut state = b.client_state().await;

                let Some(peer) = state.peers.get_mut(&body.peer_id) else {
                    continue;
                };

                let Some(object) = peer.objects.get_mut(&body.object_id) else {
                    continue;
                };

                object.props.insert(body.key, body.value.clone());

                b.broadcast(RemoteUpdateBody::ObjectUpdated {
                    channel: ChannelId::NONE,
                    id: RemoteId::new(body.peer_id, body.object_id),
                    key: body.key,
                    value: body.value,
                });
            }
            Event::ObjectCreated => {
                let body = body.decode::<ObjectCreatedBody>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.object.ty, ?body.object.id, "ObjectAdded");

                let mut state = b.client_state().await;

                let Some(peer) = state.peers.get_mut(&body.peer_id) else {
                    continue;
                };

                peer.objects.insert(body.object.id, body.object.clone());

                b.broadcast(RemoteUpdateBody::ObjectCreated {
                    channel: ChannelId::NONE,
                    id: RemoteId::new(body.peer_id, body.object.id),
                    object: body.object,
                });
            }
            Event::ImageCreated => {
                let body = body.decode::<ImageCreatedBody>()?;
                tracing::debug!(?id, ?body.peer_id, image_id = ?body.image.id, "ImageAdded");

                let mut state = b.client_state().await;
                let mut images = b.write_images().await;

                let Some(peer) = state.peers.get_mut(&body.peer_id) else {
                    continue;
                };

                peer.images.insert(body.image.id);

                let id = RemoteId::new(body.peer_id, body.image.id);
                images.store(id, body.image.bytes.clone());

                b.broadcast(RemoteUpdateBody::ImageAdded { id });
            }
            Event::ObjectRemoved => {
                let body = body.decode::<ObjectRemovedBody>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.object_id, "ObjectRemoved");

                let remove_room;

                let removed = 'found: {
                    let mut state = b.client_state().await;
                    let id = state.to_stable_id(body.peer_id, body.object_id);

                    remove_room = *state.props.get(Key::ROOM).as_stable_id() == id;

                    if let Some(peer) = state.peers.get_mut(&body.peer_id) {
                        break 'found peer.objects.remove(&body.object_id).is_some();
                    }

                    false
                };

                if removed {
                    b.broadcast(RemoteUpdateBody::ObjectRemoved {
                        channel: ChannelId::NONE,
                        id: RemoteId::new(body.peer_id, body.object_id),
                    });
                }

                if remove_room {
                    crate::web::updates(ChannelId::NONE, b, [(Key::ROOM, Value::empty())]).await?;
                }
            }
            Event::ImageRemoved => {
                let body = body.decode::<ImageRemovedBody>()?;
                tracing::debug!(?id, ?body.peer_id, ?body.image_id, "ImageRemoved");

                let id = RemoteId::new(body.peer_id, body.image_id);
                b.write_images().await.remove(&id);
                b.broadcast(RemoteUpdateBody::ImageRemoved { id });
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

    let mut peer = Peer::new(client);
    peer.hello()?;

    let mut authenticated = false;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;
                handle_peer(&mut peer, &b, &mut last_ping, ping_timeout.as_mut(), &mut authenticated).await?
            }
            () = wait.as_mut(), if authenticated => {
                let mut client_state = b.client_state().await;
                let state = &mut *client_state;

                for key in state.props_changed.drain() {
                    let value = state.props.get(key);
                    peer.peer_update(key, value)?;
                }

                for id in state.objects_changed.drain() {
                    let Some(object) = state.objects.get_mut(&id) else {
                        continue;
                    };

                    for key in object.changed.drain() {
                        let value = object.props.get(key);
                        peer.object_update(id, key, value)?;
                    }
                }

                for id in state.objects_added.drain() {
                    let Some(object) = state.objects.get(&id) else {
                        continue;
                    };

                    peer.object_create(RemoteObject { ty: object.ty, id, props: object.props.clone() })?;
                }

                for id in state.objects_removed.drain() {
                    peer.object_remove(id)?;
                }

                for id in state.images_added.drain() {
                    let Some(image) = state.images.get(&id) else {
                        continue;
                    };

                    peer.image_create(RemoteImage {
                        id: image.id,
                        content_type: image.content_type,
                        bytes: Box::from(image.bytes.as_slice()),
                        width: image.width,
                        height: image.height,
                    })?;
                }

                for id in state.images_removed.drain() {
                    peer.image_remove(id)?;
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
/// [`Backend::remote_config`] and re-reads them whenever a restart is signalled
/// by [`Backend::restart_client`]. The inner [`run`] loop is restarted on error
/// after a 5-second back-off, unless the connection is disabled.
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
            let mut state = b.client_state().await;
            state.peers.clear();
            b.broadcast(RemoteUpdateBody::RemoteLost);
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
