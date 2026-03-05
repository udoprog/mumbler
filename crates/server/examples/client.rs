use anyhow::Result;
use server::{Buf, Client};

#[tokio::main]
async fn main() -> Result<()> {
    let mut client = Client::connect("localhost:44114").await?;
    let mut read = Buf::new();

    loop {
        client.readable().await?;
        client.try_read(&mut read)?;

        let b = read.read_buf();
        let len = b.len();
        read.advance(len);
    }
}
