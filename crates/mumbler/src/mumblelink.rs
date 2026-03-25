use core::pin::pin;

use anyhow::{Context as _, Result};
use api::Key;
use async_fuse::Fuse;
use mumblelink::{Link, Position};
use tokio::time::{self, Duration};

use crate::Backend;

/// Component name for notifications.
const COMPONENT: &str = "mumble-link";

/// Number of mumble updates per second.
const UPDATES_PER_SECOND: u64 = 20;

#[tracing::instrument(skip_all)]
pub(crate) async fn run(b: Backend) -> Result<()> {
    let mut link = Link::new().context("creating link")?;

    let transform = 'transform: {
        let id = b.mumble_object();

        if id.is_zero() {
            break 'transform None;
        };

        let state = b.client_state().await;

        let Some(object) = state.objects.get(&id) else {
            break 'transform None;
        };

        let hidden = object.props.get(Key::HIDDEN).as_bool();
        let local_hidden = object.props.get(Key::LOCAL_HIDDEN).as_bool();

        if hidden || local_hidden {
            None
        } else {
            object.props.get(Key::TRANSFORM).as_transform()
        }
    };

    let mut pos = Position::FORWARD;
    pos.position = [0., 0., 0.];

    link.set_identity("mumbler");
    link.set_context(b"");
    link.set_name("Mumbler");
    link.set_description("Test link from mumbler");

    if let Some(transform) = transform {
        pos.position = *transform.position.as_array();
        pos.front = *transform.front.as_array();
        link.set_avatar(pos);
        link.set_camera(pos);
    } else {
        link.disable();
    }

    let mut update_interval = time::interval(Duration::from_millis(1000 / UPDATES_PER_SECOND));
    let mut update_all_interval = time::interval(Duration::from_secs(5));

    loop {
        tokio::select! {
            _ = update_interval.tick() => {
                link.update();
            }
            _ = update_all_interval.tick() => {
                link.update_all()?;
            }
            () = b.mumblelink_wait() => {
                let state = b.mumblelink_state().await;

                if let Some(transform) = state.transform {
                    pos.position = *transform.position.as_array();
                    pos.front = *transform.front.as_array();
                    link.set_avatar(pos);
                    link.set_camera(pos);
                    link.enable();
                } else {
                    link.disable();
                }
            }
        }
    }
}

/// Runs mumblelink and automatically restarts it on error with a 5-second
/// back-off. Reads `mumble/enabled` from the database and re-reads it
/// whenever a restart is signalled by [`Backend::restart_mumblelink`].
pub async fn managed(b: Backend) -> Result<()> {
    let settings = async || -> Result<bool> {
        let state = b.client_state().await;
        let enabled = state.props.get(Key::MUMBLE_ENABLED).as_bool();
        Ok(enabled)
    };

    let mut enabled = settings().await?;

    let build = |enabled| {
        if enabled {
            tracing::info!("enabled");
            b.notify_info(COMPONENT, "enabled");
            Fuse::new(run(b.clone()))
        } else {
            tracing::info!("disabled");
            b.notify_info(COMPONENT, "disabled");
            Fuse::empty()
        }
    };

    let mut future = pin!(build(enabled));
    let mut reconnect = pin!(Fuse::empty());

    loop {
        tokio::select! {
            result = future.as_mut() => {
                if let Err(error) = result {
                    tracing::error!(%error);

                    for cause in error.chain().skip(1) {
                        tracing::error!(%cause);
                    }

                    b.notify_error(COMPONENT, format_args!("{error:#}"));
                } else {
                    tracing::info!("stopped");
                    b.notify_info(COMPONENT, "stopped");
                }

                tracing::info!("reconnecting in 5s");
                reconnect.set(Fuse::new(time::sleep(Duration::from_secs(5))));
            }
            _ = reconnect.as_mut() => {
                future.set(Fuse::new(run(b.clone())));
            }
            () = b.mumblelink_restart_wait() => {
                enabled = settings().await?;
                reconnect.set(Fuse::empty());
                future.set(build(enabled));
            }
        }
    }
}
