use std::env;

use anyhow::{Context, Result};
use clap::Parser;
use tokio::task::LocalSet;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::EnvFilter;

const DEFUALT_FILTER: &str = "server=info";

#[derive(Parser)]
struct Opts {
    #[clap(long, default_value = "127.0.0.1:44114")]
    bind: String,
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

    runtime.block_on(async move {
        let local = LocalSet::new();
        local.run_until(server::run(&opts.bind)).await
    })?;
    Ok(())
}
