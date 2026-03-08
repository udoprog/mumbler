use core::future::Future;
use core::pin::Pin;
use core::task::Waker;
use core::task::{Context, Poll};
use core::time::Duration;

use std::collections::{BTreeSet, HashMap, hash_map};
use std::io;
use std::net::SocketAddr;
use std::path::Path;
use std::sync::Arc;
use std::task::Wake;

use anyhow::{Context as _, Result, anyhow};
use api::{Id, Key, Value};
use bstr::BStr;
use parking_lot::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::time::{self, Sleep};

use crate::remote::api::UpdatePeer;
use crate::remote::{REMOTE_PORT, REMOTE_TLS_PORT};

use super::api::{ConnectBody, Event, PingBody};
use super::{Client, Peer};

struct PeerState {
    /// The unique identifier of this peer.
    id: Id,
    /// The peer state.
    peer: Peer,
    /// If this timeout is reached, the peer is disconnected.
    ///
    /// The timeout is reset every time a ping is received.
    timeout: Pin<Box<Sleep>>,
    /// Values that the peer has set.
    values: HashMap<Key, Value>,
    /// The room the peer is in.
    room: Option<Box<[u8]>>,
}

impl PeerState {
    fn new(peer: Peer) -> Self {
        Self {
            id: Id::new(rand::random()),
            peer,
            timeout: Box::pin(time::sleep(Duration::from_secs(5))),
            values: HashMap::new(),
            room: None,
        }
    }

    fn send_to_room<F>(
        &self,
        mut f: F,
        rooms: &HashMap<Box<[u8]>, Room>,
        peers: &mut HashMap<Id, PeerState>,
    ) where
        F: FnMut(&mut Peer) -> Result<()>,
    {
        let Some(room) = self.room.as_ref().and_then(|name| rooms.get(name)) else {
            return;
        };

        for id in room.members.iter() {
            if *id == self.id {
                continue;
            }

            if let Some(peer) = peers.get_mut(id)
                && let Err(e) = f(&mut peer.peer)
            {
                tracing::error!(?id, ?e, "failed to send update");
            }
        }
    }

    fn poll(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Poll::Ready(()) = self.timeout.as_mut().poll(cx) {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "ping timeout")));
        }

        self.peer.poll(cx)
    }
}

/// A connector that listens for incoming connections and manages peer state.
pub struct ConnectorConfig<'a> {
    /// The address to bind to.
    pub bind: &'a str,
    /// Override the default port.
    pub port: Option<u16>,
    /// If the connector uses TLS.
    pub tls: bool,
    /// Path to TLS certificate in PEM format.
    pub cert: Option<&'a Path>,
    /// Path to TLS private key in PEM format.
    pub key: Option<&'a Path>,
}

struct Connector {
    listener: TcpListener,
    tls: bool,
}

pub async fn run(configs: Vec<ConnectorConfig<'_>>) -> Result<()> {
    let mut connectors: Vec<Connector> = Vec::new();

    for c in configs {
        let port = match c.port {
            Some(port) => port,
            None => {
                if c.tls {
                    REMOTE_TLS_PORT
                } else {
                    REMOTE_PORT
                }
            }
        };

        let listener = TcpListener::bind((c.bind, port))
            .await
            .with_context(|| anyhow!("Binding {}:{port}", c.bind))?;
        connectors.push(Connector {
            listener,
            tls: c.tls,
        });
    }

    for connector in connectors.iter() {
        tracing::info!(addr = ?connector.listener.local_addr()?, "listening");
    }

    let mut peers = HashMap::new();
    let mut wakers = Wakers::new();
    let mut state = State::new();

    loop {
        tokio::select! {
            result = Listen::new(&mut connectors) => {
                let (i, stream, addr) = result.context("accepting connection")?;

                let Some(connector) = connectors.get(i) else {
                    tracing::error!(?i, "invalid connector index");
                    continue;
                };

                let client = Client::plain(stream);
                let peer = Peer::new(addr, client);
                tracing::info!(addr = ?peer.addr(), "Connected");

                let peer_state = PeerState::new(peer);
                let id = peer_state.id;
                peers.insert(id, peer_state);
                state.register(id);
            }
            (id, result) = Peers::new(&mut peers, &mut wakers, &mut state) => {
                let Some(mut peer) = peers.remove(&id) else {
                    tracing::warn!(?id, "Peer not found, removed");
                    continue;
                };

                let remove = 'out: {
                    if let Err(error) = result {
                        tracing::error!(addr = ?peer.peer.addr(), ?error, "Peer errored, disconnecting");
                        break 'out true;
                    }

                    if let Err(error) = state.handle(&mut peer, &mut peers).await {
                        tracing::error!(addr = ?peer.peer.addr(), ?error, "Peer errored, disconnecting");
                        break 'out true;
                    }

                    // We have to re-poll the peer to set it up for future
                    // wakeups.
                    state.poll.insert(peer.id);
                    false
                };

                if remove {
                    state.remove_peer(peer, &mut peers, &mut wakers);
                } else {
                    peers.insert(id, peer);
                }
            }
        }
    }
}

#[derive(Clone)]
struct IndexWaker {
    id: Id,
    receiver: Arc<Mutex<BTreeSet<Id>>>,
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

struct Room {
    /// The name of this room.
    name: Box<[u8]>,
    /// The members of this room.
    members: Vec<Id>,
}

struct Wakers {
    /// The currently observed parent context. If this changes, we have to
    /// invalidate all wakers and repoll all peers.
    waker: Option<Waker>,
    /// Cached wakers for each peer.
    wakers: HashMap<Id, Waker>,
    /// Channel that child tasks used to indicate that they need to wake up.
    receiver: Arc<Mutex<BTreeSet<Id>>>,
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
    fn remove(&mut self, id: Id) {
        self.wakers.remove(&id);
    }

