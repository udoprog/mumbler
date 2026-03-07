use core::mem;
use core::pin::{Pin, pin};
use core::time::Duration;

use anyhow::{Context as _, Result, bail};
use api::Transform;
use tokio::time::{self, Instant, Sleep};

use crate::Backend;
use crate::backend::BackendEvent;
use crate::remote::api::{
    Event, JoinBody, LeaveBody, PongBody, UpdatedColorBody, UpdatedImageBody, UpdatedTransform,
};
use crate::remote::{Client, Peer};

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
                    let mut remote = b.state().await;
                    let peer = remote.peers.entry(event.id).or_default();

                    peer.transform = Transform::origin();
                }

                b.broadcast(BackendEvent::Join { peer_id: event.id });
            }
            Event::Leave => {
                let event = body.decode::<LeaveBody>()?;
                tracing::debug!(?event.id, "leave");

                {
                    let mut remote = b.state().await;
                    remote.peers.remove(&event.id);
                }

                b.broadcast(BackendEvent::Leave { peer_id: event.id });
            }
            Event::Moved => {
                let event = body.decode::<UpdatedTransform>()?;
                tracing::debug!(?event.id, ?event.transform, "moved");

                {
                    let mut remote = b.state().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        peer.transform = event.transform;
                    }
                }

                b.broadcast(BackendEvent::Moved {
                    peer_id: event.id,
                    transform: event.transform,
                });
            }
            Event::UpdatedImage => {
                let event = body.decode::<UpdatedImageBody>()?;
                tracing::debug!(?event.id, image = ?event.image.as_ref().map(|i| i.len()), "updated image");

                let image = {
                    let mut remote = b.state().await;
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

                b.broadcast(BackendEvent::ImageUpdated {
                    peer_id: event.id,
                    image,
                });
            }
            Event::UpdatedColor => {
                let event = body.decode::<UpdatedColorBody>()?;
                tracing::debug!(?event.id, color = ?event.color, "updated color");

                {
                    let mut remote = b.state().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        peer.color = event.color.clone();
                    }
                }

                b.broadcast(BackendEvent::ColorUpdated {
                    peer_id: event.id,
                    color: event.color,
                });
            }
            event => {
                tracing::debug!(?event);
            }
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn run(b: Backend, connect: &str) -> Result<()> {
    let port;

    let host = if let Some((host, port_s)) = connect.rsplit_once(':') {
        port = port_s.parse::<u16>().context("invalid port number")?;
        host
    } else {
        port = 44114u16;
        connect
    };

    let player;

    {
        let mut remote = b.state().await;
        remote.peers.clear();
        player = remote.player.clone();
    }

    b.broadcast(BackendEvent::RemoteLost);

    tracing::info!(?host, ?port, "connecting to mumbler-server");

    let client = Client::connect((host, port)).await?;
    let addr = client.addr()?;

    tracing::info!(?addr, "connected");

    let mut peer = Peer::new(addr, client);
    peer.connect(b"default")?;

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;
    let mut wait = pin!(b.client_wait());

    peer.update_transform(player.transform)?;

    let image = 'image: {
        let Some(image) = player.image else {
            break 'image None;
        };

        b.db().get_image_data(image).await?
    };

    peer.update_image(image)?;
    peer.update_color(player.color.clone())?;

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
