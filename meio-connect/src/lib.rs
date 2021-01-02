#![recursion_limit = "512"]

// TODO: Remove this shit!
pub use headers;
pub use hyper;
pub mod client;
pub mod server;
mod talker;

pub use talker::{TermReason, WsIncoming};
