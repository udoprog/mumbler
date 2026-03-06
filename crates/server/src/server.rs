use core::pin::Pin;
use core::task::{Context, Poll};

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::task::{Wake, Waker};

use anyhow::{Context as _, Result};
use slab::Slab;
use tokio::net::TcpListener;

use crate::{Client, Peer, Ready};

pub async fn run(bind: &str) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;

    tracing::info!(addr = ?listener.local_addr()?, "server listening");

    let mut peers = Slab::new();
    let mut state = State::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, addr) = result.context("accepting connection")?;
                let client = Client::from_stream(stream);
                let peer = Peer::new(addr, client);
                tracing::info!(addr = ?peer.addr(), "connected");
                let index = peers.insert(peer);
                state.register(index);
            }
            (index, ready) = Peers::new(&mut peers, &mut state) => {
                let Some(peer) = peers.get_mut(index) else {
                    continue;
                };

                if let Err(error) = peer.handle(ready) {
                    tracing::error!(addr = ?peer.addr(), ?error, "peer errored, disconnecting");
                    peers.remove(index);
                    state.deregister(index);
                }
            }
        }
    }
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

struct State {
    /// Indices of peers that needs to be polled.
    ///
    /// Peers are added here initially when they connect, or when their buffers
    /// are written to.
    poll: BTreeSet<usize>,
    /// Channel that child tasks used to indicate that they need to wake up.
    receiver: Receiver<usize>,
    sender: Sender<usize>,
    wakers: HashMap<usize, Waker>,
    /// The currently observed parent context. If this changes, we have to
    /// invalidate all wakers and repoll all peers.
    context: Option<Waker>,
}

impl State {
    fn new() -> Self {
        let (sender, receiver) = mpsc::channel();

        Self {
            poll: BTreeSet::new(),
            receiver,
            sender,
            wakers: HashMap::new(),
            context: None,
        }
    }

    fn register(&mut self, index: usize) {
        self.poll.insert(index);
    }

    fn deregister(&mut self, index: usize) {
        self.wakers.remove(&index);
    }

    fn waker_for(&mut self, index: usize, parent: &Waker) -> &Waker {
        let sender = &self.sender;

        self.wakers.entry(index).or_insert_with(|| {
            Waker::from(Arc::new(IndexWaker {
                index,
                sender: sender.clone(),
                parent: parent.clone(),
            }))
        })
    }

    fn refresh_parent(&mut self, slab: &Slab<Peer>, parent: &Waker) {
        let changed = self.context.as_ref().is_none_or(|w| !w.will_wake(parent));

        if changed {
            self.context = Some(parent.clone());
            self.wakers.clear();

            for (index, _) in slab.iter() {
                self.poll.insert(index);
            }
        }
    }
}

struct Peers<'a> {
    slab: &'a mut Slab<Peer>,
    state: &'a mut State,
}

impl<'a> Peers<'a> {
    fn new(slab: &'a mut Slab<Peer>, state: &'a mut State) -> Self {
        Self { slab, state }
    }
}

impl Future for Peers<'_> {
    type Output = (usize, Ready);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { slab, state } = unsafe { self.get_unchecked_mut() };

        state.refresh_parent(slab, cx.waker());

        while let Some(index) = state.poll.pop_first() {
            let Some(peer) = slab.get(index) else {
                continue;
            };

            let waker = state.waker_for(index, cx.waker());
            let mut cx = Context::from_waker(waker);

            if let Poll::Ready(ready) = peer.poll(&mut cx) {
                return Poll::Ready((index, ready));
            }
        }

        while let Ok(index) = state.receiver.try_recv() {
            if let Some(peer) = slab.get(index) {
                let waker = state.waker_for(index, cx.waker());
                let mut cx = Context::from_waker(waker);

                if let Poll::Ready(ready) = peer.poll(&mut cx) {
                    return Poll::Ready((index, ready));
                }
            }
        }

        Poll::Pending
    }
}
