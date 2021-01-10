use super::actor::{FromRequest, Req, Route, RouteImpl, WsReq, WsRouteImpl};
use super::HttpServer;
use anyhow::Error;
use derive_more::From;
use meio::prelude::{Action, ActionHandler, Actor, Address, InteractionHandler};
use meio_protocol::Protocol;
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
    pub async fn add_route<E, A>(&mut self, extracted: E, address: Address<A>) -> Result<(), Error>
    where
        E: FromRequest,
        A: Actor + InteractionHandler<Req<E>>,
    {
        let route = RouteImpl { extracted, address };
        let msg = AddRoute {
            route: Box::new(route),
        };
        self.address.act(msg).await
    }

    pub async fn add_ws_route<E, P, A>(
        &mut self,
        extracted: E,
        address: Address<A>,
    ) -> Result<(), Error>
    where
        E: FromRequest,
        P: Protocol + Sync,
        A: Actor + ActionHandler<WsReq<E, P>>,
    {
        let route = WsRouteImpl {
            extracted,
            protocol: PhantomData,
            address,
        };
        let msg = AddRoute {
            route: Box::new(route),
        };
        self.address.act(msg).await
    }
}
