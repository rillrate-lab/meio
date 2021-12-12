//! Actor for launch a server.

pub mod bind;
pub mod link;
pub mod route;
mod routine;
pub mod websocket;

pub use bind::WaitForAddress;
pub use route::{DirectPath, FromRequest, NoParameters, Req, WebRoute};
pub use websocket::{WsHandler, WsProcessor, WsReq, WsRoute};

use anyhow::Error;
use async_trait::async_trait;
use derive_more::From;
use meio::prelude::{Actor, Address, Context, InterruptedBy, StartedBy};
use std::net::SocketAddr;

/// The link to a HTTP server instance.
#[derive(Debug, Clone, From)]
pub struct HttpServerLink {
    address: Address<HttpServer>,
}

/// The actor that binds HTTP server to a port and handle incoming requests.
pub struct HttpServer {
    log_target: String,
    // TODO: Use submodules' types instead
    // (don't use plain fields from all modules)
    addr: SocketAddr,
    addr_state: bind::AddrState,
    routing_table: route::RoutingTable,
    /// Interval (seconds) of retry if binding failed.
    retry_interval: Option<u64>,
}

impl HttpServer {
    /// Creates a new server instance.
    /// It will be bind to `addr` and if binding failed will retry
    /// to bind again in `retry_interval` seconds.
    pub fn new(addr: SocketAddr, retry_interval: Option<u64>) -> Self {
        let log_target = format!("HttpServer::{}", addr);
        Self {
            log_target,
            addr,
            addr_state: bind::AddrState::default(),
            routing_table: route::RoutingTable::default(),
            retry_interval,
        }
    }
}

impl Actor for HttpServer {
    type GroupBy = ();

    fn log_target(&self) -> &str {
        &self.log_target
    }
}

#[async_trait]
impl<T: Actor> StartedBy<T> for HttpServer {
    async fn handle(&mut self, ctx: &mut Context<Self>) -> Result<(), Error> {
        self.start_http_listener(ctx);
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

#[cfg(test)]
mod tests {
    use serde::Deserialize;

    #[derive(Default, Deserialize)]
    struct Index {}

    #[test]
    fn qs_index() {
        let _index: Index = serde_qs::from_str("").unwrap();
    }

    #[derive(Default, Deserialize)]
    struct ApiQuery {
        query: String,
    }

    #[test]
    fn qs_query() {
        let api_query: ApiQuery = serde_qs::from_str("query=abc").unwrap();
        assert_eq!(api_query.query, "abc");
    }
}
