use core::pin::{Pin, pin};
use core::time::Duration;
use core::{future, mem};

use anyhow::{Context as _, Result, anyhow, bail};
use api::Transform;
use tokio::time::{self, Instant, Sleep};

use crate::Backend;
use crate::backend::BackendEvent;
use crate::backend::RemoteAvatarEvent;
use crate::remote::api::{
    Event, JoinBody, LeaveBody, PongBody, UpdatedColorBody, UpdatedImageBody, UpdatedLookAt,
    UpdatedNameBody, UpdatedTransform,
};
use crate::remote::{Client, Peer};

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
                let event = body.decode::<JoinBody>()?;
                tracing::debug!(?event.id, "join");

                {
                    let mut remote = b.client_state().await;
                    let peer = remote.peers.entry(event.id).or_default();

                    peer.transform = Transform::origin();
                }

                b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::Join {
                    peer_id: event.id,
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
            Event::Moved => {
                let event = body.decode::<UpdatedTransform>()?;
                tracing::debug!(?event.id, ?event.transform, "moved");

                {
                    let mut remote = b.client_state().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        peer.transform = event.transform;
                    }
                }

                b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::Moved {
                    peer_id: event.id,
                    transform: event.transform,
                }));
            }
            Event::LookedAt => {
                let event = body.decode::<UpdatedLookAt>()?;
                tracing::debug!(?event.id, ?event.look_at, "looked at");

                {
                    let mut remote = b.client_state().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        peer.look_at = event.look_at;
                    }
                }

                b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::LookAt {
                    peer_id: event.id,
                    look_at: event.look_at,
                }));
            }
            Event::UpdatedImage => {
                let event = body.decode::<UpdatedImageBody>()?;
                tracing::debug!(?event.id, image = ?event.image.as_ref().map(|i| i.len()), "updated image");

                let image = {
                    let mut remote = b.client_state().await;
                    let mut images = b.images().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        let image = if let Some(data) = event.image {
                            Some(images.store(data))
                        } else {
                            None
                        };

                        if let Some(old) = mem::replace(&mut peer.image, image) {
                            images.remove(old);
                        }

                        image
                    } else {
                        None
                    }
                };

                b.broadcast(BackendEvent::RemoteAvatar(
                    RemoteAvatarEvent::ImageUpdated {
                        peer_id: event.id,
                        image,
                    },
                ));
            }
            Event::UpdatedColor => {
                let event = body.decode::<UpdatedColorBody>()?;
                tracing::debug!(?event.id, color = ?event.color, "updated color");

                {
                    let mut remote = b.client_state().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        peer.color = event.color.clone();
                    }
                }

                b.broadcast(BackendEvent::RemoteAvatar(
                    RemoteAvatarEvent::ColorUpdated {
                        peer_id: event.id,
                        color: event.color,
                    },
                ));
            }
            Event::UpdatedName => {
                let event = body.decode::<UpdatedNameBody>()?;
                tracing::debug!(?event.id, name = ?event.name, "updated name");

                {
                    let mut remote = b.client_state().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        peer.name = event.name.clone();
                    }
                }

                b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::NameUpdated {
                    peer_id: event.id,
                    name: event.name,
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
pub(crate) async fn run(b: Backend, connect: Option<String>) -> Result<()> {
    let Some(connect) = connect else {
        tracing::info!("remote client disabled");
        future::pending::<()>().await;
        return Ok(());
    };

    let port;

    let host = if let Some((host, port_s)) = connect.rsplit_once(':') {
        port = port_s.parse::<u16>().context("invalid port number")?;
        host
    } else {
        port = 44114u16;
        connect.as_str()
    };

    let player;

    {
        let mut remote = b.client_state().await;
        remote.peers.clear();
        player = remote.player.clone();
    }

    b.broadcast(BackendEvent::RemoteAvatar(RemoteAvatarEvent::RemoteLost));

    tracing::info!(?host, ?port, "Connecting to mumbler-server");

    let client = Client::connect((host, port))
        .await
        .with_context(|| anyhow!("Connecting to {host}:{port}"))?;

    let addr = client.addr()?;

    tracing::info!(?addr, "Connected");

    let mut peer = Peer::new(addr, client);
    peer.connect(b"default")?;

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;
    let mut wait = pin!(b.client_wait());

    peer.update_transform(player.transform)?;
    peer.update_look_at(player.look_at)?;

    let image = 'image: {
        let Some(image) = player.image else {
            break 'image None;
        };

        b.db().get_image_data(image).await?
    };

    peer.update_image(image)?;
    peer.update_color(player.color.clone())?;
    peer.update_name(player.name.clone())?;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;
                handle_peer(&mut peer, &b, &mut last_ping, ping_timeout.as_mut()).await?;
            }
            () = wait.as_mut() => {
                let state = b.take_client_player().await;

                if state.is_transform() {
                    peer.update_transform(state.transform)?;
                }

                if state.is_look_at() {
                    peer.update_look_at(state.look_at)?;
                }

                if state.is_image() {
                    let image = 'image: {
                        let Some(image) = state.image else {
                            break 'image None;
                        };

                        b.db().get_image_data(image).await?
                    };

                    peer.update_image(image)?;
                }

                if state.is_color() {
                    peer.update_color(state.color)?;
                }

                if state.is_name() {
                    peer.update_name(state.name.clone())?;
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
    let settings = async || -> Result<(Option<String>, bool)> {
        let connect = b
            .db()
            .get_config::<String>("remote/server")
            .await?
            .as_deref()
            .or(default_connect)
            .map(str::to_owned);

        let enabled = b
            .db()
            .get_config::<bool>("remote/enabled")
            .await?
            .unwrap_or(true);

        Ok((connect, enabled))
    };

    let (mut connect, mut enabled) = settings().await?;

    let mut future = pin!(run(b.clone(), connect.clone()));
    let mut reconnect = pin!(time::sleep(Duration::from_secs(0)));
    let mut active = enabled;

    loop {
        tokio::select! {
            result = future.as_mut(), if active => {
                if let Err(error) = result {
                    tracing::error!(%error, "Client errored");
                    b.notify_error(COMPONENT, format_args!("{error:#}"));
                } else {
                    b.notify_info(COMPONENT, "Disconnected from server");
                }

                active = false;
                reconnect.as_mut().reset(Instant::now() + Duration::from_secs(5));
                tracing::info!("Client disconnected, reconnecting in 5s");
            }
            _ = reconnect.as_mut(), if !active && enabled => {
                b.notify_info(COMPONENT, "Reconnecting to server");
                future.set(run(b.clone(), connect.clone()));
                active = true;
            }
            () = b.client_restart_wait() => {
                (connect, enabled) = settings().await?;

                if enabled {
                    b.notify_info(COMPONENT, "Restarting");
                } else {
                    b.notify_info(COMPONENT, "Disabled");
                }

                tracing::info!(?connect, %enabled, "Remote client config updated");

                if enabled {
                    future.set(run(b.clone(), connect.clone()));
                    active = true;
                } else {
                    active = false;
                }
            }
        }
    }
}
