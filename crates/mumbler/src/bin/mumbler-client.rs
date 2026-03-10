//! This is a simple client which connects to a mumbler relay service and prints
//! the commands that it receives.

use core::pin::pin;

use anyhow::{Context, Result, bail};
use clap::Parser;
use mumbler::remote::api::{Event, JoinBody, LeaveBody, PongBody, UpdatedPeer};
use mumbler::remote::{Client, DEFAULT_PORT, DEFAULT_TLS_PORT, Peer};
use tokio::net::TcpStream;
use tokio::time::{self, Duration, Instant};
use tracing::Level;

#[derive(Parser)]
struct Opts {
    /// The room to join.
    #[clap(short, long, default_value = "default")]
    room: String,
    /// The server to connect to.
    #[clap(default_value = "localhost")]
    connect: String,
    /// Enable debug logging.
    #[clap(long)]
    debug: bool,
    /// Use a TLS connection.
    #[clap(long)]
    tls: bool,
    /// Override the TLS server name to expect.
    #[clap(long)]
    tls_name: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<()> {
    let opts = Opts::parse();

    let level = if opts.debug {
        Level::DEBUG
    } else {
        Level::INFO
    };

    tracing_subscriber::fmt().with_max_level(level).init();

    let port;

    let host = if let Some((host, port_s)) = opts.connect.rsplit_once(':') {
        port = port_s.parse::<u16>().context("invalid port number")?;
        host
    } else {
        port = if opts.tls {
            DEFAULT_TLS_PORT
        } else {
            DEFAULT_PORT
        };
        &opts.connect
    };

    let stream = TcpStream::connect((host, port)).await?;

    let client = if opts.tls {
        let name = opts.tls_name.as_deref().unwrap_or(host);
        Client::default_tls_connect(stream, name)
            .await
            .context("Opening TLS connection")?
    } else {
        Client::plain(stream)
    };

    let addr = client.addr()?;

    tracing::info!(tls = opts.tls, ?addr, "connected");

    let values = Vec::new();

    let mut peer = Peer::new(client);
    peer.connect(opts.room.as_bytes(), values)?;

    let mut ping_timeout = pin!(time::sleep(Duration::from_secs(1)));
    let mut pong_timeout = pin!(time::sleep(Duration::from_secs(0)));
    let mut last_ping = None;

    loop {
        tokio::select! {
            result = peer.ready() => {
                result?;

                while let Some((event, body)) = peer.read::<Event>()? {
                    tracing::debug!(?event, "Received event");

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
                            tracing::debug!(?event, "Join");
                        }
                        Event::Leave => {
                            let event = body.decode::<LeaveBody>()?;
                            tracing::debug!(?event, "Leave");
                        }
                        Event::Updated => {
                            let event = body.decode::<UpdatedPeer>()?;
                            tracing::debug!(?event.peer_id, ?event.key, ?event.value, "Updated");
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
