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
use api::{Id, PeerId, RemoteObject};
use bstr::BStr;
use parking_lot::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::{self, Sleep};

use crate::remote::api::{AddObjectBody, RemoveObjectBody, UpdatePeer};
use crate::remote::{DEFAULT_PORT, DEFAULT_TLS_PORT};
use crate::tls;

use super::api::{ConnectBody, Event, PingBody};
use super::{Client, Peer};

#[cfg(feature = "tls")]
async fn accept_tls(tls_acceptor: tls::TlsAcceptor, stream: TcpStream) -> Result<Client> {
    let stream = tls_acceptor.accept(stream).await?;
    Ok(Client::tls(stream.into()))
}

#[cfg(not(feature = "tls"))]
async fn accept_tls(tls_acceptor: tls::TlsAcceptor, stream: TcpStream) -> Result<Client> {
    _ = tls_acceptor;
    _ = stream;
    anyhow::bail!("Cannot accept connection, TLS support is not enabled");
}

struct Data {
    /// The socket address of the peer.
    addr: SocketAddr,
    /// The peer state.
    peer: Peer,
    /// The unique identifier of this peer.
    peer_id: PeerId,
    /// Objects that the peer has set.
    objects: HashMap<Id, RemoteObject>,
    /// The room the peer is in.
    room: Option<Box<[u8]>>,
}

impl Data {
    fn send_to_room<F>(&self, mut f: F, rooms: &HashMap<Box<[u8]>, Room>, peers: &mut Peers)
    where
        F: FnMut(Pin<&mut ServerPeer>) -> Result<()>,
    {
        let Some(room) = self.room.as_ref().and_then(|name| rooms.get(name)) else {
            return;
        };

        for id in room.members.iter() {
            if self.peer_id == *id {
                continue;
            }

            if let Some(peer) = peers.get_mut(id)
                && let Err(error) = f(peer)
            {
                tracing::error!(?id, ?error, "Sending update");
            }
        }
    }
}

struct ServerPeer {
    /// If this timeout is reached, the peer is disconnected.
    ///
    /// The timeout is reset every time a ping is received.
    timeout: Sleep,
    /// Peer data.
    data: Data,
}

impl ServerPeer {
    fn new(addr: SocketAddr, peer: Peer) -> Pin<Box<Self>> {
        Box::pin(Self {
            timeout: time::sleep(Duration::from_secs(5)),
            data: Data {
                addr,
                peer,
                peer_id: PeerId::new(rand::random()),
                objects: HashMap::new(),
                room: None,
            },
        })
    }

    #[inline]
    fn peer_mut(self: Pin<&mut Self>) -> &mut Peer {
        // SAFETY: Interior Peer is Unpin.
        let this = unsafe { self.get_unchecked_mut() };
        &mut this.data.peer
    }

    #[inline]
    fn project(self: Pin<&mut Self>) -> (Pin<&mut Sleep>, &mut Data) {
        unsafe {
            let this = self.get_unchecked_mut();
            (Pin::new_unchecked(&mut this.timeout), &mut this.data)
        }
    }

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let (timeout, data) = self.project();

        if let Poll::Ready(()) = timeout.poll(cx) {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::TimedOut, "ping timeout")));
        }

        Pin::new(&mut data.peer).poll(cx)
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

struct Peers {
    /// Map of peer ID to peer state.
    peers: HashMap<PeerId, Pin<Box<ServerPeer>>>,
}

impl Peers {
    #[inline]
    fn new() -> Self {
        Self {
            peers: HashMap::new(),
        }
    }

    #[inline]
    fn insert(&mut self, id: PeerId, peer: Pin<Box<ServerPeer>>) {
        self.peers.insert(id, peer);
    }

    #[inline]
    fn get(&self, id: &PeerId) -> Option<&ServerPeer> {
        self.peers.get(id).map(|b| &**b)
    }

    #[inline]
    fn get_mut(&mut self, id: &PeerId) -> Option<Pin<&mut ServerPeer>> {
        self.peers.get_mut(id).map(|b| b.as_mut())
    }

    #[inline]
    fn remove(&mut self, id: &PeerId) -> Option<Pin<Box<ServerPeer>>> {
        self.peers.remove(id)
    }
}