    fn refresh_parent<T>(&mut self, state: &mut State, peers: &HashMap<Id, T>, parent: &Waker) {
        let changed = self.waker.as_ref().is_none_or(|w| !w.will_wake(parent));

        if changed {
            self.waker = Some(parent.clone());
            self.wakers.clear();

            for id in peers.keys() {
                state.poll.insert(*id);
            }
        }
    }

    fn waker_for(&mut self, id: Id, parent: &Waker) -> &mut Waker {
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
    poll: BTreeSet<Id>,
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

    fn register(&mut self, id: Id) {
        self.poll.insert(id);
    }

    #[tracing::instrument(skip_all, fields(id = ?this.id, addr = ?this.peer.addr()))]
    async fn handle(
        &mut self,
        this: &mut PeerState,
        peers: &mut HashMap<Id, PeerState>,
    ) -> Result<()> {
        while let Some((message, body)) = this.peer.handle::<Event>()? {
            match message {
                Event::Ping => {
                    let ping = body.decode::<PingBody>()?;
                    this.timeout
                        .as_mut()
                        .reset(time::Instant::now() + Duration::from_secs(5));
                    this.peer.pong(ping.payload)?;
                }
                Event::Connect => {
                    let connect = body.decode::<ConnectBody>()?;
                    tracing::debug!(connect.version, connect.room = ?BStr::new(&connect.room), "connect");

                    if let Some(old_room) = this.room.replace(connect.room.clone()) {
                        self.leave_room(&old_room, this.id, peers);
                    }

                    for (key, value) in connect.values.iter() {
                        this.values.insert(*key, value.clone());
                    }

                    // We have just connected, so send all information about other peers to
                    // the new peer.
                    self.join_room(this, &connect.room, &connect.values, peers);
                }
                Event::Update => {
                    let event = body.decode::<UpdatePeer>()?;
                    tracing::debug!(?event.key, ?event.value, "update");

                    this.send_to_room(
                        |peer| peer.updated_peer(this.id, event.key, &event.value),
                        &self.rooms,
                        peers,
                    );

                    this.values.insert(event.key, event.value.clone());
                }
                event => {
                    return Err(anyhow::anyhow!("unsupported event: {event:?}"));
                }
            }
        }

        Ok(())
    }

    fn remove_peer(
        &mut self,
        peer: PeerState,
        peers: &mut HashMap<Id, PeerState>,
        wakers: &mut Wakers,
    ) {
        if let Some(room) = peer.room {
            self.leave_room(&room, peer.id, peers);
        }

        self.poll.remove(&peer.id);
        wakers.remove(peer.id);
    }

    fn leave_room(&mut self, room_name: &[u8], leaving_id: Id, peers: &mut HashMap<Id, PeerState>) {
        let Some(room) = self.rooms.get_mut(room_name) else {
            return;
        };

        room.members.retain(|&id| id != leaving_id);

        for id in room.members.iter() {
            let Some(peer) = peers.get_mut(id) else {
                continue;
            };

            if let Err(e) = peer.peer.leave(leaving_id) {
                tracing::error!(?id, ?e, "failed to send leave");
            } else {
                self.poll.insert(leaving_id);
            }
        }

        let remove = room.members.is_empty();

        if remove {
            self.rooms.remove(room_name);
        }
    }

    fn join_room(
        &mut self,
        joining: &mut PeerState,
        room: &[u8],
        values: &HashMap<Key, Value>,
        peers: &mut HashMap<Id, PeerState>,
    ) {
        let room = self
            .rooms
            .entry(Box::<[u8]>::from(room))
            .or_insert_with(|| Room {
                name: Box::from(room),
                members: Vec::new(),
            });

        for id in room.members.iter() {
            let Some(peer) = peers.get_mut(id) else {
                continue;
            };

            if let Err(e) = peer.peer.join(joining.id, values) {
                tracing::error!(?id, ?e, "failed to send join");
            } else {
                self.poll.insert(joining.id);
            }
        }

        if !room.members.contains(&joining.id) {
            room.members.push(joining.id);
        }

        tracing::debug!(room.name = ?BStr::new(&room.name), members = ?room.members, "connecting room");

        for id in room.members.iter() {
            let Some(other) = peers.get(id) else {
                continue;
            };

            if let Err(e) = joining.peer.join(other.id, &other.values) {
                tracing::error!(?id, ?e, "failed to send join");
            }

            for (key, value) in other.values.iter() {
                if let Err(e) = joining.peer.updated_peer(other.id, *key, value) {
                    tracing::error!(?id, ?e, "failed to send peer update");
                }
            }

            self.poll.insert(joining.id);
        }
    }
}

struct Listen<'a> {
    connectors: &'a mut [Connector],
}

impl<'a> Listen<'a> {
    fn new(connectors: &'a mut [Connector]) -> Self {
        Self { connectors }
    }
}

impl Future for Listen<'_> {
    type Output = io::Result<(usize, TcpStream, SocketAddr)>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.get_mut();

        for (i, c) in this.connectors.iter_mut().enumerate() {
            match c.listener.poll_accept(cx) {
                Poll::Ready(Ok((stream, addr))) => return Poll::Ready(Ok((i, stream, addr))),
                Poll::Ready(Err(e)) => return Poll::Ready(Err(e)),
                Poll::Pending => continue,
            }
        }

        Poll::Pending
    }
}

struct Peers<'a> {
    peers: &'a mut HashMap<Id, PeerState>,
    wakers: &'a mut Wakers,
    state: &'a mut State,
}

impl<'a> Peers<'a> {
    fn new(
        peers: &'a mut HashMap<Id, PeerState>,
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
    type Output = (Id, io::Result<()>);

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
