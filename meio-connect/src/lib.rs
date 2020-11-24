#![recursion_limit = "512"]

pub mod client;
pub mod filters;
pub mod server;
mod talker;

use anyhow::Error;
use meio::Action;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;

pub trait ProtocolData: Serialize + DeserializeOwned + Debug + Send + 'static {}

impl<T> ProtocolData for T where T: Serialize + DeserializeOwned + Debug + Send + 'static {}

pub trait Protocol: Send + 'static {
    type ToServer: ProtocolData;
    type ToClient: ProtocolData;
    type Codec: ProtocolCodec;
}

pub trait ProtocolCodec: Send {
    fn decode<T: ProtocolData>(data: &[u8]) -> Result<T, Error>;
    fn encode<T: ProtocolData>(value: &T) -> Result<Vec<u8>, Error>;
}

/// Incoming message of the choosen `Protocol`.
#[derive(Debug)]
pub struct WsIncoming<T: ProtocolData>(pub T);

impl<T: ProtocolData> Action for WsIncoming<T> {}