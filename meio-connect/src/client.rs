use crate::{
    talker::{Talker, TalkerCompatible},
    Protocol, ProtocolData, WsIncoming,
};
use anyhow::Error;
use async_trait::async_trait;
use async_tungstenite::{
    tokio::{connect_async, TokioAdapter},
    WebSocketStream,
};
use futures::channel::mpsc;
use futures::{select, FutureExt, StreamExt};
use meio::{
    lifecycle, ActionHandler, Actor, Address, Interaction, InteractionHandler, LiteTask,
    ShutdownReceiver,
};
use std::marker::PhantomData;
use std::time::{Duration, Instant};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::watch;
use tokio::time::delay_for;

#[derive(Debug)]
pub struct WsSender<T: ProtocolData> {
    tx: mpsc::UnboundedSender<T>,
}

impl<T: ProtocolData> WsSender<T> {
    pub fn send(&mut self, msg: T) {
        if let Err(err) = self.tx.unbounded_send(msg) {
            log::error!("Can't send a message to ws outgoing client part: {}", err);
        }
    }
}

#[derive(Debug)]
pub enum WsClientStatus<P: Protocol> {
    Connected { sender: WsSender<P::ToServer> },
    Failed { reason: WsFailReason },
}

#[derive(Error, Debug, Clone)]
pub enum WsFailReason {
    #[error("closed by server")]
    ClosedByServer,
    #[error("connection failed")]
    ConnectionFailed,
    #[error("server not available")]
    ServerNotAvailable,
}

impl<P: Protocol> Interaction for WsClientStatus<P> {
    type Output = ();

    fn is_high_priority(&self) -> bool {
        true
    }
}

pub struct WsClient<P, A>
where
    A: Actor,
{
    url: String,
    repeat_interval: Option<Duration>,
    address: Address<A>,
    _protocol: PhantomData<P>,
}

impl<P, A> WsClient<P, A>
where
    P: Protocol,
    A: Actor + InteractionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    pub fn new(url: String, repeat_interval: Option<Duration>, address: Address<A>) -> Self {
        Self {
            url: url.clone(),
            repeat_interval,
            address,
            _protocol: PhantomData,
        }
    }
}

impl<P, A> TalkerCompatible for WsClient<P, A>
where
    P: Protocol,
    A: Actor + InteractionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    type WebSocket = WebSocketStream<TokioAdapter<TcpStream>>;
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
    A: Actor + InteractionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    fn name(&self) -> String {
        format!("WsClient({})", self.url)
    }

    async fn routine(mut self, signal: ShutdownReceiver) -> Result<(), Error> {
        self.connection_routine(signal.into()).await
    }
}

impl<P, A> WsClient<P, A>
where
    P: Protocol,
    A: Actor + InteractionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    async fn connection_routine(
        &mut self,
        status_rx: watch::Receiver<lifecycle::Status>,
    ) -> Result<(), Error> {
        while *status_rx.borrow() == lifecycle::Status::Alive {
            log::trace!("Ws client conencting to: {}", self.url);
            let res = connect_async(&self.url).await;
            let mut last_success = Instant::now();
            let fail_reason;
            let original_err: Error;
            match res {
                Ok((wss, _resp)) => {
                    log::debug!("Client connected successfully to: {}", self.url);
                    last_success = Instant::now();
                    let (tx, rx) = mpsc::unbounded();
                    let sender = WsSender { tx };
                    self.address
                        .interact(WsClientStatus::<P>::Connected { sender })
                        .await?;
                    // Interruptable by status_rx
                    let mut talker =
                        Talker::<Self>::new(self.address.clone(), wss, rx, status_rx.clone());
                    let res = talker.routine().await;
                    match res {
                        Ok(reason) => {
                            if reason.is_interrupted() {
                                log::info!("Interrupted by a user");
                                return Ok(());
                            } else {
                                log::error!("Server closed a connection");
                                fail_reason = WsFailReason::ClosedByServer;
                                original_err = WsFailReason::ClosedByServer.into();
                            }
                        }
                        Err(err) => {
                            log::error!("Ws connecion to {} failed: {}", self.url, err);
                            fail_reason = WsFailReason::ConnectionFailed;
                            original_err = err.into();
                        }
                    }
                }
                Err(err) => {
                    log::error!("Can't connect to {}: {}", self.url, err);
                    fail_reason = WsFailReason::ServerNotAvailable;
                    original_err = err.into();
                }
            }
            self.address
                .interact(WsClientStatus::<P>::Failed {
                    reason: fail_reason.clone(),
                })
                .await?;
            if let Some(dur) = self.repeat_interval.clone() {
                let elapsed = last_success.elapsed();
                if elapsed < dur {
                    let remained = dur - elapsed;
                    let mut delay = delay_for(remained).fuse();
                    let mut signal_rx = status_rx.clone().fuse();
                    // TODO: Refactor this loop...
                    loop {
                        select! {
                            _timeout = delay => {
                                break;
                            }
                            status = signal_rx.next() => {
                                match status {
                                    Some(lifecycle::Status::Stop) | None => {
                                        log::debug!("Reconnection terminated by user for: {}", self.url);
                                        return Ok(());
                                    }
                                    Some(lifecycle::Status::Alive) => {
                                        // Continue working
                                    }
                                }
                            }
                        }
                    }
                }
                log::debug!("Next attempt to connect to: {}", self.url);
            } else {
                // No reconnection required by user
                return Err(original_err);
            }
        }
        Ok(())
    }
}
