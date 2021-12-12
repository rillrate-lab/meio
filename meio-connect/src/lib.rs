//! The library to establish connections using WebSockets.

#![recursion_limit = "512"]
#![warn(missing_docs)]

pub use headers;
pub use hyper;
pub use serde_qs;
pub mod client;
pub mod server;
mod talker;

pub use talker::{TermReason, WsIncoming};
