use super::HttpServer;
use crate::server::Route;
use anyhow::Error;
use derive_more::From;
use meio::prelude::{Actor, Address, InteractionHandler, InteractionRecipient};

#[derive(Debug, Clone, From)]
pub struct HttpServerLink {
    address: Address<HttpServer>,
}

pub(super) struct AddRoute {
    route: Box<dyn Route>,
}

/*
impl HttpServerLink {
    pub async fn add_route<E, I>(&mut self, extractor: E, recipient: impl Into<InteractionRecipient<HttpRequest<I>>>) -> Result<(), Error>
    where
        E: Extractor<Request = I>,
        //A: Actor + InteractionHandler<HttpRequest<I>>,
        I: Send + Sync + 'static,
    {
        let route = Route {
            extractor,
            recipient: recipient.into(),
        };
        let msg = AddRoute {
            route: Box::new(route),
        };
        Ok(())
    }
}
*/
