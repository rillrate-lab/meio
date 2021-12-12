//! The link to interact with a server instance.

use super::{
    route::{BoxedRoute, Route},
    HttpServer, HttpServerLink,
};
use anyhow::Error;
use async_trait::async_trait;
use meio::prelude::{Action, ActionHandler, Context};

impl HttpServerLink {
    /// Adds a route to the `HttpServer`.
    pub fn add_route<T>(&mut self, route: T) -> Result<(), Error>
    where
        T: Route,
    {
        let msg = AddRoute {
            route: Box::new(route),
        };
        self.address.act(msg)
    }
}

struct AddRoute {
    pub route: BoxedRoute,
}

impl Action for AddRoute {}

#[async_trait]
impl ActionHandler<AddRoute> for HttpServer {
    async fn handle(&mut self, msg: AddRoute, _ctx: &mut Context<Self>) -> Result<(), Error> {
        self.routing_table.insert_route(msg.route).await;
        Ok(())
    }
}
