use anyhow::Error;
use async_trait::async_trait;
use meio::{
    Actor, Address, Interaction, InteractionHandler, InteractionPerformer, LiteTask,
    ShutdownReceiver,
};
use std::convert::Infallible;
use std::net::SocketAddr;
use warp::{
    filters::BoxedFilter,
    http::StatusCode,
    path::Tail,
    ws::{WebSocket, Ws},
    Filter, Reply, Server,
};

pub trait ToReply {
    fn to_reply(self) -> Result<Box<dyn Reply>, Infallible>;
}

impl ToReply for Result<Box<dyn Reply>, Error> {
    fn to_reply(self) -> Result<Box<dyn Reply>, Infallible> {
        match self {
            Ok(reply) => Ok(reply),
            Err(err) => {
                log::warn!("Request failed with: {}", err);
                Ok(Box::new(StatusCode::BAD_REQUEST))
            }
        }
    }
}

#[derive(Debug)]
pub struct IndexRequest;

impl Interaction for IndexRequest {
    type Output = Box<dyn Reply>;
}

impl IndexRequest {
    pub fn filter<A>(address: Address<A>) -> BoxedFilter<(impl Reply,)>
    where
        A: Actor + InteractionHandler<IndexRequest>,
    {
        warp::path::end()
            .and_then(move || index_handler(address.clone()))
            .boxed()
    }
}

async fn index_handler<A>(mut address: Address<A>) -> Result<Box<dyn Reply>, Infallible>
where
    A: Actor + InteractionHandler<IndexRequest>,
{
    let msg = IndexRequest;
    address.interact(msg).await.to_reply()
}

#[derive(Debug)]
pub struct TailRequest {
    pub tail: Tail,
}

impl Interaction for TailRequest {
    type Output = Box<dyn Reply>;
}

impl TailRequest {
    pub fn filter<A>(address: Address<A>) -> BoxedFilter<(impl Reply,)>
    where
        A: Actor + InteractionHandler<TailRequest>,
    {
        warp::path::tail()
            .and_then(move |tail| tail_handler(tail, address.clone()))
            .boxed()
    }
}

async fn tail_handler<A>(tail: Tail, mut address: Address<A>) -> Result<Box<dyn Reply>, Infallible>
where
    A: Actor + InteractionHandler<TailRequest>,
{
    let msg = TailRequest { tail };
    address.interact(msg).await.to_reply()
}

/// This struct received by an `Actor` when a client connected.
#[derive(Debug)]
pub struct WsRequest {
    pub addr: SocketAddr,
    pub websocket: WebSocket,
}

impl Interaction for WsRequest {
    type Output = ();
}

impl WsRequest {
    pub fn filter<A>(
        section: &'static str,
        method: &'static str,
        actor: Address<A>,
    ) -> BoxedFilter<(impl Reply,)>
    where
        A: Actor + InteractionHandler<WsRequest>,
    {
        warp::path(section)
            .and(warp::path(method))
            .and(warp::ws())
            .and(warp::addr::remote())
            .map(move |ws, addr| ws_handler(ws, addr, actor.clone()))
            .boxed()
    }
}

fn ws_handler<A>(ws: Ws, addr: Option<SocketAddr>, actor: Address<A>) -> Box<dyn Reply>
where
    A: Actor + InteractionHandler<WsRequest>,
{
    match addr {
        Some(address) => {
            let upgrade =
                ws.on_upgrade(move |websocket| ws_entrypoint(address, websocket, actor.clone()));
            Box::new(upgrade)
        }
        None => {
            log::error!("Can't get peer address: {:?}", ws);
            Box::new("Peer address not provided")
        }
    }
}

async fn ws_entrypoint<A>(addr: SocketAddr, websocket: WebSocket, mut actor: Address<A>)
where
    A: Actor + InteractionHandler<WsRequest>,
{
    let msg = WsRequest { addr, websocket };
    if let Err(err) = actor.interact(msg).await {
        log::error!("Can't send notification about ws connection: {}", err);
    }
}

// TODO: Move this struct to another module
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
            .bind_with_graceful_shutdown(self.addr, signal.just_done())
            .1
            .await;
        Ok(())
    }
}
