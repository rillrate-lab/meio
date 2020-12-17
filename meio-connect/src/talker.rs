use crate::{ProtocolCodec, ProtocolData, WsIncoming};
use anyhow::Error;
use futures::channel::mpsc;
use futures::stream::Fuse;
use futures::{select, Sink, SinkExt, Stream, StreamExt};
use meio::prelude::{ActionHandler, Actor, Address, StopReceiver};
use serde::ser::StdError;
use std::fmt::Debug;
use tungstenite::{error::Error as TungError, Message as TungMessage};
use warp::{ws::Message as WarpMessage, Error as WarpError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermReason {
    Interrupted,
    Closed,
}

impl TermReason {
    pub fn is_interrupted(&self) -> bool {
        *self == Self::Interrupted
    }
}

pub trait WsError: Debug + StdError + Sync + Send + 'static {}

impl WsError for WarpError {}

impl WsError for TungError {}

pub trait WsMessage: Debug + Sized {
    fn binary(data: Vec<u8>) -> Self;
    fn is_ping(&self) -> bool;
    fn is_pong(&self) -> bool;
    fn is_text(&self) -> bool;
    fn is_binary(&self) -> bool;
    fn is_close(&self) -> bool;
    fn into_bytes(self) -> Vec<u8>;
}

impl WsMessage for WarpMessage {
    fn binary(data: Vec<u8>) -> Self {
        WarpMessage::binary(data)
    }
    fn is_ping(&self) -> bool {
        WarpMessage::is_ping(self)
    }
    fn is_pong(&self) -> bool {
        WarpMessage::is_pong(self)
    }
    fn is_text(&self) -> bool {
        WarpMessage::is_text(self)
    }
    fn is_binary(&self) -> bool {
        WarpMessage::is_binary(self)
    }
    fn is_close(&self) -> bool {
        WarpMessage::is_close(self)
    }
    fn into_bytes(self) -> Vec<u8> {
        WarpMessage::into_bytes(self)
    }
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
        address: Address<T::Actor>,
        connection: T::WebSocket,
        rx: mpsc::UnboundedReceiver<T::Outgoing>,
        stop: StopReceiver,
    ) -> Self {
        Self {
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
                    log::trace!("Incoming request: {:?}", request);
                    let msg = request.transpose()?;
                    if let Some(msg) = msg {
                        if msg.is_text() || msg.is_binary() {
                            let decoded = T::Codec::decode(&msg.into_bytes())?;
                            log::trace!("Received: {:?}", decoded);
                            let msg = WsIncoming(decoded);
                            self.address.act(msg).await?;
                        } else if msg.is_ping() || msg.is_pong() {
                            // Ignore Ping and Pong messages
                        } else if msg.is_close() {
                            log::trace!("Close message received. Draining the channel...");
                            // Start draining that will close the connection.
                            // No more messages expected. The receiver can be safely closed.
                            self.rx.close();
                        } else {
                            log::warn!("Unhandled WebSocket message: {:?}", msg);
                        }
                    } else {
                        // The connection was closed, further interaction doesn't make sense
                        log::trace!("Connection phisically closed.");
                        self.connection_drained = true;
                        if self.is_done() {
                            break;
                        }
                    }
                }
                response = self.rx.next() => {
                    log::trace!("Sending WS response: {:?}", response);
                    if let Some(msg) = response {
                        let encoded = T::Codec::encode(&msg)?;
                        let message = T::Message::binary(encoded);
                        self.connection.send(message).await?;
                    } else {
                        log::trace!("Channel with outgoing data closed. Terminating a session with the client.");
                        log::trace!("Sending close notification to the client.");
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
