use core::net::SocketAddr;
use core::pin::Pin;
use core::task::{Context, Poll, ready};

use std::io;

use anyhow::Result;
use musli_core::Encode;
use musli_core::mode::Binary;
use musli_web::api::{Broadcast, Event};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio::net::{TcpStream, ToSocketAddrs};
#[cfg(feature = "tls")]
use tokio_rustls::TlsStream;

const READ_CAP: usize = 4096;
const MAX_CAPACITY: usize = 10 * 1024 * 1024;

/// A scratch buffer for temporary data, used for serialization.
pub struct Scratch {
    data: Vec<u8>,
}

impl Scratch {
    /// Create a new scratch buffer with the given capacity.
    #[inline]
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(READ_CAP),
        }
    }

    /// Write an event to the peer.
    #[inline]
    pub fn send<E>(&mut self, event: E) -> Result<()>
    where
        E: Event,
    {
        self.write(&super::api::Header {
            request: <E::Broadcast as Broadcast>::ID.get(),
            error: 0,
        })?;

        self.write(&event)?;
        Ok(())
    }

    /// Write a value to the scratch buffer.
    #[inline]
    pub fn write<T>(&mut self, data: &T) -> Result<()>
    where
        T: ?Sized + Encode<Binary>,
    {
        musli::storage::encode(&mut self.data, data)?;
        Ok(())
    }
}

/// A client buffer.
pub struct Buf {
    // Buffer for incoming data.
    data: Vec<u8>,
    // Read position from the buffer.
    read: usize,
    // Write position in the buffer.
    write: usize,
}

impl Buf {
    /// Create a new buffer with the given capacity.
    #[inline]
    pub fn new() -> Self {
        Self {
            data: Vec::new(),
            read: 0,
            write: 0,
        }
    }

    /// Extend the buffer with data from the scratch buffer.
    #[inline]
    pub fn write_message(&mut self, body: &mut Scratch) {
        let Ok(len) = u32::try_from(body.data.len()) else {
            return;
        };

        let len = len.to_be_bytes();
        self.write_bytes(&len);
        self.write_bytes(&body.data);
        body.data.clear();
    }

    /// Read an array of `N` bytes from the buffer, advancing the read position.
    #[inline]
    pub fn read_array<const N: usize>(&mut self) -> Option<[u8; N]> {
        if self.read + N > self.write {
            return None;
        }

        let buf = self.data.get(self.read..self.read + N)?;

        let Ok(buf) = buf.try_into() else {
            return None;
        };

        self.advance_read(N);
        Some(buf)
    }

    /// Read a slice of `len` bytes from the buffer, advancing the read position.
    #[inline]
    pub fn read_slice(&mut self, len: usize) -> Option<&[u8]> {
        if self.read + len > self.write {
            return None;
        }

        let range = self.read..self.read + len;
        self.advance_read(len);
        self.data.get(range)
    }

    /// Get the unread portion of the buffer.
    #[inline]
    pub fn read_buf(&self) -> &[u8] {
        self.data.get(self.read..self.write).unwrap_or_default()
    }

    /// Write data to the buffer.
    #[inline]
    pub fn write_bytes(&mut self, data: &[u8]) {
        let next = self.write.checked_add(data.len()).expect("write overflow");

        if next > self.data.len() {
            self.data.resize(next, 0);
        }

        let Some(bytes) = self.data.get_mut(self.write..next) else {
            return;
        };

        bytes.copy_from_slice(data);
        self.write = next;
    }

    /// Get the unread portion of the buffer.
    #[inline]
    pub fn write_buf(&mut self) -> Option<&mut [u8]> {
        let needed = self.write + READ_CAP;

        if needed > MAX_CAPACITY {
            return None;
        }

        if needed > self.data.len() {
            self.data.resize(needed, 0);
        }

        let bytes = self.data.get_mut(self.write..needed).unwrap_or_default();

        Some(bytes)
    }

    /// Get the number of unread bytes in the buffer.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.write - self.read
    }

    /// Returns `true` if there are unread bytes in the buffer.
    #[inline]
    pub fn has_remaining(&self) -> bool {
        self.write > self.read
    }

    /// Get the allocated capacity of the buffer.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.data.capacity()
    }

    pub fn advance(&mut self, n: usize) {
        self.write = self.write.checked_add(n).expect("write overflow");
    }

    /// Advance the read position by `n` bytes.
    #[inline]
    pub fn advance_read(&mut self, n: usize) {
        self.read = self.read.checked_add(n).expect("read overflow");

        if self.read >= self.write {
            self.read = 0;
            self.write = 0;
        }
    }
}

#[derive(Debug)]
enum StreamState {
    Idle,
    /// The stream is currently flushing data.
    Flush,
}

struct Stream {
    kind: StreamKind,
    state: StreamState,
}

enum StreamKind {
    Plain(TcpStream),
    #[cfg(feature = "tls")]
    Tls(TlsStream<TcpStream>),
}

