use core::pin::pin;

use anyhow::{Result, bail};
use mumbler::remote::api::{Event, PongBody};
use mumbler::remote::{Client, Peer};
use tokio::time::{self, Duration, Instant};
use tracing::Level;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    let client = Client::connect("localhost:44114").await?;
    let addr = client.addr()?;

    let mut peer = Peer::new(addr, client);

    peer.connect()?;

    let mut send_ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;

                while let Some((event, body)) = peer.handle::<Event>()? {
                    tracing::info!(?event, "received event");

                    match event {
                        Event::Pong => {
                            let pong = body.decode::<PongBody>()?;

                            if Some(pong.payload) == last_ping {
                                last_ping = None;
                                send_ping_timeout.as_mut().reset(Instant::now() + Duration::from_secs(1));
                            }
                        },
                        unsupported => {
                            bail!("unsupported event: {unsupported:?}");
                        }
                    }
                }
            }
            _ = send_ping_timeout.as_mut(), if last_ping.is_none() => {
                tracing::info!("sending ping");

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
