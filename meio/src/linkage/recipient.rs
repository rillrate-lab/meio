use super::Address;
use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler, Interaction, InteractionHandler};
use crate::ids::Id;
use anyhow::Error;
use async_trait::async_trait;
use std::fmt::Debug;
//use std::hash::Hash;

/// Abstract `Address` to the `Actor` that can handle a specific message type.
#[async_trait]
pub trait ActionRecipient<T: Action>: Debug + Send + 'static {
    /// Send an `Action` to an `Actor`.
    async fn act(&mut self, msg: T) -> Result<(), Error>;

    #[doc(hidden)]
    fn dyn_clone(&self) -> Box<dyn ActionRecipient<T>>;

    #[doc(hidden)]
    fn id_ref(&self) -> &Id;
}

impl<T: Action> Clone for Box<dyn ActionRecipient<T>> {
    fn clone(&self) -> Self {
        self.dyn_clone()
    }
}

impl<T: Action> PartialEq for Box<dyn ActionRecipient<T>> {
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(self.id_ref(), other.id_ref())
    }
}

impl<T: Action> Eq for Box<dyn ActionRecipient<T>> {}

#[async_trait]
impl<T, A> ActionRecipient<T> for Address<A>
where
    T: Action,
    A: Actor + ActionHandler<T>,
{
    async fn act(&mut self, msg: T) -> Result<(), Error> {
        Address::act(self, msg).await
    }

    fn dyn_clone(&self) -> Box<dyn ActionRecipient<T>> {
        Box::new(self.clone())
    }

    fn id_ref(&self) -> &Id {
        self.raw_id()
    }
}

/// Abstract `Address` to the `Actor` that can handle an interaction.
#[async_trait]
pub trait InteractionRecipient<T: Interaction>: Debug + Send + 'static {
    /// Interact with an `Actor`.
    async fn interact(&mut self, msg: T) -> Result<T::Output, Error>;

    #[doc(hidden)]
    fn dyn_clone(&self) -> Box<dyn InteractionRecipient<T>>;
}

impl<T: Interaction> Clone for Box<dyn InteractionRecipient<T>> {
    fn clone(&self) -> Self {
        self.dyn_clone()
    }
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

    fn dyn_clone(&self) -> Box<dyn InteractionRecipient<T>> {
        Box::new(self.clone())
    }
}
