//! Contains routes and handler for WebSocket connections.

use super::route::{DirectPath, Route, RouteError, RouteResult};
use crate::client::WsSender;
use crate::talker::{Talker, TalkerCompatible, TermReason, WsIncoming};
use anyhow::Error;
use async_trait::async_trait;
use futures::channel::mpsc;
use headers::HeaderMapExt;
use hyper::upgrade::Upgraded;
use hyper::{Body, Request, Response, StatusCode};
use meio::prelude::{Action, ActionHandler, Actor, Address, LiteTask, StopReceiver};
use meio_protocol::Protocol;
use std::net::SocketAddr;
use tokio_tungstenite::WebSocketStream;
use tungstenite::protocol::Role;

/// Route checker for a WebSocket connection.
pub trait WsFromRequest: Sized + Send + Sync + 'static {
    /// The value produced from matched and parsed request.
    type Output: Send;
    /// The interaction protocol of established connection.
    /// It's the parameter to have protocols pluggable.
    type Protocol: Protocol;

    /// Tries to match a request, parse and extract it.
    ///
    /// If the request didn't match: returns `None`. If the request matched returns it.
    /// If parsing and or extracting failed `Some(Err(_))` returned.
    fn from_request(&self, request: &Request<Body>) -> Option<Result<Self::Output, Error>>;
}

impl<T> WsFromRequest for T
where
    T: DirectPath,
    T::Parameter: Protocol,
{
    type Output = <T as DirectPath>::Output;
    type Protocol = T::Parameter;

    fn from_request(&self, request: &Request<Body>) -> Option<Result<Self::Output, Error>> {
        let uri = request.uri();
        let path = uri.path();
        if Self::paths().iter().any(|p| p == &path) {
            let query = uri.query().unwrap_or("");
            let output =
                serde_qs::from_str(query).map_err(|err| RouteError::new(path, query, err).into());
            Some(output)
        } else {
            None
        }
    }
}

/// Incoming WebSocket connection request.
pub struct WsReq<T: WsFromRequest> {
    /// The request's meta data.
    pub request: T::Output,
    /// The handler for the request.
    ///
    /// Every request has its own handler, because WebSocket sessions live long.
    pub stream: WsHandler<T::Protocol>,
}

impl<T: WsFromRequest> Action for WsReq<T> {}

/// The route for a web socket connection.
pub struct WsRoute<E, A>
where
    A: Actor,
{
    extractor: E,
    address: Address<A>,
}

impl<E, A> WsRoute<E, A>
where
    A: Actor,
{
    /// Creates a new `Route` for WebSocket handler.
    ///
    /// Expects an `extractor` instance that will check incoming request
    /// in a `RoutingTable` and will forward matched to the actor using
    /// its `address`.
    pub fn new(extractor: E, address: Address<A>) -> Self {
        Self { extractor, address }
    }
}

impl<E, A> Route for WsRoute<E, A>
where
    E: WsFromRequest,
    A: Actor + ActionHandler<WsReq<E>>,
{
    fn try_route(&self, addr: &SocketAddr, mut request: Request<Body>) -> RouteResult {
        match self.extractor.from_request(&request) {
            Some(Ok(value)) => {
                let mut res = Response::new(Body::empty());
                let address = self.address.clone();
                if request.headers().typed_get::<headers::Upgrade>().is_none() {
                    *res.status_mut() = StatusCode::BAD_REQUEST;
                    // TODO: Return error
                }
                let ws_key = request.headers().typed_get::<headers::SecWebsocketKey>();
                let addr = *addr;
                tokio::task::spawn(async move {
                    match hyper::upgrade::on(&mut request).await {
                        Ok(upgraded) => {
                            let websocket = tokio_tungstenite::WebSocketStream::from_raw_socket(
                                upgraded,
                                Role::Server,
                                None,
                            )
                            .await;
                            let stream = WsHandler::new(addr, websocket);
                            let msg = WsReq {
                                request: value,
                                stream,
                            };
                            address.act(msg)?;
                            Ok(())
                        }
                        Err(err) => {
                            log::error!("upgrade error: {}", err);
                            Err(Error::from(err))
                        }
                    }
                });
                *res.status_mut() = StatusCode::SWITCHING_PROTOCOLS;
                res.headers_mut()
                    .typed_insert(headers::Connection::upgrade());
                res.headers_mut()
                    .typed_insert(headers::Upgrade::websocket());
                if let Some(value) = ws_key {
                    res.headers_mut()
                        .typed_insert(headers::SecWebsocketAccept::from(value));
                }
                let fut = futures::future::ready(Ok(res));
                Ok(Box::pin(fut))
            }
            None => Err(request),
            Some(Err(err)) => {
                let fut = async move { Err(err) };
                Ok(Box::pin(fut))
            }
        }
    }
}

