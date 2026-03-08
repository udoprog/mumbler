use core::pin::{Pin, pin};
use core::time::Duration;
use std::collections::HashMap;

use anyhow::{Context as _, Result, anyhow, bail};
use api::{Id, Key, Value};
use async_fuse::Fuse;
use tokio::time::{self, Instant, Sleep};

use crate::Backend;
use crate::backend::BackendEvent;
use crate::backend::RemoteAvatarEvent;
use crate::remote::api::{Event, JoinBody, LeaveBody, PongBody, UpdatedPeer};
use crate::remote::{Client, Peer, REMOTE_PORT};

const COMPONENT: &str = "remote-client";

async fn handle_peer(
    peer: &mut Peer,
    b: &Backend,
    last_ping: &mut Option<u64>,
    mut ping_timeout: Pin<&mut Sleep>,
) -> Result<()> {
    while let Some((event, body)) = peer.handle::<Event>()? {
        match event {
            Event::Pong => {
                let pong = body.decode::<PongBody>()?;

                if Some(pong.payload) == *last_ping {
                    *last_ping = None;
                    ping_timeout
                        .as_mut()
                        .reset(Instant::now() + Duration::from_secs(1));
                }
            }
            Event::Join => {
                let mut event = body.decode::<JoinBody>()?;
                tracing::debug!(?event.id, "join");

                if let Some(image) = event
                    .values
                    .remove(&Key::AVATAR_IMAGE_BYTES)
                    .and_then(|v| v.into_bytes())
                {
                    let mut images = b.images().await;
                    let image = images.store(image);
                    event
                        .values
                        .insert(Key::AVATAR_IMAGE_ID, Value::from(image));
                }

                {
                    let mut remote = b.client_state().await;
                    remote.peers.entry(event.id).or_default().values = event.values.clone();
                }

                b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::Join {
                    peer_id: event.id,
                    values: event.values,
                }));
            }
            Event::Leave => {
                let event = body.decode::<LeaveBody>()?;
                tracing::debug!(?event.id, "leave");

                {
                    let mut remote = b.client_state().await;
                    remote.peers.remove(&event.id);
                }

                b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::Leave {
                    peer_id: event.id,
                }));
            }
            Event::Updated => {
                let event = body.decode::<UpdatedPeer>()?;
                tracing::debug!(?event.id, ?event.key, ?event.value, "updated");

                let mut remote = b.client_state().await;

                let Some(peer) = remote.peers.get_mut(&event.id) else {
                    continue;
                };

                let (key, value) = match event.key {
                    Key::AVATAR_IMAGE_BYTES => {
                        let Some(bytes) = event.value.into_bytes() else {
                            continue;
                        };

                        let mut images = b.images().await;

                        let image = images.store(bytes);

                        if let Some(old) =
                            peer.values.insert(Key::AVATAR_IMAGE_ID, Value::from(image))
                            && let Some(id) = old.as_id()
                        {
                            images.remove(id);
                        }

                        (Key::AVATAR_IMAGE_ID, Value::from(image))
                    }
                    key => {
                        peer.values.insert(key, event.value.clone());
                        (key, event.value)
                    }
                };

                b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::Update {
                    peer_id: event.id,
                    key,
                    value,
                }));
            }
            event => {
                tracing::debug!(?event);
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
        port = REMOTE_PORT;
        connect.as_str()
    };

    let player = {
        let remote = b.client_state().await;
        remote.player.clone()
    };

    tracing::info!(?host, ?port, ?tls, "Connecting to mumbler-server");

    // TODO: use `tls` when establishing a TLS-wrapped connection.
    let client = Client::connect((host, port))
        .await
        .with_context(|| anyhow!("Connecting to {host}:{port}"))?;

    let addr = client.addr()?;

    tracing::info!(?addr, "Connected");

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;
    let mut wait = pin!(b.client_wait());

    let image = 'image: {
        let Some(image) = player.image else {
            break 'image None;
        };

        b.db().get_image_data(image).await?
    };

    let mut values = HashMap::new();

    values.insert(Key::AVATAR_TRANSFORM, Value::from(player.transform));
    values.insert(Key::AVATAR_LOOK_AT, Value::from(player.look_at));
    values.insert(Key::AVATAR_IMAGE_BYTES, Value::from(image));
    values.insert(Key::AVATAR_COLOR, Value::from(player.color));
    values.insert(Key::AVATAR_NAME, Value::from(player.name.clone()));

    let mut peer = Peer::new(addr, client);
    peer.connect(b"default", values)?;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;
                handle_peer(&mut peer, &b, &mut last_ping, ping_timeout.as_mut()).await?;
            }
            () = wait.as_mut() => {
                let state = b.take_client_player().await;

                if state.is_transform() {
                    peer.update_peer(Key::AVATAR_TRANSFORM, &Value::from(state.transform))?;
                }

                if state.is_look_at() {
                    peer.update_peer(Key::AVATAR_LOOK_AT, &Value::from(state.look_at))?;
                }

                if state.is_image() {
                    let image = 'image: {
                        let Some(image) = state.image else {
                            break 'image None;
                        };

                        b.db().get_image_data(image).await?
                    };

                    peer.update_peer(Key::AVATAR_IMAGE_BYTES, &Value::from(image))?;
                }

                if state.is_color() {
                    peer.update_peer(Key::AVATAR_COLOR, &Value::from(state.color))?;
                }

                if state.is_name() {
                    peer.update_peer(Key::AVATAR_NAME, &Value::from(state.name.clone()))?;
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
            .get::<String>(Id::GLOBAL, Key::REMOTE_SERVER)
            .await?
            .as_deref()
            .or(default_connect)
            .map(str::to_owned);

        let enabled = b
            .db()
            .get::<bool>(Id::GLOBAL, Key::REMOTE_ENABLED)
            .await?
            .unwrap_or(true);

        let tls = b
            .db()
            .get::<bool>(Id::GLOBAL, Key::REMOTE_TLS)
            .await?
            .unwrap_or(false);

        Ok((connect, enabled, tls))
    };

    let (mut connect, mut enabled, mut tls) = settings().await?;

    let build = async |connect: Option<&str>, enabled: bool, tls: bool| {
        {
            let mut remote = b.client_state().await;
            remote.peers.clear();

            b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::RemoteLost));
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
                    b.notify_error(COMPONENT, format_args!("{error:#}"));
                } else {
                    tracing::info!("Disconnected");
                    b.notify_info(COMPONENT, "Disconnected");
                }

                reconnect.set(Fuse::new(time::sleep(Duration::from_secs(5))));
                tracing::info!("Reconnecting in 5s");
            }
            _ = reconnect.as_mut() => {
                b.notify_info(COMPONENT, "Reconnecting to server");
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
