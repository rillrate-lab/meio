/// Recipients implement `Hash` for a boxed instance, but it's not useful in cases
/// if you want to keep it in a `HashSet` for example, because the contents of
/// hash sets can't be borrowed as mutable and that means you can't call `act`
/// method of it.
/// It's recommended to use `HashSet<Id, Box<dyn Recipient>>` instead.
use super::Address;
use crate::actor_runtime::Actor;
use crate::handlers::{Action, ActionHandler, Interact, Interaction};
use crate::ids::Id;
use anyhow::Error;
use async_trait::async_trait;
use std::fmt::Debug;
use std::hash::{Hash, Hasher};

/// Abstract `Address` to the `Actor` that can handle a specific message type.
#[async_trait]
pub trait ActionRecipient<T: Action>: Debug + Send + 'static {
    /// Send an `Action` to an `Actor`.
    async fn act(&mut self, msg: T) -> Result<(), Error>;

    /// Returns a reference to `Id` of an `Address` inside.
    #[doc(hidden)]
    fn id_ref(&self) -> &Id;

    #[doc(hidden)]
    fn dyn_clone(&self) -> Box<dyn ActionRecipient<T>>;

    #[doc(hidden)]
    fn dyn_hash(&self, state: &mut dyn Hasher);
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

impl<T: Action> Hash for Box<dyn ActionRecipient<T>> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.dyn_hash(state);
    }
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

    fn id_ref(&self) -> &Id {
        self.raw_id()
    }

    fn dyn_clone(&self) -> Box<dyn ActionRecipient<T>> {
        Box::new(self.clone())
    }

    fn dyn_hash(&self, state: &mut dyn Hasher) {
        ActionRecipient::id_ref(self).hash(&mut Box::new(state));
    }
}

/// Abstract `Address` to the `Actor` that can handle an interaction.
#[async_trait]
pub trait InteractionRecipient<T: Interaction>: Debug + Send + 'static {
    /// Interact with an `Actor`.
    async fn interact_and_wait(&mut self, msg: T) -> Result<T::Output, Error>;

    /// Returns a reference to `Id` of an `Address` inside.
    #[doc(hidden)]
    fn id_ref(&self) -> &Id;

    #[doc(hidden)]
    fn dyn_clone(&self) -> Box<dyn InteractionRecipient<T>>;

    #[doc(hidden)]
    fn dyn_hash(&self, state: &mut dyn Hasher);
}

impl<T: Interaction> Clone for Box<dyn InteractionRecipient<T>> {
    fn clone(&self) -> Self {
        self.dyn_clone()
    }
}

impl<T: Interaction> PartialEq for Box<dyn InteractionRecipient<T>> {
    fn eq(&self, other: &Self) -> bool {
        PartialEq::eq(self.id_ref(), other.id_ref())
    }
}

impl<T: Interaction> Eq for Box<dyn InteractionRecipient<T>> {}

impl<T: Interaction> Hash for Box<dyn InteractionRecipient<T>> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.dyn_hash(state);
    }
}

#[async_trait]
impl<T, A> InteractionRecipient<T> for Address<A>
where
    T: Interaction,
    A: Actor + ActionHandler<Interact<T>>,
{
    async fn interact_and_wait(&mut self, msg: T) -> Result<T::Output, Error> {
        Address::interact_and_wait(self, msg).await
    }

    fn id_ref(&self) -> &Id {
        self.raw_id()
    }

    fn dyn_clone(&self) -> Box<dyn InteractionRecipient<T>> {
        Box::new(self.clone())
    }

    fn dyn_hash(&self, state: &mut dyn Hasher) {
        InteractionRecipient::id_ref(self).hash(&mut Box::new(state));
    }
}
