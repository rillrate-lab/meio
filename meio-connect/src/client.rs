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
use tokio_tungstenite::{connect_async, WebSocketStream};

#[derive(Debug)]
pub struct WsSender<T: ProtocolData> {
    tx: mpsc::UnboundedSender<T>,
}

impl<T: ProtocolData> WsSender<T> {
    pub fn send(&self, msg: T) {
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

impl<P: Protocol> InstantAction for WsClientStatus<P> {}

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
    A: Actor + InstantActionHandler<WsClientStatus<P>> + ActionHandler<WsIncoming<P::ToClient>>,
{
    pub fn new(url: String, repeat_interval: Option<Duration>, address: Address<A>) -> Self {
        Self {
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
    type WebSocket = WebSocketStream<TcpStream>;
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

    fn name(&self) -> String {
        format!("WsClient({})", self.url)
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
                        .instant(WsClientStatus::<P>::Connected { sender })?;
                    // Interruptable by a stop
                    let mut talker =
                        Talker::<Self>::new(self.address.clone(), wss, rx, stop.clone());
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
                            original_err = err;
                        }
                    }
                }
                Err(err) => {
                    log::error!("Can't connect to {}: {}", self.url, err);
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
                log::debug!("Next attempt to connect to: {}", self.url);
            } else {
                // No reconnection required by user
                return Err(original_err);
            }
        }
        Ok(())
    }
}
