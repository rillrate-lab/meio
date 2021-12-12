//! Contains basic routing capabilities.

use anyhow::Error;
use hyper::{Body, Request, Response};
use meio::handlers::Interact;
use meio::prelude::{ActionHandler, Actor, Address, Interaction};
use serde::{de::DeserializeOwned, Deserialize};
use slab::Slab;
use std::future::Future;
use std::net::SocketAddr;
use std::ops::Deref;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;
use tokio::sync::RwLock;

/// A boxed route alias to hold them in the `RoutingTable`.
pub type BoxedRoute = Box<dyn Route>;

#[derive(Clone, Default)]
pub(super) struct RoutingTable {
    routes: Arc<RwLock<Slab<BoxedRoute>>>,
}

impl RoutingTable {
    pub async fn insert_route(&mut self, route: BoxedRoute) {
        let mut routes = self.routes.write().await;
        routes.insert(route);
    }

    pub async fn routes(&self) -> impl Deref<Target = Slab<BoxedRoute>> + '_ {
        self.routes.read().await
    }
}

/// Empty parameters.
#[derive(Default, Deserialize)]
pub struct NoParameters {}

/// Error when route failed (matched, but can't be processed).
#[derive(Debug, Error)]
#[error("route error [path = {path}, query = {query}]: {reason}")]
pub struct RouteError {
    /// Path of a request.
    pub path: String,
    /// Query of a request.
    pub query: String,
    /// The reason of failing.
    pub reason: String,
}

impl RouteError {
    /// Creates a new error instance.
    /// Expects parameters of a request.
    pub fn new(path: impl ToString, query: impl ToString, reason: impl ToString) -> Self {
        Self {
            path: path.to_string(),
            query: query.to_string(),
            reason: reason.to_string(),
        }
    }
}

/// The special handler for plain routes.
pub trait DirectPath: Sized + Send + Sync + 'static {
    /// Extracted value from the request.
    type Output: DeserializeOwned + Send;
    /// Extra type parameter.
    /// Used for web sockets.
    type Parameter;
    /// Slice of plain paths for matching a request.
    fn paths() -> &'static [&'static str];
}

impl<T> FromRequest for T
where
    T: DirectPath,
{
    type Output = <T as DirectPath>::Output;

    fn from_request(&self, request: &Request<Body>) -> Option<Result<Self::Output, Error>> {
        let uri = request.uri();
        let path = uri.path();
        if Self::paths().iter().any(|p| p == &path) {
            // `Body` should be parsed in the handler, becuase it doesn't participate in routing.
            // And `Body` extracting requires an ownership.
            let query = uri.query().unwrap_or("");
            let output =
                serde_qs::from_str(query).map_err(|err| RouteError::new(path, query, err).into());
            Some(output)
        } else {
            None
        }
    }
}

/// Gets the data from a request.
pub trait FromRequest: Sized + Send + Sync + 'static {
    /// The expected result of data extraction.
    type Output: Send;

    /// Matches and tries to extract data for the corresponding route.
    fn from_request(&self, request: &Request<Body>) -> Option<Result<Self::Output, Error>>;
}

/// The request wrapper.
pub struct Req<T: FromRequest> {
    /// Address of a client.
    pub addr: SocketAddr,
    /// Parsed request value.
    pub data: T::Output,
    /// Body of the request.
    pub body: Body,
}

impl<T: FromRequest> Interaction for Req<T> {
    type Output = Response<Body>;
}

/// The result of `Route`'s matching.
///
/// The future for handing the request of instant error response.
pub type RouteResult =
    Result<Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>, Request<Body>>;

/// The abstract route.
pub trait Route: Send + Sync + 'static {
    /// Tries to process a request and produce a response for it.
    ///
    /// Returns a `Future` tha tproduces a response.
    fn try_route(&self, addr: &SocketAddr, request: Request<Body>) -> RouteResult;
}

/// The route matcher for ordinary HTTP request.
pub struct WebRoute<E, A>
where
    A: Actor,
{
    extractor: E,
    address: Address<A>,
}

impl<E, A> WebRoute<E, A>
where
    A: Actor,
{
    /// Creates a new `Route` checker for plain requests.
    ///
    /// Expects an `extractor` that sends a matched request
    /// to an `address`.
    pub fn new(extractor: E, address: Address<A>) -> Self {
        Self { extractor, address }
    }
}

/// Here used `ActionHandler` instead of `InteractionHandler`
/// to make it possible to use both types of handlers.
/// And `ActionHandler` is useful to handle long running requests.
impl<E, A> Route for WebRoute<E, A>
where
    E: FromRequest,
    A: Actor + ActionHandler<Interact<Req<E>>>,
{
    fn try_route(&self, addr: &SocketAddr, request: Request<Body>) -> RouteResult {
        match self.extractor.from_request(&request) {
            Some(Ok(data)) => {
                let msg = Req {
                    addr: *addr,
                    data,
                    body: request.into_body(),
                };
                let fut = self.address.interact(msg).recv();
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
