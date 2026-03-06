use core::pin::pin;

use anyhow::Result;
use mumblelink::{Link, Position};
use tokio::time::{self, Duration, Instant};

use crate::Backend;

#[tracing::instrument(skip_all)]
pub async fn run(b: Backend) -> Result<()> {
    let mut link = Link::new()?;

    let mut pos = Position::FORWARD;
    pos.position = [0., 0., 0.];

    link.set_identity("mumbler");
    link.set_context(b"");
    link.set_name("Mumbler");
    link.set_description("Test link from mumbler");
    link.set_avatar(pos);
    link.set_camera(pos);

    let mut sleep = pin!(time::sleep(Duration::from_millis(10)));

    loop {
        link.update();

        tokio::select! {
            _ = sleep.as_mut() => {
                sleep
                    .as_mut()
                    .reset(Instant::now() + Duration::from_millis(10));
            }
            () = b.transform_wait() => {
                let transform = b.transform();
                pos.position = transform.position.as_array();
                pos.front = transform.front.as_array();
                link.set_avatar(pos);
                link.set_camera(pos);
            }
        }
    }
}
