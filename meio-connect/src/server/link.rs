use super::actor::Route;
use super::HttpServer;
use anyhow::Error;
use derive_more::From;
use meio::{Action, Address, Interaction, InteractionTask};
use std::net::SocketAddr;

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

pub struct WaitForAddress;

impl Interaction for WaitForAddress {
    type Output = SocketAddr;
}

impl HttpServerLink {
    pub fn wait_for_address(&self) -> InteractionTask<WaitForAddress> {
        self.address.interact(WaitForAddress)
    }
}
