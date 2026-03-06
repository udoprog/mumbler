//! This is a simple client which connects to a mumbler relay service and prints
//! the commands that it receives.

use core::pin::pin;

use anyhow::{Context, Result, bail};
use clap::Parser;
use mumbler::remote::api::{
    Event, JoinBody, LeaveBody, PongBody, UpdatedImageBody, UpdatedTransform,
};
use mumbler::remote::{Client, Peer};
use tokio::time::{self, Duration, Instant};
use tracing::Level;

#[derive(Parser)]
struct Opts {
    /// The room to join.
    #[clap(short, long, default_value = "default")]
    room: String,
    /// The server to connect to.
    #[clap(default_value = "localhost:44114")]
    connect: String,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let opts = Opts::parse();

    tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .init();

    let port;

    let host = if let Some((host, port_s)) = opts.connect.rsplit_once(':') {
        port = port_s.parse::<u16>().context("invalid port number")?;
        host
    } else {
        port = 44114u16;
        &opts.connect
    };

    let client = Client::connect((host, port)).await?;
    let addr = client.addr()?;

    tracing::info!(?addr, "connected");

    let mut peer = Peer::new(addr, client);
    peer.connect(opts.room.as_bytes())?;

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
                        Event::Join => {
                            let event = body.decode::<JoinBody>()?;
                            tracing::debug!(?event, "join");
                        }
                        Event::Leave => {
                            let event = body.decode::<LeaveBody>()?;
                            tracing::debug!(?event, "leave");
                        }
                        Event::Moved => {
                            let event = body.decode::<UpdatedTransform>()?;
                            tracing::debug!(?event, "moved");
                        }
                        Event::UpdatedImage => {
                            let event = body.decode::<UpdatedImageBody>()?;
                            tracing::debug!(?event, "updated image");
                        }
                        event => {
                            tracing::debug!(?event);
                        }
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
