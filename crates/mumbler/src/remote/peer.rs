use core::net::SocketAddr;
use core::task::{Context, Poll};

use std::io;
use std::pin::Pin;

use anyhow::Result;
use api::{Color, Id, Transform};
use musli::alloc::Global;
use musli::mode::Binary;
use musli::reader::SliceReader;
use musli::storage;
use musli_core::Decode;
use musli_web::api::{ErrorMessage, MessageId};

use crate::remote::api::{
    UpdateColorBody, UpdateImageBody, UpdateTransform, UpdatedColorBody, UpdatedImageBody,
    UpdatedTransform,
};

use super::api::{ConnectBody, Header, JoinBody, LeaveBody, PingBody, PongBody};
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
    pub fn connect(&mut self, room: &[u8]) -> Result<()> {
        self.scratch.send(ConnectBody {
            version: 1,
            room: Box::from(room),
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
    pub fn join(&mut self, id: Id) -> Result<()> {
        self.scratch.send(JoinBody { id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having left the room.
    pub fn leave(&mut self, id: Id) -> Result<()> {
        self.scratch.send(LeaveBody { id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Move the peer to the given position and front.
    pub fn update_transform(&mut self, transform: Transform) -> Result<()> {
        self.scratch.send(UpdateTransform { transform })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having moved to the given position and front.
    pub fn updated_transform(&mut self, id: Id, transform: Transform) -> Result<()> {
        self.scratch.send(UpdatedTransform { id, transform })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Update the peer's image.
    pub fn update_image(&mut self, image: Option<Vec<u8>>) -> Result<()> {
        self.scratch.send(UpdateImageBody { image })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Update the peer's image.
    pub fn updated_image(&mut self, peer_id: Id, image: Option<Vec<u8>>) -> Result<()> {
        self.scratch.send(UpdatedImageBody { id: peer_id, image })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Update the peer's color.
    pub fn update_color(&mut self, color: Color) -> Result<()> {
        self.scratch.send(UpdateColorBody { color })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Update the peer's color.
    pub fn updated_color(&mut self, peer_id: Id, color: Color) -> Result<()> {
        self.scratch.send(UpdatedColorBody { id: peer_id, color })?;
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
        self.get_mut().peer.poll(cx)
    }
}

impl Unpin for ReadyFuture<'_> {}

impl Peer {
    /// Polls the peer for readiness.
    pub fn poll(&mut self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.write.has_remaining() {
            if let Poll::Ready(result) = self.client.poll_write_ready(cx) {
                if let Err(e) = result {
                    return Poll::Ready(Err(e));
                };

                if let Err(e) = self.client.try_write(&mut self.write) {
                    return Poll::Ready(Err(e));
                };
            }
        }

        if let Poll::Ready(result) = self.client.poll_read_ready(cx) {
            if let Err(e) = result {
                return Poll::Ready(Err(e));
            };

            if let Err(e) = self.client.try_read(&mut self.read) {
                return Poll::Ready(Err(e));
            };

            return Poll::Ready(Ok(()));
        }

        Poll::Pending
    }
}
