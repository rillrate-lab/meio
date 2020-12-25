use anyhow::Error;
use async_trait::async_trait;
use hyper::server::conn::AddrStream;
use hyper::service::{make_service_fn, service_fn, Service};
use hyper::{Body, Request, Response, Server};
use meio::prelude::{Actor, LiteTask, StopReceiver};
use slab::Slab;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{self, Poll};
use tokio::sync::RwLock;

struct Handler {
    // TODO: Add path checker
// TODO: Add inter_action recipient
}

#[derive(Clone, Default)]
struct RoutingTable {
    handlers: Arc<RwLock<Slab<Handler>>>,
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

struct ServerTask {
    addr: SocketAddr,
}

impl ServerTask {
    async fn make_service(
        addr_stream: &AddrStream,
    ) -> Result<impl Service<Request<Body>>, hyper::Error> {
        Ok(service_fn(Self::handler))
    }

    async fn handler(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
        todo!()
    }
}

#[async_trait]
impl LiteTask for ServerTask {
    async fn routine(self, stop: StopReceiver) -> Result<(), Error> {
        let make_svc =
            make_service_fn(|addr_stream| async { Ok::<_, Error>(service_fn(Self::handler)) });
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
        let routing_table = self.routing_table.clone();
        let fut = async move {
            let handlers = routing_table.handlers.read().await;
            for (_idx, handler) in handlers.iter() {}
            todo!()
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
