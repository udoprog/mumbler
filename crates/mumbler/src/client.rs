use core::mem;
use core::pin::{Pin, pin};
use core::time::Duration;

use anyhow::{Result, bail};
use api::Vec3;
use tokio::time::{self, Instant, Sleep};

use crate::Backend;
use crate::backend::BackendEvent;
use crate::remote::api::{Event, JoinBody, LeaveBody, MovedToBody, PongBody, UpdatedImageBody};
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
                tracing::info!(?event.id, "join");

                {
                    let mut remote = b.state().await;
                    let peer = remote.peers.entry(event.id).or_default();

                    peer.position = Vec3::ZERO;
                    peer.front = Vec3::FORWARD;
                }

                b.broadcast(BackendEvent::Join { peer_id: event.id });
            }
            Event::Leave => {
                let event = body.decode::<LeaveBody>()?;
                tracing::info!(?event.id, "leave");

                {
                    let mut remote = b.state().await;
                    remote.peers.remove(&event.id);
                }

                b.broadcast(BackendEvent::Leave { peer_id: event.id });
            }
            Event::Moved => {
                let event = body.decode::<MovedToBody>()?;
                tracing::info!(?event.id, ?event.position, ?event.front, "moved");

                {
                    let mut remote = b.state().await;

                    if let Some(peer) = remote.peers.get_mut(&event.id) {
                        peer.position = event.position;
                        peer.front = event.front;
                    }
                }

                b.broadcast(BackendEvent::Moved {
                    peer_id: event.id,
                    position: event.position,
                    front: event.front,
                });
            }
            Event::UpdatedImage => {
                let event = body.decode::<UpdatedImageBody>()?;
                tracing::info!(?event.id, image = ?event.image.as_ref().map(|i| i.len()), "updated image");

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
            event => {
                tracing::info!(?event);
            }
        }
    }

    Ok(())
}

#[tracing::instrument(skip_all)]
pub async fn run(b: Backend) -> Result<()> {
    let player;

    {
        let mut remote = b.state().await;
        remote.peers.clear();
        player = remote.player;
    }

    b.broadcast(BackendEvent::RemoteLost);

    let client = Client::connect("localhost:44114").await?;
    let addr = client.addr()?;

    tracing::info!(?addr, "connected");

    let mut peer = Peer::new(addr, client);
    peer.connect(b"default")?;

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;
    let mut wait = pin!(b.wait());

    peer.move_to(player.position, player.front)?;

    let image = 'image: {
        let Some(image) = player.image else {
            break 'image None;
        };

        b.db().get_image_data(image).await?
    };

    peer.update_image(image)?;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;
                handle_peer(&mut peer, &b, &mut last_ping, ping_timeout.as_mut()).await?;
            }
            () = wait.as_mut() => {
                let state = b.take_player().await;

                if state.is_translated() {
                    peer.move_to(state.position, state.front)?;
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

                wait.set(b.wait());
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
