use core::pin::pin;
use core::time::Duration;
use std::env;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use clap::Parser;
use directories_next::ProjectDirs;
use mumbler::{Backend, Database, Paths, client};
use tokio::runtime::Builder;
use tokio::time;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const DEFUALT_FILTER: &str = "mumbler=info";

#[derive(Parser)]
struct Opts {
    /// Use an in-memory database.
    #[arg(long)]
    memory: bool,
    /// Configuration directory.
    #[arg(long, name = "config")]
    config: Option<PathBuf>,
    /// Print project paths.
    #[arg(long)]
    paths: bool,
    /// Work as development server.
    #[arg(long)]
    dev: bool,
}

pub fn main() -> Result<()> {
    let opts = Opts::parse();

    let builder = EnvFilter::builder().with_default_directive(LevelFilter::INFO.into());
    let env_filter;

    if let Ok(log) = env::var("MUMBLER_LOG") {
        env_filter = builder.parse(log).context("parsing MUMBLER_LOG")?;
    } else {
        env_filter = builder
            .parse(DEFUALT_FILTER)
            .context("parsing default log filter")?;
    }

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let dirs =
        ProjectDirs::from("se.tedro", "setbac", "Mumbler").context("missing project dirs")?;

    let config = match &opts.config {
        Some(config) => config,
        None => dirs.config_dir(),
    };

    let paths = Arc::new(Paths::new(config));

    if opts.paths {
        println!("Database: {}", paths.db.display());
        return Ok(());
    }

    let runtime = Builder::new_current_thread().enable_all().build()?;
    let c = Database::open(&paths, opts.memory)?;
    let b = Backend::new(c, paths);

    runtime.block_on(async move {
        let mut client = pin!(client::run(b.clone()));
        let mut reconnect_timeout = pin!(time::sleep(Duration::from_secs(0)));
        let mut client_setup = true;
        let mut mumbler = pin!(mumbler::run(b.clone(), !opts.dev));

        loop {
            tokio::select! {
                result = client.as_mut(), if client_setup => {
                    if let Err(error) = result {
                        tracing::error!(%error, "client errored");
                    }

                    client_setup = false;
                    reconnect_timeout.as_mut().reset(time::Instant::now() + Duration::from_secs(5));
                    tracing::info!("shutting down client, trying to reconnect in 5s");
                },
                _ = reconnect_timeout.as_mut(), if !client_setup => {
                    tracing::info!("reconnecting client");
                    client.set(client::run(b.clone()));
                    reconnect_timeout.as_mut().reset(time::Instant::now() + Duration::from_secs(0));
                    client_setup = true;
                }
                result = mumbler.as_mut() => {
                    result.context("mumbler")?;
                    bail!("mumbler exited, shutting down");
                }
            }
        }
    })
}
