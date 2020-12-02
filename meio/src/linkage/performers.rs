//! This module contains extensions for the `Address` type
//! to allow to send specific messages to actors.

use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler, Interaction, InteractionHandler};
use crate::linkage::address::Address;
use anyhow::Error;
use async_trait::async_trait;

/// A generic trait for `Action` functionality.
/// It represents a function with a reaction and can be implemented directly
/// of as an `Actor`.
/// You can use this trait to have any generic functionality inside other
/// actors that can be replaced by a simple function or a self-performed `Actor`.
///
/// `'static` reqirement added to make possible to use `as_ref` for returning
/// for a multi-purpose links.
#[async_trait]
pub trait ActionPerformer<I: Action>: Send + 'static {
    /// Send `Action` message to an `Actor`.
    async fn act(&mut self, input: I) -> Result<(), Error>;
}

#[async_trait]
impl<A, I> ActionPerformer<I> for Address<A>
where
    A: Actor,
    A: ActionHandler<I>,
    I: Action,
{
    async fn act(&mut self, input: I) -> Result<(), Error> {
        Address::act(self, input).await
        /*
        let high_priority = input.is_high_priority();
        let envelope = Envelope::action(input);
        self.send(envelope, high_priority).await
        */
    }
}

/// A generic trait for `Interaction` functionality.
/// The same as for `ActionPerformer`. This trait can be implemented
/// be an `Actor` or just a simple function that can use async `Mutex`
/// and provide a full functionality without a spawned actor.
///
/// `'static` reqirement added to make possible to use `as_ref` for returning
/// for a multi-purpose links.
#[async_trait]
pub trait InteractionPerformer<I: Interaction>: Send + 'static {
    /// Send `Interaction` message to an `Actor` and wait for the response.
    async fn interact(&mut self, input: I) -> Result<I::Output, Error>;
}

#[async_trait]
impl<A, I> InteractionPerformer<I> for Address<A>
where
    A: Actor,
    A: InteractionHandler<I>,
    I: Interaction,
{
    async fn interact(&mut self, input: I) -> Result<I::Output, Error> {
        Address::interact(self, input).await
        /*
        let high_priority = input.is_high_priority();
        let (envelope, rx) = Envelope::interaction(input);
        self.send(envelope, high_priority).await?;
        let res = rx.await??;
        Ok(res)
        */
    }
}
