use crate::link;
use anyhow::Error;
use async_trait::async_trait;
use hyper::header::{self, HeaderValue};
use hyper::service::Service;
use hyper::upgrade::Upgraded;
use hyper::{Body, Request, Response, Server, StatusCode};
use meio::prelude::{
    Action, ActionHandler, Actor, Address, Context, IdOf, Interaction, InteractionHandler,
    InterruptedBy, LiteTask, StartedBy, StopReceiver, TaskEliminated,
};
use slab::Slab;
use std::future::Future;
use std::marker::PhantomData;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{self, Poll};
use tokio::sync::RwLock;

pub trait DirectPath: Default + Sized + Send + Sync + 'static {
    fn paths() -> &'static [&'static str];
}

impl<T> FromRequest for T
where
    T: DirectPath,
{
    fn from_request(request: &Request<Body>) -> Option<Self> {
        let path = request.uri().path();
        if Self::paths().iter().any(|p| p == &path) {
            Some(Self::default())
        } else {
            None
        }
    }
}

pub trait FromRequest: Sized + Send + Sync + 'static {
    fn from_request(request: &Request<Body>) -> Option<Self>;
}

pub struct Req<T> {
    pub request: T,
}

impl<T: Send + 'static> Interaction for Req<T> {
    type Output = Response<Body>;
}

pub(crate) trait Route: Send + Sync {
    fn try_route(
        &self,
        request: Request<Body>,
    ) -> Result<Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>, Request<Body>>;
}

pub(crate) struct RouteImpl<E, A>
where
    A: Actor,
{
    pub extracted: PhantomData<E>,
    pub address: Address<A>,
}

impl<E, A> Route for RouteImpl<E, A>
where
    E: FromRequest,
    A: Actor + InteractionHandler<Req<E>>,
{
    fn try_route(
        &self,
        request: Request<Body>,
    ) -> Result<Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>, Request<Body>>
    {
        if let Some(value) = E::from_request(&request) {
            let mut address = self.address.clone();
            let msg = Req { request: value };
            let fut = async move { address.interact(msg).await };
            Ok(Box::pin(fut))
        } else {
            Err(request)
        }
    }
}

pub struct WsReq<T> {
    pub request: T,
    pub stream: Upgraded,
}

impl<T: Send + 'static> Action for WsReq<T> {}

pub(crate) struct WsRouteImpl<E, A>
where
    A: Actor,
{
    pub extracted: PhantomData<E>,
    pub address: Address<A>,
}

impl<E, A> Route for WsRouteImpl<E, A>
where
    E: FromRequest,
    A: Actor + ActionHandler<WsReq<E>>,
{
    fn try_route(
        &self,
        request: Request<Body>,
    ) -> Result<Pin<Box<dyn Future<Output = Result<Response<Body>, Error>> + Send>>, Request<Body>>
    {
        if let Some(value) = E::from_request(&request) {
            let mut res = Response::new(Body::empty());
            let mut address = self.address.clone();
            if !request.headers().contains_key(header::UPGRADE) {
                *res.status_mut() = StatusCode::BAD_REQUEST;
            }
            tokio::task::spawn(async move {
                let res = request.into_body().on_upgrade().await;
                match res {
                    Ok(upgraded) => {
                        let msg = WsReq {
                            request: value,
                            stream: upgraded,
                        };
                        address.act(msg).await?;
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
                .insert(header::UPGRADE, HeaderValue::from_static("protocol"));
            let fut = futures::future::ready(Ok(res));
            Ok(Box::pin(fut))
        } else {
            Err(request)
        }
    }
}

#[derive(Clone, Default)]
struct RoutingTable {
    routes: Arc<RwLock<Slab<Box<dyn Route>>>>,
}

pub struct HttpServer {
    addr: SocketAddr,
    routing_table: RoutingTable,
}

impl HttpServer {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            routing_table: RoutingTable::default(),
        }
    }
}

impl Actor for HttpServer {
    type GroupBy = ();
}

#[async_trait]
impl<T: Actor> StartedBy<T> for HttpServer {
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
        let server_task = HyperRoutine {
            addr: self.addr.clone(),
            routing_table: self.routing_table.clone(),
        };
        ctx.spawn_task(server_task, ());
        Ok(())
    }
}

#[async_trait]
impl<T: Actor> InterruptedBy<T> for HttpServer {
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
        ctx.shutdown();
        Ok(())
    }
}

#[async_trait]
impl TaskEliminated<HyperRoutine> for HttpServer {
    async fn handle(
        &mut self,
        _id: IdOf<HyperRoutine>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        ctx.shutdown();
        Ok(())
    }
}

#[async_trait]
impl ActionHandler<link::AddRoute> for HttpServer {
    async fn handle(&mut self, msg: link::AddRoute, _ctx: &mut Context<Self>) -> Result<(), Error> {
        let mut routes = self.routing_table.routes.write().await;
        routes.insert(msg.route);
        Ok(())
    }
}

struct HyperRoutine {
    addr: SocketAddr,
    routing_table: RoutingTable,
}

#[async_trait]
impl LiteTask for HyperRoutine {
    async fn routine(self, stop: StopReceiver) -> Result<(), Error> {
        let routing_table = self.routing_table.clone();
        let make_svc = MakeSvc { routing_table };
        let server = Server::bind(&self.addr).serve(make_svc);
        server.with_graceful_shutdown(stop.into_future()).await?;
        Ok(())
    }
}

struct Svc {
    routing_table: RoutingTable,
}

impl Service<Request<Body>> for Svc {
    type Response = Response<Body>;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut task::Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let uri = req.uri().to_owned();
        log::trace!("Incoming request path: {}", uri.path());
        let routing_table = self.routing_table.clone();
        let fut = async move {
            let mut route = None;
            {
                let routes = routing_table.routes.read().await;
                let mut opt_req = Some(req);
                for (_idx, r) in routes.iter() {
                    if let Some(req) = opt_req.take() {
                        let res = r.try_route(req);
                        match res {
                            Ok(r) => {
                                route = Some(r);
                                break;
                            }
                            Err(req) => {
                                opt_req = Some(req);
                            }
                        }
                    }
                }
            }
            let mut response;
            if let Some(route) = route {
                let resp = route.await;
                match resp {
                    Ok(resp) => {
                        response = resp;
                    }
                    Err(err) => {
                        log::error!("Server error for {}: {}", uri, err);
                        response = Response::new(Body::empty());
                        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    }
                }
            } else {
                response = Response::new(Body::empty());
                *response.status_mut() = StatusCode::NOT_FOUND;
            }
            Ok(response)
        };
        Box::pin(fut)
    }
}

struct MakeSvc {
    routing_table: RoutingTable,
}

impl<T> Service<T> for MakeSvc {
    type Response = Svc;
    type Error = hyper::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut task::Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: T) -> Self::Future {
        let routing_table = self.routing_table.clone();
        let fut = async move { Ok(Svc { routing_table }) };
        Box::pin(fut)
    }
}
