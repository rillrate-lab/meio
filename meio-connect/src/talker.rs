use anyhow::Error;
use futures::channel::mpsc;
use futures::stream::Fuse;
use futures::{select, Sink, SinkExt, Stream, StreamExt};
use meio::prelude::{Action, ActionHandler, Actor, Address, StopReceiver};
use meio_protocol::{ProtocolCodec, ProtocolData};
use serde::ser::StdError;
use std::fmt::Debug;
use tungstenite::{error::Error as TungError, Message as TungMessage};

/// Incoming message of the choosen `Protocol`.
#[derive(Debug)]
pub struct WsIncoming<T: ProtocolData>(pub T);

impl<T: ProtocolData> Action for WsIncoming<T> {}

/// The reason of connection termination.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermReason {
    /// The connection was interrupted.
    Interrupted,
    /// The connection was closed (properly).
    Closed,
}

impl TermReason {
    /// Was the handler interrupted?
    pub fn is_interrupted(&self) -> bool {
        *self == Self::Interrupted
    }
}

/// The abstract error for WebSockets.
pub trait WsError: Debug + StdError + Sync + Send + 'static {}

impl WsError for TungError {}

/// The abstract `WebSocket` message.
///
/// The trait provides generic metods to `Talker`s to work with messages.
pub trait WsMessage: Debug + Sized {
    /// Creates a message instance from a binary data.
    fn binary(data: Vec<u8>) -> Self;
    /// Is it the ping message?
    fn is_ping(&self) -> bool;
    /// Is it the ping message?
    fn is_pong(&self) -> bool;
    /// Is it the text message?
    fn is_text(&self) -> bool;
    /// Is it the binary message?
    fn is_binary(&self) -> bool;
    /// Is it the closing signal message?
    fn is_close(&self) -> bool;
    /// Convert the message into bytes.
    fn into_bytes(self) -> Vec<u8>;
}

impl WsMessage for TungMessage {
    fn binary(data: Vec<u8>) -> Self {
        TungMessage::binary(data)
    }
    fn is_ping(&self) -> bool {
        TungMessage::is_ping(self)
    }
    fn is_pong(&self) -> bool {
        TungMessage::is_pong(self)
    }
    fn is_text(&self) -> bool {
        TungMessage::is_text(self)
    }
    fn is_binary(&self) -> bool {
        TungMessage::is_binary(self)
    }
    fn is_close(&self) -> bool {
        TungMessage::is_close(self)
    }
    fn into_bytes(self) -> Vec<u8> {
        TungMessage::into_data(self)
    }
}

pub trait TalkerCompatible {
    type WebSocket: Stream<Item = Result<Self::Message, Self::Error>>
        + Sink<Self::Message, Error = Self::Error>
        + Unpin;
    type Message: WsMessage;
    type Error: WsError;
    type Actor: Actor + ActionHandler<WsIncoming<Self::Incoming>>;
    type Codec: ProtocolCodec;
    type Incoming: ProtocolData;
    type Outgoing: ProtocolData;
}

pub struct Talker<T: TalkerCompatible> {
    log_target: String,
    address: Address<T::Actor>,
    connection: Fuse<T::WebSocket>,
    rx: mpsc::UnboundedReceiver<T::Outgoing>,
    stop: StopReceiver,
    rx_drained: bool,
    connection_drained: bool,
    interrupted: bool,
}

impl<T: TalkerCompatible> Talker<T> {
    pub fn new(
        base_log_target: &str,
        address: Address<T::Actor>,
        connection: T::WebSocket,
        rx: mpsc::UnboundedReceiver<T::Outgoing>,
        stop: StopReceiver,
    ) -> Self {
        let log_target = format!("{}::Talker", base_log_target);
        Self {
            log_target,
            address,
            connection: connection.fuse(),
            rx,
            stop,
            rx_drained: false,
            connection_drained: false,
            interrupted: false,
        }
    }

    fn is_done(&self) -> bool {
        self.rx_drained && self.connection_drained
    }

    pub async fn routine(&mut self) -> Result<TermReason, Error> {
        let mut done = self.stop.clone().into_future();
        loop {
            select! {
                _ = done => {
                    self.interrupted = true;
                    // Just close the channel and wait when it will be drained
                    self.rx.close();
                }
                request = self.connection.next() => {
                    let msg = request.transpose()?;
                    if let Some(msg) = msg {
                        if msg.is_text() || msg.is_binary() {
                            let decoded = T::Codec::decode(&msg.into_bytes())?;
                            log::trace!(target: &self.log_target, "MEIO-WS-RECV: {:?}", decoded);
                            let msg = WsIncoming(decoded);
                            self.address.act(msg)?;
                        } else if msg.is_ping() || msg.is_pong() {
                            // Ignore Ping and Pong messages
                        } else if msg.is_close() {
                            log::trace!(target: &self.log_target, "Close message received. Draining the channel...");
                            // Start draining that will close the connection.
                            // No more messages expected. The receiver can be safely closed.
                            self.rx.close();
                        } else {
                            log::warn!(target: &self.log_target, "Unhandled WebSocket message: {:?}", msg);
                        }
                    } else {
                        // The connection was closed, further interaction doesn't make sense
                        log::trace!(target: &self.log_target, "Connection phisically closed.");
                        self.connection_drained = true;
                        if self.is_done() {
                            break;
                        }
                    }
                }
                response = self.rx.next() => {
                    if let Some(msg) = response {
                        log::trace!(target: &self.log_target, "MEIO-WS-SEND: {:?}", msg);
                        let encoded = T::Codec::encode(&msg)?;
                        let message = T::Message::binary(encoded);
                        self.connection.send(message).await?;
                    } else {
                        log::trace!(target: &self.log_target, "Channel with outgoing data closed. Terminating a session with the client.");
                        log::trace!(target: &self.log_target, "Sending close notification to the client.");
                        self.connection.close().await?;
                        self.rx_drained = true;
                        if self.is_done() {
                            break;
                        }
                    }
                }
            }
        }
        if self.interrupted {
            Ok(TermReason::Interrupted)
        } else {
            Ok(TermReason::Closed)
        }
    }
}
