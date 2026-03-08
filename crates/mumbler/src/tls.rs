use std::path::Path;
use std::sync::Arc;

use anyhow::{Context as _, Result, anyhow, bail};

use rustls::ServerConfig;
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use tokio::fs;

/// TLS acceptor.
pub use tokio_rustls::TlsAcceptor;

/// Load TLS configuration from two paths.
async fn load_config(cert_path: &Path, key_path: &Path) -> Result<ServerConfig> {
    let cert = fs::read(cert_path)
        .await
        .with_context(|| anyhow!("reading certificate from {}", cert_path.display()))?;

    let certs = CertificateDer::pem_slice_iter(&cert)
        .collect::<Result<Vec<_>, _>>()
        .with_context(|| anyhow!("parsing certificate from {}", cert_path.display()))?;

    let key = fs::read(key_path)
        .await
        .with_context(|| anyhow!("reading key from {}", key_path.display()))?;

    let key = PrivateKeyDer::from_pem_slice(&key)
        .with_context(|| anyhow!("parsing key from {}", key_path.display()))?;

    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;

    Ok(config)
}

pub(crate) async fn setup_acceptor(
    cert_path: Option<&Path>,
    key_path: Option<&Path>,
) -> Result<TlsAcceptor> {
    let Some(cert_path) = cert_path else {
        bail!("TLS connector is configured, but no certificate path provided");
    };

    let Some(key_path) = key_path else {
        bail!("TLS connector is configured, but no private key path provided");
    };

    let config = load_config(cert_path, key_path).await?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