enum StreamKindProjected<'a> {
    Plain(Pin<&'a mut TcpStream>),
    #[cfg(feature = "tls")]
    Tls(Pin<&'a mut TlsStream<TcpStream>>),
}

impl Stream {
    fn peer_addr(&self) -> io::Result<SocketAddr> {
        match &self.kind {
            StreamKind::Plain(stream) => stream.peer_addr(),
            #[cfg(feature = "tls")]
            StreamKind::Tls(stream) => stream.get_ref().0.peer_addr(),
        }
    }

    #[inline]
    fn project(self: Pin<&mut Self>) -> (StreamKindProjected<'_>, &mut StreamState) {
        unsafe {
            let this = self.get_unchecked_mut();

            let kind = match &mut this.kind {
                StreamKind::Plain(stream) => StreamKindProjected::Plain(Pin::new_unchecked(stream)),
                #[cfg(feature = "tls")]
                StreamKind::Tls(stream) => StreamKindProjected::Tls(Pin::new_unchecked(stream)),
            };

            (kind, &mut this.state)
        }
    }

    #[inline]
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut Buf,
    ) -> Poll<io::Result<()>> {
        fn handle_read(
            cx: &mut Context<'_>,
            stream: Pin<&mut dyn AsyncRead>,
            buf: &mut Buf,
        ) -> Poll<io::Result<()>> {
            let Some(bytes) = buf.write_buf() else {
                return Poll::Ready(Err(io::Error::other("receive buffer capacity exceeded")));
            };

            tracing::trace!(bytes = bytes.len(), "Polling read");

            let mut b = ReadBuf::new(bytes);
            let result = ready!(stream.poll_read(cx, &mut b));

            if let Err(e) = result {
                return Poll::Ready(Err(e));
            }

            let filled = b.filled().len();

            if filled == 0 {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "connection closed",
                )));
            }

            buf.advance(filled);
            Poll::Ready(Ok(()))
        }

        let (kind, _) = self.project();

        let stream: Pin<&mut dyn AsyncRead> = match kind {
            StreamKindProjected::Plain(stream) => stream,
            #[cfg(feature = "tls")]
            StreamKindProjected::Tls(stream) => stream,
        };

        handle_read(cx, stream, buf)
    }

    #[inline]
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut Buf,
    ) -> Poll<io::Result<()>> {
        fn handle_write(
            cx: &mut Context<'_>,
            mut stream: Pin<&mut dyn AsyncWrite>,
            buf: &mut Buf,
            state: &mut StreamState,
        ) -> Poll<io::Result<()>> {
            while buf.has_remaining() || matches!(state, StreamState::Flush) {
                tracing::trace!(remaining = buf.remaining(), ?state, "Polling write");

                match state {
                    StreamState::Idle => {
                        match ready!(stream.as_mut().poll_write(cx, buf.read_buf())) {
                            Ok(n) => {
                                tracing::trace!(written = n, "Written to stream");
                                buf.advance_read(n);
                                *state = StreamState::Flush;
                            }
                            Err(e) => return Poll::Ready(Err(e)),
                        }
                    }
                    StreamState::Flush => {
                        if let Err(e) = ready!(stream.as_mut().poll_flush(cx)) {
                            return Poll::Ready(Err(e));
                        }

                        *state = StreamState::Idle;
                    }
                }
            }

            Poll::Ready(Ok(()))
        }

        let (kind, state) = self.project();

        let stream: Pin<&mut dyn AsyncWrite> = match kind {
            StreamKindProjected::Plain(stream) => stream,
            #[cfg(feature = "tls")]
            StreamKindProjected::Tls(stream) => stream,
        };

        handle_write(cx, stream, buf, state)
    }
}

/// A client connection.
pub struct Client {
    stream: Stream,
}

impl Client {
    fn project(self: Pin<&mut Self>) -> Pin<&mut Stream> {
        unsafe { self.map_unchecked_mut(|s| &mut s.stream) }
    }

    /// Construct a client from a TCP stream.
    #[inline]
    pub(crate) fn plain(stream: TcpStream) -> Self {
        Self {
            stream: Stream {
                kind: StreamKind::Plain(stream),
                state: StreamState::Idle,
            },
        }
    }

    /// Construct a client from a TLS stream.
    #[inline]
    #[cfg(feature = "tls")]
    pub(crate) fn tls(stream: TlsStream<TcpStream>) -> Self {
        Self {
            stream: Stream {
                kind: StreamKind::Tls(stream),
                state: StreamState::Idle,
            },
        }
    }

    /// Open a plain TCP connection to the given address.
    #[inline]
    pub async fn connect(addr: impl ToSocketAddrs) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self::plain(stream))
    }

    /// Get the socket address of the client.
    #[inline]
    pub fn addr(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    /// Read data from server into buffer.
    #[inline]
    pub fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut Buf,
    ) -> Poll<io::Result<()>> {
        self.project().poll_read(cx, buf)
    }

    /// Poll the client for write readiness.
    #[inline]
    pub fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut Buf,
    ) -> Poll<io::Result<()>> {
        self.project().poll_write(cx, buf)
    }
}
