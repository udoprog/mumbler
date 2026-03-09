#![allow(clippy::new_without_default)]

pub mod server;

mod client;
pub use client::{Buf, Client, Scratch};

mod peer;
pub use self::peer::Peer;

pub mod api;

/// Default remote port.
pub const DEFAULT_PORT: u16 = 44114;

/// Default remote TLS port.
pub const DEFAULT_TLS_PORT: u16 = 44115;
