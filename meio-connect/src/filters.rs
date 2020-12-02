use anyhow::Error;
use meio::prelude::{Actor, Address, Interaction, InteractionHandler};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::str::FromStr;
use warp::{
    filters::BoxedFilter,
    http::StatusCode,
    path::Tail,
    ws::{WebSocket, Ws},
    Filter, Reply,
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

/// `Section` of the path.
pub trait Section: FromStr + Send + 'static {}

impl<T> Section for T where T: FromStr + Send + 'static {}

/// This struct received by an `Actor` when a client connected.
#[derive(Debug)]
pub struct WsRequest<T> {
    pub addr: SocketAddr,
    pub websocket: WebSocket,
    pub section: T,
}

impl<T: Section> Interaction for WsRequest<T> {
    // TODO: Maybe support `not found` responses (etc.) or use an error for that.
    type Output = ();
}

impl<T: Section> WsRequest<T> {
    pub fn filter<A>(namespace: &'static str, actor: Address<A>) -> BoxedFilter<(impl Reply,)>
    where
        A: Actor + InteractionHandler<WsRequest<T>>,
    {
        warp::path(namespace)
            .and(warp::path::param::<T>())
            .and(warp::ws())
            .and(warp::addr::remote())
            // TODO: Use `ws_handler` directly (don't wrap with a closure) if there is a
            // filter that can add a cloneable value (actor's `Address`) to the request.
            .map(move |section, ws, addr| ws_handler(section, ws, addr, actor.clone()))
            .boxed()
    }
}

fn ws_handler<A, T>(
    section: T,
    ws: Ws,
    addr: Option<SocketAddr>,
    actor: Address<A>,
) -> Box<dyn Reply>
where
    T: Section,
    A: Actor + InteractionHandler<WsRequest<T>>,
{
    match addr {
        Some(address) => {
            let upgrade = ws.on_upgrade(move |websocket| {
                ws_entrypoint(address, websocket, section, actor.clone())
            });
            Box::new(upgrade)
        }
        None => {
            log::error!("Can't get peer address: {:?}", ws);
            Box::new("Peer address not provided")
        }
    }
}

async fn ws_entrypoint<A, T>(
    addr: SocketAddr,
    websocket: WebSocket,
    section: T,
    mut actor: Address<A>,
) where
    T: Section,
    A: Actor + InteractionHandler<WsRequest<T>>,
{
    let msg = WsRequest {
        addr,
        websocket,
        section,
    };
    if let Err(err) = actor.interact(msg).await {
        log::error!("Can't send notification about ws connection: {}", err);
    }
}
