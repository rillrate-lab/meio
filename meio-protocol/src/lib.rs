//! The generic protocol types.

#![warn(missing_docs)]

use anyhow::Error;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::Debug;

/// Generic procotol type.
///
/// The trait gives the following guarantees to protocol data type:
///
/// - Data can be serialized and deserialized;
/// - Instance can be sent to another thread (to be sent from a separate thread);
/// - Be printable for debugging purposes;
/// - Type has `'static` lifetime to be compatible with actor's handlers.
///
pub trait ProtocolData: Serialize + DeserializeOwned + Debug + Send + 'static {}

impl<T> ProtocolData for T where T: Serialize + DeserializeOwned + Debug + Send + 'static {}

/// The set of types for provide interaction capabilities for parties.
pub trait Protocol: Send + 'static {
    /// The message type sent to a server (from a client).
    type ToServer: ProtocolData;
    /// The message type sent to a client (from a server).
    type ToClient: ProtocolData;
    /// The codec (serialization format) used to pack and unpack messages.
    type Codec: ProtocolCodec;
}

/// The serialization format for a `Protocol`.
pub trait ProtocolCodec: Send {
    // TODO: Consider adding `self` reference to implement stateful (smart) formats.
    /// Decodes binary data to a type.
    fn decode<T: ProtocolData>(data: &[u8]) -> Result<T, Error>;
    /// Encodes value to a binary data.
    fn encode<T: ProtocolData>(value: &T) -> Result<Vec<u8>, Error>;
}
