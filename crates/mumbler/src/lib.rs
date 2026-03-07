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

use core::pin::pin;
use std::net::SocketAddr;

use anyhow::{Result, bail};
use tokio::net::TcpListener;

use self::web::default_bind;

pub async fn run(b: Backend, bundle: bool, bind: &str) -> Result<()> {
    let addr: SocketAddr = default_bind(bundle, bind).parse()?;

    tracing::info!("Listening on http://{addr}");

    let listener = TcpListener::bind(addr).await?;
    let mut future = pin!(web::setup(listener, b, bundle)?);

    tokio::select! {
        result = future.as_mut() => {
            result?;
        }
    }

    bail!("web exited unexpectedly");
}
