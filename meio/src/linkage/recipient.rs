use super::Address;
use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler, Interaction, InteractionHandler};
use anyhow::Error;
use async_trait::async_trait;
use std::fmt::Debug;
use std::hash::Hash;

/// Abstract `Address` to the `Actor` that can handle a specific message type.
#[async_trait]
pub trait ActionRecipient<T: Action>:
    Debug + Clone + Eq + PartialEq + Hash + Send + 'static
{
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

/// Abstract `Address` to the `Actor` that can handle an interaction.
#[async_trait]
pub trait InteractionRecipient<T: Interaction>:
    Debug + Clone + Eq + PartialEq + Hash + Send + 'static
{
    /// Interact with an `Actor`.
    async fn interact(&mut self, msg: T) -> Result<T::Output, Error>;
}

#[async_trait]
impl<T, A> InteractionRecipient<T> for Address<A>
where
    T: Interaction,
    A: Actor + InteractionHandler<T>,
{
    async fn interact(&mut self, msg: T) -> Result<T::Output, Error> {
        Address::interact(self, msg).await
    }
}
