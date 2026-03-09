use core::pin::pin;
use core::time::Duration;

use tokio::io::{self, AsyncBufReadExt as _};
use tokio::time;
use tokio::time::Instant;

use mumblelink::{Link, Position};

const HELP: &str = "Commands:
  left - Move avatar and camera to the left
  right - Move avatar and camera to the right
  middle - Move avatar and camera to the middle
  far - Move avatar and camera far away
  red - Set context to 'red'
  blue - Set context to 'blue'
  free - Deactivate Link
  reconnect - Reconnect Link
  exit - Exit this program";

#[tokio::main]
async fn main() -> io::Result<()> {
    println!("Attempting to open Link...");
    let mut link = Link::new()?;

    let mut pos = Position::FORWARD;
    pos.position = [0., 0., 0.];

    let mut line = String::new();
    let mut stdin = io::BufReader::new(io::stdin());

    link.set_identity("setbac");
    link.set_context(b"");
    link.set_name("Mumbler");
    link.set_description("Test link from mumbler");
    link.set_avatar(pos);
    link.set_camera(pos);

    let mut sleep = pin!(time::sleep(time::Duration::from_millis(10)));

    println!("{HELP}");

    let mut status = true;

    loop {
        if status {
            println!("# active: {}", link.is_enabled());
            status = false;
        }

        link.update();

        sleep
            .as_mut()
            .reset(Instant::now() + Duration::from_millis(10));

        tokio::select! {
            _ = sleep.as_mut() => {
            }
            _ = stdin.read_line(&mut line) => {
                match line.trim() {
                    "left" => {
                        pos.position = [-2., 0., 0.];
                        link.set_avatar(pos);
                        link.set_camera(pos);
                    }
                    "right" => {
                        pos.position = [2., 0., 0.];
                        link.set_avatar(pos);
                        link.set_camera(pos);
                    }
                    "middle" => {
                        pos.position = [0., 0., 0.];
                        link.set_avatar(pos);
                        link.set_camera(pos);
                    }
                    "far" => {
                        pos.position = [0., 0., 5.];
                        link.set_avatar(pos);
                        link.set_camera(pos);
                    }
                    "empty" => {
                        link.set_context(b"");
                    }
                    "red" => {
                        link.set_context(b"red");
                    }
                    "blue" => {
                        link.set_context(b"blue");
                    }
                    "free" => {
                        link.disable();
                    }
                    "reconnect" => {
                        link.reconnect()?;
                    }
                    "" | "exit" => {
                        break;
                    }
                    _ => {
                        println!("{HELP}");
                    }
                }

                status = true;
                line.clear();
            }
        }
    }

    println!("Exiting");
    Ok(())
}