struct Connector {
    listener: TcpListener,
    tls_acceptor: Option<tls::TlsAcceptor>,
}

pub async fn run(configs: Vec<ConnectorConfig<'_>>) -> Result<()> {
    let mut connectors: Vec<Connector> = Vec::new();

    for c in configs {
        let port = match c.port {
            Some(port) => port,
            None => {
                if c.tls {
                    DEFAULT_TLS_PORT
                } else {
                    DEFAULT_PORT
                }
            }
        };

        let listener = TcpListener::bind((c.bind, port))
            .await
            .with_context(|| anyhow!("binding {}:{port}", c.bind))?;

        let tls_acceptor = if c.tls {
            Some(crate::tls::setup_acceptor(c.cert, c.key).await?)
        } else {
            None
        };

        connectors.push(Connector {
            listener,
            tls_acceptor,
        });
    }

    for connector in connectors.iter() {
        tracing::info!(tls = connector.tls_acceptor.is_some(), addr = ?connector.listener.local_addr()?, "Listening");
    }

    let mut peers = Peers::new();
    let mut wakers = Wakers::new();
    let mut state = State::new();
    let mut accepting = JoinSet::new();

    loop {
        tokio::select! {
            result = Listen::new(&mut connectors) => {
                let (i, stream, addr) = result.context("accepting connection")?;

                let Some(connector) = connectors.get(i) else {
                    continue;
                };

                // There is some extra work that needs to happen for TLS, so we
                // move it into a separately polled future.
                if let Some(tls_acceptor) = &connector.tls_acceptor {
                    let tls_acceptor = tls_acceptor.clone();

                    accepting.spawn_local(async move {
                        let fut = accept_tls(tls_acceptor, stream);
                        (addr, fut.await)
                    });
                } else {
                    accepting.spawn_local(async move { (addr, Ok(Client::plain(stream))) });
                }
            }
            client = accepting.join_next(), if !accepting.is_empty() => {
                let Some(result) = client else {
                    continue;
                };

                let (addr, client) = result.context("accept connection task panicked")?;

                let client = match client {
                    Ok(client) => client,
                    Err(error) => {
                        tracing::error!(?error, "Accepting connection failed");
                        continue;
                    }
                };

                let peer = Peer::new(client);
                tracing::info!(tls = peer.is_tls(), ?addr, "Connected");

                let peer_state = ServerPeer::new(addr, peer);
                let peer_id = peer_state.data.peer_id;
                peers.insert(peer_id, peer_state);
                state.register(peer_id);
            }
            (id, result) = PollPeers::new(&mut peers, &mut wakers, &mut state) => {
                let Some(mut peer) = peers.remove(&id) else {
                    tracing::warn!(?id, "Peer not found, removed");
                    continue;
                };

                let span = tracing::info_span!("peer", id = ?id, addr = ?peer.data.addr);
                let _guard = span.enter();

                let remove = 'out: {
                    if let Err(error) = result {
                        tracing::error!(%error);
                        break 'out true;
                    }

                    if let Err(error) = state.handle(peer.as_mut(), &mut peers).await {
                        tracing::error!(%error);
                        break 'out true;
                    }

                    // We have to re-poll the peer to set it up for future
                    // wakeups.
                    state.poll.insert(peer.data.peer_id);
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

struct Room {
    /// The name of this room.
    name: Box<[u8]>,
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

    fn register(&mut self, peer_id: PeerId) {
        self.poll.insert(peer_id);
    }

    #[tracing::instrument(skip_all)]
    async fn handle(&mut self, this: Pin<&mut ServerPeer>, peers: &mut Peers) -> Result<()> {
        let (mut timeout, data) = this.project();

        while let Some((message, body)) = data.peer.read::<Event>()? {
            match message {
                Event::Ping => {
                    let ping = body.decode::<PingBody>()?;

                    timeout
                        .as_mut()
                        .reset(time::Instant::now() + Duration::from_secs(5));

                    data.peer.pong(ping.payload)?;
                }
                Event::Connect => {
                    let connect = body.decode::<ConnectBody>()?;
                    tracing::debug!(connect.version, connect.room = ?BStr::new(&connect.room), "Connect");

                    if let Some(old_room) = data.room.replace(connect.room.clone()) {
                        self.leave_room(&old_room, data.peer_id, peers);
                    }

                    for object in connect.objects.iter() {
                        data.objects.insert(object.id, object.clone());
                    }

                    // We have just connected, so send all information about other peers to
                    // the new peer.
                    self.join_room(data, &connect.room, peers);
                }
                Event::Update => {
                    let event = body.decode::<UpdatePeer>()?;
                    tracing::debug!(?event.key, ?event.value, "Update");

                    let Some(object) = data.objects.get_mut(&event.object_id) else {
                        continue;
                    };

                    object.props.insert(event.key, event.value.clone());

                    data.send_to_room(
                        |peer| {
                            peer.peer_mut().updated_peer(
                                data.peer_id,
                                event.object_id,
                                event.key,
                                &event.value,
                            )
                        },
                        &self.rooms,
                        peers,
                    );
                }
                Event::AddObject => {
                    let event = body.decode::<AddObjectBody>()?;
                    tracing::debug!(id = ?event.object.id, "AddObject");

                    let object = event.object.clone();
                    data.objects.insert(object.id, object);

                    data.send_to_room(
                        |peer| {
                            peer.peer_mut()
                                .object_added(data.peer_id, event.object.clone())
                        },
                        &self.rooms,
                        peers,
                    );
                }
                Event::RemoveObject => {
                    let event = body.decode::<RemoveObjectBody>()?;
                    tracing::debug!(id = ?event.object_id, "RemoveObject");

                    data.objects.remove(&event.object_id);

                    data.send_to_room(
                        |peer| {
                            peer.peer_mut()
                                .object_removed(data.peer_id, event.object_id)
                        },
                        &self.rooms,
                        peers,
                    );
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
        mut peer: Pin<Box<ServerPeer>>,
        peers: &mut Peers,
        wakers: &mut Wakers,
    ) {
        let (_, data) = peer.as_mut().project();

        if let Some(room) = &data.room {
            self.leave_room(room, data.peer_id, peers);
        }

        self.poll.remove(&data.peer_id);
        wakers.remove(data.peer_id);
    }

    fn leave_room(&mut self, room_name: &[u8], leaving_id: PeerId, peers: &mut Peers) {
        let Some(room) = self.rooms.get_mut(room_name) else {
            return;
        };

        room.members.retain(|&id| id != leaving_id);

        for id in room.members.iter() {
            let Some(peer) = peers.get_mut(id) else {
                continue;
            };

            if let Err(e) = peer.peer_mut().leave(leaving_id) {
                tracing::error!(?id, ?e, "Failed to send leave");
            } else {
                self.poll.insert(leaving_id);
            }
        }

        let remove = room.members.is_empty();

        if remove {
            self.rooms.remove(room_name);
        }
    }

    fn join_room(&mut self, data: &mut Data, room: &[u8], peers: &mut Peers) {
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

            let objects = data.objects.values().cloned().collect::<Vec<_>>();

            if let Err(error) = peer.peer_mut().join(data.peer_id, &objects) {
                tracing::error!(?id, %error, "Sending join");
            } else {
                self.poll.insert(data.peer_id);
            }
        }

        tracing::debug!(room.name = ?BStr::new(&room.name), members = ?room.members, "Connecting room");

        for id in room.members.iter() {
            let Some(other) = peers.get(id) else {
                continue;
            };

            let objects = other.data.objects.values().cloned().collect::<Vec<_>>();

            if let Err(error) = data.peer.join(other.data.peer_id, &objects) {
                tracing::error!(?id, %error, "Sending join");
            } else {
                self.poll.insert(data.peer_id);
            }
        }

        if !room.members.contains(&data.peer_id) {
            room.members.push(data.peer_id);
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

struct PollPeers<'a> {
    peers: &'a mut Peers,
    wakers: &'a mut Wakers,
    state: &'a mut State,
}

impl<'a> PollPeers<'a> {
    fn new(peers: &'a mut Peers, wakers: &'a mut Wakers, state: &'a mut State) -> Self {
        Self {
            peers,
            wakers,
            state,
        }
    }
}

impl Future for PollPeers<'_> {
    type Output = (PeerId, io::Result<()>);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self {
            peers,
            wakers,
            state,
        } = unsafe { self.get_unchecked_mut() };

        wakers.refresh_parent(state, &peers.peers, cx.waker());

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
