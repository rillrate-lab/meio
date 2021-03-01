use super::actor::Route;
use super::HttpServer;
use anyhow::Error;
use derive_more::From;
use meio::{Action, Actor, Address, Context, Interaction, InteractionDone};
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
    pub fn wait_for_address<T: Actor>(&self, ctx: &mut Context<T>, group: T::GroupBy)
    where
        T: InteractionDone<WaitForAddress>,
    {
        let msg = WaitForAddress;
        ctx.interact(&self.address, msg, group);
    }
}
