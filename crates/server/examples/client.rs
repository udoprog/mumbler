use anyhow::Result;
use server::{Client, Peer};

#[tokio::main]
async fn main() -> Result<()> {
    let client = Client::connect("localhost:44114").await?;
    let addr = client.addr()?;

    let mut peer = Peer::new(addr, client);

    peer.connect()?;

    loop {
        let ready = peer.ready().await;
        peer.handle(ready)?;
    }
}
