use std::future::pending;

use anyhow::Result;

use crate::Backend;

pub async fn run(backend: Backend) -> Result<()> {
    pending::<()>().await;
    Ok(())
}
