use std::path::Path;

use anyhow::{Result, bail};

#[derive(Clone)]
pub struct TlsAcceptor;

pub(crate) async fn setup_acceptor(
    cert_path: Option<&Path>,
    key_path: Option<&Path>,
) -> Result<TlsAcceptor> {
    _ = cert_path;
    _ = key_path;
    bail!("Cannot setup TLS connector because the TLS feature is not enabled")
}
