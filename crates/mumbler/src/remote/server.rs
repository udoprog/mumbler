use core::future::Future;
use core::pin::Pin;
use core::task::Waker;
use core::task::{Context, Poll};
use core::time::Duration;

use std::collections::{BTreeSet, HashMap};
use std::io;
use std::sync::Arc;
use std::task::Wake;

use anyhow::{Context as _, Result};
use parking_lot::Mutex;
use slab::Slab;
use tokio::net::TcpListener;
use tokio::time::{self, Sleep};

use super::api::{ConnectBody, Event, PingBody};
use super::{Client, Peer};

struct PeerState {
    peer: Peer,
    /// If this timeout is reached, the peer is disconnected.
    ///
    /// The timeout is reset every time a ping is received.
    timeout: Pin<Box<Sleep>>,
}

impl PeerState {
    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Poll::Ready(()) = self.timeout.as_mut().poll(cx) {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "ping timeout")));
        }

        self.peer.poll(cx)
    }
}

#[tracing::instrument(skip(peer), fields(addr = ?peer.peer.addr()))]
async fn handle(peer: &mut PeerState) -> Result<()> {
    while let Some((message, body)) = peer.peer.handle::<Event>()? {
        tracing::info!(?message, len = body.len(), "event");

        match message {
            Event::Ping => {
                let ping = body.decode::<PingBody>()?;
                peer.timeout
                    .as_mut()
                    .reset(time::Instant::now() + Duration::from_secs(5));
                peer.peer.pong(ping.payload)?;
            }
            Event::Connect => {
                let connect = body.decode::<ConnectBody>()?;
                tracing::info!(version = connect.version, "client connected");
            }
            event => {
                return Err(anyhow::anyhow!("unsupported event: {event:?}"));
            }
        }
    }

    Ok(())
}

pub async fn run(bind: &str) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;

    tracing::info!(addr = ?listener.local_addr()?, "listening");

    let mut peers = Slab::new();
    let mut state = State::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, addr) = result.context("accepting connection")?;
                let client = Client::from_stream(stream);
                let peer = Peer::new(addr, client);
                tracing::info!(addr = ?peer.addr(), "connected");
                let index = peers.insert(PeerState { peer, timeout: Box::pin(time::sleep(Duration::from_secs(5))) });
                state.register(index);
            }
            (index, result) = Peers::new(&mut peers, &mut state) => {
                let Some(peer) = peers.get_mut(index) else {
                    continue;
                };

                'out: {
                    if let Err(error) = result {
                        tracing::error!(addr = ?peer.peer.addr(), ?error, "peer errored, disconnecting");
                        peers.remove(index);
                        state.deregister(index);
                        break 'out;
                    }

                    if let Err(error) = handle(peer).await {
                        tracing::error!(addr = ?peer.peer.addr(), ?error, "peer errored, disconnecting");
                        peers.remove(index);
                        state.deregister(index);
                        break 'out;
                    }

                    // We have to re-poll the peer to set it up for future
                    // wakeups.
                    state.poll.insert(index);
                };
            }
        }
    }
}

#[derive(Clone)]
struct IndexWaker {
    index: usize,
    receiver: Arc<Mutex<BTreeSet<usize>>>,
    parent: Waker,
}

impl Wake for IndexWaker {
    fn wake(self: Arc<Self>) {
        let mut receiver = self.receiver.lock();
        receiver.insert(self.index);
        self.parent.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let mut receiver = self.receiver.lock();
        receiver.insert(self.index);
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
    receiver: Arc<Mutex<BTreeSet<usize>>>,
    wakers: HashMap<usize, Waker>,
    /// The currently observed parent context. If this changes, we have to
    /// invalidate all wakers and repoll all peers.
    context: Option<Waker>,
}

impl State {
    fn new() -> Self {
        Self {
            poll: BTreeSet::new(),
            receiver: Arc::new(Mutex::new(BTreeSet::new())),
            wakers: HashMap::new(),
            context: None,
        }
    }

    fn register(&mut self, index: usize) {
        self.poll.insert(index);
    }

    fn deregister(&mut self, index: usize) {
        self.poll.remove(&index);
        self.wakers.remove(&index);
    }

    fn waker_for(&mut self, index: usize, parent: &Waker) -> &Waker {
        let receiver = &self.receiver;

        self.wakers.entry(index).or_insert_with(|| {
            Waker::from(Arc::new(IndexWaker {
                index,
                receiver: receiver.clone(),
                parent: parent.clone(),
            }))
        })
    }

    fn refresh_parent<T>(&mut self, slab: &Slab<T>, parent: &Waker) {
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
    slab: &'a mut Slab<PeerState>,
    state: &'a mut State,
}

impl<'a> Peers<'a> {
    fn new(slab: &'a mut Slab<PeerState>, state: &'a mut State) -> Self {
        Self { slab, state }
    }
}

impl Future for Peers<'_> {
    type Output = (usize, io::Result<()>);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { slab, state } = unsafe { self.get_unchecked_mut() };

        state.refresh_parent(slab, cx.waker());

        while let Some(index) = state.poll.pop_first() {
            let Some(peer) = slab.get_mut(index) else {
                continue;
            };

            let waker = state.waker_for(index, cx.waker());
            let mut cx = Context::from_waker(waker);

            if let Poll::Ready(result) = peer.poll(&mut cx) {
                tracing::warn!("READY");
                return Poll::Ready((index, result));
            }
        }

        loop {
            let index = {
                let Some(index) = state.receiver.lock().pop_first() else {
                    break;
                };

                index
            };

            if let Some(peer) = slab.get_mut(index) {
                let waker = state.waker_for(index, cx.waker());
                let mut cx = Context::from_waker(waker);

                if let Poll::Ready(result) = peer.poll(&mut cx) {
                    return Poll::Ready((index, result));
                }
            }
        }

        Poll::Pending
    }
}
