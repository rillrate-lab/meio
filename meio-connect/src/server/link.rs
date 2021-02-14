use super::actor::Route;
use super::HttpServer;
use anyhow::Error;
use derive_more::From;
use meio::prelude::{Action, Address};

#[derive(Debug, Clone, From)]
pub struct HttpServerLink {
    address: Address<HttpServer>,
}

pub(super) struct AddRoute {
    pub route: Box<dyn Route>,
}

impl Action for AddRoute {}

impl HttpServerLink {
    pub async fn add_route<T>(&mut self, route: T) -> Result<(), Error>
    where
        T: Route,
    {
        let msg = AddRoute {
            route: Box::new(route),
        };
        self.address.act(msg).await
    }
}
