use core::future::Future;
use core::net::SocketAddr;
use core::pin::Pin;
use core::task::Waker;
use core::task::{Context, Poll};
use core::time::Duration;

use std::collections::{BTreeSet, HashMap, hash_map};
use std::io;
use std::sync::Arc;
use std::task::Wake;

use anyhow::{Context as _, Result};
use bstr::BStr;
use parking_lot::Mutex;
use tokio::net::TcpListener;
use tokio::time::{self, Sleep};

use crate::remote::api::{MoveToBody, PeerId};

use super::api::{ConnectBody, Event, PingBody};
use super::{Client, Peer};

struct PeerState {
    /// The unique identifier of this peer.
    id: PeerId,
    /// The peer state.
    peer: Peer,
    /// If this timeout is reached, the peer is disconnected.
    ///
    /// The timeout is reset every time a ping is received.
    timeout: Pin<Box<Sleep>>,
    /// The room the peer is in.
    room: Option<Box<[u8]>>,
}

impl PeerState {
    fn new(peer: Peer) -> Self {
        Self {
            id: PeerId::random(),
            peer,
            timeout: Box::pin(time::sleep(Duration::from_secs(5))),
            room: None,
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Poll::Ready(()) = self.timeout.as_mut().poll(cx) {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "ping timeout")));
        }

        self.peer.poll(cx)
    }
}

pub async fn run(bind: &str) -> Result<()> {
    let listener = TcpListener::bind(bind).await?;

    tracing::info!(addr = ?listener.local_addr()?, "listening");

    let mut peers = HashMap::new();
    let mut wakers = Wakers::new();
    let mut state = State::new();

    loop {
        tokio::select! {
            result = listener.accept() => {
                let (stream, addr) = result.context("accepting connection")?;
                let client = Client::from_stream(stream);
                let peer = Peer::new(addr, client);
                tracing::info!(addr = ?peer.addr(), "connected");

                let peer_state = PeerState::new(peer);
                let id = peer_state.id;
                peers.insert(id, peer_state);
                state.register(id);
            }
            (id, result) = Peers::new(&mut peers, &mut wakers, &mut state) => {
                let Some(addr) = peers.get(&id).map(|p| p.peer.addr()) else {
                    continue;
                };

                let remove = 'out: {
                    if let Err(error) = result {
                        tracing::error!(?addr, ?error, "peer errored, disconnecting");
                        break 'out true;
                    }

                    if let Err(error) = state.handle(addr, &mut peers, id).await {
                        tracing::error!(?addr, ?error, "peer errored, disconnecting");
                        break 'out true;
                    }

                    // We have to re-poll the peer to set it up for future
                    // wakeups.
                    state.poll.insert(id);
                    false
                };

                if remove {
                    state.remove_peer(id, &mut wakers, &mut peers);
                }
            }
        }
    }
}

#[derive(Clone)]
struct IndexWaker {
    id: PeerId,
    receiver: Arc<Mutex<BTreeSet<PeerId>>>,
    parent: Waker,
}

impl Wake for IndexWaker {
    fn wake(self: Arc<Self>) {
        let mut receiver = self.receiver.lock();
        receiver.insert(self.id);
        self.parent.wake_by_ref();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        let mut receiver = self.receiver.lock();
        receiver.insert(self.id);
        self.parent.wake_by_ref();
    }
}

#[derive(Default)]
struct Room {
    /// The members of this room.
    members: Vec<PeerId>,
}

struct Wakers {
    /// The currently observed parent context. If this changes, we have to
    /// invalidate all wakers and repoll all peers.
    waker: Option<Waker>,
    /// Cached wakers for each peer.
    wakers: HashMap<PeerId, Waker>,
    /// Channel that child tasks used to indicate that they need to wake up.
    receiver: Arc<Mutex<BTreeSet<PeerId>>>,
}

impl Wakers {
    #[inline]
    fn new() -> Self {
        Self {
            waker: None,
            wakers: HashMap::new(),
            receiver: Arc::new(Mutex::new(BTreeSet::new())),
        }
    }

    #[inline]
    fn remove(&mut self, id: PeerId) {
        self.wakers.remove(&id);
    }

    fn refresh_parent<T>(&mut self, state: &mut State, peers: &HashMap<PeerId, T>, parent: &Waker) {
        let changed = self.waker.as_ref().is_none_or(|w| !w.will_wake(parent));

        if changed {
            self.waker = Some(parent.clone());
            self.wakers.clear();

            for id in peers.keys() {
                state.poll.insert(*id);
            }
        }
    }

    fn waker_for(&mut self, id: PeerId, parent: &Waker) -> &mut Waker {
        match self.wakers.entry(id) {
            hash_map::Entry::Occupied(entry) => entry.into_mut(),
            hash_map::Entry::Vacant(entry) => entry.insert(Waker::from(Arc::new(IndexWaker {
                id,
                receiver: self.receiver.clone(),
                parent: parent.clone(),
            }))),
        }
    }
}

struct State {
    /// Indices of peers that needs to be polled.
    ///
    /// Peers are added here initially when they connect, or when their buffers
    /// are written to.
    poll: BTreeSet<PeerId>,
    /// Available client contexts.
    rooms: HashMap<Box<[u8]>, Room>,
}

impl State {
    fn new() -> Self {
        Self {
            poll: BTreeSet::new(),
            rooms: HashMap::new(),
        }
    }

    fn register(&mut self, id: PeerId) {
        self.poll.insert(id);
    }

