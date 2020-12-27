use super::Address;
use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler};
use anyhow::Error;
use async_trait::async_trait;
use std::fmt::Debug;

/// Abstract `Address` to the `Actor` that can handle a sepcific message type.
#[async_trait]
pub trait ActionRecipient<T>: Debug + Clone + Send + 'static {
    /// Send an `Action` to an `Actor`.
    async fn act(&mut self, msg: T) -> Result<(), Error>;
}

#[async_trait]
impl<T, A> ActionRecipient<T> for Address<A>
where
    T: Action,
    A: Actor + ActionHandler<T>,
{
    async fn act(&mut self, msg: T) -> Result<(), Error> {
        Address::act(self, msg).await
    }
}
