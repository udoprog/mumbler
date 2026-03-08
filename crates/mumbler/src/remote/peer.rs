use core::net::SocketAddr;
use core::task::{Context, Poll};

use std::collections::HashMap;
use std::io;
use std::pin::Pin;

use anyhow::Result;
use api::{Id, Key, Value};
use musli::alloc::Global;
use musli::mode::Binary;
use musli::reader::SliceReader;
use musli::storage;
use musli_core::Decode;
use musli_web::api::{ErrorMessage, MessageId};

use crate::remote::api::{JoinBodyRef, UpdatePeerRef, UpdatedPeerRef};

use super::api::{ConnectBody, Header, LeaveBody, PingBody, PongBody};
use super::{Buf, Client, Scratch};

const MAX_MESSAGE: usize = 1024 * 1024;

enum State {
    Idle,
    Recv(usize),
}

/// A connected peer.
pub struct Peer {
    addr: SocketAddr,
    client: Client,
    read: Buf,
    write: Buf,
    scratch: Scratch,
    state: State,
}

impl Peer {
    /// Constructs a connected peer.
    pub fn new(addr: SocketAddr, client: Client) -> Self {
        Self {
            addr,
            client,
            read: Buf::new(),
            write: Buf::new(),
            scratch: Scratch::new(),
            state: State::Idle,
        }
    }

    /// Returns the socket address of the peer.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Returns whether the peer is connected over TLS.
    pub fn is_tls(&self) -> bool {
        self.client.is_tls()
    }

    /// Read messages from the peer. Returns `Ok(None)` when no more messages
    /// are currently available.
    #[inline]
    pub fn handle<M>(&mut self) -> Result<Option<(M, Body<'_>)>>
    where
        M: musli_web::api::Id,
    {
        loop {
            match self.state {
                State::Idle => {
                    let Some(buf) = self.read.read_array::<4>() else {
                        return Ok(None);
                    };

                    let len = u32::from_be_bytes(buf) as usize;

                    if len > MAX_MESSAGE {
                        return Err(anyhow::anyhow!(
                            "received message {len} is larger than max {MAX_MESSAGE} bytes"
                        ));
                    }

                    self.state = State::Recv(len);
                }
                State::Recv(len) => {
                    let Some(body) = self.read.read_slice(len) else {
                        return Ok(None);
                    };

                    let mut body = SliceReader::new(body);

                    let header: Header = storage::decode(&mut body)?;

                    if let Some(id) = MessageId::new(header.error) {
                        let error = if id == MessageId::ERROR_MESSAGE {
                            storage::decode(&mut body)?
                        } else {
                            ErrorMessage {
                                message: "Unknown error",
                            }
                        };

                        return Err(anyhow::anyhow!("{}", error.message));
                    }

                    let Some(id) = MessageId::new(header.request) else {
                        anyhow::bail!("invalid request type: {}", header.request);
                    };

                    let m = M::from_id(id);

                    self.state = State::Idle;

                    return Ok(Some((
                        m,
                        Body {
                            data: body.as_slice(),
                        },
                    )));
                }
            }
        }
    }

    /// Connects the peer by sending a connect request.
    pub fn connect(&mut self, room: &[u8], values: HashMap<Key, Value>) -> Result<()> {
        self.scratch.send(ConnectBody {
            version: 1,
            room: Box::from(room),
            values,
        })?;

        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Send a ping message to the peer.
    pub fn ping(&mut self, payload: u64) -> Result<()> {
        self.scratch.send(PingBody { payload })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Send a pong message to the peer.
    pub fn pong(&mut self, payload: u64) -> Result<()> {
        self.scratch.send(PongBody { payload })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having joined the room.
    pub fn join(&mut self, id: Id, values: &HashMap<Key, Value>) -> Result<()> {
        self.scratch.send(JoinBodyRef { id, values })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having left the room.
    pub fn leave(&mut self, id: Id) -> Result<()> {
        self.scratch.send(LeaveBody { id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Update the peer with the given key and value.
    pub fn update_peer(&mut self, key: Key, value: &Value) -> Result<()> {
        self.scratch.send(UpdatePeerRef { key, value })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having updated the given key and value.
    pub fn updated_peer(&mut self, id: Id, key: Key, value: &Value) -> Result<()> {
        self.scratch.send(UpdatedPeerRef { id, key, value })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Poll the peer for readiness.
    pub async fn ready(&mut self) -> io::Result<()> {
        let ready = ReadyFuture { peer: self };
        ready.await
    }
}

pub struct Body<'a> {
    data: &'a [u8],
}

impl<'de> Body<'de> {
    /// Decode the body as the given type.
    pub fn decode<T>(self) -> Result<T>
    where
        T: Decode<'de, Binary, Global>,
    {
        let mut reader = SliceReader::new(self.data);
        let value = storage::decode(&mut reader)?;
        Ok(value)
    }

    /// Get the length of the body in bytes.
    pub fn len(&self) -> usize {
        self.data.len()
    }
}

struct ReadyFuture<'a> {
    peer: &'a mut Peer,
}

impl Future for ReadyFuture<'_> {
    type Output = io::Result<()>;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut *self.get_mut().peer).poll(cx)
    }
}

impl Peer {
    fn project(self: Pin<&mut Self>) -> (Pin<&mut Client>, &mut Buf, &mut Buf) {
        unsafe {
            let this = self.get_unchecked_mut();
            (
                Pin::new_unchecked(&mut this.client),
                &mut this.write,
                &mut this.read,
            )
        }
    }

    /// Polls the peer for readiness.
    pub fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let (mut client, write, read) = self.project();

        let write_result = client.as_mut().poll_write(cx, write);
        let read_result = client.as_mut().poll_read(cx, read);

        // We ignore `Ok(())`, since it just means that there is nothing in the
        // write buffer at the moment.
        if let Poll::Ready(Err(e)) = write_result {
            return Poll::Ready(Err(e));
        }

        if let Poll::Ready(result) = read_result {
            return Poll::Ready(result);
        }

        Poll::Pending
    }
}
