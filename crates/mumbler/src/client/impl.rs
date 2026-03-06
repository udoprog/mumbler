use core::pin::pin;
use core::time::Duration;

use anyhow::{Result, bail};
use tokio::time::{self, Instant};

use crate::Backend;
use crate::remote::api::{Event, PongBody};
use crate::remote::{Client, Peer};

pub async fn run(b: Backend) -> Result<()> {
    let client = Client::connect("localhost:44114").await?;
    let addr = client.addr()?;

    tracing::info!(?addr, "connected");

    let mut peer = Peer::new(addr, client);
    peer.connect(b"default")?;

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;

                while let Some((event, body)) = peer.handle::<Event>()? {
                    match event {
                        Event::Pong => {
                            let pong = body.decode::<PongBody>()?;

                            if Some(pong.payload) == last_ping {
                                last_ping = None;
                                ping_timeout.as_mut().reset(Instant::now() + Duration::from_secs(1));
                            }
                        },
                        event => {
                            tracing::info!(?event);
                        }
                    }
                }
            }
            ev = b.event() => {
                let Some(ev) = ev else {
                    bail!("backend event stream ended");
                };

                tracing::info!(?ev);

                match ev {
                    crate::backend::Event::Move(position, front) => {
                        peer.move_to(position, front)?;
                    }
                }
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
