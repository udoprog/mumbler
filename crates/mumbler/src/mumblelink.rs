use core::mem;
use core::pin::pin;

use anyhow::Result;
use mumblelink::{Link, Position};
use tokio::time::{self, Duration, Instant};

use crate::Backend;

/// Number of mumble updates per second.
const UPDATES_PER_SECOND: u64 = 20;

#[tracing::instrument(skip_all)]
pub async fn run(b: Backend) -> Result<()> {
    let mut enabled = b.mumblelink_state().await.enabled;

    let mut link = if enabled {
        Link::new()?
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
                    }
                }

                if mem::take(&mut state.restart) {
                    tracing::info!("restarting link");
                    link.reconnect()?;
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
