use core::net::SocketAddr;
use core::task::{Context, Poll};

use std::io;

use anyhow::{Context as _, Result};
use musli_core::Encode;
use musli_core::mode::Binary;
use tokio::net::{TcpStream, ToSocketAddrs};

const CAP: usize = 4096;

/// A scratch buffer for temporary data, used for serialization.
pub struct Scratch {
    data: Vec<u8>,
}

impl Scratch {
    /// Create a new scratch buffer with the given capacity.
    #[inline]
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(CAP),
        }
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

        self.advance(N);
        Some(buf)
    }

    /// Read a slice of `len` bytes from the buffer, advancing the read position.
    #[inline]
    pub fn read_slice(&mut self, len: usize) -> Option<&[u8]> {
        if self.read + len > self.write {
            return None;
        }

        let range = self.read..self.read + len;
        self.advance(len);
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
        if self.write + data.len() > self.data.len() {
            self.data.resize(self.write + data.len(), 0);
        }

        let Some(bytes) = self.data.get_mut(self.write..self.write + data.len()) else {
            return;
        };

        bytes.copy_from_slice(data);
        self.write += data.len();
    }

    /// Get the unread portion of the buffer.
    #[inline]
    pub fn write_buf(&mut self) -> &mut [u8] {
        if self.write + CAP > self.data.len() {
            self.data.resize(self.write + CAP, 0);
        }

        self.data
            .get_mut(self.write..self.write + CAP)
            .unwrap_or_default()
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

    /// Advance the read position by `n` bytes.
    #[inline]
    pub fn advance(&mut self, n: usize) {
        self.read += n;

        if self.read >= self.write {
            self.read = 0;
            self.write = 0;
        }
    }
}

/// A client connection.
pub struct Client {
    stream: TcpStream,
}

impl Client {
    /// Create a client from a TCP stream.
    #[inline]
    pub(crate) fn from_stream(stream: TcpStream) -> Self {
        Self { stream }
    }

    /// Connect a client to the given address.
    #[inline]
    pub async fn connect<A>(addr: A) -> Result<Self>
    where
        A: ToSocketAddrs,
    {
        let stream = TcpStream::connect(addr)
            .await
            .context("connecting to server")?;

        Ok(Self::from_stream(stream))
    }

    /// Get the socket address of the client.
    #[inline]
    pub fn addr(&self) -> io::Result<SocketAddr> {
        self.stream.peer_addr()
    }

    /// Wait until the client is readable.
    #[inline]
    pub async fn readable(&self) -> io::Result<()> {
        self.stream.readable().await
    }

    /// Read data from server into buffer.
    #[inline]
    pub fn try_read(&mut self, buf: &mut Buf) -> io::Result<()> {
        loop {
            match self.stream.try_read(buf.write_buf()) {
                Ok(0) => return Err(io::Error::from(io::ErrorKind::UnexpectedEof)),
                Ok(n) => {
                    buf.write += n;

                    if buf.remaining() > CAP {
                        return Ok(());
                    }
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(e),
            }
        }
    }

    /// Wait until the client is writable.
    #[inline]
    pub async fn writable(&self) -> io::Result<()> {
        self.stream.writable().await
    }

    /// Write data from the buffer.
    #[inline]
    pub fn try_write(&mut self, buf: &mut Buf) -> io::Result<()> {
        while buf.has_remaining() {
            match self.stream.try_write(buf.read_buf()) {
                Ok(n) => {
                    buf.advance(n);
                }
                Err(e) if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                Err(e) => return Err(e),
            }
        }

        Ok(())
    }

    /// Poll the client for write readiness.
    #[inline]
    pub fn poll_write_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream.poll_write_ready(cx)
    }

    /// Poll the client for read readiness.
    #[inline]
    pub fn poll_read_ready(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.stream.poll_read_ready(cx)
    }
}
