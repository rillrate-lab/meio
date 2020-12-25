use anyhow::Error;
use async_trait::async_trait;
use hyper::service::Service;
use hyper::{Body, Request, Response, Server, StatusCode};
use meio::prelude::{
    Actor, Context, IdOf, Interaction, InteractionPerformer, InteractionRecipient, LiteTask,
    StartedBy, StopReceiver, TaskEliminated,
};
use slab::Slab;
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{self, Poll};
use tokio::sync::RwLock;

struct Handler {
    route: Box<dyn TryRoute>,
}

trait Extractor: Send + Sync + 'static {
    type Request;

    fn try_extract(&self, request: &Request<Body>) -> Option<Self::Request>;
}

struct ServerInteraction<T> {
    pub value: T,
}

impl<T: Send + 'static> Interaction for ServerInteraction<T> {
    type Output = Response<Body>;
}

/// Checks an incoming `Request` and forward it to a recipient if it was valid.
struct Route<T, E> {
    extractor: E,
    recipient: InteractionRecipient<ServerInteraction<T>>,
}

trait TryRoute: Send + Sync + 'static {
    fn try_route(&self, request: &Request<Body>) -> Option<Box<dyn RouteExecutor>>;
}

impl<T, E> TryRoute for Route<T, E>
where
    Self: Sync,
    E: Extractor<Request = T>,
    T: Send + 'static,
{
    fn try_route(&self, request: &Request<Body>) -> Option<Box<dyn RouteExecutor>> {
        if let Some(value) = self.extractor.try_extract(request) {
            let recipient = self.recipient.clone();
            let route_interaction = RouterExecutorImpl {
                value: Some(value),
                recipient,
            };
            Some(Box::new(route_interaction))
        } else {
            None
        }
    }
}

#[async_trait]
trait RouteExecutor: Send {
    async fn execute(&mut self) -> Result<Response<Body>, Error>;
}

struct RouterExecutorImpl<T> {
    value: Option<T>,
    recipient: InteractionRecipient<ServerInteraction<T>>,
}

#[async_trait]
impl<T> RouteExecutor for RouterExecutorImpl<T>
where
    T: Send + 'static,
{
    async fn execute(&mut self) -> Result<Response<Body>, Error> {
        let value = self.value.take().unwrap();
        let msg = ServerInteraction { value };
        self.recipient.interact(msg).await
    }
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
        let routing_table = self.routing_table.clone();
        let fut = async move {
            let mut route = None;
            {
                let handlers = routing_table.handlers.read().await;
                for (_idx, handler) in handlers.iter() {
                    route = handler.route.try_route(&req);
                    if route.is_some() {
                        break;
                    }
                }
            }
            let mut response;
            if let Some(mut route) = route {
                let resp = route.execute().await;
                match resp {
                    Ok(resp) => {
                        response = resp;
                    }
                    Err(err) => {
                        log::error!("Server error for {}: {}", req.uri(), err);
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
