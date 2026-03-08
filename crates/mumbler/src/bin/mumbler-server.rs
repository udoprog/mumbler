use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use mumbler::remote::server::ConnectorConfig;
use tokio::task::LocalSet;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const DEFUALT_FILTER: &str = "mumbler=info";

#[derive(Parser)]
struct Opts {
    /// Specify the host to bind to.
    #[clap(long, default_value = "127.0.0.1")]
    bind: String,
    /// Enable TLS when setting up the server.
    #[clap(long)]
    tls: bool,
    /// Path to TLS certificate in PEM format.
    #[clap(long, short = 'c')]
    cert: Option<PathBuf>,
    /// Path to TLS private key in PEM format.
    #[clap(long, short = 'k')]
    key: Option<PathBuf>,
}

fn main() -> Result<()> {
    let opts = Opts::parse();

    let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());
    let env_filter;

    if let Ok(log) = env::var("MUMBLER_SERVER_LOG") {
        env_filter = builder.parse(log).context("parsing MUMBLER_SERVER_LOG")?;
    } else {
        env_filter = builder
            .parse(DEFUALT_FILTER)
            .context("parsing default log filter")?;
    }

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("building runtime")?;

    let mut connectors = Vec::new();

    connectors.push(ConnectorConfig {
        bind: opts.bind.as_str(),
        port: None,
        tls: false,
        cert: None,
        key: None,
    });

    if opts.tls {
        connectors.push(ConnectorConfig {
            bind: opts.bind.as_str(),
            port: None,
            tls: true,
            cert: opts.cert.as_deref(),
            key: opts.key.as_deref(),
        });
    }

    runtime.block_on(async move {
        let local = LocalSet::new();

        local
            .run_until(mumbler::remote::server::run(connectors))
            .await
            .context("Running server")
    })?;

    Ok(())
}
