use core::task::{Context, Poll};

use std::io;
use std::pin::Pin;

use anyhow::Result;
use api::{Id, Key, PeerId, Properties, PublicKey, RemoteObject, Value};
use musli::alloc::Global;
use musli::mode::Binary;
use musli::reader::SliceReader;
use musli::storage;
use musli_core::Decode;
use musli_web::api::{ErrorMessage, MessageId};

use crate::remote::api::{ImageCreatedBodyRef, ObjectCreatedBodyRef};

use super::api::{
    ChallengeBodyRef, ConnectBodyRef, Header, HelloBody, ImageCreateBody, ImageRemoveBody,
    ImageRemovedBody, ObjectCreateBody, ObjectRemoveBody, ObjectRemovedBody, ObjectUpdateBodyRef,
    ObjectUpdatedBodyRef, PeerConnectedBodyRef, PeerDisconnectBody, PeerJoinBodyRef, PeerLeaveBody,
    PeerUpdateBodyRef, PeerUpdatedBodyRef, PingBody, PongBody, RemoteImage, Signature,
};
use super::{Buf, Client, Scratch, VERSION};

const MAX_MESSAGE: usize = 1024 * 1024 * 10;

enum State {
    Idle,
    Recv(usize),
}

/// A connected peer.
pub struct Peer {
    client: Client,
    read: Buf,
    write: Buf,
    scratch: Scratch,
    state: State,
}

impl Peer {
    /// Constructs a connected peer.
    pub fn new(client: Client) -> Self {
        Self {
            client,
            read: Buf::new(),
            write: Buf::new(),
            scratch: Scratch::new(),
            state: State::Idle,
        }
    }

    /// Returns whether the peer is connected over TLS.
    pub fn is_tls(&self) -> bool {
        self.client.is_tls()
    }

    /// Read messages from the peer. Returns `Ok(None)` when no more messages
    /// are currently available.
    #[inline]
    pub fn read<M>(&mut self) -> Result<Option<(M, Body<'_>)>>
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

    /// Sends a challenge nonce to the peer. The client must respond with a
    /// signed `Connect` message.
    pub fn challenge(&mut self, nonce: &[u8]) -> Result<()> {
        self.scratch.send(ChallengeBodyRef { nonce })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Sends a hello to the server, announcing the desired room and initial
    /// state. The server responds with a [`ChallengeBody`].
    pub fn hello(&mut self) -> Result<()> {
        self.scratch.send(HelloBody { version: VERSION })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Connect to the remote server, registering a peer id and providing a challenge response.
    pub fn connect(
        &mut self,
        public_key: PublicKey,
        signature: Signature,
        objects: &[RemoteObject],
        images: &[RemoteImage],
        props: &Properties,
    ) -> Result<()> {
        self.scratch.send(ConnectBodyRef {
            public_key,
            signature,
            objects,
            images,
            props,
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

    /// Mark the given peer as having connected.
    pub fn peer_connected(
        &mut self,
        public_key: PublicKey,
        peer_id: PeerId,
        props: &Properties,
    ) -> Result<()> {
        self.scratch.send(PeerConnectedBodyRef {
            public_key,
            peer_id,
            props,
        })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having joined the room.
    pub fn peer_join(&mut self, peer_id: PeerId, objects: &[RemoteObject]) -> Result<()> {
        self.scratch.send(PeerJoinBodyRef { peer_id, objects })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Update the peer with the given key and value.
    pub fn peer_update(&mut self, key: Key, value: &Value) -> Result<()> {
        self.scratch.send(PeerUpdateBodyRef { key, value })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having updated the given key and value.
    pub fn peer_updated(&mut self, peer_id: PeerId, key: Key, value: &Value) -> Result<()> {
        self.scratch.send(PeerUpdatedBodyRef {
            peer_id,
            key,
            value,
        })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having disconnected.
    pub fn peer_disconnected(&mut self, id: PeerId) -> Result<()> {
        self.scratch.send(PeerDisconnectBody { id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Mark the given peer as having left the room.
    pub fn peer_leave(&mut self, id: PeerId) -> Result<()> {
        self.scratch.send(PeerLeaveBody { id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Indicate that the given object value has been updated.
    pub fn object_update(&mut self, id: Id, key: Key, value: &Value) -> Result<()> {
        self.scratch.send(ObjectUpdateBodyRef {
            object_id: id,
            key,
            value,
        })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Send indication that the given object value has been updated by a peer.
    pub fn object_updated(
        &mut self,
        peer_id: PeerId,
        object_id: Id,
        key: Key,
        value: &Value,
    ) -> Result<()> {
        self.scratch.send(ObjectUpdatedBodyRef {
            peer_id,
            object_id,
            key,
            value,
        })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Send a request to add a new object.
    pub fn object_create(&mut self, object: RemoteObject) -> Result<()> {
        self.scratch.send(ObjectCreateBody { object })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Send a request to remove an object.
    pub fn object_remove(&mut self, object_id: Id) -> Result<()> {
        self.scratch.send(ObjectRemoveBody { object_id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Broadcast that a peer has removed an object.
    pub fn object_removed(&mut self, peer_id: PeerId, object_id: Id) -> Result<()> {
        self.scratch
            .send(ObjectRemovedBody { peer_id, object_id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Broadcast that a peer has added an object.
    pub fn object_created(&mut self, peer_id: PeerId, object: &RemoteObject) -> Result<()> {
        self.scratch
            .send(ObjectCreatedBodyRef { peer_id, object })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Send a request to add a new image.
    pub fn image_create(&mut self, image: RemoteImage) -> Result<()> {
        self.scratch.send(ImageCreateBody { image })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Broadcast that a peer has added an image.
    pub fn image_created(&mut self, peer_id: PeerId, image: &RemoteImage) -> Result<()> {
        self.scratch.send(ImageCreatedBodyRef { peer_id, image })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Send a request to remove an image.
    pub fn image_remove(&mut self, image_id: Id) -> Result<()> {
        self.scratch.send(ImageRemoveBody { image_id })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Broadcast that a peer has removed an image.
    pub fn image_removed(&mut self, peer_id: PeerId, image_id: Id) -> Result<()> {
        self.scratch.send(ImageRemovedBody { peer_id, image_id })?;
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
