//! The implementation of a WebSocket client.

use crate::talker::{Talker, TalkerCompatible, WsIncoming};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::mpsc;
use meio::prelude::{
    ActionHandler, Actor, Address, InstantAction, InstantActionHandler, LiteTask, StopReceiver,
};
use meio_protocol::{Protocol, ProtocolData};
use std::marker::PhantomData;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::time::sleep;
use tokio_tungstenite::{connect_async, MaybeTlsStream, WebSocketStream};

/// Sender for outgoing messages from a client.
#[derive(Debug, Clone)]
pub struct WsSender<T: ProtocolData> {
    log_target: String,
    tx: mpsc::UnboundedSender<T>,
}

impl<T: ProtocolData> WsSender<T> {
    /// Creates a new instance of the sender.
    pub(crate) fn new(base_log_target: &str, tx: mpsc::UnboundedSender<T>) -> Self {
        let log_target = format!("{}::WsSender", base_log_target);
        Self { log_target, tx }
    }

    /// Send outgoing message to a server.
    pub fn send(&self, msg: T) {
        if let Err(err) = self.tx.unbounded_send(msg) {
            log::error!(target: &self.log_target, "Can't send a message to ws outgoing client part: {}", err);
        }
    }
}

/// The active status of a connection.
#[derive(Debug)]
pub enum WsClientStatus<P: Protocol> {
    /// Connected to a server.
    Connected {
        /// The sender for outgoing messages.
        sender: WsSender<P::ToServer>,
    },
    /// Connection failed. The sender is not valid anymore.
    Failed {
        /// The reason of failing.
        reason: WsFailReason,
    },
}

/// The reason of failing of a WebSocket connection.
#[derive(Error, Debug, Clone)]
pub enum WsFailReason {
    /// Connection was closed by a server.
    #[error("closed by server")]
    ClosedByServer,
    /// Connection failed.
    #[error("connection failed")]
    ConnectionFailed,
    /// Server is not available.
    #[error("server not available")]
    ServerNotAvailable,
}

impl<P: Protocol> InstantAction for WsClientStatus<P> {}

/// The client to connect to a WebSocket server.
///
/// It's a `LiteTask` attached to an `Actor` that recieves incoming messages and
/// a `WsSender` instance and uses it send outgoing messages back to a server.
pub struct WsClient<P, A>
where
    A: Actor,
{
    log_target: String,
    url: String,
    repeat_interval: Option<Duration>,
    address: Address<A>,
    _protocol: PhantomData<P>,
}

impl<P, A> WsClient<P, A>
where
    P: Protocol,
    A: Actor + InstantActionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    /// Created a new client.
    ///
    /// The client connects to `url`, but if connection failed it retries to connect
    /// in `repeat_interval`. When connection espablished it send incoming data to an
    /// actor using the `address`.
    pub fn new(url: String, repeat_interval: Option<Duration>, address: Address<A>) -> Self {
        let log_target = format!("WsClient::{}", url);
        Self {
            log_target,
            url,
            repeat_interval,
            address,
            _protocol: PhantomData,
        }
    }
}

impl<P, A> TalkerCompatible for WsClient<P, A>
where
    P: Protocol,
    A: Actor + InstantActionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    type WebSocket = WebSocketStream<MaybeTlsStream<TcpStream>>;
    type Message = tungstenite::Message;
    type Error = tungstenite::Error;
    type Actor = A;
    type Codec = P::Codec;
    type Incoming = P::ToClient;
    type Outgoing = P::ToServer;
}

#[async_trait]
impl<P, A> LiteTask for WsClient<P, A>
where
    P: Protocol,
    A: Actor + InstantActionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    type Output = ();

    fn log_target(&self) -> &str {
        &self.log_target
    }

    async fn routine(mut self, stop: StopReceiver) -> Result<Self::Output, Error> {
        self.connection_routine(stop).await
    }
}

impl<P, A> WsClient<P, A>
where
    P: Protocol,
    A: Actor + InstantActionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    // TODO: Return fail `TermReason` like server does
    async fn connection_routine(&mut self, mut stop: StopReceiver) -> Result<(), Error> {
        while stop.is_alive() {
            log::trace!(target: &self.log_target, "Ws client conencting to: {}", self.url);
            let res = connect_async(&self.url).await;
            let mut last_success = Instant::now();
            let fail_reason;
            let original_err: Error;
            match res {
                Ok((wss, _resp)) => {
                    log::debug!(target: &self.log_target, "Client connected successfully to: {}", self.url);
                    last_success = Instant::now();
                    let (tx, rx) = mpsc::unbounded();
                    let sender = WsSender::new(&self.log_target, tx);
                    self.address
                        .instant(WsClientStatus::<P>::Connected { sender })?;
                    // Interruptable by a stop
                    let mut talker = Talker::<Self>::new(
                        &self.log_target,
                        self.address.clone(),
                        wss,
                        rx,
                        stop.clone(),
                    );
                    let res = talker.routine().await;
                    match res {
                        Ok(reason) => {
                            if reason.is_interrupted() {
                                log::info!(target: &self.log_target, "Interrupted by a user");
                                return Ok(());
                            } else {
                                log::error!(target: &self.log_target, "Server closed a connection");
                                fail_reason = WsFailReason::ClosedByServer;
                                original_err = WsFailReason::ClosedByServer.into();
                            }
                        }
                        Err(err) => {
                            log::error!(target: &self.log_target, "Ws connecion to {} failed: {}", self.url, err);
                            fail_reason = WsFailReason::ConnectionFailed;
                            original_err = err;
                        }
                    }
                }
                Err(err) => {
                    log::error!(target: &self.log_target, "Can't connect to {}: {}", self.url, err);
                    fail_reason = WsFailReason::ServerNotAvailable;
                    original_err = err.into();
                }
            }
            self.address.instant(WsClientStatus::<P>::Failed {
                reason: fail_reason.clone(),
            })?;
            if let Some(dur) = self.repeat_interval {
                let elapsed = last_success.elapsed();
                if elapsed < dur {
                    let remained = dur - elapsed;
                    stop.or(sleep(remained)).await?;
                }
                log::debug!(target: &self.log_target, "Next attempt to connect to: {}", self.url);
            } else {
                // No reconnection required by user
                return Err(original_err);
            }
        }
        Ok(())
    }
}
