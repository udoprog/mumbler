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
use api::{Id, Key, PeerId, Properties, RemoteId, RemoteObject, Type};
use parking_lot::Mutex;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinSet;
use tokio::time::{self, Sleep};

use crate::crypto;
use crate::remote::api::{
    ImageCreateBody, ImageRemoveBody, ObjectCreateBody, ObjectRemoveBody, ObjectUpdateBody,
    PeerUpdateBody, RemoteImage,
};
use crate::remote::{DEFAULT_PORT, DEFAULT_TLS_PORT};
use crate::tls;

use super::api::{ConnectBody, Event, HelloBody, PingBody};
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

struct ServerPeerState {
    /// The socket address of the peer.
    addr: SocketAddr,
    /// The peer state.
    peer: Peer,
    /// The unique identifier of this peer.
    peer_id: PeerId,
    /// Server-generated nonce sent as a challenge once the client sends Hello.
    /// The client must sign this to prove ownership of its private key.
    /// `None` until Hello is received; cleared to `None` after successful auth.
    challenge: Option<[u8; 32]>,
    /// Objects that the peer has set.
    objects: HashMap<Id, RemoteObject>,
    /// Images that the peer has set.
    images: HashMap<Id, RemoteImage>,
    /// Remote properties of the peer.
    props: Properties,
}

impl ServerPeerState {
    /// Get the room that this peer is currently in, if any.
    fn room(&self) -> &RemoteId {
        self.props.get(Key::ROOM).as_remote_id()
    }
}

struct ServerPeer {
    /// If this timeout is reached, the peer is disconnected.
    ///
    /// The timeout is reset every time a ping is received.
    timeout: Sleep,
    /// Peer data.
    state: ServerPeerState,
}

impl ServerPeer {
    fn new(addr: SocketAddr, peer: Peer) -> Pin<Box<Self>> {
        Box::pin(Self {
            timeout: time::sleep(Duration::from_secs(5)),
            state: ServerPeerState {
                addr,
                peer,
                peer_id: PeerId::ZERO,
                challenge: None,
                objects: HashMap::new(),
                images: HashMap::new(),
                props: Properties::new(),
            },
        })
    }

    #[inline]
    fn peer_mut(self: Pin<&mut Self>) -> &mut Peer {
        // SAFETY: Interior Peer is Unpin.
        let this = unsafe { self.get_unchecked_mut() };
        &mut this.state.peer
    }

