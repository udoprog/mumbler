#![allow(clippy::async_yields_async)]
#![allow(clippy::single_match)]
#![allow(clippy::collapsible_match)]

mod web;

mod auth_store;

mod database;
pub use database::Database;

mod backend;
pub use self::backend::Backend;

mod paths;
pub use self::paths::Paths;

pub mod remote;

pub mod client;

pub mod mumblelink;

pub mod crypto;

#[cfg(feature = "tls")]
mod tls;
#[cfg(not(feature = "tls"))]
#[path = "tls/disabled.rs"]
mod tls;

use core::pin::pin;

use std::net::IpAddr;

use anyhow::{Result, bail};
use auth::{Auth, Bootstrap, BootstrapAdmin, CookieConfig};
use tokio::net::TcpListener;

use self::auth_store::AuthStore;
use self::web::default_bind;

/// The login of the admin created on the first run that needs sign-in.
const BOOTSTRAP_ADMIN: &str = "admin";

pub async fn run(
    b: Backend,
    dev: bool,
    bind: &str,
    no_open: bool,
    recover: Option<&str>,
) -> Result<()> {
    let (host, port, open_port) = default_bind(dev, bind)?;

    let auth = if is_loopback(host) {
        if recover.is_some() {
            tracing::warn!("--recover does nothing on a loopback address, which needs no sign-in");
        }

        None
    } else {
        Some(setup_auth(&b, host, port, recover).await?)
    };

    tracing::info!("listening on http://{host}:{port}");

    #[cfg(feature = "webbrowser")]
    if !no_open {
        webbrowser::open(&format!("http://{host}:{open_port}"))?;
    }

    let listener = TcpListener::bind((host, port)).await?;
    let mut future = pin!(web::setup(listener, b, dev, auth)?);

    tokio::select! {
        result = future.as_mut() => {
            result?;
        }
    }

    bail!("web exited unexpectedly");
}

/// Whether `host` only accepts connections from this machine, which is when
/// the web UI needs no sign-in.
fn is_loopback(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');

    match host.parse::<IpAddr>() {
        Ok(ip) => ip.to_canonical().is_loopback(),
        Err(_) => host.eq_ignore_ascii_case("localhost"),
    }
}

/// Sign-in over the database's users. On a first run this creates the
/// bootstrap admin and logs the link to register it with, and `recover` logs
/// a new link for the named user.
async fn setup_auth(b: &Backend, host: &str, port: u16, recover: Option<&str>) -> Result<Auth> {
    let store = AuthStore::new(b.db().clone());

    let auth = Auth::builder_from_store(store)
        .await?
        .cookie(CookieConfig::new("mumbler_session"))
        .build();

    let unspecified = host
        .trim_start_matches('[')
        .trim_end_matches(']')
        .parse::<IpAddr>()
        .is_ok_and(|ip| ip.is_unspecified());

    let public_url = (!unspecified).then(|| format!("http://{host}:{port}"));
    let public_url = public_url.as_deref();

    tracing::info!("{host} is not a loopback address, so the web UI requires sign-in");

    let bootstrap = auth
        .bootstrap_admin(&BootstrapAdmin::login(BOOTSTRAP_ADMIN))
        .await?;

    match &bootstrap {
        Bootstrap::Created(link) | Bootstrap::Reissued(link) => {
            tracing::info!(
                "Open {} to choose a password for `{BOOTSTRAP_ADMIN}`, before {}",
                link.url(public_url),
                link.expires_at
            );
        }
        Bootstrap::Pending { expires_at } => {
            tracing::info!(
                "`{BOOTSTRAP_ADMIN}` has not registered with the link logged earlier, which works until {expires_at}; \
                 run with `--recover {BOOTSTRAP_ADMIN}` for a new one"
            );
        }
        Bootstrap::Skipped => {}
    }

    if let Some(name) = recover {
        let link = auth.recovery_link(name).await?;

        tracing::info!(
            "Open {} to choose a new password for `{name}`, before {}",
            link.url(public_url),
            link.expires_at
        );
    }

    Ok(auth)
}