    #[tracing::instrument(skip_all, fields(addr))]
    async fn handle(
        &mut self,
        addr: SocketAddr,
        peers: &mut HashMap<PeerId, PeerState>,
        peer_id: PeerId,
    ) -> Result<()> {
        _ = addr;

        let Some(peer) = peers.get_mut(&peer_id) else {
            return Ok(());
        };

        let mut join = Vec::new();
        let mut moves = Vec::new();

        while let Some((message, body)) = peer.peer.handle::<Event>()? {
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
                    tracing::info!(connect.version, connect.room = ?BStr::new(&connect.room), "connect");
                    join.push(connect.room);
                }
                Event::Move => {
                    let m = body.decode::<MoveToBody>()?;
                    moves.push((m.position, m.front));
                }
                event => {
                    return Err(anyhow::anyhow!("unsupported event: {event:?}"));
                }
            }
        }

        for room in join {
            if let Some(room_name) = peers
                .get_mut(&peer_id)
                .and_then(|p| p.room.replace(room.clone()))
            {
                self.leave_room(&room_name, peer_id, peers);
            }

            if let Some(room) = self.rooms.get(&room)
                && let Some(peer) = peers.get_mut(&peer_id)
            {
                for id in room.members.iter() {
                    if let Err(e) = peer.peer.join(*id) {
                        tracing::error!(?id, ?e, "failed to send join message");
                    } else {
                        self.poll.insert(peer_id);
                    }
                }
            }

            self.join_room(&room, peer_id, peers);
        }

        for (position, front) in moves {
            let Some(room) = peers
                .get(&peer_id)
                .and_then(|p| p.room.as_ref())
                .and_then(|r| self.rooms.get(r))
            else {
                continue;
            };

            for id in room.members.iter() {
                if *id == peer_id {
                    continue;
                }

                if let Some(peer) = peers.get_mut(id) {
                    if let Err(e) = peer.peer.moved_to(peer_id, position, front) {
                        tracing::error!(?id, ?e, "failed to send move message");
                    } else {
                        self.poll.insert(*id);
                    }
                }
            }
        }

        Ok(())
    }

    fn remove_peer(
        &mut self,
        id: PeerId,
        wakers: &mut Wakers,
        peers: &mut HashMap<PeerId, PeerState>,
    ) {
        let Some(peer) = peers.remove(&id) else {
            tracing::warn!(?id, "peer already removed");
            return;
        };

        if let Some(room) = peer.room {
            self.leave_room(&room, peer.id, peers);
        }

        self.poll.remove(&id);
        wakers.remove(id);
    }

    fn leave_room(
        &mut self,
        room_name: &[u8],
        peer_id: PeerId,
        peers: &mut HashMap<PeerId, PeerState>,
    ) {
        let Some(room) = self.rooms.get_mut(room_name) else {
            return;
        };

        room.members.retain(|&id| id != peer_id);

        for id in room.members.iter() {
            let Some(peer) = peers.get_mut(id) else {
                continue;
            };

            if let Err(e) = peer.peer.leave(peer_id) {
                tracing::error!(?id, ?e, "failed to send leave message");
            } else {
                self.poll.insert(peer_id);
            }
        }

        let remove = room.members.is_empty();

        if remove {
            self.rooms.remove(room_name);
        }
    }

    fn join_room(&mut self, room: &[u8], peer_id: PeerId, peers: &mut HashMap<PeerId, PeerState>) {
        let room = self.rooms.entry(Box::<[u8]>::from(room)).or_default();

        for id in room.members.iter() {
            let Some(peer) = peers.get_mut(id) else {
                continue;
            };

            if let Err(e) = peer.peer.join(peer_id) {
                tracing::error!(?id, ?e, "failed to send leave message");
            } else {
                self.poll.insert(peer_id);
            }
        }

        if !room.members.contains(&peer_id) {
            room.members.push(peer_id);
        }
    }
}

struct Peers<'a> {
    peers: &'a mut HashMap<PeerId, PeerState>,
    wakers: &'a mut Wakers,
    state: &'a mut State,
}

impl<'a> Peers<'a> {
    fn new(
        peers: &'a mut HashMap<PeerId, PeerState>,
        wakers: &'a mut Wakers,
        state: &'a mut State,
    ) -> Self {
        Self {
            peers,
            wakers,
            state,
        }
    }
}

impl Future for Peers<'_> {
    type Output = (PeerId, io::Result<()>);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            peers,
            wakers,
            state,
        } = unsafe { self.get_unchecked_mut() };

        wakers.refresh_parent(state, peers, cx.waker());

        while let Some(id) = state.poll.pop_first() {
            let Some(peer) = peers.get_mut(&id) else {
                continue;
            };

            let waker = wakers.waker_for(id, cx.waker());
            let mut cx = Context::from_waker(waker);

            if let Poll::Ready(result) = peer.poll(&mut cx) {
                tracing::warn!("READY");
                return Poll::Ready((id, result));
            }
        }

        loop {
            let id = {
                let Some(id) = wakers.receiver.lock().pop_first() else {
                    break;
                };

                id
            };

            if let Some(peer) = peers.get_mut(&id) {
                let waker = wakers.waker_for(id, cx.waker());
                let mut cx = Context::from_waker(waker);

                if let Poll::Ready(result) = peer.poll(&mut cx) {
                    return Poll::Ready((id, result));
                }
            }
        }

        Poll::Pending
    }
}
