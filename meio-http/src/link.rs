use super::HttpServer;
use crate::server::{Extractor, Route, RouteImpl};
use anyhow::Error;
use derive_more::From;
use hyper::{Body, Response};
use meio::prelude::{
    Action, Actor, Address, Interaction, InteractionHandler, InteractionRecipient,
};

#[derive(Debug, Clone, From)]
pub struct HttpServerLink {
    address: Address<HttpServer>,
}

pub(super) struct AddRoute {
    pub route: Box<dyn Route>,
}

impl Action for AddRoute {}

impl HttpServerLink {
    pub async fn add_route<E, A>(&mut self, extractor: E, address: Address<A>) -> Result<(), Error>
    where
        E: Extractor,
        E::Request: Interaction<Output = Response<Body>>,
        A: Actor + InteractionHandler<E::Request>,
    {
        let route = RouteImpl { extractor, address };
        let msg = AddRoute {
            route: Box::new(route),
        };
        self.address.act(msg).await
    }
}
