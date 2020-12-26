use super::HttpServer;
use crate::server::{FromRequest, Req, Route, RouteImpl};
use anyhow::Error;
use derive_more::From;
use meio::prelude::{Action, Actor, Address, InteractionHandler};
use std::marker::PhantomData;

#[derive(Debug, Clone, From)]
pub struct HttpServerLink {
    address: Address<HttpServer>,
}

pub(super) struct AddRoute {
    pub route: Box<dyn Route>,
}

impl Action for AddRoute {}

impl HttpServerLink {
    pub async fn add_route<E, A>(&mut self, address: Address<A>) -> Result<(), Error>
    where
        E: FromRequest,
        A: Actor + InteractionHandler<Req<E>>,
    {
        let route = RouteImpl {
            extracted: PhantomData,
            address,
        };
        let msg = AddRoute {
            route: Box::new(route),
        };
        self.address.act(msg).await
    }
}
