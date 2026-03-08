use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use directories_next::ProjectDirs;
use mumbler::{Backend, Database, Paths, client, mumblelink};
use tokio::runtime::Builder;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
struct Opts {
    /// Enable debug logging.
    #[arg(long)]
    debug: bool,
    /// Specify custom logging directives.
    #[arg(long)]
    log: Vec<String>,
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
    #[arg(long, default_value = "127.0.0.1:41114")]
    bind: String,
    #[arg(long)]
    connect: Option<String>,
}

pub fn main() -> Result<()> {
    let opts = Opts::parse();

    let (default_level, default_filter) = if opts.debug {
        (LevelFilter::DEBUG, "")
    } else {
        (LevelFilter::INFO, "mumbler=info")
    };

    let builder = EnvFilter::builder().with_default_directive(default_level.into());

    let mut env_filter;

    if let Ok(log) = env::var("MUMBLER_LOG") {
        env_filter = builder.parse(log).context("parsing MUMBLER_LOG")?;
    } else {
        env_filter = builder
            .parse(default_filter)
            .context("parsing default log filter")?;
    }

    for log in opts.log {
        env_filter = env_filter.add_directive(log.parse().context("parsing --log directive")?);
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

    let paths = Paths::new(config);

    if opts.paths {
        println!("Database: {}", paths.db.display());
        return Ok(());
    }

    let runtime = Builder::new_current_thread().enable_all().build()?;
    let c = Database::open(&paths, opts.memory)?;

    runtime.block_on(async move {
        let b = Backend::new(c, paths).await?;

        tokio::try_join!(
            client::managed(b.clone(), opts.connect.as_deref()),
            mumblelink::managed(b.clone()),
            mumbler::run(b, !opts.dev, &opts.bind),
        )?;

        Ok(())
    })
}
