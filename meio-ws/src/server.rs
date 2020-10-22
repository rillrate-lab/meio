use crate::{
    talker::{Talker, TalkerCompatible},
    Protocol, WsIncoming,
};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::mpsc;
use meio::{ActionHandler, Actor, Address, LiteTask, ShutdownReceiver};
use std::net::SocketAddr;
use warp::ws::WebSocket;

struct WsInfo<P: Protocol> {
    addr: SocketAddr,
    connection: WebSocket,
    rx: mpsc::UnboundedReceiver<P::ToClient>,
}

/// This struct wraps a `WebSocket` connection
/// and produces processors for every incoming connection.
pub struct WsHandler<P: Protocol> {
    info: Option<WsInfo<P>>,
    tx: mpsc::UnboundedSender<P::ToClient>,
}

impl<P: Protocol> WsHandler<P> {
    pub fn new(addr: SocketAddr, websocket: WebSocket) -> Self {
        let (tx, rx) = mpsc::unbounded();
        let info = WsInfo {
            addr,
            connection: websocket,
            rx,
        };
        Self {
            info: Some(info),
            tx,
        }
    }

    pub fn worker<A>(&mut self, address: Address<A>) -> WsProcessor<P, A>
    where
        A: Actor + ActionHandler<WsIncoming<P::ToServer>>,
    {
        let info = self.info.take().expect("already started");
        WsProcessor { info, address }
    }

    pub fn send(&mut self, msg: P::ToClient) {
        if let Err(err) = self.tx.unbounded_send(msg) {
            log::error!("Can't send outgoing WS message: {}", err);
        }
    }
}

pub struct WsProcessor<P: Protocol, A: Actor> {
    info: WsInfo<P>,
    address: Address<A>,
}

impl<P, A> TalkerCompatible for WsProcessor<P, A>
where
    P: Protocol,
    A: Actor + ActionHandler<WsIncoming<P::ToServer>>,
{
    type WebSocket = WebSocket;
    type Message = warp::ws::Message;
    type Error = warp::Error;
    type Actor = A;
    type Codec = P::Codec;
    type Incoming = P::ToServer;
    type Outgoing = P::ToClient;
}

#[async_trait]
impl<P, A> LiteTask for WsProcessor<P, A>
where
    P: Protocol,
    A: Actor + ActionHandler<WsIncoming<P::ToServer>>,
{
    fn name(&self) -> String {
        format!("WsProcessor({})", self.info.addr)
    }

    async fn routine(self, signal: ShutdownReceiver) -> Result<(), Error> {
        let mut talker = Talker::<Self>::new(
            self.address,
            self.info.connection,
            self.info.rx,
            signal.into(),
        );
        talker.routine().await.map(drop)
    }
}
