use crate::{
    filters::{Section, WsRequest},
    talker::{Talker, TalkerCompatible},
    Protocol, WsIncoming,
};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::mpsc;
use meio::prelude::{ActionHandler, Actor, Address, LiteTask, ShutdownReceiver};
use std::net::SocketAddr;
use warp::{ws::WebSocket, Filter, Reply, Server};

pub struct WebServer<F> {
    addr: SocketAddr,
    server: Server<F>,
}

impl<F> WebServer<F>
where
    F: Filter + Clone + Send + Sync + 'static,
    F::Extract: Reply,
{
    pub fn new(addr: SocketAddr, routes: F) -> Self {
        let server = warp::serve(routes);
        Self { addr, server }
    }
}

#[async_trait]
impl<F> LiteTask for WebServer<F>
where
    F: Filter + Clone + Send + Sync + 'static,
    F::Extract: Reply,
{
    fn name(&self) -> String {
        format!("WebServer({})", self.addr)
    }

    async fn routine(self, signal: ShutdownReceiver) -> Result<(), Error> {
        self.server
            .try_bind_with_graceful_shutdown(self.addr, signal.just_done())?
            .1
            .await;
        Ok(())
    }
}

struct WsInfo<P: Protocol> {
    addr: SocketAddr,
    connection: WebSocket,
    rx: mpsc::UnboundedReceiver<P::ToClient>,
}

/// This struct wraps a `WebSocket` connection
/// and produces processors for every incoming connection.
pub struct WsHandler<P: Protocol> {
    addr: SocketAddr,
    info: Option<WsInfo<P>>,
    tx: mpsc::UnboundedSender<P::ToClient>,
}

impl<P: Protocol, T: Section> From<WsRequest<T>> for WsHandler<P> {
    fn from(request: WsRequest<T>) -> Self {
        Self::new(request.addr, request.websocket)
    }
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
            addr,
            info: Some(info),
            tx,
        }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
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