/// Alias to the upgraded web socket stream.
pub type WebSocket = WebSocketStream<Upgraded>;

struct WsInfo<P: Protocol> {
    //addr: SocketAddr,
    connection: WebSocket,
    rx: mpsc::UnboundedReceiver<P::ToClient>,
}

/// This struct wraps a `WebSocket` connection
/// and produces processors for every incoming connection.
pub struct WsHandler<P: Protocol> {
    log_target: String,
    addr: SocketAddr,
    info: Option<WsInfo<P>>,
    tx: mpsc::UnboundedSender<P::ToClient>,
}

impl<P: Protocol> WsHandler<P> {
    fn new(addr: SocketAddr, websocket: WebSocket) -> Self {
        let log_target = format!("WsHandler::{}", addr);
        let (tx, rx) = mpsc::unbounded();
        let info = WsInfo {
            //addr,
            connection: websocket,
            rx,
        };
        Self {
            log_target,
            addr,
            info: Some(info),
            tx,
        }
    }

    /// Address of a handler.
    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Creates a worker task for handling WebSocket messages.
    pub fn worker<A>(&mut self, address: Address<A>) -> WsProcessor<P, A>
    where
        A: Actor + ActionHandler<WsIncoming<P::ToServer>>,
    {
        let info = self.info.take().expect("already started");
        let log_target = format!("{}::WsProcessor", self.log_target);
        WsProcessor {
            log_target,
            info,
            address,
        }
    }

    /// Send an outgoing message to a client.
    ///
    /// It duplicates the purpose of the `WsSender` type that
    /// allows to send messages from other sources.
    // TODO: Consider replace it with using `WsSender` instances only.
    pub fn send(&self, msg: P::ToClient) {
        if let Err(err) = self.tx.unbounded_send(msg) {
            log::error!(target: &self.log_target, "Can't send outgoing WS message: {}", err);
        }
    }

    /// Returns a sender instance for sending outgoing messages.
    pub fn sender(&self) -> WsSender<P::ToClient> {
        WsSender::new(&self.log_target, self.tx.clone())
    }
}

/// The `LiteTask` that handles incoming and outgoing messages.
pub struct WsProcessor<P: Protocol, A: Actor> {
    log_target: String,
    info: WsInfo<P>,
    address: Address<A>,
}

/// Compatibility with the `tungstenite` crate.
impl<P, A> TalkerCompatible for WsProcessor<P, A>
where
    P: Protocol,
    A: Actor + ActionHandler<WsIncoming<P::ToServer>>,
{
    type WebSocket = WebSocket;
    type Message = tungstenite::Message;
    type Error = tungstenite::Error;
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
    type Output = TermReason;

    fn log_target(&self) -> &str {
        &self.log_target
    }

    async fn routine(self, stop: StopReceiver) -> Result<Self::Output, Error> {
        let mut talker = Talker::<Self>::new(
            &self.log_target,
            self.address,
            self.info.connection,
            self.info.rx,
            stop,
        );
        talker.routine().await
    }
}
