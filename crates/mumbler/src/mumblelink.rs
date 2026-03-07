use core::mem;
use core::pin::pin;

use anyhow::{Context as _, Result};
use mumblelink::{Link, Position};
use tokio::time::{self, Duration};

use crate::Backend;

/// Component name for notifications.
const COMPONENT: &str = "mumble-link";

/// Number of mumble updates per second.
const UPDATES_PER_SECOND: u64 = 20;

#[tracing::instrument(skip_all)]
pub(crate) async fn run(b: Backend) -> Result<()> {
    let mut enabled = b.mumblelink_state().await.enabled;

    let mut link = if enabled {
        Link::new().context("Creating link")?
    } else {
        Link::disabled()
    };

    let mut pos = Position::FORWARD;
    pos.position = [0., 0., 0.];

    let setup_link = |link: &mut Link, pos| {
        link.set_identity("mumbler");
        link.set_context(b"");
        link.set_name("Mumbler");
        link.set_description("Test link from mumbler");
        link.set_avatar(pos);
        link.set_camera(pos);
    };

    setup_link(&mut link, pos);

    let mut update_interval = time::interval(Duration::from_millis(1000 / UPDATES_PER_SECOND));
    let mut update_all_interval = time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = update_interval.tick(), if enabled => {
                link.update();
            }
            _ = update_all_interval.tick(), if enabled => {
                link.update_all()?;
            }
            () = b.mumblelink_wait() => {
                let mut state = b.mumblelink_state().await;

                if enabled != state.enabled {
                    enabled = state.enabled;

                    if enabled {
                        link = Link::new()?;
                        setup_link(&mut link, pos);
                    } else {
                        link = Link::disabled();
                    }

                    if enabled {
                        update_interval.reset();
                        update_all_interval.reset();
                        b.notify_info(COMPONENT, "Mumblelink enabled");
                    } else {
                        b.notify_info(COMPONENT, "Mumblelink disabled");
                    }
                }

                if mem::take(&mut state.restart) {
                    b.notify_info(COMPONENT, "Mumblelink restarted");
                    tracing::info!("restarting link");
                    link.reconnect().context("Reconnecting link")?;
                    setup_link(&mut link, pos);
                }

                pos.position = state.transform.position.as_array();
                pos.front = state.transform.front.as_array();
                link.set_avatar(pos);
                link.set_camera(pos);
            }
        }
    }
}

/// Runs mumblelink and automatically restarts it on error with a 5-second
/// back-off.
pub async fn managed(b: Backend) -> Result<()> {
    let mut future = pin!(run(b.clone()));
    let mut reconnect = pin!(time::sleep(Duration::from_secs(0)));
    let mut active = true;

    loop {
        tokio::select! {
            result = future.as_mut(), if active => {
                if let Err(error) = result {
                    tracing::error!(%error, "mumblelink errored");
                    b.notify_error(COMPONENT, format_args!("{error:#}"));
                }

                tracing::info!("mumblelink stopped, restarting in 5s");
                reconnect.as_mut().reset(time::Instant::now() + Duration::from_secs(5));
                active = false;
            }
            _ = reconnect.as_mut(), if !active => {
                b.notify_info(COMPONENT, "Reconnecting");
                future.set(run(b.clone()));
                active = true;
            }
        }
    }
}
