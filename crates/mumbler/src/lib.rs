#![allow(clippy::async_yields_async)]
#![allow(clippy::single_match)]

mod web;

mod database;
pub use database::Database;

mod backend;
pub use self::backend::Backend;

mod paths;
pub use self::paths::Paths;

pub mod remote;

pub mod client;

pub mod mumblelink;

#[cfg(feature = "tls")]
mod tls;
#[cfg(not(feature = "tls"))]
#[path = "tls/disabled.rs"]
mod tls;

use core::pin::pin;

use anyhow::{Result, bail};
use tokio::net::TcpListener;

use self::web::default_bind;

pub async fn run(b: Backend, dev: bool, bind: &str) -> Result<()> {
    let (host, port, open_port) = default_bind(dev, bind)?;

    tracing::info!("Listening on http://{host}:{port}");
    webbrowser::open(&format!("http://{host}:{open_port}"))?;

    let listener = TcpListener::bind((host, port)).await?;
    let mut future = pin!(web::setup(listener, b, dev)?);

    tokio::select! {
        result = future.as_mut() => {
            result?;
        }
    }

    bail!("web exited unexpectedly");
}
