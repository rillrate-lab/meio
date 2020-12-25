use anyhow::Error;
use async_trait::async_trait;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Response, Server};
use meio::prelude::{Actor, LiteTask, StopReceiver};
use std::net::SocketAddr;

pub struct HttpServer {
    addr: SocketAddr,
}

impl HttpServer {
    pub fn new(addr: SocketAddr) -> Self {
        Self { addr }
    }
}

impl Actor for HttpServer {
    type GroupBy = ();
}

struct ServerTask {
    addr: SocketAddr,
}

#[async_trait]
impl LiteTask for ServerTask {
    async fn routine(self, stop: StopReceiver) -> Result<(), Error> {
        let make_svc = make_service_fn(|_| async {
            Ok::<_, Error>(service_fn(|_req| async {
                Ok::<_, Error>(Response::new(Body::from("meio-http")))
            }))
        });
        let server = Server::bind(&self.addr).serve(make_svc);
        server.with_graceful_shutdown(stop.into_future()).await?;
        Ok(())
    }
}
