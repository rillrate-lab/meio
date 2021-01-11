use super::actor::{FromRequest, Req, Route, RouteImpl, WsFromRequest, WsReq, WsRouteImpl};
use super::HttpServer;
use anyhow::Error;
use derive_more::From;
use meio::prelude::{Action, ActionHandler, Actor, Address, InteractionHandler};

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

    pub async fn add_ws_route<E, A>(
        &mut self,
        extracted: E,
        address: Address<A>,
    ) -> Result<(), Error>
    where
        E: WsFromRequest,
        A: Actor + ActionHandler<WsReq<E>>,
    {
        let route = WsRouteImpl { extracted, address };
        let msg = AddRoute {
            route: Box::new(route),
        };
        self.address.act(msg).await
    }
}
