use super::{bind::AddrReady, route::RoutingTable, HttpServer};
use anyhow::Error;
use async_trait::async_trait;
use futures::future::{self, Either, FutureExt};
use hyper::server::conn::AddrStream;
use hyper::service::Service;
use hyper::{Body, Request, Response, Server, StatusCode};
use meio::prelude::{
    Address, Context, IdOf, LiteTask, Scheduled, StopReceiver, TaskEliminated, TaskError,
};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{self, Poll};
use std::time::{Duration, Instant};
use tokio::time::sleep;

impl HttpServer {
    pub(super) fn start_http_listener(&mut self, ctx: &mut Context<Self>) {
        let log_target = Arc::new(format!("{}::HyperRoutine", self.log_target));
        let server_task = HyperRoutine {
            log_target,
            owner: ctx.address().clone(),
            addr: self.addr,
            routing_table: self.routing_table.clone(),
        };
        ctx.spawn_task(server_task, (), ());
    }
}

const SHUTDOWN_TIMEOUT_SEC: u64 = 5;

pub(super) struct HyperRoutine {
    log_target: Arc<String>,
    owner: Address<HttpServer>,
    addr: SocketAddr,
    routing_table: RoutingTable,
}

#[async_trait]
impl LiteTask for HyperRoutine {
    type Output = ();

    fn log_target(&self) -> &str {
        &self.log_target
    }

    async fn routine(mut self, stop: StopReceiver) -> Result<Self::Output, Error> {
        let log_target = self.log_target.clone();
        let routing_table = self.routing_table.clone();
        let make_svc = MakeSvc {
            log_target,
            routing_table,
        };
        let server = Server::try_bind(&self.addr)?.serve(make_svc);
        let addr = server.local_addr();
        let ready = AddrReady::from(addr);
        self.owner.act(ready)?;
        let delayed_stop = stop
            .clone()
            .into_future()
            .map(drop)
            .then(|_| sleep(Duration::from_secs(SHUTDOWN_TIMEOUT_SEC)));
        let server_fut = server.with_graceful_shutdown(stop.into_future());
        tokio::pin!(delayed_stop);
        let res = future::select(delayed_stop, server_fut).await;
        if let Either::Right((result, _)) = res {
            result.map_err(Error::from)
        } else {
            Ok(())
        }
    }
}

#[async_trait]
impl TaskEliminated<HyperRoutine, ()> for HttpServer {
    async fn handle(
        &mut self,
        _id: IdOf<HyperRoutine>,
        _tag: (),
        result: Result<(), TaskError>,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        if !ctx.is_terminating() {
            if let Err(err) = result {
                log::error!(target: &self.log_target, "Server failed: {}", err);
                if let Some(interval) = self.retry_interval {
                    let when = Instant::now() + Duration::from_secs(interval);
                    log::debug!(target: &self.log_target, "Schedule restarting at {:?}", when);
                    ctx.address().schedule(RestartListener, when)?;
                }
            }
        }
        Ok(())
    }
}

struct RestartListener;

#[async_trait]
impl Scheduled<RestartListener> for HttpServer {
    async fn handle(
        &mut self,
        _timestamp: Instant,
        _action: RestartListener,
        ctx: &mut Context<Self>,
    ) -> Result<(), Error> {
        log::info!(target: &self.log_target, "Attempt to restart the server");
        self.start_http_listener(ctx);
        Ok(())
    }
}

struct Svc {
    log_target: Arc<String>,
    addr: SocketAddr,
    routing_table: RoutingTable,
}

pub type SvcFut<T, E> = Pin<Box<dyn Future<Output = Result<T, E>> + Send>>;

impl Service<Request<Body>> for Svc {
    type Response = Response<Body>;
    type Error = hyper::Error;
    type Future = SvcFut<Self::Response, Self::Error>;

    fn poll_ready(&mut self, _: &mut task::Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: Request<Body>) -> Self::Future {
        let uri = req.uri().to_owned();
        log::trace!(target: &self.log_target, "Incoming request path: {}", uri.path());
        let routing_table = self.routing_table.clone();
        let addr = self.addr;
        let log_target = self.log_target.clone();
        let fut = async move {
            let mut route = None;
            {
                let routes = routing_table.routes().await;
                let mut opt_req = Some(req);
                for (_idx, r) in routes.iter() {
                    if let Some(req) = opt_req.take() {
                        let res = r.try_route(&addr, req);
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
                        log::error!(target: &log_target, "Server error for {}: {}", uri, err);
                        let reason: Body = err.to_string().into();
                        response = Response::new(reason);
                        *response.status_mut() = StatusCode::INTERNAL_SERVER_ERROR;
                    }
                }
            } else {
                log::warn!(target: &log_target, "No route for {}", uri);
                response = Response::new(Body::empty());
                *response.status_mut() = StatusCode::NOT_FOUND;
            }
            Ok(response)
        };
        Box::pin(fut)
    }
}

struct MakeSvc {
    log_target: Arc<String>,
    routing_table: RoutingTable,
}

impl<'a> Service<&'a AddrStream> for MakeSvc {
    type Response = Svc;
    type Error = hyper::Error;
    type Future = SvcFut<Self::Response, Self::Error>;

    fn poll_ready(&mut self, _: &mut task::Context) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, addr_stream: &'a AddrStream) -> Self::Future {
        let routing_table = self.routing_table.clone();
        let log_target = self.log_target.clone();
        let addr = addr_stream.remote_addr();
        let fut = async move {
            Ok(Svc {
                log_target,
                addr,
                routing_table,
            })
        };
        Box::pin(fut)
    }
}