    #[inline]
    fn project(self: Pin<&mut Self>) -> (Pin<&mut Sleep>, &mut ServerPeerState) {
        unsafe {
            let this = self.get_unchecked_mut();
            (Pin::new_unchecked(&mut this.timeout), &mut this.state)
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

    /// Iterate over all known peer ids.
    #[inline]
    fn ids(&self) -> impl Iterator<Item = PeerId> + '_ {
        self.peers.keys().copied()
    }

    /// Iterate over peers.
    #[inline]
    fn iter(&self) -> impl Iterator<Item = (PeerId, &ServerPeer)> {
        self.peers.iter().map(|(id, peer)| (*id, &**peer))
    }

    /// Iterate mutably over all peers.
    #[inline]
    fn iter_mut(&mut self) -> impl Iterator<Item = (PeerId, Pin<&mut ServerPeer>)> {
        self.peers.iter_mut().map(|(id, peer)| (*id, peer.as_mut()))
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
        tracing::info!(tls = connector.tls_acceptor.is_some(), addr = ?connector.listener.local_addr()?, "listening");
    }

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
                        tracing::error!(?error, "accepting connection failed");
                        continue;
                    }
                };

                let peer = Peer::new(client);
                tracing::info!(tls = peer.is_tls(), ?addr, "connected");

                let peer_state = ServerPeer::new(addr, peer);
                let peer_id = peer_state.state.peer_id;
                state.peers.insert(peer_id, peer_state);
                state.poll.insert(peer_id);
            }
            (id, result) = PollPeers::new(&mut wakers, &mut state) => {
                let Some(mut peer) = state.peers.remove(&id) else {
                    tracing::warn!(?id, "peer not found, removed");
                    continue;
                };

                let span = tracing::info_span!("peer", id = ?id, addr = ?peer.state.addr);
                let _guard = span.enter();

                let remove = 'out: {
                    if let Err(error) = result {
                        tracing::error!(%error);
                        break 'out true;
                    }

                    if let Err(error) = state.handle(peer.as_mut()).await {
                        tracing::error!(%error);
                        break 'out true;
                    }

                    // We have to re-poll the peer to set it up for future
                    // wakeups.
                    state.poll.insert(peer.state.peer_id);
                    false
                };

                if remove {
                    state.remove_peer(peer, &mut wakers);
                } else {
                    // peer_id may have changed during authentication (temp random → real key)
                    state.peers.insert(peer.state.peer_id, peer);
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
struct ServerRoom {
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

    fn refresh_parent(&mut self, state: &mut State, parent: &Waker) {
        let changed = self.waker.as_ref().is_none_or(|w| !w.will_wake(parent));

        if changed {
            self.waker = Some(parent.clone());
            self.wakers.clear();

            for id in state.peers.ids() {
                state.poll.insert(id);
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
    /// Available client contexts, keyed by RoomId (owner PeerId + name).
    rooms: HashMap<RemoteId, ServerRoom>,
    /// Peers that are currently connected.
    peers: Peers,
}

impl State {
    fn new() -> Self {
        Self {
            poll: BTreeSet::new(),
            rooms: HashMap::new(),
            peers: Peers::new(),
        }
    }

    async fn handle(&mut self, this: Pin<&mut ServerPeer>) -> Result<()> {
        let (mut timeout, this) = this.project();

        while let Some((message, body)) = this.peer.read::<Event>()? {
            match message {
                Event::Ping => {
                    let body = body.decode::<PingBody>()?;
                    tracing::trace!(?message, payload = ?body.payload);

                    timeout
                        .as_mut()
                        .reset(time::Instant::now() + Duration::from_secs(5));

                    this.peer.pong(body.payload)?;
                }
                Event::Hello => {
                    let body = body.decode::<HelloBody>()?;
                    tracing::debug!(?message, body.version);

                    if this.challenge.is_some() {
                        anyhow::bail!("received duplicate Hello");
                    }

                    let nonce: [u8; 32] = rand::random();
                    this.peer.challenge(&nonce)?;
                    this.challenge = Some(nonce);
                }
                Event::Connect => {
                    let body = body.decode::<ConnectBody>()?;
                    tracing::debug!(?message, ?body.props);

                    let Some(challenge) = this.challenge.take() else {
                        anyhow::bail!("received Connect without a prior Hello");
                    };

                    crypto::verify(body.peer_id, &challenge, &body.signature)?;

                    if self.peers.get(&body.peer_id).is_some() {
                        anyhow::bail!("a peer with that identity is already connected");
                    }

                    this.peer_id = body.peer_id;
                    this.props = body.props;

                    for object in body.objects {
                        match object.ty {
                            Type::ROOM => {
                                self.create_room(this, object.id);
                            }
                            _ => {}
                        }

                        this.objects.insert(object.id, object);
                    }

                    for image in body.images {
                        this.images.insert(image.id, image);
                    }

                    // Send all global objects of *other* peers to this peer.
                    for (id, peer) in self.peers.iter() {
                        if this.peer_id == id {
                            continue;
                        }

                        for object in peer.state.objects.values().filter(|o| o.ty.is_global()) {
                            tracing::debug!(?object.id, ?object.ty, ?object.props, "global create object to new peer");

                            this.peer
                                .object_created(peer.state.peer_id, object.clone())?;
                        }
                    }

                    self.connected(this);

                    let room = this.room();

                    if !room.is_zero() {
                        let room = room.clone();
                        self.join_room(this, &room);
                    }
                }
                Event::PeerUpdate if !this.peer_id.is_zero() => {
                    let body = body.decode::<PeerUpdateBody>()?;
                    tracing::debug!(?message, ?body.key, ?body.value);

                    let old = this.props.insert(body.key, body.value.clone());

                    self.send_to_room(this, |data, peer| {
                        peer.peer_mut()
                            .peer_updated(data.peer_id, body.key, &body.value)
                    });

                    match body.key {
                        Key::ROOM => {
                            let old_room = old.as_remote_id();
                            let new_room = body.value.as_remote_id();

                            if !old_room.is_zero() {
                                self.leave_room(this, old_room, this.peer_id);
                            }

                            if !new_room.is_zero() {
                                self.join_room(this, new_room);
                            }
                        }
                        _ => {}
                    }
                }
                Event::ObjectCreate if !this.peer_id.is_zero() => {
                    let body = body.decode::<ObjectCreateBody>()?;
                    tracing::debug!(?message, ?body.object.ty, ?body.object.id, ?body.object.props);

                    let object = body.object.clone();

                    this.objects.insert(object.id, object);

                    let action = |this: &mut ServerPeerState, peer: Pin<&mut ServerPeer>| {
                        peer.peer_mut()
                            .object_created(this.peer_id, body.object.clone())
                    };

                    match body.object.ty {
                        Type::ROOM => {
                            self.create_room(this, body.object.id);
                            self.send_to_all(this, action);
                        }
                        ty if ty.is_global() => {
                            self.send_to_all(this, action);
                        }
                        _ => {
                            self.send_to_room(this, action);
                        }
                    }
                }
                Event::ObjectUpdate if !this.peer_id.is_zero() => {
                    let body = body.decode::<ObjectUpdateBody>()?;
                    tracing::debug!(?message, ?body.object_id, ?body.key, ?body.value);

                    let Some(object) = this.objects.get_mut(&body.object_id) else {
                        continue;
                    };

                    object.props.insert(body.key, body.value.clone());

                    let action = |this: &mut ServerPeerState, peer: Pin<&mut ServerPeer>| {
                        peer.peer_mut().object_updated(
                            this.peer_id,
                            body.object_id,
                            body.key,
                            &body.value,
                        )
                    };

                    if object.ty.is_global() {
                        self.send_to_all(this, action);
                    } else {
                        self.send_to_room(this, action);
                    }
                }
                Event::ObjectRemove if !this.peer_id.is_zero() => {
                    let body = body.decode::<ObjectRemoveBody>()?;
                    tracing::debug!(?message, ?body.object_id);

                    let Some(object) = this.objects.remove(&body.object_id) else {
                        continue;
                    };

                    tracing::debug!(?message, ?object.id, ?object.ty, "removing object");

                    let action = |this: &mut ServerPeerState, peer: Pin<&mut ServerPeer>| {
                        peer.peer_mut().object_removed(this.peer_id, body.object_id)
                    };

                    match object.ty {
                        Type::ROOM => {
                            let room = RemoteId::new(this.peer_id, body.object_id);
                            self.rooms.remove(&room);
                            self.send_to_all(this, action);
                        }
                        ty if ty.is_global() => {
                            self.send_to_all(this, action);
                        }
                        _ => {
                            self.send_to_room(this, action);
                        }
                    }
                }
                Event::ImageCreate if !this.peer_id.is_zero() => {
                    let body = body.decode::<ImageCreateBody>()?;
                    tracing::debug!(?message, id = ?body.image.id);

                    let image = body.image.clone();
                    this.images.insert(image.id, image);

                    self.send_to_room(this, |data, peer| {
                        peer.peer_mut()
                            .image_created(data.peer_id, body.image.clone())
                    });
                }
                Event::ImageRemove if !this.peer_id.is_zero() => {
                    let body = body.decode::<ImageRemoveBody>()?;
                    tracing::debug!(?message, id = ?body.image_id);

                    this.images.remove(&body.image_id);

                    self.send_to_room(this, |data, peer| {
                        peer.peer_mut().image_removed(data.peer_id, body.image_id)
                    });
                }
                event => {
                    return Err(anyhow::anyhow!("unsupported event: {event:?}"));
                }
            }
        }

        Ok(())
    }

    /// Create a new room, and ensure that all peers that have it as their
    /// property gets added to it.
    fn create_room(&mut self, this: &mut ServerPeerState, object_id: Id) {
        let room = RemoteId::new(this.peer_id, object_id);
        let r = self.rooms.entry(room).or_default();

        r.members.clear();

        // When a new room is created, make sure we add all
        // existing members to it.
        for (id, peer) in self.peers.iter() {
            if peer.state.room() == &room {
                r.members.push(id);
            }
        }

        // If the room creator has it as its room.
        if this.room() == &room {
            r.members.push(this.peer_id);
        }
    }

    fn remove_peer(&mut self, mut peer: Pin<Box<ServerPeer>>, wakers: &mut Wakers) {
        let (_, data) = peer.as_mut().project();

        self.disconnected(data);
        self.poll.remove(&data.peer_id);
        wakers.remove(data.peer_id);
    }

    fn connected(&mut self, this: &mut ServerPeerState) {
        let objects = this
            .objects
            .values()
            .filter(|o| o.ty.is_global())
            .cloned()
            .collect::<Vec<_>>();

        self.send_to_all(this, |data, peer| {
            peer.peer_mut()
                .peer_connected(data.peer_id, &objects, &data.props)
        });

        for (id, peer) in self.peers.iter_mut() {
            let objects = peer
                .state
                .objects
                .values()
                .filter(|o| o.ty.is_global())
                .cloned()
                .collect::<Vec<_>>();

            let props = &peer.state.props;

            if let Err(error) = this.peer.peer_connected(id, &objects, props) {
                tracing::error!(?id, %error, "sending connected");
            } else {
                self.poll.insert(this.peer_id);
            }
        }
    }

    fn disconnected(&mut self, this: &mut ServerPeerState) {
        self.send_to_all(this, |this, peer| {
            peer.peer_mut().peer_disconnected(this.peer_id)
        });
    }

    fn join_room(&mut self, this: &mut ServerPeerState, room: &RemoteId) {
        let Some(r) = self.rooms.get_mut(room) else {
            return;
        };

        tracing::debug!(members = ?r.members, "joining room");

        for id in r.members.iter() {
            let Some(mut peer) = self.peers.get_mut(id) else {
                continue;
            };

            let objects = this
                .objects
                .values()
                .filter(|o| !o.ty.is_global())
                .cloned()
                .collect::<Vec<_>>();

            let images = this.images.values().cloned().collect::<Vec<_>>();

            if let Err(error) = peer
                .as_mut()
                .peer_mut()
                .peer_join(this.peer_id, &objects, &images)
            {
                tracing::error!(?id, %error, "sending join");
            } else {
                self.poll.insert(peer.state.peer_id);
            }

            let objects = peer
                .state
                .objects
                .values()
                .filter(|o| !o.ty.is_global())
                .cloned()
                .collect::<Vec<_>>();

            let images = peer.state.images.values().cloned().collect::<Vec<_>>();

            if let Err(error) = this.peer.peer_join(peer.state.peer_id, &objects, &images) {
                tracing::error!(?id, %error, "sending join");
            } else {
                self.poll.insert(this.peer_id);
            }
        }

        if !r.members.contains(&this.peer_id) {
            r.members.push(this.peer_id);
        }
    }

    fn leave_room(&mut self, this: &mut ServerPeerState, room: &RemoteId, leaving_id: PeerId) {
        let Some(r) = self.rooms.get_mut(room) else {
            return;
        };

        r.members.retain(|&id| id != leaving_id);

        for id in r.members.iter() {
            let Some(peer) = self.peers.get_mut(id) else {
                continue;
            };

            // Send leave to other peers.
            if let Err(e) = peer.peer_mut().peer_leave(leaving_id) {
                tracing::error!(?id, ?e, "sending leave");
            } else {
                self.poll.insert(leaving_id);
            }

            // Send leave messages to the peer that is leaving the room.
            if let Err(e) = this.peer.peer_leave(*id) {
                tracing::error!(?id, ?e, "sending leave");
            } else {
                self.poll.insert(this.peer_id);
            }
        }

        let remove = r.members.is_empty();

        if remove {
            self.rooms.remove(room);
        }
    }

    /// Send to the room this peer belongs to, if any.
    fn send_to_room<F>(&mut self, this: &mut ServerPeerState, mut f: F)
    where
        F: FnMut(&mut ServerPeerState, Pin<&mut ServerPeer>) -> Result<()>,
    {
        let Some(room) = self.rooms.get(this.room()) else {
            return;
        };

        for id in room.members.iter() {
            if this.peer_id == *id {
                continue;
            }

            if let Some(peer) = self.peers.get_mut(id) {
                if let Err(error) = f(this, peer) {
                    tracing::error!(?id, ?error, "Sending update");
                } else {
                    self.poll.insert(*id);
                }
            }
        }
    }

    /// Send to all other peers.
    fn send_to_all<F>(&mut self, this: &mut ServerPeerState, mut f: F)
    where
        F: FnMut(&mut ServerPeerState, Pin<&mut ServerPeer>) -> Result<()>,
    {
        for (id, peer) in self.peers.iter_mut() {
            if this.peer_id == id {
                continue;
            }

            tracing::debug!(?id, "sending update to peer");

            if let Err(error) = f(this, peer) {
                tracing::error!(?id, ?error, "Sending update");
            } else {
                self.poll.insert(id);
            }
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
    wakers: &'a mut Wakers,
    state: &'a mut State,
}

impl<'a> PollPeers<'a> {
    fn new(wakers: &'a mut Wakers, state: &'a mut State) -> Self {
        Self { wakers, state }
    }
}

impl Future for PollPeers<'_> {
    type Output = (PeerId, io::Result<()>);

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let Self { wakers, state } = unsafe { self.get_unchecked_mut() };

        wakers.refresh_parent(state, cx.waker());

        while let Some(id) = state.poll.pop_first() {
            let Some(peer) = state.peers.get_mut(&id) else {
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

            if let Some(peer) = state.peers.get_mut(&id) {
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
