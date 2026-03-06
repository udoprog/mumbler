use core::fmt;
use core::net::SocketAddr;
use core::task::{Context, Poll};

use std::io;
use std::pin::Pin;

use anyhow::{Context as _, Result};
use api::server::{ConnectRequest, Header, Request};
use musli::reader::SliceReader;
use musli_web::api::MessageId;

use crate::{Buf, Client, Scratch};

/// A ready event from a peer.
pub struct Ready {
    kind: ReadyKind,
}

impl fmt::Debug for Ready {
    #[inline]
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.kind.fmt(f)
    }
}

#[derive(Debug)]
enum ReadyKind {
    Read,
    Write,
    Error(io::Error),
}

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

    /// Handles a ready event from the peer.
    pub fn handle(&mut self, ready: Ready) -> Result<()> {
        match ready.kind {
            ReadyKind::Read => {
                self.client.try_read(&mut self.read)?;

                loop {
                    match self.state {
                        State::Idle => {
                            let Some(buf) = self.read.read_array::<4>() else {
                                return Ok(());
                            };

                            let len = u32::from_be_bytes(buf) as usize;
                            self.state = State::Recv(len);
                        }
                        State::Recv(len) => {
                            use musli_web::api::Id;

                            let Some(body) = self.read.read_slice(len) else {
                                return Ok(());
                            };

                            let mut body = SliceReader::new(body);

                            let header: Header = musli::storage::decode(&mut body)?;

                            if let Some(id) = MessageId::new(header.error) {
                                let error = if id == MessageId::ERROR_MESSAGE {
                                    musli::storage::decode(&mut body)?
                                } else {
                                    musli_web::api::ErrorMessage {
                                        message: "Unknown error",
                                    }
                                };

                                return Err(anyhow::anyhow!("{}", error.message));
                            }

                            let Some(request) = Request::from_raw(header.request) else {
                                anyhow::bail!("invalid request type: {}", header.request);
                            };

                            match request {
                                Request::Connect => {
                                    let request: ConnectRequest =
                                        musli::storage::decode(&mut body)?;
                                    tracing::info!(?self.addr, ?request, "client connected");
                                }
                            }

                            self.state = State::Idle;
                        }
                    }
                }
            }
            ReadyKind::Write => {
                self.client.try_write(&mut self.write)?;
                Ok(())
            }
            ReadyKind::Error(error) => Err(error).context("peer error"),
        }
    }

    /// Connects the peer by sending a connect request.
    pub fn connect(&mut self) -> Result<()> {
        self.scratch.request(ConnectRequest { version: 1 })?;
        self.write.write_message(&mut self.scratch);
        Ok(())
    }

    /// Poll the peer for readiness.
    pub async fn ready(&self) -> Ready {
        let ready = ReadyFuture { peer: self };
        ready.await
    }
}

struct ReadyFuture<'a> {
    peer: &'a Peer,
}

impl Future for ReadyFuture<'_> {
    type Output = Ready;

    #[inline]
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        self.peer.poll(cx)
    }
}

impl Peer {
    /// Polls the peer for readiness.
    pub fn poll(&self, cx: &mut Context<'_>) -> Poll<Ready> {
        if self.write.has_remaining()
            && let Poll::Ready(result) = self.client.poll_write_ready(cx)
        {
            cx.waker().wake_by_ref();

            let kind = match result {
                Ok(()) => ReadyKind::Write,
                Err(e) => ReadyKind::Error(e),
            };

            return Poll::Ready(Ready { kind });
        }

        if let Poll::Ready(result) = self.client.poll_read_ready(cx) {
            cx.waker().wake_by_ref();

            let kind = match result {
                Ok(()) => ReadyKind::Read,
                Err(e) => ReadyKind::Error(e),
            };

            return Poll::Ready(Ready { kind });
        }

        Poll::Pending
    }
}
