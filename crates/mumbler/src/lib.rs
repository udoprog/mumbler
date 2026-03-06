mod web;

mod database;
pub use database::Database;

mod backend;
pub use self::backend::Backend;

mod paths;
pub use self::paths::Paths;

use core::pin::pin;
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use tokio::net::TcpListener;

use self::web::default_bind;

pub async fn run(b: Backend, bundle: bool) -> Result<()> {
    let addr: SocketAddr = default_bind(bundle).parse()?;

    let backend = Arc::new(b);

    tracing::info!("Listening on http://{addr}");

    let listener = TcpListener::bind(addr).await?;
    let mut future = pin!(web::setup(listener, backend.clone(), bundle)?);

    tokio::select! {
        result = future.as_mut() => {
            result?;
            tracing::info!("Web shut down gracefully");
        }
    }

    Ok(())
}
