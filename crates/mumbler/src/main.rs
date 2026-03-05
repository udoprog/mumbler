use std::env;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;
use directories_next::ProjectDirs;
use mumbler::{Backend, Database, Paths};
use tokio::runtime::Builder;
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

    let paths = Paths::new(&config);

    if opts.paths {
        println!("Database: {}", paths.db.display());
        return Ok(());
    }

    let runtime = Builder::new_current_thread().enable_all().build()?;
    let c = Database::open(&paths, opts.memory)?;
    let b = Backend::new(c, paths);

    runtime.block_on(async move {
        mumbler::run(b, !opts.dev).await?;
        Ok(())
    })
}
