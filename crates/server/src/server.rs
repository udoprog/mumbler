use core::net::SocketAddr;
use core::pin::Pin;
use core::task::{Context, Poll};

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::task::{Wake, Waker};

use anyhow::{Context as _, Result};
use slab::Slab;
use tokio::net::TcpListener;

use crate::{Buf, Client};

struct Peer {
    addr: SocketAddr,
    client: Client,
    read: Buf,
    write: Buf,
}

impl Peer {
    fn new(addr: SocketAddr, client: Client) -> Self {
        Self {
            addr,
            client,
            read: Buf::new(),
            write: Buf::new(),
        }
    }

    fn handle(&mut self, ready: Ready) -> Result<()> {
        match ready {
            Ready::Read => {
                self.client.try_read(&mut self.read)?;
                Ok(())
            }
            Ready::Write => {
                self.client.try_write(&mut self.write)?;
                Ok(())
            }
            Ready::Error(error) => Err(error).context("peer error"),
        }
    }
}

impl Peer {
    fn poll(&self, cx: &mut Context<'_>) -> Poll<Ready> {
        if self.write.remaining() > 0
            && let Poll::Ready(result) = self.client.poll_write_ready(cx)
        {
            cx.waker().wake_by_ref();

            match result {
                Ok(()) => return Poll::Ready(Ready::Write),
                Err(e) => return Poll::Ready(Ready::Error(e)),
            }
        }

        if let Poll::Ready(result) = self.client.poll_read_ready(cx) {
            cx.waker().wake_by_ref();

            match result {
                Ok(()) => return Poll::Ready(Ready::Read),
                Err(e) => return Poll::Ready(Ready::Error(e)),
            }
        }

        Poll::Pending
    }
}

pub async fn run(bind: &str) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;

    tracing::info!(addr = ?listener.local_addr()?, "server listening");

    let mut peers = Slab::new();
    let mut state = PeersState::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, addr) = result.context("accepting connection")?;
                let client = Client::from_stream(stream);
                let peer = Peer::new(addr, client);
                tracing::info!(?peer.addr, "connected");
                let index = peers.insert(peer);
                state.register(index);
            }
            (index, ready) = Peers::new(&mut peers, &mut state) => {
                let Some(peer) = peers.get_mut(index) else {
                    continue;
                };

                if let Err(error) = peer.handle(ready) {
                    tracing::error!(?peer.addr, ?error, "peer errored, disconnecting");
                    peers.remove(index);
                    state.deregister(index);
                }
            }
        }
    }
}

#[derive(Debug)]
enum Ready {
    Read,
    Write,
    Error(io::Error),
}

#[derive(Clone)]
struct IndexWaker {
    index: usize,
    sender: Sender<usize>,
    parent: Waker,
}

impl Wake for IndexWaker {
    fn wake(self: Arc<Self>) {
        let _ = self.sender.send(self.index);
        self.parent.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let _ = self.sender.send(self.index);
        self.parent.wake_by_ref();
    }
}

struct PeersState {
    unregistered: BTreeSet<usize>,
    receiver: Receiver<usize>,
    sender: Sender<usize>,
    wakers: HashMap<usize, Arc<IndexWaker>>,
    last_parent: Option<Waker>,
}

impl PeersState {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();

        Self {
            unregistered: BTreeSet::new(),
            receiver,
            sender,
            wakers: HashMap::new(),
            last_parent: None,
        }
    }

    fn register(&mut self, index: usize) {
        self.unregistered.insert(index);
    }

    fn deregister(&mut self, index: usize) {
        self.wakers.remove(&index);
    }

    fn waker_for(&mut self, index: usize, parent: &Waker) -> Waker {
        let sender = &self.sender;

        let arc = self.wakers.entry(index).or_insert_with(|| {
            Arc::new(IndexWaker {
                index,
                sender: sender.clone(),
                parent: parent.clone(),
            })
        });

        Waker::from(arc.clone())
    }

    fn refresh_parent(&mut self, slab: &Slab<Peer>, parent: &Waker) {
        let changed = self
            .last_parent
            .as_ref()
            .map_or(true, |w| !w.will_wake(parent));

        if changed {
            self.last_parent = Some(parent.clone());
            self.wakers.clear();

            for (index, _) in slab.iter() {
                self.unregistered.insert(index);
            }
        }
    }
}

struct Peers<'a> {
    slab: &'a mut Slab<Peer>,
    state: &'a mut PeersState,
}

impl<'a> Peers<'a> {
    fn new(slab: &'a mut Slab<Peer>, state: &'a mut PeersState) -> Self {
        Self { slab, state }
    }
}

impl Future for Peers<'_> {
    type Output = (usize, Ready);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { slab, state } = unsafe { self.get_unchecked_mut() };

        state.refresh_parent(slab, cx.waker());

        while let Some(index) = state.unregistered.pop_first() {
            let Some(peer) = slab.get(index) else {
                continue;
            };

            let waker = state.waker_for(index, cx.waker());
            let mut cx = Context::from_waker(&waker);

            if let Poll::Ready(ready) = peer.poll(&mut cx) {
                return Poll::Ready((index, ready));
            }
        }

        while let Ok(index) = state.receiver.try_recv() {
            if let Some(peer) = slab.get(index) {
                let waker = state.waker_for(index, cx.waker());
                let mut cx = Context::from_waker(&waker);

                if let Poll::Ready(ready) = peer.poll(&mut cx) {
                    return Poll::Ready((index, ready));
                }
            }
        }

        Poll::Pending
    }
}
